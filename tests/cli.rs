#![cfg(feature = "desktop")]

use std::process::Command;

#[test]
fn help_and_invalid_arguments_do_not_require_a_desktop() {
    for (arguments, expected_status) in [
        (vec!["--help"], 0),
        (vec!["-h"], 0),
        (vec![], 2),
        (vec!["--sample"], 2),
        (vec!["--unknown"], 2),
        (vec!["fixtures/sample.har"], 2),
        (vec!["/tmp/capture.har"], 2),
        (vec!["--sample", "extra.har"], 2),
        (vec!["--input"], 2),
        (vec!["--input", ""], 2),
        (vec!["--input", "--sample"], 2),
        (vec!["--input", "capture.har", "extra.har"], 2),
        (vec!["--input", "capture.har", "--sample"], 2),
        (vec!["--input", "one.har", "--input", "two.har"], 2),
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
        let message = String::from_utf8(message).unwrap();
        assert!(message.contains("Usage:"));
        assert!(message.contains("--input <har file>"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_launch_without_a_display_reports_an_actionable_error() {
    for arguments in [
        vec!["--input", "fixtures/sample.har"],
        vec!["--input", "captures/my capture ü.har"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_har-viewer"))
            .args(&arguments)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "arguments: {arguments:?}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("desktop")
        );
    }
}
