#![cfg(feature = "desktop")]

use std::process::Command;

#[test]
fn help_and_invalid_arguments_do_not_require_a_desktop() {
    for (arguments, expected_status) in [
        (vec!["--help"], 0),
        (vec!["-h"], 0),
        (vec!["--unknown"], 2),
        (vec!["--sample", "extra.har"], 2),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_har-viewer"))
            .args(arguments)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(expected_status));
        let message = if expected_status == 0 {
            output.stdout
        } else {
            output.stderr
        };
        assert!(String::from_utf8(message).unwrap().contains("Usage:"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_launch_without_a_display_reports_an_actionable_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_har-viewer"))
        .arg("--sample")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("desktop")
    );
}
