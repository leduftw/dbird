//! Optional cloud-backed leaderboard support.
//!
//! Online play is always explicit. The game keeps a small local profile only
//! for the player's private credential, a cached best, and a score waiting to
//! be retried; the leaderboard service remains the source of truth.

use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::storage::{atomic_write, state_file_path};

const PROFILE_VERSION: u8 = 1;
const PROFILE_DIRECTORY: &str = "online";
const ENDPOINT_ENVIRONMENT: &str = "DBIRD_LEADERBOARD_URL";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const RESPONSE_LIMIT: u64 = 64 * 1024;
const CREDENTIAL_BYTES: usize = 32;

/// One row in the global leaderboard.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub username: String,
    pub score: u32,
}

/// Current connectivity and persistence state for an online player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncStatus {
    Connecting,
    Synced,
    Queued,
    Unavailable,
}

impl SyncStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connecting => "CONNECTING",
            Self::Synced => "SYNCED",
            Self::Queued => "QUEUED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

/// Read-only state used by the renderer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineView {
    pub username: String,
    pub high_score: u32,
    pub rank: Option<u32>,
    pub entries: Vec<LeaderboardEntry>,
    pub status: SyncStatus,
    pub detail: Option<String>,
}

/// Configuration or local-state failure while enabling online play.
#[derive(Debug)]
pub struct OnlineError {
    message: String,
}

impl OnlineError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OnlineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OnlineError {}

impl From<io::Error> for OnlineError {
    fn from(error: io::Error) -> Self {
        Self::new(error.to_string())
    }
}

/// Validated, case-insensitive player name.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Username {
    display: String,
    key: String,
}

impl Username {
    fn parse(value: &str) -> Result<Self, OnlineError> {
        let length = value.chars().count();
        if !(3..=16).contains(&length) {
            return Err(OnlineError::new(
                "username must contain between 3 and 16 characters",
            ));
        }
        if !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        {
            return Err(OnlineError::new(
                "username must start with an ASCII letter or number",
            ));
        }
        if !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        {
            return Err(OnlineError::new(
                "username may only contain ASCII letters, numbers, `_`, and `-`",
            ));
        }

        Ok(Self {
            display: value.to_owned(),
            key: value.to_ascii_lowercase(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredProfile {
    version: u8,
    username: String,
    credential: String,
    cached_high_score: u32,
    pending_high_score: u32,
}

impl StoredProfile {
    fn load_or_create(path: &Path, username: &Username) -> Result<Self, OnlineError> {
        match fs::read(path) {
            Ok(document) => {
                let profile: Self = serde_json::from_slice(&document).map_err(|error| {
                    OnlineError::new(format!(
                        "online profile `{}` is malformed: {error}",
                        path.display()
                    ))
                })?;
                profile.validate(path, username)?;
                Ok(profile)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let profile = Self {
                    version: PROFILE_VERSION,
                    username: username.display.clone(),
                    credential: generate_credential()?,
                    cached_high_score: 0,
                    pending_high_score: 0,
                };
                profile.save(path)?;
                Ok(profile)
            }
            Err(error) => Err(OnlineError::new(format!(
                "online profile `{}` could not be read: {error}",
                path.display()
            ))),
        }
    }

    fn validate(&self, path: &Path, requested: &Username) -> Result<(), OnlineError> {
        let stored = Username::parse(&self.username).map_err(|_| {
            OnlineError::new(format!(
                "online profile `{}` contains an invalid username",
                path.display()
            ))
        })?;
        if self.version != PROFILE_VERSION
            || stored.key != requested.key
            || !valid_credential(&self.credential)
        {
            return Err(OnlineError::new(format!(
                "online profile `{}` is unsupported or invalid",
                path.display()
            )));
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<(), OnlineError> {
        let mut document = serde_json::to_vec(self).map_err(|error| {
            OnlineError::new(format!("online profile could not be encoded: {error}"))
        })?;
        document.push(b'\n');
        atomic_write(path, &document).map_err(|error| {
            OnlineError::new(format!(
                "online profile `{}` could not be saved: {error}",
                path.display()
            ))
        })
    }

    fn best(&self) -> u32 {
        self.cached_high_score.max(self.pending_high_score)
    }
}

fn generate_credential() -> Result<String, OnlineError> {
    let mut bytes = [0_u8; CREDENTIAL_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| {
        OnlineError::new(format!("secure credential generation failed: {error}"))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn valid_credential(value: &str) -> bool {
    value.len() == CREDENTIAL_BYTES * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// A live online-player session. Network calls run on a background thread.
pub struct OnlineSession {
    profile_path: PathBuf,
    profile: StoredProfile,
    commands: Sender<WorkerCommand>,
    events: Receiver<WorkerEvent>,
    view: OnlineView,
}

impl OnlineSession {
    /// Enables online play for `username` using the configured leaderboard URL.
    pub fn connect(username: &str) -> Result<Self, OnlineError> {
        let username = Username::parse(username)?;
        let endpoint = configured_endpoint()?;
        let profile_path =
            state_file_path(Path::new(PROFILE_DIRECTORY).join(format!("{}.json", username.key)))
                .ok_or_else(|| {
                    OnlineError::new("online play needs a writable user state directory")
                })?;
        Self::connect_at(username, endpoint, profile_path)
    }

    fn connect_at(
        username: Username,
        endpoint: String,
        profile_path: PathBuf,
    ) -> Result<Self, OnlineError> {
        let endpoint = validate_endpoint(endpoint)?;
        let profile = StoredProfile::load_or_create(&profile_path, &username)?;
        let initial_best = profile.best();
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let worker_profile = profile.clone();

        thread::Builder::new()
            .name("dbird-leaderboard".into())
            .spawn(move || {
                leaderboard_worker(
                    endpoint,
                    worker_profile.username,
                    worker_profile.credential,
                    worker_profile.pending_high_score,
                    command_receiver,
                    event_sender,
                );
            })
            .map_err(|error| {
                OnlineError::new(format!("leaderboard worker could not start: {error}"))
            })?;

        Ok(Self {
            profile_path,
            view: OnlineView {
                username: profile.username.clone(),
                high_score: initial_best,
                rank: None,
                entries: Vec::new(),
                status: SyncStatus::Connecting,
                detail: None,
            },
            profile,
            commands: command_sender,
            events: event_receiver,
        })
    }

    pub fn view(&self) -> &OnlineView {
        &self.view
    }

    pub fn high_score(&self) -> u32 {
        self.profile.best()
    }

    /// Persists a score for retry before asking the background worker to sync it.
    pub fn submit(&mut self, score: u32) -> Result<(), OnlineError> {
        if score <= self.profile.cached_high_score || score <= self.profile.pending_high_score {
            return Ok(());
        }

        self.profile.pending_high_score = score;
        self.profile.save(&self.profile_path)?;
        self.view.high_score = self.profile.best();
        self.view.status = SyncStatus::Queued;
        self.view.detail = Some("High score saved locally until cloud sync succeeds.".into());
        self.commands
            .send(WorkerCommand::Submit(score))
            .map_err(|_| OnlineError::new("leaderboard worker stopped unexpectedly"))
    }

    /// Refreshes the global leaderboard without blocking the game loop.
    pub fn refresh(&mut self) {
        self.view.status = SyncStatus::Connecting;
        self.view.detail = None;
        if self.commands.send(WorkerCommand::Refresh).is_err() {
            self.view.status = SyncStatus::Unavailable;
            self.view.detail = Some("Leaderboard worker stopped unexpectedly.".into());
        }
    }

    /// Applies every completed background operation and returns the latest best.
    pub fn poll(&mut self) -> Result<u32, OnlineError> {
        loop {
            match self.events.try_recv() {
                Ok(WorkerEvent::Snapshot(snapshot)) => self.apply_snapshot(snapshot)?,
                Ok(WorkerEvent::Failed(message)) => {
                    if self.profile.pending_high_score > self.profile.cached_high_score {
                        self.view.status = SyncStatus::Queued;
                        self.view.detail = Some(format!(
                            "Cloud unavailable; the high score remains queued. {message}"
                        ));
                    } else {
                        self.view.status = SyncStatus::Unavailable;
                        self.view.detail = Some(message);
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        Ok(self.profile.best())
    }

    fn apply_snapshot(&mut self, snapshot: ApiSnapshot) -> Result<(), OnlineError> {
        self.profile.username = snapshot.player.username.clone();
        self.profile.cached_high_score = self.profile.cached_high_score.max(snapshot.player.score);
        if self.profile.pending_high_score <= self.profile.cached_high_score {
            self.profile.pending_high_score = 0;
        }
        self.profile.save(&self.profile_path)?;

        self.view.username = self.profile.username.clone();
        self.view.high_score = self.profile.best();
        self.view.rank = snapshot.player.rank;
        self.view.entries = snapshot.leaderboard;
        self.view.status = if self.profile.pending_high_score > self.profile.cached_high_score {
            SyncStatus::Queued
        } else {
            SyncStatus::Synced
        };
        self.view.detail = None;
        Ok(())
    }
}

fn configured_endpoint() -> Result<String, OnlineError> {
    let runtime = env::var(ENDPOINT_ENVIRONMENT)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let embedded = option_env!("DBIRD_LEADERBOARD_URL")
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty());
    runtime.or(embedded).ok_or_else(|| {
        OnlineError::new("online play is not configured in this build; set DBIRD_LEADERBOARD_URL")
    })
}

fn validate_endpoint(value: String) -> Result<String, OnlineError> {
    let endpoint = value.trim();
    let parsed = url::Url::parse(endpoint)
        .map_err(|_| OnlineError::new("DBIRD_LEADERBOARD_URL must be an absolute HTTPS URL"))?;
    let local_http =
        parsed.scheme() == "http" && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"));
    if parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || (parsed.scheme() != "https" && !local_http)
    {
        return Err(OnlineError::new(
            "DBIRD_LEADERBOARD_URL must be HTTPS (HTTP is allowed only for localhost)",
        ));
    }
    Ok(endpoint.trim_end_matches('/').to_owned())
}

enum WorkerCommand {
    Submit(u32),
    Refresh,
}

enum WorkerEvent {
    Snapshot(ApiSnapshot),
    Failed(String),
}

#[derive(Serialize)]
struct Registration<'a> {
    username: &'a str,
    credential: &'a str,
}

#[derive(Serialize)]
struct ScoreSubmission {
    score: u32,
}

#[derive(Debug, Deserialize)]
struct ApiSnapshot {
    player: ApiPlayer,
    leaderboard: Vec<LeaderboardEntry>,
}

#[derive(Debug, Deserialize)]
struct ApiPlayer {
    username: String,
    score: u32,
    rank: Option<u32>,
}

#[derive(Deserialize)]
struct ApiFailure {
    error: String,
}

fn leaderboard_worker(
    endpoint: String,
    username: String,
    credential: String,
    pending_score: u32,
    commands: Receiver<WorkerCommand>,
    events: Sender<WorkerEvent>,
) {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut pending_score = pending_score;
    let mut registered = synchronize(
        &agent,
        &endpoint,
        &username,
        &credential,
        &mut pending_score,
        &events,
    );

    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::Refresh => {
                registered = synchronize(
                    &agent,
                    &endpoint,
                    &username,
                    &credential,
                    &mut pending_score,
                    &events,
                );
            }
            WorkerCommand::Submit(score) => {
                pending_score = pending_score.max(score);
                if !registered {
                    registered = synchronize(
                        &agent,
                        &endpoint,
                        &username,
                        &credential,
                        &mut pending_score,
                        &events,
                    );
                    continue;
                }

                match submit_score(&agent, &endpoint, &username, &credential, pending_score) {
                    Ok(snapshot) => {
                        pending_score = 0;
                        let _ = events.send(WorkerEvent::Snapshot(snapshot));
                    }
                    Err(error) => {
                        let _ = events.send(WorkerEvent::Failed(error));
                    }
                }
            }
        }
    }
}

fn synchronize(
    agent: &ureq::Agent,
    endpoint: &str,
    username: &str,
    credential: &str,
    pending_score: &mut u32,
    events: &Sender<WorkerEvent>,
) -> bool {
    match register(agent, endpoint, username, credential) {
        Ok(snapshot) => {
            let _ = events.send(WorkerEvent::Snapshot(snapshot));
        }
        Err(error) => {
            let _ = events.send(WorkerEvent::Failed(error));
            return false;
        }
    }

    if *pending_score > 0 {
        match submit_score(agent, endpoint, username, credential, *pending_score) {
            Ok(snapshot) => {
                *pending_score = 0;
                let _ = events.send(WorkerEvent::Snapshot(snapshot));
            }
            Err(error) => {
                let _ = events.send(WorkerEvent::Failed(error));
            }
        }
    }
    true
}

fn register(
    agent: &ureq::Agent,
    endpoint: &str,
    username: &str,
    credential: &str,
) -> Result<ApiSnapshot, String> {
    let result = agent
        .post(format!("{endpoint}/v1/players"))
        .header("User-Agent", concat!("dbird/", env!("CARGO_PKG_VERSION")))
        .send_json(Registration {
            username,
            credential,
        });
    decode_response(result).and_then(|snapshot| validate_snapshot(snapshot, username))
}

fn submit_score(
    agent: &ureq::Agent,
    endpoint: &str,
    username: &str,
    credential: &str,
    score: u32,
) -> Result<ApiSnapshot, String> {
    let result = agent
        .put(format!("{endpoint}/v1/players/{username}/score"))
        .header("User-Agent", concat!("dbird/", env!("CARGO_PKG_VERSION")))
        .header("Authorization", &format!("Bearer {credential}"))
        .send_json(ScoreSubmission { score });
    decode_response(result).and_then(|snapshot| validate_snapshot(snapshot, username))
}

fn decode_response(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ApiSnapshot, String> {
    let mut response = response.map_err(|error| format!("leaderboard request failed: {error}"))?;
    let status = response.status().as_u16();
    if response.status().is_success() {
        return response
            .body_mut()
            .with_config()
            .limit(RESPONSE_LIMIT)
            .read_json::<ApiSnapshot>()
            .map_err(|error| format!("leaderboard returned invalid data: {error}"));
    }

    let message = response
        .body_mut()
        .with_config()
        .limit(RESPONSE_LIMIT)
        .read_json::<ApiFailure>()
        .map(|failure| failure.error)
        .unwrap_or_else(|_| format!("leaderboard returned HTTP {status}"));
    Err(message)
}

fn validate_snapshot(
    snapshot: ApiSnapshot,
    expected_username: &str,
) -> Result<ApiSnapshot, String> {
    let player = Username::parse(&snapshot.player.username)
        .map_err(|_| "leaderboard returned an invalid player name".to_owned())?;
    if player.key != expected_username.to_ascii_lowercase()
        || snapshot.leaderboard.len() > 10
        || snapshot.player.rank == Some(0)
    {
        return Err("leaderboard returned an inconsistent player snapshot".into());
    }
    for (index, entry) in snapshot.leaderboard.iter().enumerate() {
        if Username::parse(&entry.username).is_err()
            || entry.rank != u32::try_from(index + 1).unwrap_or(u32::MAX)
        {
            return Err("leaderboard returned invalid ranking data".into());
        }
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "dbird-online-test-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn usernames_are_case_insensitive_and_filesystem_safe() {
        let username = Username::parse("Bird_Player-7").expect("valid username");
        assert_eq!(username.display, "Bird_Player-7");
        assert_eq!(username.key, "bird_player-7");

        for invalid in ["ab", "way-too-long-for-dbird", "_bird", "bird name", "bïrd"] {
            assert!(Username::parse(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn generated_credentials_are_strong_lowercase_hex() {
        let first = generate_credential().expect("generate credential");
        let second = generate_credential().expect("generate credential");
        assert!(valid_credential(&first));
        assert!(valid_credential(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn profile_creation_and_reload_preserve_identity_and_pending_score() {
        let directory = TestDirectory::new();
        let path = directory.0.join("player.json");
        let username = Username::parse("PlayerOne").expect("valid username");
        let mut first = StoredProfile::load_or_create(&path, &username).expect("create profile");
        first.cached_high_score = 12;
        first.pending_high_score = 18;
        first.save(&path).expect("save profile");

        let loaded = StoredProfile::load_or_create(&path, &username).expect("reload profile");
        assert_eq!(loaded.username, "PlayerOne");
        assert_eq!(loaded.credential, first.credential);
        assert_eq!(loaded.best(), 18);
    }

    #[test]
    fn submitting_persists_the_retry_queue_before_the_network_finishes() {
        let directory = TestDirectory::new();
        let path = directory.0.join("player.json");
        let username = Username::parse("QueueBird").expect("valid username");
        let mut session =
            OnlineSession::connect_at(username.clone(), "http://127.0.0.1:9".into(), path.clone())
                .expect("create online session");

        session.submit(27).expect("queue score");

        let stored = StoredProfile::load_or_create(&path, &username).expect("reload profile");
        assert_eq!(stored.cached_high_score, 0);
        assert_eq!(stored.pending_high_score, 27);
        assert_eq!(session.high_score(), 27);
        assert_eq!(session.view().status, SyncStatus::Queued);
    }

    #[test]
    fn malformed_existing_profile_is_not_replaced_with_a_new_identity() {
        let directory = TestDirectory::new();
        let path = directory.0.join("player.json");
        fs::write(&path, b"not json").expect("write malformed profile");
        let username = Username::parse("PlayerOne").expect("valid username");

        let error = StoredProfile::load_or_create(&path, &username)
            .expect_err("malformed profile should fail");
        assert!(error.to_string().contains("malformed"));
        assert_eq!(fs::read(&path).expect("profile remains"), b"not json");
    }

    #[test]
    fn endpoints_require_https_except_for_local_development() {
        assert_eq!(
            validate_endpoint("https://scores.example.com/".into()).expect("https endpoint"),
            "https://scores.example.com"
        );
        assert!(validate_endpoint("http://localhost:8787".into()).is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8787".into()).is_ok());
        assert!(validate_endpoint("http://scores.example.com".into()).is_err());
        assert!(validate_endpoint("http://localhost:80@evil.example".into()).is_err());
        assert!(validate_endpoint("https://scores.example.com?a=b".into()).is_err());
    }

    #[test]
    fn api_snapshot_contract_deserializes() {
        let snapshot: ApiSnapshot = serde_json::from_str(
            r#"{
                "player":{"username":"Bird","score":42,"rank":2},
                "leaderboard":[{"rank":1,"username":"Ace","score":50}]
            }"#,
        )
        .expect("deserialize snapshot");

        assert_eq!(snapshot.player.score, 42);
        assert_eq!(snapshot.player.rank, Some(2));
        assert_eq!(snapshot.leaderboard[0].username, "Ace");
        assert!(validate_snapshot(snapshot, "bird").is_ok());
    }

    #[test]
    fn api_snapshot_rejects_the_wrong_player_and_nonsequential_ranks() {
        let wrong_player = ApiSnapshot {
            player: ApiPlayer {
                username: "OtherBird".into(),
                score: 10,
                rank: Some(1),
            },
            leaderboard: Vec::new(),
        };
        assert!(validate_snapshot(wrong_player, "LocalBird").is_err());

        let wrong_rank = ApiSnapshot {
            player: ApiPlayer {
                username: "LocalBird".into(),
                score: 10,
                rank: Some(1),
            },
            leaderboard: vec![LeaderboardEntry {
                rank: 2,
                username: "OtherBird".into(),
                score: 20,
            }],
        };
        assert!(validate_snapshot(wrong_rank, "LocalBird").is_err());
    }
}
