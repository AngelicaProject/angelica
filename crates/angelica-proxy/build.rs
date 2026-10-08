use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=dinput8.def");
    println!("cargo:rerun-if-env-changed=ANGELICA_RELEASE_PUBLIC_KEY");
    let key = env::var("ANGELICA_RELEASE_PUBLIC_KEY").ok().map(|value| {
        assert!(
            value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "ANGELICA_RELEASE_PUBLIC_KEY must contain 64 hexadecimal digits"
        );
        (0..32)
            .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).unwrap())
            .collect::<Vec<_>>()
    });
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("release_key.rs"),
        format!(
            "pub const RELEASE_KEY: Option<[u8; 32]> = {};",
            key.map_or_else(|| "None".to_owned(), |bytes| format!("Some({bytes:?})"))
        ),
    )
    .unwrap();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        assert_eq!(env::var("CARGO_CFG_TARGET_ARCH").as_deref(), Ok("x86_64"));
        assert_eq!(env::var("CARGO_CFG_TARGET_ENV").as_deref(), Ok("msvc"));
        let def = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("dinput8.def");
        println!("cargo:rustc-link-arg-cdylib=/DEF:{}", def.display());
    }
}
