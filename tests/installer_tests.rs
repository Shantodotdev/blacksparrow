//! # Standalone Installer Pipeline Integration Tests
//!
//! Validates `install.sh` CLI argument handling, fail-closed validation,
//! and protection against arbitrary command injection.

use std::process::Command;

#[test]
fn test_installer_help_flag() {
    let output = Command::new("bash")
        .arg("install.sh")
        .arg("--help")
        .output()
        .expect("Execute install.sh --help");

    assert!(output.status.success(), "install.sh --help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: install.sh [OPTIONS]"));
    assert!(stdout.contains("--dir <path>"));
    assert!(stdout.contains("--no-modify-path"));
    assert!(stdout.contains("--no-verify"));
}

#[test]
fn test_installer_rejects_missing_dir_argument() {
    let output = Command::new("bash")
        .arg("install.sh")
        .arg("--dir")
        .output()
        .expect("Execute install.sh --dir");

    assert!(
        !output.status.success(),
        "install.sh --dir without arg must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--dir requires a directory path argument"),
        "Expected error message for missing --dir argument, got: {stderr}"
    );
}

#[test]
fn test_installer_rejects_missing_version_argument() {
    let output = Command::new("bash")
        .arg("install.sh")
        .arg("--version")
        .output()
        .expect("Execute install.sh --version");

    assert!(
        !output.status.success(),
        "install.sh --version without arg must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--version requires a version tag argument"),
        "Expected error message for missing --version argument, got: {stderr}"
    );
}

#[test]
fn test_installer_rejects_unknown_argument() {
    let output = Command::new("bash")
        .arg("install.sh")
        .arg("--unknown-flag-12345")
        .output()
        .expect("Execute install.sh with unknown flag");

    assert!(!output.status.success(), "Unknown flag must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown argument: --unknown-flag-12345"),
        "Expected unknown argument error, got: {stderr}"
    );
}

#[test]
fn test_installer_prevents_command_injection_via_dir() {
    let canary_file = std::env::temp_dir().join(format!(
        "blacksparrow_canary_{}_{}.txt",
        std::process::id(),
        fastrand_like_nanos()
    ));

    if canary_file.exists() {
        let _ = std::fs::remove_file(&canary_file);
    }

    let payload = format!("$(touch {})", canary_file.display());

    // Execute with help so it doesn't try downloading
    let _ = Command::new("bash")
        .arg("install.sh")
        .arg("--dir")
        .arg(&payload)
        .arg("--help")
        .output();

    assert!(
        !canary_file.exists(),
        "SECURITY VULNERABILITY: Arbitrary command in --dir was executed via eval!"
    );
}

fn fastrand_like_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
