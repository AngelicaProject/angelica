use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=ANGELICA_RELEASE_PUBLIC_KEY");
    println!("cargo:rerun-if-changed=../../wit/host.wit");
    let key = env::var("ANGELICA_RELEASE_PUBLIC_KEY").ok().map(|text| {
        assert!(
            text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()),
            "release key must contain 64 hexadecimal digits"
        );
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    });
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("release_key.rs"),
        format!(
            "pub const RELEASE_KEY: Option<[u8; 32]> = {};",
            key.map_or_else(|| "None".into(), |key| format!("Some({key:?})"))
        ),
    )
    .unwrap();
}
