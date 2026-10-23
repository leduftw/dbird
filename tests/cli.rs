use std::process::{Command, Output, Stdio};

fn dbird(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dbird"))
        .args(args)
        .stdin(Stdio::null())
        .env_remove("NO_COLOR")
        .env("TERM", "xterm-256color")
        .env("CLICOLOR_FORCE", "1")
        .output()
        .unwrap()
}

#[test]
fn a_bare_run_without_a_terminal_prints_help_and_succeeds() {
    let output = dbird(&[]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("Controls:"));
    assert!(help.ends_with("dbird needs an interactive terminal to play.\n"));
    assert!(!help.contains('\x1b'));
}

#[test]
fn explicit_game_options_still_require_a_terminal() {
    for args in [
        &["--mute"][..],
        &["--seed=5"],
        &["--no-color"],
        &["--online=PlayerOne"],
    ] {
        let output = dbird(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("terminal")
        );
    }
}

#[test]
fn help_accepts_no_color_in_every_position_and_pipes_stay_plain() {
    let root = dbird(&["--help"]);
    let update = dbird(&["update", "--help"]);
    for (expected, forms) in [
        (
            &root.stdout,
            vec![
                vec!["-h"],
                vec!["help"],
                vec!["--no-color", "help"],
                vec!["help", "--no-color"],
            ],
        ),
        (
            &update.stdout,
            vec![
                vec!["help", "update"],
                vec!["help", "update", "--no-color"],
                vec!["help", "--no-color", "update"],
                vec!["--no-color", "help", "update"],
                vec!["--no-color", "update", "-h"],
                vec!["update", "--no-color", "-h"],
            ],
        ),
    ] {
        assert!(!expected.contains(&0x1b));
        for args in forms {
            let output = dbird(&args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(&output.stdout, expected, "{args:?}");
        }
    }
}

#[test]
fn online_name_mistakes_are_reported_before_terminal_or_profile_access() {
    for name in [
        "",
        "ab",
        "bird name",
        "_bird",
        "bïrd",
        "way-too-long-for-dbird",
    ] {
        let output = dbird(&["--online", name]);
        assert_eq!(output.status.code(), Some(2), "{name:?}: {output:?}");
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("--online"));
        assert!(error.contains("--help"));
        assert!(!error.contains('\x1b'));
    }
}

#[test]
fn all_version_forms_print_the_name_and_version() {
    for args in [
        &["-V"][..],
        &["--version"],
        &["version"],
        &["--no-color", "version"],
        &["version", "--no-color"],
    ] {
        let output = dbird(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(
            output.stdout,
            format!("dbird {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
    }
}
