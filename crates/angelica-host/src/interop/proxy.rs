//! The proxy Windows uses for a URL, as the browser finds it: the user's
//! proxy settings, with automatic detection (WPAD) and a configuration
//! script (PAC) through `WinHTTP`, then the manual proxy and its bypass list.
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::GlobalFree,
    Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
        WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
        WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS,
        WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO,
        WinHttpGetIEProxyConfigForCurrentUser, WinHttpGetProxyForUrl, WinHttpOpen,
    },
};

/// How long a host's proxy is kept before Windows is asked again.
const KEEP: Duration = Duration::from_mins(5);

/// Sends this process's HTTPS requests through the proxy Windows uses for
/// them. `log` hears which way each host goes, once until it changes.
pub fn use_system_proxy(log: fn(&str)) {
    let known: Mutex<HashMap<String, (Option<String>, Instant)>> = Mutex::default();
    crate::http::set_proxy_finder(Box::new(move |url| {
        let host = url
            .parse::<ureq::http::Uri>()
            .ok()
            .and_then(|uri| uri.host().map(str::to_owned))
            .unwrap_or_default();
        let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((proxy, at)) = known.get(&host)
            && at.elapsed() < KEEP
        {
            return proxy.clone();
        }
        let proxy = find(url, &host);
        if known.get(&host).map(|(before, _)| before) != Some(&proxy) {
            log(&match &proxy {
                Some(proxy) => format!("network: {host} goes through the proxy {proxy}"),
                None => format!("network: {host} goes directly"),
            });
        }
        known.insert(host, (proxy.clone(), Instant::now()));
        proxy
    }));
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// A string Windows allocated, read and freed.
///
/// # Safety
/// `text` is null or a NUL-terminated UTF-16 string from `GlobalAlloc`
/// that nothing else frees.
unsafe fn take(text: *mut u16) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a NUL-terminated string.
    let length = unsafe {
        (0..usize::from(u16::MAX))
            .take_while(|&i| *text.add(i) != 0)
            .count()
    };
    // SAFETY: `length` units before the NUL are readable.
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: the caller guarantees the string came from GlobalAlloc and is
    // freed only here.
    unsafe { GlobalFree(text.cast()) };
    Some(value)
}

/// The `WinHTTP` session automatic detection runs in, opened once.
fn session() -> *mut core::ffi::c_void {
    static SESSION: OnceLock<usize> = OnceLock::new();
    *SESSION.get_or_init(|| {
        let agent = wide("Angelica");
        // SAFETY: the agent string is NUL-terminated; no proxy strings for a
        // session that only asks for proxies.
        unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            ) as usize
        }
    }) as *mut core::ffi::c_void
}

/// The proxy for `url` on `host`, or none to connect directly.
fn find(url: &str, host: &str) -> Option<String> {
    let mut settings = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    // SAFETY: `settings` is owned and writable; the strings it receives are
    // taken and freed below.
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&raw mut settings) } == 0 {
        return None;
    }
    // SAFETY: the strings come from WinHttpGetIEProxyConfigForCurrentUser
    // and are freed once each.
    let (script, manual, bypass) = unsafe {
        (
            take(settings.lpszAutoConfigUrl),
            take(settings.lpszProxy),
            take(settings.lpszProxyBypass),
        )
    };
    let detect = settings.fAutoDetect != 0;
    if detect || script.is_some() {
        match automatic(url, detect, script.as_deref()) {
            Answer::Direct => return None,
            Answer::Proxy(proxy) => return Some(proxy),
            Answer::None => {}
        }
    }
    let manual = manual?;
    if bypass
        .as_deref()
        .is_some_and(|bypass| crate::http::bypassed(bypass, host))
    {
        return None;
    }
    crate::http::proxy_from_list(&manual)
}

/// What automatic detection or the configuration script says.
enum Answer {
    Direct,
    Proxy(String),
    /// No answer: no script found, or it failed.
    None,
}

/// What automatic detection or the configuration script says for `url`.
fn automatic(url: &str, detect: bool, script: Option<&str>) -> Answer {
    let session = session();
    if session.is_null() {
        return Answer::None;
    }
    let script = script.map(wide);
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: if detect {
            WINHTTP_AUTOPROXY_AUTO_DETECT
        } else {
            0
        } | if script.is_some() {
            WINHTTP_AUTOPROXY_CONFIG_URL
        } else {
            0
        },
        dwAutoDetectFlags: if detect {
            WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A
        } else {
            0
        },
        lpszAutoConfigUrl: script.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
        fAutoLogonIfChallenged: 1,
        ..Default::default()
    };
    let url = wide(url);
    let mut info = WINHTTP_PROXY_INFO::default();
    // SAFETY: the session is open, the strings are NUL-terminated and
    // outlive the call, and `info` receives strings freed below.
    if unsafe { WinHttpGetProxyForUrl(session, url.as_ptr(), &raw mut options, &raw mut info) } == 0
    {
        return Answer::None;
    }
    // SAFETY: the strings come from WinHttpGetProxyForUrl and are freed once
    // each.
    let (proxy, _bypass) = unsafe { (take(info.lpszProxy), take(info.lpszProxyBypass)) };
    match proxy.as_deref().and_then(crate::http::proxy_from_list) {
        Some(proxy) if info.dwAccessType == WINHTTP_ACCESS_TYPE_NAMED_PROXY => Answer::Proxy(proxy),
        _ => Answer::Direct,
    }
}
