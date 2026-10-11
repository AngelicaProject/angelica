//! The release's files inside the installer (`build.rs`).

/// What a release build of the installer carries.
pub struct Payload {
    pub version: &'static str,
    pub channel: &'static str,
    pub runtime_dll: &'static [u8],
    pub runtime_dll_sig: &'static [u8],
    pub plugin_installer_exe: &'static [u8],
    pub plugin_installer_exe_sig: &'static [u8],
    pub dinput8_dll: &'static [u8],
    pub dinput8_dll_sig: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/payload.rs"));

/// The release key the installer is built with.
#[must_use]
pub fn release_key() -> Option<[u8; 32]> {
    let text = option_env!("ANGELICA_RELEASE_PUBLIC_KEY")?;
    if text.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(key)
}

impl Payload {
    /// The files and their signatures by name.
    #[must_use]
    pub fn files(&self) -> [(&'static str, &'static [u8], &'static [u8]); 3] {
        [
            ("runtime.dll", self.runtime_dll, self.runtime_dll_sig),
            (
                "plugin-installer.exe",
                self.plugin_installer_exe,
                self.plugin_installer_exe_sig,
            ),
            ("dinput8.dll", self.dinput8_dll, self.dinput8_dll_sig),
        ]
    }

    /// Checks every file against its release signature before anything is
    /// written.
    ///
    /// # Errors
    /// A file whose signature does not verify with `key`.
    pub fn verify(&self, key: &[u8; 32]) -> Result<(), String> {
        let key = ed25519_dalek::VerifyingKey::from_bytes(key).map_err(|e| e.to_string())?;
        for (name, bytes, signature) in self.files() {
            let signature = ed25519_dalek::Signature::from_slice(signature)
                .map_err(|_| format!("{name}.sig is not a signature"))?;
            key.verify_strict(bytes, &signature)
                .map_err(|_| format!("{name} is not signed with the release key"))?;
        }
        Ok(())
    }
}
