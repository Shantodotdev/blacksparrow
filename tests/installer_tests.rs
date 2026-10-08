//! # Standalone Installer Pipeline Integration Tests
//!
//! Validates `install.sh` CLI argument handling, fail-closed validation,
//! and protection against arbitrary command injection.

use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// `bash` for running `install.sh`. On Windows, `Command::new("bash")` resolves to the WSL
/// launcher in System32 before Git Bash on `PATH`, so Git for Windows' bash is used directly.
fn bash() -> Command {
    #[cfg(windows)]
    for candidate in [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
    ] {
        if std::path::Path::new(candidate).exists() {
            return Command::new(candidate);
        }
    }
    Command::new("bash")
}

#[test]
fn test_installer_help_flag() {
    let output = bash()
        .arg("install.sh")
        .arg("--help")
        .output()
        .expect("Execute install.sh --help");

    assert!(
        output.status.success(),
        "install.sh --help should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: install.sh [OPTIONS]"));
    assert!(stdout.contains("--dir <path>"));
    assert!(stdout.contains("--no-modify-path"));
    assert!(stdout.contains("--no-verify"));
}

#[test]
fn test_installer_rejects_missing_dir_argument() {
    let output = bash()
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
    let output = bash()
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
    let output = bash()
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
    #[cfg(unix)]
    {
        let root = std::env::temp_dir().join(format!(
            "blacksparrow_installer_{}_{}",
            std::process::id(),
            fastrand_like_nanos()
        ));
        let fake_bin = root.join("fake-bin");
        let home = root.join("home");
        let canary_file = root.join("profile_canary");
        std::fs::create_dir_all(&fake_bin).expect("create fake command directory");
        std::fs::create_dir_all(&home).expect("create fake home directory");

        write_executable(
            &fake_bin.join("curl"),
            "#!/usr/bin/env bash\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"-o\" ]; then : > \"$2\"; exit 0; fi; shift; done\n",
        );
        write_executable(
            &fake_bin.join("tar"),
            concat!(
                "#!/usr/bin/env bash\n",
                "while [ \"$1\" != \"-C\" ]; do shift; done\n",
                "shift\n",
                "printf '%s\\n' '#!/usr/bin/env bash' 'if [ \"${1:-}\" = \"--version\" ]; then printf %s \"blacksparrow test\"; fi' > \"$1/blacksparrow\"\n",
                "chmod 755 \"$1/blacksparrow\"\n"
            ),
        );

        let install_dir = root.join("install");
        let payload = format!(
            "{}\"; /usr/bin/touch {}; #",
            install_dir.display(),
            canary_file.display()
        );
        let path = format!("{}:/usr/bin:/bin", fake_bin.display());

        let output = Command::new("bash")
            .arg("install.sh")
            .arg("--dir")
            .arg(&payload)
            .arg("--no-verify")
            .env("HOME", &home)
            .env("SHELL", "/bin/bash")
            .env("PATH", &path)
            .output()
            .expect("execute installer with a shell metacharacter in --dir");

        assert!(
            output.status.success(),
            "installer should succeed with a valid path containing shell metacharacters: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let profile = home.join(".bashrc");
        assert!(profile.exists(), "installer should update the bash profile");

        let source = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "source \"$HOME/.bashrc\""])
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("source generated bash profile");

        assert!(
            source.status.success(),
            "generated bash profile should remain valid shell syntax: {}",
            String::from_utf8_lossy(&source.stderr)
        );
        assert!(
            !canary_file.exists(),
            "shell metacharacters in --dir must not execute when the generated profile is sourced"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, contents: &str) {
    std::fs::write(path, contents).expect("write fake executable");
    let mut permissions = std::fs::metadata(path)
        .expect("read fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("make fake executable runnable");
}

fn fastrand_like_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
