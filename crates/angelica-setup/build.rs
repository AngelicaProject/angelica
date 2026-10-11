//! Embeds the release's files when `ANGELICA_SETUP_PAYLOAD` names a folder
//! with `release.json` (`{"version", "channel"}`), `runtime.dll`,
//! `plugin-installer.exe`, `dinput8.dll`, and their `.sig` files. Without it
//! the installer is a development build that installs nothing.
use std::{fmt::Write as _, path::PathBuf};

const FILES: [&str; 6] = [
    "runtime.dll",
    "runtime.dll.sig",
    "plugin-installer.exe",
    "plugin-installer.exe.sig",
    "dinput8.dll",
    "dinput8.dll.sig",
];

fn main() {
    println!("cargo::rerun-if-env-changed=ANGELICA_SETUP_PAYLOAD");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let mut code = String::new();
    match std::env::var("ANGELICA_SETUP_PAYLOAD") {
        Ok(folder) => {
            let folder = PathBuf::from(folder);
            println!("cargo::rerun-if-changed={}", folder.display());
            let release = std::fs::read_to_string(folder.join("release.json"))
                .expect("the payload's release.json");
            let release: serde_json::Value =
                serde_json::from_str(&release).expect("release.json is JSON");
            let field = |name: &str| {
                release[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("release.json has no {name}"))
                    .to_owned()
            };
            let _ = writeln!(code, "pub const PAYLOAD: Option<Payload> = Some(Payload {{");
            let _ = writeln!(code, "    version: {:?},", field("version"));
            let _ = writeln!(code, "    channel: {:?},", field("channel"));
            for name in FILES {
                let path = folder.join(name);
                assert!(path.exists(), "the payload has no {name}");
                let _ = writeln!(
                    code,
                    "    {}: include_bytes!({:?}),",
                    name.replace(['.', '-'], "_"),
                    path.display().to_string()
                );
            }
            let _ = writeln!(code, "}});");
        }
        Err(_) => {
            let _ = writeln!(code, "pub const PAYLOAD: Option<Payload> = None;");
        }
    }
    std::fs::write(out.join("payload.rs"), code).expect("payload.rs");
}
