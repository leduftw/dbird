use std::error::Error;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use dbird::cli::{self, CliCommand, CliOptions};
use dbird::game::{Game, Phase};
use dbird::signals::ShutdownSignals;
use dbird::storage::HighScoreStore;
use dbird::terminal::{TerminalSession, install_panic_hook};
use dbird::ui::{self, UiOptions};
use ratatui::layout::Rect;

const PHYSICS_STEP: Duration = Duration::from_nanos(8_333_333);
const FRAME_TIME: Duration = Duration::from_nanos(16_666_667);
const MAX_FRAME_DELTA: Duration = Duration::from_millis(100);

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dbird: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    match cli::parse_env()? {
        CliCommand::Help => write_stdout(cli::HELP_TEXT),
        CliCommand::Version => write_stdout(&format!("{}\n", cli::version_text())),
        CliCommand::Run(options) => run_game(options),
    }
}

fn run_game(options: CliOptions) -> Result<(), Box<dyn Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other("an interactive terminal is required").into());
    }

    let score_store = HighScoreStore::new();
    if options.reset_score {
        score_store.reset()?;
    }
    let mut high_score = score_store.load().min(u64::from(u32::MAX)) as u32;

    let shutdown = ShutdownSignals::install()?;
    install_panic_hook();
    let mut terminal = TerminalSession::enter()?;
    let initial_area: Rect = terminal.terminal_mut().size()?.into();
    let (field_width, field_height) = ui::field_size(initial_area);
    let seed = options.seed.unwrap_or_else(random_seed);
    let mut game = Game::new(field_width, field_height, seed);
    let ui_options = UiOptions {
        ascii: options.ascii,
        color: !options.no_color,
    };

    let mut last_loop = Instant::now();
    let mut accumulator = Duration::ZERO;
    let mut was_too_small = !ui::fits(initial_area, &game);
    let mut new_best = false;
    let mut unsaved_best = false;
    let mut save_attempted = false;
    let mut save_warning = None;
    let mut should_quit = false;

    while !should_quit {
        if shutdown.requested() {
            break;
        }

        let frame_started = Instant::now();
        let frame_delta = frame_started
            .saturating_duration_since(last_loop)
            .min(MAX_FRAME_DELTA);
        last_loop = frame_started;

        let area: Rect = terminal.terminal_mut().size()?.into();
        let fits = ui::fits(area, &game);
        if !fits {
            was_too_small = true;
            accumulator = Duration::ZERO;
        } else {
            if was_too_small {
                game.pause();
                was_too_small = false;
                accumulator = Duration::ZERO;
            }

            if game.phase == Phase::Playing {
                accumulator = accumulator.saturating_add(frame_delta);
                while accumulator >= PHYSICS_STEP {
                    game.update(PHYSICS_STEP.as_secs_f64());
                    accumulator -= PHYSICS_STEP;
                }
            } else {
                accumulator = Duration::ZERO;
            }
        }

        if game.score > high_score {
            high_score = game.score;
            new_best = true;
            unsaved_best = true;
            save_attempted = false;
        }

        if game.phase == Phase::GameOver && unsaved_best && !save_attempted {
            save_attempted = true;
            match score_store.save(u64::from(high_score)) {
                Ok(()) => unsaved_best = false,
                Err(error) => save_warning = Some(error),
            }
        }

        terminal.terminal_mut().draw(|frame| {
            ui::draw(frame, &game, high_score, new_best, ui_options);
        })?;

        let poll_timeout = FRAME_TIME.saturating_sub(frame_started.elapsed());
        if event::poll(poll_timeout)? {
            loop {
                if let Event::Key(key) = event::read()?
                    && key.kind != KeyEventKind::Release
                    && handle_key(key, area, fits, &mut game, &mut new_best)
                {
                    should_quit = true;
                    break;
                }

                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }

    if unsaved_best {
        match score_store.save(u64::from(high_score)) {
            Ok(()) => save_warning = None,
            Err(error) => save_warning = Some(error),
        }
    }

    terminal.restore()?;
    if let Some(error) = save_warning {
        eprintln!("dbird: high score could not be saved: {error}");
    }

    Ok(())
}

fn handle_key(
    key: KeyEvent,
    terminal_area: Rect,
    terminal_fits: bool,
    game: &mut Game,
    new_best: &mut bool,
) -> bool {
    if key.code == KeyCode::Esc
        || matches!(key.code, KeyCode::Char('q' | 'Q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return true;
    }

    if key.kind == KeyEventKind::Press
        && matches!(key.code, KeyCode::Char(character) if character.eq_ignore_ascii_case(&'r'))
    {
        if ui::can_start_round(terminal_area) {
            let (width, height) = ui::field_size(terminal_area);
            game.restart(width, height);
            *new_best = false;
        }
        return false;
    }

    if !terminal_fits {
        return false;
    }

    match key.code {
        KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Up => {
            flap_or_start(game, terminal_area, new_best);
        }
        KeyCode::Char(character) if matches!(character.to_ascii_lowercase(), 'w' | 'k') => {
            flap_or_start(game, terminal_area, new_best);
        }
        KeyCode::Char(character)
            if key.kind == KeyEventKind::Press && character.eq_ignore_ascii_case(&'p') =>
        {
            game.toggle_pause();
        }
        _ => {}
    }

    false
}

fn flap_or_start(game: &mut Game, terminal_area: Rect, new_best: &mut bool) {
    match game.phase {
        Phase::Ready | Phase::Playing => game.flap(),
        Phase::Paused => {
            game.resume();
            game.flap();
        }
        Phase::GameOver => {
            let (width, height) = ui::field_size(terminal_area);
            game.restart(width, height);
            *new_best = false;
            game.flap();
        }
    }
}

fn random_seed() -> u64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    timestamp ^ u64::from(std::process::id()).rotate_left(32)
}

fn write_stdout(text: &str) -> Result<(), Box<dyn Error>> {
    match io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn repeated_key(code: KeyCode) -> KeyEvent {
        let mut key = key(code);
        key.kind = KeyEventKind::Repeat;
        key
    }

    #[test]
    fn quit_keys_exit_even_when_the_terminal_is_too_small() {
        let mut game = Game::new(80, 20, 7);
        let mut new_best = false;
        let area = Rect::new(0, 0, 20, 10);

        assert!(handle_key(
            key(KeyCode::Esc),
            area,
            false,
            &mut game,
            &mut new_best
        ));
        assert!(handle_key(
            key(KeyCode::Char('q')),
            area,
            false,
            &mut game,
            &mut new_best
        ));
    }

    #[test]
    fn flap_starts_and_pause_key_toggles() {
        let mut game = Game::new(80, 20, 7);
        let mut new_best = false;
        let area = Rect::new(0, 0, 82, 26);

        assert!(!handle_key(
            key(KeyCode::Char(' ')),
            area,
            true,
            &mut game,
            &mut new_best
        ));
        assert_eq!(game.phase, Phase::Playing);

        handle_key(
            key(KeyCode::Char('p')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        assert_eq!(game.phase, Phase::Paused);
    }

    #[test]
    fn gameplay_keys_are_ignored_while_terminal_is_too_small() {
        let mut game = Game::new(80, 20, 7);
        let mut new_best = false;

        handle_key(
            key(KeyCode::Char(' ')),
            Rect::new(0, 0, 20, 10),
            false,
            &mut game,
            &mut new_best,
        );

        assert_eq!(game.phase, Phase::Ready);
    }

    #[test]
    fn restart_can_adopt_a_smaller_but_still_playable_terminal() {
        let mut game = Game::new(100, 30, 7);
        game.flap();
        let mut new_best = true;

        handle_key(
            key(KeyCode::Char('r')),
            Rect::new(0, 0, 80, 24),
            false,
            &mut game,
            &mut new_best,
        );

        assert_eq!((game.width, game.height), (78, 18));
        assert_eq!(game.phase, Phase::Ready);
        assert!(!new_best);
    }

    #[test]
    fn repeated_non_flap_keys_do_not_toggle_or_restart() {
        let mut game = Game::new(80, 20, 7);
        game.flap();
        let original_pipes = game.pipes.clone();
        let mut new_best = false;
        let area = Rect::new(0, 0, 82, 26);

        handle_key(
            repeated_key(KeyCode::Char('p')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        handle_key(
            repeated_key(KeyCode::Char('r')),
            area,
            true,
            &mut game,
            &mut new_best,
        );

        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.pipes, original_pipes);
    }
}
