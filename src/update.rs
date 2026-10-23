//! `dbird update`: moves to the latest release through whatever installed dbird.
//!
//! The channel comes from where the executable really is, after following
//! links: Homebrew's `bin/dbird` is only a link into its `Cellar`.

use std::cmp::Ordering;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

const LATEST_RELEASE: &str = "https://github.com/leduftw/dbird/releases/latest";
const INSTALLER_SH: &str =
    "https://github.com/leduftw/dbird/releases/latest/download/dbird-installer.sh";
const INSTALLER_PS1: &str =
    "https://github.com/leduftw/dbird/releases/latest/download/dbird-installer.ps1";
/// Where a version's WinGet manifest appears once Microsoft has accepted it.
const WINGET_MANIFESTS: &str =
    "https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/l/leduftw/dbird";
const USER_AGENT: &str = concat!("dbird/", env!("CARGO_PKG_VERSION"));
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How dbird was installed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Channel {
    Homebrew,
    WinGet,
    Cargo,
    /// Built from source (`target/debug` or `target/release`), so there's nothing to update.
    LocalBuild,
    /// The one-line installer, which put dbird in this folder.
    Installer(PathBuf),
}

impl Channel {
    /// Tells the channel from the executable's resolved path. `cargo_bin` is
    /// Cargo's `bin` folder, resolved the same way, if there is one.
    pub fn of(executable: &Path, cargo_bin: Option<&Path>) -> Self {
        let path = executable.to_string_lossy().replace('\\', "/");
        let folder = executable.parent().unwrap_or(Path::new(""));
        if path.contains("/Cellar/dbird/") {
            Self::Homebrew
        } else if path.to_ascii_lowercase().contains("/microsoft/winget/") {
            Self::WinGet
        } else if cargo_bin == Some(folder) {
            Self::Cargo
        } else if is_local_build(&path) {
            Self::LocalBuild
        } else {
            Self::Installer(folder.to_path_buf())
        }
    }
}

/// `.../target/release/dbird`, `.../target/debug/dbird` or `.../target/<triple>/release/dbird`.
fn is_local_build(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let count = parts.len();
    count >= 3
        && matches!(parts[count - 2], "debug" | "release")
        && parts[..count - 2].contains(&"target")
}

/// The command that updates dbird through its channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    /// What does the update, as the user knows it.
    pub manager: String,
    pub program: String,
    pub args: Vec<String>,
    /// `DBIRD_INSTALL_DIR` for the installer, so it replaces this copy.
    pub install_dir: Option<PathBuf>,
    /// Windows can't replace a running program, so dbird starts this and exits.
    pub detach: bool,
}

/// What updates dbird on this channel; nothing for a local build.
pub fn plan(channel: &Channel, windows: bool) -> Option<Plan> {
    let plan = |manager: &str,
                program: &str,
                args: &[&str],
                install_dir: Option<PathBuf>,
                detach: bool| Plan {
        manager: manager.to_owned(),
        program: program.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        install_dir,
        detach,
    };
    match channel {
        Channel::Homebrew => Some(plan(
            "Homebrew",
            "brew",
            &["upgrade", "leduftw/tap/dbird"],
            None,
            false,
        )),
        Channel::WinGet => Some(plan(
            "WinGet",
            "winget",
            &["upgrade", "--id", "leduftw.dbird"],
            None,
            true,
        )),
        Channel::Cargo => Some(plan(
            "Cargo",
            "cargo",
            &["install", "dbird", "--locked"],
            None,
            windows,
        )),
        Channel::LocalBuild => None,
        Channel::Installer(folder) if windows => {
            let script = format!("irm {INSTALLER_PS1} | iex");
            Some(plan(
                "the installer",
                "powershell",
                &["-NoProfile", "-Command", &script],
                Some(folder.clone()),
                true,
            ))
        }
        Channel::Installer(folder) => {
            // Download first, so a failed download fails the update instead of
            // piping nothing into `sh`, which would succeed.
            let script =
                format!("script=$(curl -fsSL {INSTALLER_SH}) && printf '%s\\n' \"$script\" | sh");
            Some(plan(
                "the installer",
                "sh",
                &["-c", &script],
                Some(folder.clone()),
                false,
            ))
        }
    }
}

/// A version as three numbers, so 1.10.0 is newer than 1.9.0.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// The version that GitHub's `releases/latest` redirect points at, from its
/// `Location` header, such as `https://github.com/leduftw/dbird/releases/tag/v1.4.0`.
pub fn version_from_location(location: &str) -> Option<String> {
    let version = location.rsplit_once("/releases/tag/v")?.1;
    parse_version(version).map(|_| version.to_owned())
}

/// Runs `dbird update` and reports the outcome.
pub fn run() -> ExitCode {
    match update() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("dbird: {message}");
            ExitCode::FAILURE
        }
    }
}

fn update() -> Result<(), String> {
    let current = env!("CARGO_PKG_VERSION");
    let executable = env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| format!("can't tell where dbird is installed: {error}"))?;
    let channel = Channel::of(&executable, cargo_bin().as_deref());
    let Some(plan) = plan(&channel, cfg!(windows)) else {
        return Err(format!(
            "{} is a local build; update it with `git pull` and `cargo build --release`",
            executable.display()
        ));
    };

    let agent = agent();
    let latest = latest_version(&agent)
        .map_err(|error| format!("can't check the latest release: {error}"))?;
    match parse_version(&latest).cmp(&parse_version(current)) {
        Ordering::Less => {
            println!("dbird {current} is newer than the latest release, {latest}.");
            return Ok(());
        }
        Ordering::Equal => {
            println!("dbird {current} is the latest version.");
            return Ok(());
        }
        Ordering::Greater => {}
    }
    if channel == Channel::WinGet
        && !on_winget(&agent, &latest)
            .map_err(|error| format!("can't check WinGet for {latest}: {error}"))?
    {
        println!(
            "dbird {latest} is out, but not on WinGet yet: Microsoft reviews each new version first, \
             which can take a few days."
        );
        return Ok(());
    }

    println!(
        "Updating dbird {current} to {latest} with {}.",
        plan.manager
    );
    run_plan(&plan, &latest)
}

fn update_command(plan: &Plan, version: &str) -> Command {
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    if let Some(folder) = &plan.install_dir {
        command
            .env("DBIRD_INSTALL_DIR", without_verbatim_prefix(folder))
            // An old installation pin must not undo the version check above.
            .env("DBIRD_VERSION", version);
    }
    command
}

fn run_plan(plan: &Plan, version: &str) -> Result<(), String> {
    let mut command = update_command(plan, version);
    if plan.detach {
        command
            .spawn()
            .map_err(|error| format!("can't start {}: {error}", plan.program))?;
        println!(
            "{} finishes the update in this window once dbird has exited.",
            plan.manager
        );
        return Ok(());
    }
    let status = command
        .status()
        .map_err(|error| format!("can't run {}: {error}", plan.program))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} didn't finish the update ({status})",
            plan.manager
        ))
    }
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        // The redirect is the answer, so it isn't followed.
        .max_redirects(0)
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(config)
}

fn latest_version(agent: &ureq::Agent) -> Result<String, String> {
    let response = agent
        .get(LATEST_RELEASE)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?;
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    version_from_location(location)
        .ok_or_else(|| format!("GitHub answered {} without a release", response.status()))
}

fn on_winget(agent: &ureq::Agent, version: &str) -> Result<bool, String> {
    let response = agent
        .get(format!("{WINGET_MANIFESTS}/{version}/leduftw.dbird.yaml"))
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| error.to_string())?;
    match response.status().as_u16() {
        200 => Ok(true),
        404 => Ok(false),
        status => Err(format!("its manifests answered {status}")),
    }
}

/// Cargo's `bin` folder: `$CARGO_HOME/bin`, by default `~/.cargo/bin`.
fn cargo_bin() -> Option<PathBuf> {
    let cargo_home = non_empty_env("CARGO_HOME").map(PathBuf::from).or_else(|| {
        non_empty_env("HOME")
            .or_else(|| non_empty_env("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".cargo"))
    })?;
    fs::canonicalize(cargo_home.join("bin")).ok()
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

/// Windows resolves paths to `\\?\C:\...`, which the installer would also
/// write into the user's `PATH`; it gets the ordinary form instead.
fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Resolves links, as `dbird update` does before it reads the channel.
pub fn resolve(executable: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct TestDirectory(PathBuf);

    #[cfg(unix)]
    impl TestDirectory {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};

            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = env::temp_dir().join(format!(
                "dbird-update-process-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn script(&self, name: &str, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;

            let path = self.0.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }

    #[cfg(unix)]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn homebrew_keeps_dbird_in_its_cellar() {
        for path in [
            "/opt/homebrew/Cellar/dbird/1.3.0/bin/dbird",
            "/usr/local/Cellar/dbird/1.3.0/bin/dbird",
            "/home/linuxbrew/.linuxbrew/Cellar/dbird/1.3.0/bin/dbird",
        ] {
            assert_eq!(
                Channel::of(Path::new(path), None),
                Channel::Homebrew,
                "{path}"
            );
        }
    }

    #[test]
    fn winget_keeps_dbird_in_its_packages_folder() {
        let path = r"C:\Users\me\AppData\Local\Microsoft\WinGet\Packages\leduftw.dbird_Microsoft.Winget.Source_8wekyb3d8bbwe\dbird.exe";
        assert_eq!(Channel::of(Path::new(path), None), Channel::WinGet);
        let verbatim = r"\\?\C:\Users\me\AppData\Local\Microsoft\WinGet\Links\dbird.exe";
        assert_eq!(Channel::of(Path::new(verbatim), None), Channel::WinGet);
    }

    #[test]
    fn cargo_installs_into_its_bin_folder() {
        let bin = Path::new("/home/me/.cargo/bin");
        assert_eq!(Channel::of(&bin.join("dbird"), Some(bin)), Channel::Cargo);
        assert_ne!(
            Channel::of(Path::new("/usr/local/bin/dbird"), Some(bin)),
            Channel::Cargo
        );
    }

    #[test]
    fn local_builds_are_recognized() {
        for path in [
            "/home/me/dbird/target/release/dbird",
            "/home/me/dbird/target/debug/dbird",
            "/home/me/dbird/target/aarch64-apple-darwin/release/dbird",
            r"C:\src\dbird\target\release\dbird.exe",
        ] {
            assert_eq!(
                Channel::of(Path::new(path), None),
                Channel::LocalBuild,
                "{path}"
            );
        }
    }

    #[test]
    fn anything_else_came_from_the_installer() {
        assert_eq!(
            Channel::of(Path::new("/home/me/.local/bin/dbird"), None),
            Channel::Installer(PathBuf::from("/home/me/.local/bin"))
        );
        assert_eq!(
            Channel::of(Path::new("/usr/local/bin/dbird"), None),
            Channel::Installer(PathBuf::from("/usr/local/bin"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_are_followed_before_reading_the_channel() {
        let root = env::temp_dir().join(format!("dbird-update-test-{}", std::process::id()));
        let cellar = root.join("Cellar/dbird/1.3.0/bin");
        let bin = root.join("bin");
        fs::create_dir_all(&cellar).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(cellar.join("dbird"), b"").unwrap();
        let link = bin.join("dbird");
        std::os::unix::fs::symlink(cellar.join("dbird"), &link).unwrap();

        let unresolved = Channel::of(&link, None);
        let resolved = Channel::of(&resolve(&link).unwrap(), None);
        fs::remove_dir_all(&root).unwrap();

        assert!(
            matches!(unresolved, Channel::Installer(_)),
            "the link alone looks like the installer"
        );
        assert_eq!(resolved, Channel::Homebrew);
    }

    #[test]
    fn each_channel_updates_with_its_own_tool() {
        let command = |channel: Channel, windows: bool| {
            let plan = plan(&channel, windows).expect("a plan");
            (
                format!("{} {}", plan.program, plan.args.join(" ")),
                plan.detach,
            )
        };
        assert_eq!(
            command(Channel::Homebrew, false),
            ("brew upgrade leduftw/tap/dbird".into(), false)
        );
        assert_eq!(
            command(Channel::WinGet, true),
            ("winget upgrade --id leduftw.dbird".into(), true)
        );
        assert_eq!(
            command(Channel::Cargo, false),
            ("cargo install dbird --locked".into(), false)
        );
        assert!(
            command(Channel::Cargo, true).1,
            "Windows can't replace the running dbird.exe"
        );
        assert_eq!(plan(&Channel::LocalBuild, false), None);
    }

    #[test]
    fn the_installer_reinstalls_into_the_same_folder() {
        let folder = PathBuf::from("/home/me/.local/bin");
        let unix = plan(&Channel::Installer(folder.clone()), false).unwrap();
        assert_eq!(unix.program, "sh");
        assert!(unix.args[1].contains(INSTALLER_SH));
        assert_eq!(unix.install_dir.as_deref(), Some(folder.as_path()));
        assert!(!unix.detach);

        let windows = plan(&Channel::Installer(folder.clone()), true).unwrap();
        assert_eq!(windows.program, "powershell");
        assert!(windows.args.last().unwrap().contains(INSTALLER_PS1));
        assert!(windows.detach);
    }

    #[cfg(unix)]
    #[test]
    fn the_installer_receives_the_checked_version_and_destination() {
        let directory = TestDirectory::new();
        let destination = directory.0.join("installed game");
        fs::create_dir(&destination).unwrap();
        directory.script(
            "curl",
            r#"printf '%s\n' 'printf "%s\n" "$DBIRD_VERSION" > "$DBIRD_INSTALL_DIR/requested-version"'"#,
        );
        let plan = plan(&Channel::Installer(destination.clone()), false).unwrap();
        let output = update_command(&plan, "1.4.0")
            .env(
                "PATH",
                env::join_paths([&directory.0, Path::new("/usr/bin"), Path::new("/bin")]).unwrap(),
            )
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            fs::read_to_string(destination.join("requested-version")).unwrap(),
            "1.4.0\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_download_cannot_execute_a_partial_installer() {
        let directory = TestDirectory::new();
        directory.script(
            "curl",
            r#"printf '%s\n' 'touch "$DBIRD_INSTALL_DIR/partial-installer-ran"'
exit 22"#,
        );
        let plan = plan(&Channel::Installer(directory.0.clone()), false).unwrap();
        let output = update_command(&plan, "1.4.0")
            .env(
                "PATH",
                env::join_paths([&directory.0, Path::new("/usr/bin"), Path::new("/bin")]).unwrap(),
            )
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(22));
        assert!(!directory.0.join("partial-installer-ran").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_package_manager_failure_is_reported() {
        let directory = TestDirectory::new();
        let manager = directory.script("manager", "exit 9");
        let mut plan = plan(&Channel::Homebrew, false).unwrap();
        plan.program = manager.to_str().unwrap().to_owned();
        let error = run_plan(&plan, "1.4.0").unwrap_err();
        assert!(
            error.contains("Homebrew didn't finish the update"),
            "{error}"
        );
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(parse_version("1.10.0"), Some((1, 10, 0)));
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
        for text in ["1.2", "1.2.3.4", "v1.2.3", "1.2.x", ""] {
            assert_eq!(parse_version(text), None, "{text}");
        }
    }

    #[test]
    fn the_latest_release_comes_from_the_redirect() {
        assert_eq!(
            version_from_location("https://github.com/leduftw/dbird/releases/tag/v1.4.0")
                .as_deref(),
            Some("1.4.0")
        );
        assert_eq!(
            version_from_location("https://github.com/leduftw/dbird/releases"),
            None
        );
        assert_eq!(version_from_location(""), None);
    }

    #[test]
    fn the_installer_folder_drops_the_verbatim_prefix() {
        assert_eq!(
            without_verbatim_prefix(Path::new(
                r"\\?\C:\Users\me\AppData\Local\Programs\dbird\bin"
            )),
            PathBuf::from(r"C:\Users\me\AppData\Local\Programs\dbird\bin")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new("/usr/local/bin")),
            PathBuf::from("/usr/local/bin")
        );
    }
}
