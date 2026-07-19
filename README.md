# dbird

A fully playable terminal recreation of the classic Flappy Bird game.

## Quick start

### Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer — only for building from source
- An interactive terminal

The minimum terminal size is 36 columns by 20 rows. A window at least 36 rows tall is recommended for the fuller portrait view.

### Download a prebuilt binary

Every release ships executables for macOS (Apple Silicon and Intel), Linux (x86_64 and arm64), and Windows (x86_64). Grab the archive for your platform from the [latest release](https://github.com/leduftw/dbird/releases/latest), extract it, and run `dbird` from a terminal — no Rust toolchain required. On Linux, audio uses ALSA (`libasound2`), which desktop distributions ship by default. If macOS Gatekeeper blocks a browser-downloaded binary, clear the quarantine flag with `xattr -d com.apple.quarantine dbird`.

### Run from source

```sh
git clone https://github.com/leduftw/dbird.git
cd dbird
cargo run --release
```

### Install

Install with a package manager:

```sh
cargo install dbird                 # crates.io, any platform with Rust
brew install leduftw/tap/dbird      # Homebrew on macOS or Linux
winget install leduftw.dbird        # Windows (pending winget-pkgs approval)
```

Or install the latest development version directly from GitHub:

```sh
cargo install --locked --git https://github.com/leduftw/dbird.git
dbird
```

## Play

### Controls

| Key | Action |
| --- | --- |
| `Enter` | Start / retry |
| `Space`, `↑`, `W`, `K`, or `Left Click` | Flap during flight |
| `P` | Pause or resume |
| `T` | Cycle System / Light / Dark theme |
| `L` | Show or refresh the global leaderboard in online mode |
| `Q`, `Esc`, or `Ctrl-C` | Quit |

Pass through each pipe opening to score. Pipe speed, pipe spacing, opening size, and flight physics stay fixed for the entire run, just like the original game. Scores of 10, 20, 30, and 40 award Bronze, Silver, Gold, and Platinum medals respectively. If the terminal becomes too small, the round is safely suspended; enlarge it and resume when ready.

Retro-style sound effects accompany flaps, points, collisions, and result transitions. They were synthesized from scratch for dbird by [`assets/sounds/generate.py`](assets/sounds/generate.py); provenance details are in [`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md).

The theme starts in System mode on every launch. On macOS and Windows it follows the current light or dark app appearance; press `T` to choose a daytime Light world, a nighttime Dark world, or return to System. The selection lasts for the current run, and `--no-color` remains the final override for terminal colors.

### Offline and online play

Offline is the default and makes no network requests:

```sh
dbird
```

Online play is an explicit opt-in. Choose a public username with 3-16 ASCII letters, numbers, `_`, or `-`:

```sh
dbird --online BirdPlayer
```

The two modes keep independent best scores. Offline mode continues to use the local high-score file. Online mode uses the cloud score associated with that username; press `L` to see the global top ten and your rank. A private installation credential, the last confirmed cloud score, and any score awaiting retry are kept under dbird's local state directory. If the service is unreachable, gameplay continues and a new best is queued for the next online run.

Connectivity never selects a mode automatically. Internet availability can change during a run, and sending a public username and score should remain a deliberate choice.

The official leaderboard is hosted at [`dbird-leaderboard.leduftw.workers.dev`](https://dbird-leaderboard.leduftw.workers.dev/v1/leaderboard) and is configured automatically in normal builds. The Cloudflare Workers + D1 service lives in [`leaderboard/`](https://github.com/leduftw/dbird/tree/main/leaderboard). Set `DBIRD_LEADERBOARD_URL` only to override the official service for local development or staging:

```sh
DBIRD_LEADERBOARD_URL=http://127.0.0.1:8787 \
  cargo run --release -- --online BirdPlayer
```

See the [leaderboard deployment guide](https://github.com/leduftw/dbird/blob/main/leaderboard/README.md) for local development, deployment, cost notes, and the API's security boundary.

## Options

```text
--ascii          use plain ASCII graphics
--no-color       disable the color palette
--mute           disable sound effects
--seed <NUMBER>  play the same deterministic course every round
--online <NAME>  opt in to the global leaderboard as this username
--reset-score    clear the saved offline high score before starting
-h, --help       show help
-V, --version    show the version
```

High scores are stored outside the repository. On macOS the default is:

```text
~/Library/Application Support/dbird/high-score.json
```

`$XDG_STATE_HOME` takes precedence when it is set. A missing or malformed score file never prevents the game from starting.

## Develop

After cloning the repository, run the complete local check suite with:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The leaderboard service has its own checks:

```sh
cd leaderboard
npm ci
npm test
npx wrangler d1 migrations apply DB --local
npx wrangler deploy --dry-run
```

The physics and seeded pipe generation are independent from terminal rendering, so gameplay behavior can be tested without driving an interactive terminal.

## License

dbird is released under the [MIT License](LICENSE). The bundled sound effects are original works covered by the same terms; see [`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md).
