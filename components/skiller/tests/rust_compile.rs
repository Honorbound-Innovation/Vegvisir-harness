use std::process::Command;

#[test]
fn cli_help_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_skiller"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Compile technical sources"));
}
