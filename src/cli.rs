//! Minimal dependency-free command-line parsing.

use std::env;
use std::ffi::OsString;
use std::fmt;

/// One-line syntax suitable for parse-error output.
pub const USAGE: &str = "Usage: dbird [OPTIONS]";

/// Full command-line help.
pub const HELP_TEXT: &str = "\
dbird - a terminal recreation of the classic Flappy Bird game

Usage: dbird [OPTIONS]

Controls:
  Enter                Start / retry
  Space, Up, W, or K   Flap during flight
  P                    Pause / resume
  T                    Cycle System / Light / Dark theme
  Q, Esc, or Ctrl-C    Quit

Options:
      --ascii          Use ASCII-only graphics
      --no-color       Disable colored output
      --mute           Disable sound effects
      --seed <u64>     Use a deterministic random seed
      --reset-score    Reset the saved high score before starting
  -h, --help           Print help
  -V, --version        Print version
";

/// Options used for a normal game run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CliOptions {
    pub ascii: bool,
    pub no_color: bool,
    pub mute: bool,
    pub seed: Option<u64>,
    pub reset_score: bool,
}

/// The action selected by command-line parsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    Run(CliOptions),
    Help,
    Version,
}

/// A command-line parse failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliError {
    UnknownArgument(String),
    MissingSeedValue,
    InvalidSeed(String),
    DuplicateSeed,
    NonUnicodeArgument,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownArgument(argument) => {
                write!(formatter, "unknown argument `{argument}` (try `--help`)")
            }
            Self::MissingSeedValue => write!(formatter, "`--seed` requires a u64 value"),
            Self::InvalidSeed(value) => {
                write!(
                    formatter,
                    "invalid seed `{value}`: expected an unsigned 64-bit integer"
                )
            }
            Self::DuplicateSeed => write!(formatter, "`--seed` may only be specified once"),
            Self::NonUnicodeArgument => {
                write!(formatter, "command-line arguments must be valid UTF-8")
            }
        }
    }
}

impl std::error::Error for CliError {}

/// Parses options supplied to the current process.
pub fn parse_env() -> Result<CliCommand, CliError> {
    parse_args(env::args_os().skip(1))
}

/// Parses an iterator of command-line options (excluding the executable name).
pub fn parse_args<I, S>(args: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut options = CliOptions::default();
    let mut args = args.into_iter().map(Into::into);

    while let Some(raw_argument) = args.next() {
        let argument = raw_argument
            .into_string()
            .map_err(|_| CliError::NonUnicodeArgument)?;

        match argument.as_str() {
            "--ascii" => options.ascii = true,
            "--no-color" => options.no_color = true,
            "--mute" => options.mute = true,
            "--reset-score" => options.reset_score = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            "-V" | "--version" => return Ok(CliCommand::Version),
            "--seed" => {
                if options.seed.is_some() {
                    return Err(CliError::DuplicateSeed);
                }

                let raw_value = args.next().ok_or(CliError::MissingSeedValue)?;
                let value = raw_value
                    .into_string()
                    .map_err(|_| CliError::NonUnicodeArgument)?;
                options.seed = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| CliError::InvalidSeed(value))?,
                );
            }
            _ => return Err(CliError::UnknownArgument(argument)),
        }
    }

    Ok(CliCommand::Run(options))
}

/// Returns the version line printed for `--version`.
pub fn version_text() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_options(arguments: &[&str]) -> CliOptions {
        match parse_args(arguments.iter().copied()).expect("arguments should parse") {
            CliCommand::Run(options) => options,
            command => panic!("expected a run command, got {command:?}"),
        }
    }

    #[test]
    fn no_arguments_uses_run_defaults() {
        assert_eq!(run_options(&[]), CliOptions::default());
    }

    #[test]
    fn parses_all_run_options_in_any_order() {
        assert_eq!(
            run_options(&[
                "--no-color",
                "--seed",
                "18446744073709551615",
                "--ascii",
                "--reset-score"
            ]),
            CliOptions {
                ascii: true,
                no_color: true,
                mute: false,
                seed: Some(u64::MAX),
                reset_score: true,
            }
        );
    }

    #[test]
    fn zero_is_a_valid_seed() {
        assert_eq!(run_options(&["--seed", "0"]).seed, Some(0));
    }

    #[test]
    fn recognizes_both_help_spellings() {
        assert_eq!(parse_args(["-h"]), Ok(CliCommand::Help));
        assert_eq!(parse_args(["--help"]), Ok(CliCommand::Help));
    }

    #[test]
    fn recognizes_both_version_spellings() {
        assert_eq!(parse_args(["-V"]), Ok(CliCommand::Version));
        assert_eq!(parse_args(["--version"]), Ok(CliCommand::Version));
    }

    #[test]
    fn rejects_unknown_options_and_positionals() {
        assert_eq!(
            parse_args(["--colour"]),
            Err(CliError::UnknownArgument("--colour".into()))
        );
        assert_eq!(
            parse_args(["fly-now"]),
            Err(CliError::UnknownArgument("fly-now".into()))
        );
    }

    #[test]
    fn rejects_a_missing_seed_value() {
        assert_eq!(parse_args(["--seed"]), Err(CliError::MissingSeedValue));
    }

    #[test]
    fn rejects_bad_or_out_of_range_seeds() {
        for seed in ["bird", "-1", "1.5", "18446744073709551616"] {
            assert_eq!(
                parse_args(["--seed", seed]),
                Err(CliError::InvalidSeed(seed.into()))
            );
        }
    }

    #[test]
    fn rejects_duplicate_seed_options() {
        assert_eq!(
            parse_args(["--seed", "1", "--seed", "2"]),
            Err(CliError::DuplicateSeed)
        );
    }

    #[test]
    fn repeated_boolean_flags_are_idempotent() {
        assert_eq!(
            run_options(&[
                "--ascii",
                "--ascii",
                "--no-color",
                "--no-color",
                "--mute",
                "--mute",
            ]),
            CliOptions {
                ascii: true,
                no_color: true,
                mute: true,
                seed: None,
                reset_score: false,
            }
        );
    }

    #[test]
    fn help_and_usage_document_every_option() {
        assert!(HELP_TEXT.contains(USAGE));
        for option in [
            "--ascii",
            "--no-color",
            "--seed <u64>",
            "--reset-score",
            "--help",
            "--version",
        ] {
            assert!(HELP_TEXT.contains(option), "help omitted {option}");
        }
        for control in ["Enter", "Flap during flight", "Cycle System / Light / Dark"] {
            assert!(HELP_TEXT.contains(control), "help omitted {control}");
        }
    }

    #[test]
    fn errors_are_clear_and_actionable() {
        assert_eq!(
            CliError::MissingSeedValue.to_string(),
            "`--seed` requires a u64 value"
        );
        assert!(
            CliError::UnknownArgument("--wat".into())
                .to_string()
                .contains("--help")
        );
    }

    #[test]
    fn version_line_uses_package_metadata() {
        assert_eq!(version_text(), "dbird 1.0.0");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_unicode_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let argument = OsString::from_vec(vec![0xff]);
        assert_eq!(parse_args([argument]), Err(CliError::NonUnicodeArgument));
    }
}
