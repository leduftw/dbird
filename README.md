# dbird

A fully playable terminal recreation of the classic Flappy Bird game.

## Quick start

### Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer
- An interactive terminal

The minimum terminal size is 36 columns by 20 rows. A window at least 36 rows tall is recommended for the fuller portrait view.

### Run from source

```sh
git clone https://github.com/leduftw/dbird.git
cd dbird
cargo run --release
```

### Install

Install the latest version directly from GitHub to make `dbird` available on your shell path:

```sh
cargo install --locked --git https://github.com/leduftw/dbird.git
dbird
```

## Play

### Controls

| Key | Action |
| --- | --- |
| `Enter` | Start / retry |
| `Space`, `↑`, `W`, or `K` | Flap during flight |
| `P` | Pause or resume |
| `T` | Cycle System / Light / Dark theme |
| `Q`, `Esc`, or `Ctrl-C` | Quit |

Pass through each pipe opening to score. Pipe speed, pipe spacing, opening size, and flight physics stay fixed for the entire run, just like the original game. Scores of 10, 20, 30, and 40 award Bronze, Silver, Gold, and Platinum medals respectively. If the terminal becomes too small, the round is safely suspended; enlarge it and resume when ready.

Retro-style sound effects accompany flaps, points, collisions, and result transitions. They were synthesized from scratch for dbird by [`assets/sounds/generate.py`](assets/sounds/generate.py); provenance details are in [`assets/sounds/NOTICE.md`](assets/sounds/NOTICE.md).

The theme starts in System mode on every launch. On macOS and Windows it follows the current light or dark app appearance; press `T` to choose a daytime Light world, a nighttime Dark world, or return to System. The selection lasts for the current run, and `--no-color` remains the final override for terminal colors.

## Options

```text
--ascii          use plain ASCII graphics
--no-color       disable the color palette
--mute           disable sound effects
--seed <NUMBER>  use deterministic pipe placement
--reset-score    clear the saved high score before starting
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

The physics and seeded pipe generation are independent from terminal rendering, so gameplay behavior can be tested without driving an interactive terminal.
