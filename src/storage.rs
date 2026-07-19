//! Persistent high-score storage.
//!
//! The score lives in the user's state directory, never in the checkout. Reads
//! deliberately fail closed to a score of zero so a damaged state file cannot
//! prevent the game from starting.

use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const APP_DIRECTORY: &str = "dbird";
const SCORE_FILE: &str = "high-score.json";
const FORMAT_VERSION: u64 = 1;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The platform-appropriate location of the high-score file.
///
/// `XDG_STATE_HOME` takes precedence. Without it, macOS uses Application
/// Support, Windows uses Local AppData, and other platforms use
/// `~/.local/state`. If no suitable state root can be determined, this returns
/// `None`.
pub fn high_score_path() -> Option<PathBuf> {
    let xdg_state_home = non_empty_env("XDG_STATE_HOME");
    let home = non_empty_env("HOME").or_else(|| non_empty_env("USERPROFILE"));
    let local_app_data = non_empty_env("LOCALAPPDATA");

    path_from_environment(
        xdg_state_home.as_deref(),
        home.as_deref(),
        local_app_data.as_deref(),
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
    )
}

/// Resolves another file beneath dbird's platform-appropriate state directory.
pub(crate) fn state_file_path(path: impl AsRef<Path>) -> Option<PathBuf> {
    high_score_path()?.parent().map(|root| root.join(path))
}

fn non_empty_env(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn path_from_environment(
    xdg_state_home: Option<&OsStr>,
    home: Option<&OsStr>,
    local_app_data: Option<&OsStr>,
    is_macos: bool,
    is_windows: bool,
) -> Option<PathBuf> {
    let state_root = if let Some(root) = xdg_state_home {
        PathBuf::from(root)
    } else if is_windows {
        if let Some(root) = local_app_data {
            PathBuf::from(root)
        } else {
            PathBuf::from(home?).join("AppData").join("Local")
        }
    } else {
        let home = PathBuf::from(home?);
        if is_macos {
            home.join("Library").join("Application Support")
        } else {
            home.join(".local").join("state")
        }
    };

    Some(state_root.join(APP_DIRECTORY).join(SCORE_FILE))
}

/// A high-score store resolved for the current user.
#[derive(Clone, Debug)]
pub struct HighScoreStore {
    path: Option<PathBuf>,
}

impl HighScoreStore {
    /// Resolves the store's path from the current process environment.
    pub fn new() -> Self {
        Self {
            path: high_score_path(),
        }
    }

    /// Returns the resolved file path, or `None` when no state location exists.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Loads the high score.
    ///
    /// A missing, unreadable, malformed, or unsupported file is treated as an
    /// empty store and returns zero.
    pub fn load(&self) -> u64 {
        self.path
            .as_deref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| parse_document(&contents))
            .unwrap_or(0)
    }

    /// Atomically replaces the persisted high score.
    ///
    /// When the process has no usable state directory (for example, `HOME` is
    /// absent), saving is a non-fatal no-op.
    pub fn save(&self, high_score: u64) -> io::Result<()> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };

        let document = format!("{{\"version\":{FORMAT_VERSION},\"high_score\":{high_score}}}\n");
        atomic_write(path, document.as_bytes())
    }

    /// Deletes the persisted score. Missing files and missing state roots are
    /// successful no-ops.
    pub fn reset(&self) -> io::Result<()> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };

        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    fn at(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

impl Default for HighScoreStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "state-file path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(SCORE_FILE);

    for _ in 0..100 {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{file_name}.tmp-{}-{sequence}",
            std::process::id()
        ));

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);

        let mut temporary_file = match options.open(&temporary_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let write_result = (|| {
            temporary_file.write_all(contents)?;
            temporary_file.sync_all()?;
            drop(temporary_file);
            fs::rename(&temporary_path, path)
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        return write_result;
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary high-score file",
    ))
}

fn parse_document(input: &str) -> Option<u64> {
    let mut parser = JsonParser::new(input);
    parser.consume_whitespace();
    parser.expect('{')?;
    parser.consume_whitespace();

    let mut version = None;
    let mut high_score = None;

    if parser.consume('}') {
        return None;
    }

    loop {
        let key = parser.parse_string()?;
        parser.consume_whitespace();
        parser.expect(':')?;
        parser.consume_whitespace();

        match key.as_str() {
            "version" if version.is_none() => version = Some(parser.parse_u64()?),
            "high_score" if high_score.is_none() => high_score = Some(parser.parse_u64()?),
            _ => return None,
        }

        parser.consume_whitespace();
        if parser.consume('}') {
            break;
        }
        parser.expect(',')?;
        parser.consume_whitespace();
    }

    parser.consume_whitespace();
    if !parser.is_finished() || version != Some(FORMAT_VERSION) {
        return None;
    }

    high_score
}

struct JsonParser<'a> {
    input: &'a str,
    offset: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, offset: 0 }
    }

    fn is_finished(&self) -> bool {
        self.offset == self.input.len()
    }

    fn next_char(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.next_char() != Some(expected) {
            return false;
        }
        self.offset += expected.len_utf8();
        true
    }

    fn expect(&mut self, expected: char) -> Option<()> {
        self.consume(expected).then_some(())
    }

    fn consume_whitespace(&mut self) {
        while matches!(self.next_char(), Some(' ' | '\n' | '\r' | '\t')) {
            let character = self.next_char().expect("matched a character");
            self.offset += character.len_utf8();
        }
    }

    fn parse_string(&mut self) -> Option<String> {
        self.expect('"')?;
        let mut value = String::new();

        loop {
            let character = self.next_char()?;
            self.offset += character.len_utf8();

            match character {
                '"' => return Some(value),
                '\\' => value.push(self.parse_escape()?),
                character if character <= '\u{1f}' => return None,
                character => value.push(character),
            }
        }
    }

    fn parse_escape(&mut self) -> Option<char> {
        let escaped = self.next_char()?;
        self.offset += escaped.len_utf8();

        match escaped {
            '"' => Some('"'),
            '\\' => Some('\\'),
            '/' => Some('/'),
            'b' => Some('\u{0008}'),
            'f' => Some('\u{000c}'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'u' => {
                let digits = self.input.get(self.offset..self.offset + 4)?;
                if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return None;
                }
                self.offset += 4;
                char::from_u32(u32::from_str_radix(digits, 16).ok()?)
            }
            _ => None,
        }
    }

    fn parse_u64(&mut self) -> Option<u64> {
        let first = self.next_char()?;
        if !first.is_ascii_digit() {
            return None;
        }

        let mut value = 0_u64;
        let mut digits = 0;
        while let Some(character) = self.next_char() {
            let Some(digit) = character.to_digit(10) else {
                break;
            };
            if digits == 1 && value == 0 {
                return None;
            }
            value = value.checked_mul(10)?.checked_add(u64::from(digit))?;
            self.offset += character.len_utf8();
            digits += 1;
        }

        (digits > 0).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "dbird-storage-test-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).expect("create test directory");
            Self { path }
        }

        fn join(&self, path: impl AsRef<Path>) -> PathBuf {
            self.path.join(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn xdg_state_home_has_precedence_on_every_platform() {
        let path = path_from_environment(
            Some(OsStr::new("/state")),
            Some(OsStr::new("/home/player")),
            Some(OsStr::new("/local-app-data")),
            true,
            false,
        );

        assert_eq!(path, Some(PathBuf::from("/state/dbird/high-score.json")));
    }

    #[test]
    fn macos_falls_back_to_application_support() {
        let path = path_from_environment(None, Some(OsStr::new("/home/player")), None, true, false);

        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/player/Library/Application Support/dbird/high-score.json"
            ))
        );
    }

    #[test]
    fn other_platforms_fall_back_to_local_state() {
        let path =
            path_from_environment(None, Some(OsStr::new("/home/player")), None, false, false);

        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/player/.local/state/dbird/high-score.json"
            ))
        );
    }

    #[test]
    fn missing_state_and_home_produces_no_path() {
        assert_eq!(path_from_environment(None, None, None, true, false), None);
        assert_eq!(path_from_environment(None, None, None, false, false), None);
        assert_eq!(path_from_environment(None, None, None, false, true), None);
    }

    #[test]
    fn windows_uses_local_app_data_then_user_profile() {
        let local_app_data = path_from_environment(
            None,
            Some(OsStr::new("C:/Users/player")),
            Some(OsStr::new("D:/State")),
            false,
            true,
        );
        assert_eq!(
            local_app_data,
            Some(PathBuf::from("D:/State/dbird/high-score.json"))
        );

        let user_profile =
            path_from_environment(None, Some(OsStr::new("C:/Users/player")), None, false, true);
        assert_eq!(
            user_profile,
            Some(PathBuf::from(
                "C:/Users/player/AppData/Local/dbird/high-score.json"
            ))
        );
    }

    #[test]
    fn missing_file_loads_as_zero() {
        let directory = TestDirectory::new();
        let store = HighScoreStore::at(Some(directory.join("missing.json")));

        assert_eq!(store.load(), 0);
    }

    #[test]
    fn malformed_or_unsupported_files_load_as_zero() {
        let invalid_documents = [
            "",
            "not json",
            "{}",
            "[]",
            r#"{"version":1}"#,
            r#"{"high_score":12}"#,
            r#"{"version":2,"high_score":12}"#,
            r#"{"version":1,"high_score":-1}"#,
            r#"{"version":1,"high_score":1.5}"#,
            r#"{"version":1,"high_score":"12"}"#,
            r#"{"version":1,"high_score":01}"#,
            r#"{"version":1,"high_score":18446744073709551616}"#,
            r#"{"version":1,"version":1,"high_score":12}"#,
            r#"{"version":1,"high_score":12,"extra":true}"#,
            r#"{"version":1,"high_score":12} trailing"#,
        ];

        for document in invalid_documents {
            assert_eq!(parse_document(document), None, "accepted: {document}");
        }
    }

    #[test]
    fn parser_accepts_whitespace_and_either_field_order() {
        assert_eq!(
            parse_document(" { \n \"high_score\" : 73, \"version\" : 1 } \n"),
            Some(73)
        );
        assert_eq!(parse_document(r#"{"version":1,"high_score":0}"#), Some(0));
        assert_eq!(
            parse_document(r#"{"\u0076ersion":1,"high_score":42}"#),
            Some(42)
        );
    }

    #[test]
    fn save_creates_parent_directories_and_round_trips() {
        let directory = TestDirectory::new();
        let score_path = directory.join("nested/state/high-score.json");
        let store = HighScoreStore::at(Some(score_path.clone()));

        store.save(98_765).expect("save score");

        assert_eq!(store.load(), 98_765);
        assert_eq!(
            fs::read_to_string(score_path).expect("read score document"),
            "{\"version\":1,\"high_score\":98765}\n"
        );
    }

    #[test]
    fn save_atomically_replaces_an_existing_score() {
        let directory = TestDirectory::new();
        let score_path = directory.join("high-score.json");
        let store = HighScoreStore::at(Some(score_path));

        store.save(5).expect("save initial score");
        store.save(u64::MAX).expect("replace score");

        assert_eq!(store.load(), u64::MAX);
        let remaining_files = fs::read_dir(&directory.path)
            .expect("list test directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read directory entries");
        assert_eq!(remaining_files.len(), 1, "temporary file was left behind");
    }

    #[test]
    fn reset_deletes_the_score_and_is_idempotent() {
        let directory = TestDirectory::new();
        let store = HighScoreStore::at(Some(directory.join("high-score.json")));
        store.save(11).expect("save score");

        store.reset().expect("reset score");
        store.reset().expect("reset an already empty store");

        assert_eq!(store.load(), 0);
        assert!(!store.path().expect("store path").exists());
    }

    #[test]
    fn missing_home_is_nonfatal_for_all_operations() {
        let store = HighScoreStore::at(None);

        assert_eq!(store.path(), None);
        assert_eq!(store.load(), 0);
        store.save(100).expect("missing state root is a no-op");
        store.reset().expect("missing state root is a no-op");
    }
}
