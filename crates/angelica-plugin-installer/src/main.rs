//! `plugin-installer.exe`: the host runs it outside the game to install a
//! plugin from its author's link, update the plugins installed from links,
//! and compile installed plugins again after an update of the runtime. It
//! ships next to `runtime.dll`, signed like it, and is built with the same
//! engine. It writes one JSON document to standard output.
//!
//! ```text
//! plugin-installer install <https link> --installation <folder>
//! plugin-installer update --installation <folder>
//! plugin-installer repair <plugin id>... --installation <folder>
//! ```
use runtime::installer::{self, Installation};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    // Downloads go through the proxy Windows uses, as the host's do.
    #[cfg(windows)]
    runtime::interop::proxy::use_system_proxy(|_| {});
    let result = run(std::env::args().skip(1).collect());
    let (document, code) = match result {
        Ok(value) => (value, ExitCode::SUCCESS),
        Err(message) => (
            serde_json::json!({"error": {"message": message}}),
            ExitCode::FAILURE,
        ),
    };
    println!("{document}");
    code
}

fn run(mut args: Vec<String>) -> Result<serde_json::Value, String> {
    let at = args
        .iter()
        .position(|arg| arg == "--installation")
        .ok_or("--installation <folder> is missing")?;
    let folder = args
        .get(at + 1)
        .cloned()
        .ok_or("--installation <folder> is missing")?;
    args.drain(at..=at + 1);
    let installation = Installation::open(&PathBuf::from(folder))?;
    let (command, rest) = args.split_first().ok_or("no command")?;
    match (command.as_str(), rest) {
        ("install", [url]) => {
            if !url.starts_with("https://") {
                return Err(format!("{url}: plugins install only from https links"));
            }
            let file = installer::fetch(url)?;
            let installed = installer::install(&installation, file, Some(url))?;
            Ok(serde_json::json!({ "installed": installed }))
        }
        ("update", []) => {
            let mut updates = serde_json::Map::new();
            for (id, outcome) in installer::update(&installation, installer::fetch) {
                updates.insert(id, serde_json::to_value(outcome).map_err(|e| e.to_string())?);
            }
            Ok(serde_json::json!({ "updates": updates }))
        }
        ("repair", ids) if !ids.is_empty() => {
            let mut repaired = serde_json::Map::new();
            for id in ids {
                let outcome = match installer::repair(&installation, id) {
                    Ok(installed) => serde_json::json!({"version": installed.version}),
                    Err(message) => serde_json::json!({"error": message}),
                };
                repaired.insert(id.clone(), outcome);
            }
            Ok(serde_json::json!({ "repaired": repaired }))
        }
        _ => Err(
            "usage: install <https link> | update | repair <plugin id>..., with --installation <folder>"
                .into(),
        ),
    }
}
