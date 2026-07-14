# dbird

A fully playable terminal recreation of the classic Flappy Bird game.

## Play

You need Rust 1.88 or newer and an interactive terminal. The minimum window is 56x20; 80x24 or larger is recommended.

```sh
cd ~/Developer/dbird
cargo run --release
```

### Controls

| Key | Action |
| --- | --- |
| `Space`, `↑`, `W`, or `K` | Flap / start / resume |
| `P` | Pause or resume |
| `R` | Restart the round |
| `Q`, `Esc`, or `Ctrl-C` | Quit |

Pass through each pipe opening to score. The flight speed rises gradually as the score climbs. If the terminal becomes too small, the round is safely suspended; enlarge it and resume when ready.

## Options

```text
--ascii          use plain ASCII graphics
--no-color       disable the color palette
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

## Install locally

To make `dbird` available on your shell path:

```sh
cargo install --path .
dbird
```

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The physics and seeded pipe generation are independent from terminal rendering, so gameplay behavior can be tested without driving an interactive terminal.
