//! Runtime theme selection and system-appearance detection.

/// The user-facing theme choice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeMode {
    /// Follow the current operating-system appearance.
    #[default]
    System,
    /// Force the daytime palette.
    Light,
    /// Force the nighttime palette.
    Dark,
}

impl ThemeMode {
    /// Advance through the choices shown in the TUI.
    pub const fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    /// Compact label used by the footer.
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "SYS",
            Self::Light => "LGT",
            Self::Dark => "DRK",
        }
    }

    fn resolve(self) -> ResolvedTheme {
        match self {
            Self::System => detect_system_theme(),
            Self::Light => ResolvedTheme::Light,
            Self::Dark => ResolvedTheme::Dark,
        }
    }
}

/// The concrete palette currently being rendered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedTheme {
    Light,
    Dark,
}

/// A theme choice paired with its cached concrete appearance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeState {
    mode: ThemeMode,
    resolved: ResolvedTheme,
}

impl ThemeState {
    /// Start in System mode and read the current OS appearance once.
    pub fn system() -> Self {
        Self {
            mode: ThemeMode::System,
            resolved: detect_system_theme(),
        }
    }

    /// Construct a deterministic explicit theme, primarily for rendering tests.
    pub const fn explicit(mode: ThemeMode) -> Self {
        let resolved = match mode {
            ThemeMode::Light => ResolvedTheme::Light,
            ThemeMode::Dark | ThemeMode::System => ResolvedTheme::Dark,
        };
        Self { mode, resolved }
    }

    /// Current user-facing choice.
    pub const fn mode(self) -> ThemeMode {
        self.mode
    }

    /// Concrete light or dark palette currently in use.
    pub const fn resolved(self) -> ResolvedTheme {
        self.resolved
    }

    /// Select the next theme. Returning to System refreshes the OS appearance.
    pub fn cycle(&mut self) {
        self.mode = self.mode.next();
        self.resolved = self.mode.resolve();
    }
}

impl Default for ThemeState {
    fn default() -> Self {
        Self::system()
    }
}

#[cfg(target_os = "macos")]
fn detect_system_theme() -> ResolvedTheme {
    use std::process::{Command, Stdio};

    let output = Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(output) => theme_from_macos_defaults(
            output.status.success(),
            output.status.code(),
            &output.stdout,
        ),
        Err(_) => ResolvedTheme::Dark,
    }
}

#[cfg(not(target_os = "macos"))]
const fn detect_system_theme() -> ResolvedTheme {
    ResolvedTheme::Dark
}

#[cfg(any(test, target_os = "macos"))]
fn theme_from_macos_defaults(
    success: bool,
    status_code: Option<i32>,
    stdout: &[u8],
) -> ResolvedTheme {
    if !success {
        // In the light appearance this global preference is normally absent,
        // and `defaults read` reports that ordinary missing-key case as code 1.
        return if status_code == Some(1) {
            ResolvedTheme::Light
        } else {
            ResolvedTheme::Dark
        };
    }

    match std::str::from_utf8(stdout).map(str::trim) {
        Ok(value) if value.eq_ignore_ascii_case("light") => ResolvedTheme::Light,
        Ok(value) if value.eq_ignore_ascii_case("dark") => ResolvedTheme::Dark,
        _ => ResolvedTheme::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_mode_cycles_in_display_order() {
        let mut theme = ThemeState::explicit(ThemeMode::System);
        assert_eq!(theme.mode(), ThemeMode::System);
        assert_eq!(theme.mode().label(), "SYS");

        theme.cycle();
        assert_eq!(theme, ThemeState::explicit(ThemeMode::Light));
        assert_eq!(theme.mode().label(), "LGT");

        theme.cycle();
        assert_eq!(theme, ThemeState::explicit(ThemeMode::Dark));
        assert_eq!(theme.mode().label(), "DRK");

        theme.cycle();
        assert_eq!(theme.mode(), ThemeMode::System);
    }

    #[test]
    fn macos_defaults_output_distinguishes_day_and_night() {
        assert_eq!(
            theme_from_macos_defaults(true, Some(0), b" Dark\n"),
            ResolvedTheme::Dark
        );
        assert_eq!(
            theme_from_macos_defaults(true, Some(0), b"LIGHT\n"),
            ResolvedTheme::Light
        );
        assert_eq!(
            theme_from_macos_defaults(false, Some(1), b""),
            ResolvedTheme::Light
        );
    }

    #[test]
    fn ambiguous_system_detection_safely_keeps_the_dark_palette() {
        for (success, code, output) in [
            (true, Some(0), b"surprise".as_slice()),
            (true, Some(0), &[0xff]),
            (false, Some(2), b"".as_slice()),
            (false, None, b"".as_slice()),
        ] {
            assert_eq!(
                theme_from_macos_defaults(success, code, output),
                ResolvedTheme::Dark
            );
        }
    }
}
