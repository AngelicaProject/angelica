fn trace(message: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var_os("ANGELICA_TEST_TRACE").unwrap()).unwrap();
    writeln!(file, "{message}").unwrap();
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(_: *mut std::ffi::c_void, reason: u32, _: *mut std::ffi::c_void) -> i32 {
    std::panic::catch_unwind(|| { if reason == 1 { trace("load"); } 1 }).unwrap_or(0)
}

#[unsafe(no_mangle)]
#[cfg(not(no_start))]
extern "system" fn angelica_start_v1() -> i32 {
    std::panic::catch_unwind(|| {
        trace("host");
        std::env::var("ANGELICA_TEST_STATUS").unwrap_or_default().parse().unwrap_or(0)
    }).unwrap_or(1)
}
