//! Command-line parsing with clap, and help in the playbook's colors.
//!
//! clap parses commands and game options, including `-h`, `--help`,
//! `help [<command>]` and `-V`/`--version`. The global `--no-color` flag is
//! read first so it works anywhere, even alongside help. The help is drawn
//! from clap's definitions: the playbook copies Python 3.14 argparse's
//! palette, which colors short and long options differently.

use std::env;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::builder::StyledStr;
use clap::builder::styling::{AnsiColor, Style, Styles};
use clap::error::ErrorKind;
use clap::{Arg, Command, CommandFactory, FromArgMatches, Parser, Subcommand};

/// The repo's GitHub description: dbird's help and its repo describe it alike.
pub const DESCRIPTION: &str =
    "a fully playable terminal recreation of the classic Flappy Bird game";

/// The game's controls, listed after the options in dbird's help.
const CONTROLS: [(&str, &str); 6] = [
    ("Enter", "Start / retry"),
    ("Space, Up, W, K, or Click", "Flap during flight"),
    ("P", "Pause / resume"),
    ("T", "Cycle System / Light / Dark theme"),
    ("L", "Show the global leaderboard (online only)"),
    ("Q, Esc, or Ctrl-C", "Quit"),
];

// Python 3.14 argparse's palette. Inside the usage line, values lose the bold.
const HEADING: Style = AnsiColor::Blue.on_default().bold();
const PROGRAM: Style = AnsiColor::Magenta.on_default().bold();
const COMMAND: Style = AnsiColor::Green.on_default().bold();
const SHORT: Style = AnsiColor::Green.on_default().bold();
const LONG: Style = AnsiColor::Cyan.on_default().bold();
const VALUE: Style = AnsiColor::Yellow.on_default().bold();
const USAGE_VALUE: Style = AnsiColor::Yellow.on_default();

/// Options used for a normal game run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CliOptions {
    pub ascii: bool,
    pub no_color: bool,
    pub mute: bool,
    pub seed: Option<u64>,
    pub online_username: Option<String>,
    pub reset_score: bool,
}

/// What the command line asks dbird to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    Run(CliOptions),
    Update,
    Version,
}

#[derive(Debug, Parser)]
#[command(
    name = "dbird",
    version,
    about = DESCRIPTION,
    args_conflicts_with_subcommands = true,
    styles = clap_styles(),
)]
struct Arguments {
    /// Use ASCII-only graphics
    #[arg(long, overrides_with = "ascii")]
    ascii: bool,
    /// Disable colored output
    #[arg(long, global = true, overrides_with = "no_color")]
    no_color: bool,
    /// Disable sound effects
    #[arg(long, overrides_with = "mute")]
    mute: bool,
    /// Play the same deterministic course every round
    #[arg(long, value_name = "NUMBER")]
    seed: Option<u64>,
    /// Opt in to the global leaderboard as this username
    #[arg(
        long,
        value_name = "NAME",
        conflicts_with = "reset_score",
        value_parser = crate::online::parse_username
    )]
    online: Option<String>,
    /// Reset the saved offline high score before starting
    #[arg(long, overrides_with = "reset_score")]
    reset_score: bool,
    #[command(subcommand)]
    command: Option<Subcommands>,
}

#[derive(Debug, Subcommand)]
enum Subcommands {
    /// Update dbird to the latest release
    #[command(alias = "upgrade")]
    Update,
    /// Print the version
    Version,
}

/// clap's own styles, which reach the usage line and its error messages.
fn clap_styles() -> Styles {
    Styles::styled()
        .header(HEADING)
        .usage(HEADING)
        .literal(PROGRAM)
        .placeholder(USAGE_VALUE)
}

/// dbird's command line, with every command's help drawn by [`render_help`].
pub fn command() -> Command {
    let mut command = Arguments::command();
    // Building adds clap's `-h`/`--help`, `-V`/`--version` and `help` command,
    // so the help drawn below lists them too.
    command.build();
    let subcommands: Vec<String> = command
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_owned())
        .collect();
    let help = render_help(&command, true);
    let mut command = command.help_template("{before-help}").before_help(help);
    for name in subcommands {
        command = command.mut_subcommand(name, |subcommand| {
            let help = render_help(&subcommand, false);
            subcommand.help_template("{before-help}").before_help(help)
        });
    }
    command
}

/// dbird's help, as `dbird --help` prints it.
pub fn help() -> StyledStr {
    command().render_help()
}

/// Parses options supplied to the current process.
pub fn parse_env() -> Result<CliCommand, clap::Error> {
    parse_args(env::args_os().skip(1))
}

/// Parses an iterator of command-line options (excluding the executable name).
pub fn parse_args<I, S>(args: I) -> Result<CliCommand, clap::Error>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let no_color = no_color_requested(&args);
    // Like playbook, take this global presentation flag out before parsing:
    // clap conflicts a root flag with subcommands, and its built-in `help`
    // treats everything after it as a command name. Preserve data after `--`.
    let mut options = true;
    let arguments =
        std::iter::once(OsString::from("dbird")).chain(args.into_iter().filter(|argument| {
            if argument == "--" {
                options = false;
            }
            !(options && argument == "--no-color")
        }));
    let mut matches = command().try_get_matches_from(arguments)?;
    let arguments = Arguments::from_arg_matches_mut(&mut matches)?;
    Ok(match arguments.command {
        Some(Subcommands::Update) => CliCommand::Update,
        Some(Subcommands::Version) => CliCommand::Version,
        None => CliCommand::Run(CliOptions {
            ascii: arguments.ascii,
            no_color: arguments.no_color || no_color,
            mute: arguments.mute,
            seed: arguments.seed,
            online_username: arguments.online,
            reset_score: arguments.reset_score,
        }),
    })
}

/// Whether `--no-color` appears anywhere, read before clap so it also governs
/// the help clap prints the moment it reaches `-h`.
pub fn no_color_requested<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> bool {
    args.iter()
        .any(|argument| argument.as_ref() == "--no-color")
}

/// Whether the `NO_COLOR` variable asks for plain output (set and not empty).
pub fn no_color_variable() -> bool {
    env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

/// Colors only reach a terminal, and `--no-color`, `NO_COLOR` or `TERM=dumb`
/// turn them off.
pub fn colors_allowed(terminal: bool, no_color: bool) -> bool {
    let allowed = terminal
        && !no_color
        && !no_color_variable()
        && env::var_os("TERM").is_none_or(|term| term != "dumb");
    // Our custom help writes ANSI directly, so enable Windows console support
    // before writing it, just as crossterm does for the game.
    #[cfg(windows)]
    let allowed = allowed && crossterm::ansi_support::supports_ansi();
    allowed
}

/// Prints what clap stopped at: help or the version to stdout with exit 0, or
/// a mistake to stderr with exit 2.
pub fn report(error: &clap::Error, no_color: bool) -> ExitCode {
    let text = error.render();
    let shown = if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        write_styled(
            &mut io::stdout(),
            &text,
            io::stdout().is_terminal(),
            no_color,
        )
    } else {
        write_styled(
            &mut io::stderr(),
            &text,
            io::stderr().is_terminal(),
            no_color,
        )
    };
    match shown {
        Ok(()) => ExitCode::from(error.exit_code().clamp(0, 255) as u8),
        Err(_) => ExitCode::FAILURE,
    }
}

/// Writes styled text, keeping its colors only where [`colors_allowed`] says.
pub fn write_styled(
    output: &mut impl Write,
    text: &StyledStr,
    terminal: bool,
    no_color: bool,
) -> io::Result<()> {
    let written = if colors_allowed(terminal, no_color) {
        write!(output, "{}", text.ansi())
    } else {
        write!(output, "{text}")
    };
    match written.and_then(|()| output.flush()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

/// Returns the version line printed for `--version`.
pub fn version_text() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// Draws a command's help in clap's layout and argparse's colors.
fn render_help(command: &Command, controls: bool) -> StyledStr {
    let mut help = StyledStr::new();
    if let Some(about) = command.get_about() {
        let _ = write!(help, "{about}\n\n");
    }
    let _ = write!(help, "{}", command.clone().render_usage().ansi());

    let commands: Vec<Row> = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .map(|subcommand| {
            let name = subcommand.get_name();
            let about = subcommand
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default();
            Row::new(format!("{COMMAND}{name}{COMMAND:#}"), name.len(), about)
        })
        .collect();
    write_section(&mut help, "Commands", &commands);

    let visible = |arg: &&Arg| !arg.is_hide_set();
    let arguments: Vec<Row> = command
        .get_positionals()
        .filter(visible)
        .map(|arg| {
            let name = value_names(arg).join(" ");
            let (open, close) = if arg.is_required_set() {
                ("<", ">")
            } else {
                ("[", "]")
            };
            let repeated = if arg
                .get_num_args()
                .is_some_and(|count| count.max_values() > 1)
            {
                "..."
            } else {
                ""
            };
            let width = name.len() + 2 + repeated.len();
            let spec = format!("{COMMAND}{open}{name}{close}{repeated}{COMMAND:#}");
            Row::new(spec, width, help_text(arg))
        })
        .collect();
    write_section(&mut help, "Arguments", &arguments);

    // In the order clap keeps them: dbird's own options, then help and version.
    let options: Vec<Row> = command
        .get_arguments()
        .filter(|arg| !arg.is_positional())
        .filter(visible)
        .map(option_row)
        .collect();
    write_section(&mut help, "Options", &options);

    if controls {
        let rows: Vec<Row> = CONTROLS
            .iter()
            .map(|(keys, action)| Row::new((*keys).to_owned(), keys.len(), (*action).to_owned()))
            .collect();
        write_section(&mut help, "Controls", &rows);
    }
    help
}

/// One line of a help section: a styled name, its width on screen, and its help.
struct Row {
    spec: String,
    width: usize,
    text: String,
}

impl Row {
    fn new(spec: String, width: usize, text: String) -> Self {
        Self { spec, width, text }
    }
}

fn option_row(arg: &Arg) -> Row {
    let mut spec = String::new();
    let mut width = 0;
    match arg.get_short() {
        Some(short) => {
            let _ = write!(spec, "{SHORT}-{short}{SHORT:#}");
            width += 2;
            if arg.get_long().is_some() {
                spec.push_str(", ");
                width += 2;
            }
        }
        None => {
            spec.push_str("    ");
            width += 4;
        }
    }
    if let Some(long) = arg.get_long() {
        let _ = write!(spec, "{LONG}--{long}{LONG:#}");
        width += 2 + long.len();
    }
    if arg.get_action().takes_values() {
        for name in value_names(arg) {
            let _ = write!(spec, " {VALUE}<{name}>{VALUE:#}");
            width += 3 + name.len();
        }
    }
    Row::new(spec, width, help_text(arg))
}

fn value_names(arg: &Arg) -> Vec<String> {
    match arg.get_value_names() {
        Some(names) => names.iter().map(ToString::to_string).collect(),
        None => vec![arg.get_id().as_str().to_uppercase()],
    }
}

fn help_text(arg: &Arg) -> String {
    arg.get_help().map(ToString::to_string).unwrap_or_default()
}

fn write_section(help: &mut StyledStr, heading: &str, rows: &[Row]) {
    if rows.is_empty() {
        return;
    }
    let width = rows.iter().map(|row| row.width).max().unwrap_or(0);
    let _ = write!(help, "\n\n{HEADING}{heading}:{HEADING:#}");
    for row in rows {
        let padding = " ".repeat(width - row.width);
        let _ = write!(help, "\n  {}{padding}  {}", row.spec, row.text);
    }
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

    fn stop(arguments: &[&str]) -> clap::Error {
        parse_args(arguments.iter().copied()).expect_err("clap should stop before running")
    }

    /// What the user sees for this command line when output isn't a terminal.
    fn shown(arguments: &[&str]) -> (String, i32) {
        let error = stop(arguments);
        (error.render().to_string(), error.exit_code())
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
                "--online",
                "PlayerOne"
            ]),
            CliOptions {
                ascii: true,
                no_color: true,
                mute: false,
                seed: Some(u64::MAX),
                online_username: Some("PlayerOne".into()),
                reset_score: false,
            }
        );
    }

    #[test]
    fn values_can_follow_an_equals_sign() {
        let options = run_options(&["--seed=5", "--online=PlayerOne"]);
        assert_eq!(options.seed, Some(5));
        assert_eq!(options.online_username.as_deref(), Some("PlayerOne"));
    }

    #[test]
    fn zero_is_a_valid_seed() {
        assert_eq!(run_options(&["--seed", "0"]).seed, Some(0));
    }

    #[test]
    fn subcommands_and_the_upgrade_alias() {
        assert_eq!(parse_args(["update"]).unwrap(), CliCommand::Update);
        assert_eq!(parse_args(["upgrade"]).unwrap(), CliCommand::Update);
        assert_eq!(parse_args(["version"]).unwrap(), CliCommand::Version);
        assert_eq!(
            parse_args(["update", "--no-color"]).unwrap(),
            CliCommand::Update
        );
        for name in ["update", "upgrade", "version"] {
            assert_eq!(
                parse_args(["--no-color", name]).unwrap(),
                parse_args([name, "--no-color"]).unwrap(),
                "{name} accepts the global flag on either side"
            );
        }
    }

    #[test]
    fn help_flags_and_the_help_command_print_the_same_help() {
        let (expected, code) = shown(&["--help"]);
        assert_eq!(code, 0);
        assert_eq!(expected, help().to_string());
        for arguments in [
            &["-h"][..],
            &["help"],
            &["--no-color", "-h"],
            &["--no-color", "help"],
            &["help", "--no-color"],
        ] {
            assert_eq!(shown(arguments), (expected.clone(), 0), "{arguments:?}");
        }
    }

    #[test]
    fn help_of_one_command() {
        let (expected, code) = shown(&["update", "--help"]);
        assert_eq!(code, 0);
        assert!(expected.starts_with("Update dbird to the latest release\n\nUsage: dbird update"));
        assert_eq!(shown(&["help", "update"]), (expected.clone(), 0));
        for arguments in [
            &["update", "-h"][..],
            &["--no-color", "update", "-h"],
            &["update", "--no-color", "-h"],
            &["help", "update", "--no-color"],
            &["help", "--no-color", "update"],
            &["--no-color", "help", "update"],
        ] {
            assert_eq!(shown(arguments), (expected.clone(), 0), "{arguments:?}");
        }
    }

    #[test]
    fn help_describes_dbird_and_lists_everything_once() {
        let help = help().to_string();
        assert!(help.starts_with(&format!("{DESCRIPTION}\n\nUsage: dbird [OPTIONS]\n")));
        for line in [
            "  update   Update dbird to the latest release",
            "  version  Print the version",
            "      --seed <NUMBER>  Play the same deterministic course every round",
            "      --online <NAME>  Opt in to the global leaderboard as this username",
            "  -h, --help           Print help",
            "  -V, --version        Print version",
            "  Space, Up, W, K, or Click  Flap during flight",
        ] {
            assert_eq!(
                help.lines().filter(|shown| *shown == line).count(),
                1,
                "{line:?} in\n{help}"
            );
        }
        for option in ["--ascii", "--no-color", "--mute", "--reset-score"] {
            assert_eq!(help.matches(option).count(), 1, "{option} in\n{help}");
        }
        assert!(!help.contains("upgrade"), "the alias stays out of the help");
    }

    #[test]
    fn help_uses_argparse_colors() {
        let help = help().ansi().to_string();
        let heading = HEADING.render();
        let program = PROGRAM.render();
        for piece in [
            format!("{heading}Usage:"),
            format!("{program}dbird{}", PROGRAM.render_reset()),
            format!("{heading}Commands:"),
            format!("{}update", COMMAND.render()),
            format!("{}-h", SHORT.render()),
            format!("{}--help", LONG.render()),
            format!("{}<NUMBER>", VALUE.render()),
            format!("{}[OPTIONS]", USAGE_VALUE.render()),
        ] {
            assert!(help.contains(&piece), "{piece:?} missing from\n{help:?}");
        }
    }

    #[test]
    fn version_flags_print_the_name_and_version() {
        for arguments in [&["-V"][..], &["--version"]] {
            assert_eq!(
                shown(arguments),
                (format!("{}\n", version_text()), 0),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn mistakes_exit_2() {
        for arguments in [
            &["--colour"][..],
            &["fly-now"],
            &["--seed"],
            &["--seed", "bird"],
            &["--seed", "-1"],
            &["--seed", "1.5"],
            &["--seed", "18446744073709551616"],
            &["--seed", "1", "--seed", "2"],
            &["--online"],
            &["--online="],
            &["--online", "ab"],
            &["--online", "bird name"],
            &["--online", "_bird"],
            &["--online", "bïrd"],
            &["--online", "way-too-long-for-dbird"],
            &["--online", "one", "--online", "two"],
            &["--reset-score", "--online", "PlayerOne"],
            &["--seed", "1", "update"],
            &["update", "--mute"],
            &["help", "fly-now"],
            &["--", "--no-color"],
            &["--no-color=true"],
        ] {
            let error = stop(arguments);
            assert_eq!(error.exit_code(), 2, "{arguments:?}: {error}");
            assert!(
                error.to_string().contains("--help"),
                "{arguments:?} points at --help"
            );
        }
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
                online_username: None,
                reset_score: false,
            }
        );
    }

    #[test]
    fn no_color_is_found_anywhere() {
        assert!(no_color_requested(&["update", "--no-color"]));
        assert!(no_color_requested(&["--no-color", "-h"]));
        assert!(!no_color_requested(&["--ascii"]));
    }

    #[test]
    fn colors_only_reach_a_terminal_without_no_color() {
        assert!(!colors_allowed(false, false));
        assert!(!colors_allowed(true, true));
    }

    #[test]
    fn plain_output_has_no_escape_codes() {
        let mut output = Vec::new();
        write_styled(&mut output, &help(), false, false).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(!text.contains('\x1b'));
        assert_eq!(text, help().to_string());
    }

    #[test]
    fn version_line_uses_package_metadata() {
        assert_eq!(version_text(), concat!("dbird ", env!("CARGO_PKG_VERSION")));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_unicode_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let argument = OsString::from_vec(vec![0xff]);
        assert_eq!(parse_args([argument]).unwrap_err().exit_code(), 2);
    }
}
