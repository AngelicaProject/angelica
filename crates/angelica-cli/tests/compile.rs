use std::process::Command;

#[test]
fn compiles_component_and_preserves_destination_on_invalid_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.wasm");
    let artifact = directory.path().join("plugin.cwasm");
    std::fs::write(&source, b"\0asm\x0d\0\x01\0").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_angelica"))
            .args(["plugin", "compile", "--component"])
            .arg(&source)
            .arg("--output")
            .arg(&artifact)
            .arg("--json")
            .output()
            .unwrap()
    };
    let output = run();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["component_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(report["artifact_sha256"].as_str().unwrap().len(), 64);
    let compiled = std::fs::read(&artifact).unwrap();
    assert!(!compiled.is_empty());
    std::fs::write(&source, b"invalid component").unwrap();
    let failed = run();
    assert!(!failed.status.success());
    assert_eq!(std::fs::read(&artifact).unwrap(), compiled);
}
