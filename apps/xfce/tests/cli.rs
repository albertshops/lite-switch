use std::process::Command;

#[test]
fn help_and_version_work_without_a_desktop() {
    for argument in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lite-switch-xfce"))
            .arg(argument)
            .env_remove("DISPLAY")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
    }
}

#[test]
fn missing_display_and_injectable_ids_fail_with_actionable_errors() {
    let output = Command::new(env!("CARGO_BIN_EXE_lite-switch-xfce"))
        .arg("doctor")
        .env_remove("DISPLAY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("DISPLAY"));
    let output = Command::new(env!("CARGO_BIN_EXE_lite-switch-xfce"))
        .args(["focus", "42;kill"])
        .env_remove("DISPLAY")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("numeric"));
}
