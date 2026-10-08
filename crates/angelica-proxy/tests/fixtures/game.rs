#[link(name = "dinput8.dll.lib", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "system" { fn GetdfDIJoystick() -> *const std::ffi::c_void; }

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *const std::ffi::c_void;
}

fn check_exports() {
    let name: Vec<u16> = "dinput8.dll\0".encode_utf16().collect();
    // SAFETY: NUL-terminated name of the statically imported proxy.
    let module = unsafe { GetModuleHandleW(name.as_ptr()) };
    assert!(!module.is_null());
    for (index, name) in [c"DirectInput8Create", c"DllCanUnloadNow", c"DllGetClassObject", c"DllRegisterServer", c"DllUnregisterServer", c"GetdfDIJoystick"].iter().enumerate() {
        // SAFETY: loaded module and NUL-terminated export name.
        let named = unsafe { GetProcAddress(module, name.as_ptr().cast()) };
        // SAFETY: Windows represents an ordinal by a pointer with a low 16-bit value.
        let ordinal = unsafe { GetProcAddress(module, (index + 1) as *const u8) };
        assert!(!named.is_null());
        assert_eq!(named, ordinal, "incorrect ordinal for {name:?}");
    }
}
fn main() {
    check_exports();
    // SAFETY: this system export takes no arguments.
    assert!(!unsafe { GetdfDIJoystick() }.is_null());
    use std::io::Write;
    let mut trace = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var_os("ANGELICA_TEST_TRACE").unwrap()).unwrap();
    writeln!(trace, "game").unwrap();
}
