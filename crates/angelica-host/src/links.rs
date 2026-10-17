//! Pages plugins open in the player's browser (the `links` interface),
//! checked here and opened by the overlay at the end of the frame.
#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::error::{Error, Kind, Result};

/// The longest link, in bytes.
pub const MOST_BYTES: usize = 2048;
/// The least time between two links of one plugin.
pub const SPACING: Duration = Duration::from_secs(1);

/// Checks that `url` is a link a plugin may open: an absolute `https://`
/// URL of at most [`MOST_BYTES`], without user name or password, on port
/// 443, to one of `hosts` (patterns as `[links] hosts` lists them).
///
/// # Errors
/// `invalid-argument` for a URL that is not such a link, `permission-denied`
/// for a host `hosts` does not list.
pub fn check(url: &str, hosts: &[String]) -> Result<()> {
    if url.len() > MOST_BYTES {
        return Err(Error::invalid(format!(
            "a link has at most {MOST_BYTES} bytes"
        )));
    }
    if url.chars().any(char::is_control) || url.contains(' ') {
        return Err(Error::invalid("a link has no spaces or control characters"));
    }
    let uri: ureq::http::Uri = url
        .parse()
        .map_err(|e| Error::invalid(format!("{url:?} is not a URL: {e}")))?;
    if uri.scheme_str() != Some("https") {
        return Err(Error::invalid(format!("{url} is not an https URL")));
    }
    let Some(authority) = uri.authority() else {
        return Err(Error::invalid(format!("{url} has no host")));
    };
    if authority.as_str().contains('@') {
        return Err(Error::invalid(format!(
            "{url} carries a user name or password"
        )));
    }
    if authority.port_u16().is_some_and(|port| port != 443) {
        return Err(Error::invalid(format!("{url} is not on port 443")));
    }
    if !crate::http::listed(hosts, authority.host()) {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "{} is not a host the plugin's manifest lists in [links] hosts",
                authority.host()
            ),
        ));
    }
    Ok(())
}

/// The links plugins opened this frame, and when each plugin last opened
/// one.
#[derive(Default)]
pub struct Links {
    opened: Mutex<(Vec<String>, BTreeMap<String, Instant>)>,
}

impl Links {
    /// Keeps `url`, which [`check`] passed, for the end of the frame.
    ///
    /// # Errors
    /// `limit-exceeded` when the plugin opened a link less than
    /// [`SPACING`] before `now`.
    pub fn open(&self, plugin: &str, url: &str, now: Instant) -> Result<()> {
        let mut opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
        if opened
            .1
            .get(plugin)
            .is_some_and(|last| now.saturating_duration_since(*last) < SPACING)
        {
            return Err(Error::new(
                Kind::LimitExceeded,
                "a plugin opens at most one link a second",
            ));
        }
        opened.1.insert(plugin.to_owned(), now);
        opened.0.push(url.to_owned());
        Ok(())
    }

    /// The links opened since the last call, in order.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.opened.lock().unwrap_or_else(PoisonError::into_inner).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_links_to_listed_hosts_are_opened() {
        let hosts = vec!["www.example.com".to_owned(), "*.wiki.example".to_owned()];
        assert!(check("https://www.example.com/item/4", &hosts).is_ok());
        assert!(check("https://www.example.com:443/item?id=4#top", &hosts).is_ok());
        assert!(check("https://en.wiki.example/Mythrite_Ingot", &hosts).is_ok());
        for (url, kind) in [
            ("http://www.example.com/", Kind::InvalidArgument),
            ("file:///C:/Windows/notepad.exe", Kind::InvalidArgument),
            ("ms-settings:privacy", Kind::InvalidArgument),
            ("https://www.example.com:8443/", Kind::InvalidArgument),
            ("https://user:pass@www.example.com/", Kind::InvalidArgument),
            ("https://www.example.com/a b", Kind::InvalidArgument),
            ("https://www.example.com/\n", Kind::InvalidArgument),
            ("/item/4", Kind::InvalidArgument),
            ("https://example.com/", Kind::PermissionDenied),
            ("https://wiki.example/", Kind::PermissionDenied),
            ("https://www.example.com.evil.test/", Kind::PermissionDenied),
        ] {
            assert_eq!(check(url, &hosts).unwrap_err().kind, kind, "{url}");
        }
        let long = format!("https://www.example.com/{}", "a".repeat(MOST_BYTES));
        assert_eq!(
            check(&long, &hosts).unwrap_err().kind,
            Kind::InvalidArgument
        );
    }

    #[test]
    fn a_plugin_opens_one_link_a_second() {
        let links = Links::default();
        let now = Instant::now();
        links.open("a", "https://www.example.com/1", now).unwrap();
        assert_eq!(
            links
                .open(
                    "a",
                    "https://www.example.com/2",
                    now + Duration::from_millis(500)
                )
                .unwrap_err()
                .kind,
            Kind::LimitExceeded
        );
        links.open("b", "https://www.example.com/3", now).unwrap();
        links
            .open("a", "https://www.example.com/4", now + SPACING)
            .unwrap();
        assert_eq!(
            links.take(),
            [
                "https://www.example.com/1",
                "https://www.example.com/3",
                "https://www.example.com/4"
            ]
        );
        assert!(links.take().is_empty());
    }
}
