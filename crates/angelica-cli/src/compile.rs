//! Component compilation runs outside the game process.
use crate::Output;
use clap::Args;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Debug, Args)]
pub struct CompileArgs {
    /// WebAssembly component source.
    #[arg(long)]
    component: PathBuf,
    /// Destination compiler artifact for the current platform and runtime version.
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    reporting: Output,
}

pub fn run(args: &CompileArgs) -> ExitCode {
    match compile(args) {
        Ok(report) => {
            if args.reporting.json {
                println!("{report}");
            } else {
                println!(
                    "Compiled {} to {}",
                    args.component.display(),
                    args.output.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if args.reporting.json {
                println!(
                    "{}",
                    serde_json::json!({"error": {"code": "component-compile", "message": error}})
                );
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn compile(args: &CompileArgs) -> Result<serde_json::Value, String> {
    let mut source = Vec::new();
    std::fs::File::open(&args.component)
        .map_err(|e| e.to_string())?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() > 64 * 1024 * 1024 {
        return Err("component source exceeds 64 MiB".into());
    }
    let sandbox = runtime::sandbox::Sandbox::new()?;
    let component =
        wasmtime::component::Component::new(&sandbox.engine, &source).map_err(|e| e.to_string())?;
    let artifact = component.serialize().map_err(|e| e.to_string())?;
    if artifact.len() > 64 * 1024 * 1024 {
        return Err("compiler artifact exceeds 64 MiB".into());
    }
    let temporary = args
        .output
        .with_extension(format!("cwasm-{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let published = (|| {
        file.write_all(&artifact).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temporary, &args.output).map_err(|e| e.to_string())
    })();
    if let Err(error) = published {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(
        serde_json::json!({"component": args.component, "artifact": args.output, "component_sha256": runtime::package::hex(&Sha256::digest(&source)), "artifact_sha256": runtime::package::hex(&Sha256::digest(&artifact)), "target_arch": std::env::consts::ARCH, "target_os": std::env::consts::OS}),
    )
}
