# dbird

A fully playable terminal recreation of Flappy Bird in Rust (ratatui, crossterm,
rodio), plus an opt-in global leaderboard: a Cloudflare Worker with D1 in
`leaderboard/`.

## Commands

```sh
cargo run --release                     # play
cargo run --release -- --online Name    # play against the global leaderboard
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
```

The leaderboard has its own checks, run from `leaderboard/`:

```sh
npm ci && npm test
npx wrangler d1 migrations apply DB --local
npx wrangler deploy --dry-run
```

Deploying the leaderboard is still manual (`npm run deploy` after `npx wrangler login`).

## Layout

- `src/game.rs` holds the physics and seeded pipe generation, independent of the
  terminal, so gameplay is tested without driving one. `src/ui.rs` and
  `src/terminal.rs` draw it; `src/audio.rs` plays the sounds.
- `src/online.rs` talks to the leaderboard. Its address is built in, and the
  `DBIRD_LEADERBOARD_URL` variable overrides it at run time or at build time.
- `src/storage.rs` keeps scores and online state outside the repository.
- `assets/sounds/generate.py` synthesizes every sound effect; there are no
  third-party samples.

<!-- playbook:begin -->
## How work lands here

> Managed by [leduftw/playbook](https://github.com/leduftw/playbook) v1. Change it there, not here; `playbook sync` brings this section back in line.

The facts about this repo live in `.github/playbook.toml`. Everything in this section is the same in every repo that follows the playbook.

### Every change

1. **Start from an issue.** Reuse the issue that describes the change, or create one assigned to `leduftw` with at least one label.
2. **Work in your own worktree.** Run `playbook start <issue>`: it creates `~/Developer/.worktrees/dbird/<issue>-<slug>` on branch `dev/leduftw/<issue>-<slug>`, based on the latest `main`. Work only there. Another session may be working at the same time, so never edit the primary checkout; it stays on `main` and only moves with `git pull`.
3. **Commit and push** each finished slice to that branch, without pausing for confirmation.
4. **Open the PR** with `gh pr create --base main`. Its title becomes the commit on `main`, so it follows the commit style below. Put `Closes #<issue>` in the body when the PR fully resolves the issue.
5. **Land it with `playbook finish`.** It waits for the required checks, merges `main` into the branch if `main` has moved on, squash-merges, removes the worktree and the branch, pulls `main`, and confirms the issue is closed. If a check fails, fix it on the branch and run `playbook finish` again.

Sessions that only read, plan or answer questions need no issue and no worktree.

**Never** commit or push to `main` (hooks and a GitHub ruleset refuse it), rebase a branch that's already pushed, or force-push. To catch up with `main`, merge it into your branch.

**Commit style:** start with a lowercase verb that says what the change does, then plain words, with no `feat:`-style prefix and no full stop. Names keep their capitals: `fix Windows installer replacement`, `add opt-in global leaderboard`. The `playbook / title` check rejects PR titles that break this.

**Without the `playbook` command** (a cloud session, a fresh machine), do the same by hand:

- start: `git fetch origin`, then `git worktree add -b dev/leduftw/<issue>-<slug> ~/Developer/.worktrees/dbird/<issue>-<slug> origin/main` (a cloud session is already isolated, so a branch from `origin/main` is enough)
- finish: `gh pr checks --watch --required`, then `gh pr merge --squash`; once the PR shows `MERGED`, `git worktree remove <path>`, `git branch -D <branch>` (`-d` refuses, because squash commits aren't ancestors of `main`) and `git pull --ff-only` in the primary checkout

### Releasing

This repo is published: other people install it.

- `playbook release patch|minor|major` opens a PR that only bumps the version; nobody types a version number. The first release is always 1.0.0, and every later one is exactly the next patch, minor or major.
- That PR builds and smoke-tests every release file on all six platforms. Landing it with `playbook finish` tags the release, publishes it on GitHub Releases and updates the Homebrew tap, WinGet, the one-line installers and crates.io.
- A published release is locked. A broken one is never fixed in place; ship the next patch.
<!-- playbook:end -->
