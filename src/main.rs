use std::collections::VecDeque;
use std::error::Error;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use dbird::audio::{Audio, Sound};
use dbird::cli::{self, CliCommand, CliOptions};
use dbird::game::{DeathCause, Game, Phase};
use dbird::signals::ShutdownSignals;
use dbird::storage::HighScoreStore;
use dbird::terminal::{TerminalSession, install_panic_hook};
use dbird::theme::ThemeState;
use dbird::ui::{self, UiOptions};
use ratatui::layout::Rect;

// Preserve the original 60 Hz mechanics while sampling motion at 120 Hz.
const PHYSICS_STEP: Duration = Duration::from_nanos(16_666_667);
const RENDER_FRAME_TIME: Duration = Duration::from_nanos(8_333_333);
const MAX_FRAME_DELTA: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyAction {
    None,
    Quit,
    Flap,
    Start,
    CycleTheme,
}

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
    let mut audio = Audio::new(!options.mute);
    let mut terminal = TerminalSession::enter()?;
    let initial_area: Rect = terminal.terminal_mut().size()?.into();
    let (field_width, field_height) = ui::field_size(initial_area);
    let seed = options.seed.unwrap_or_else(random_seed);
    let mut game = Game::new(field_width, field_height, seed);
    let mut ui_options = UiOptions {
        ascii: options.ascii,
        color: !options.no_color,
        theme: ThemeState::default(),
    };

    let mut last_loop = Instant::now();
    let mut accumulator = Duration::ZERO;
    let mut was_too_small = !ui::fits(initial_area, &game);
    let mut new_best = false;
    let mut unsaved_best = false;
    let mut save_attempted = false;
    let mut save_warning = None;
    let mut should_quit = false;
    let mut pending_sounds = VecDeque::new();

    while !should_quit {
        if shutdown.requested() {
            break;
        }

        let frame_started = Instant::now();
        while pending_sounds
            .front()
            .is_some_and(|(play_at, _)| *play_at <= frame_started)
        {
            if let Some((_, sound)) = pending_sounds.pop_front() {
                audio.play(sound);
            }
        }
        let frame_delta = frame_started
            .saturating_duration_since(last_loop)
            .min(MAX_FRAME_DELTA);
        last_loop = frame_started;

        let area: Rect = terminal.terminal_mut().size()?.into();
        let fits = ui::fits(area, &game);
        let score_before_update = game.score;
        let phase_before_update = game.phase;
        if !fits {
            was_too_small = true;
            accumulator = Duration::ZERO;
        } else {
            if was_too_small {
                game.pause();
                was_too_small = false;
                accumulator = Duration::ZERO;
            }

            if matches!(game.phase, Phase::Playing | Phase::Dying) {
                accumulator = accumulator.saturating_add(frame_delta);
                while accumulator >= PHYSICS_STEP {
                    game.update(PHYSICS_STEP.as_secs_f64());
                    accumulator -= PHYSICS_STEP;
                }
            } else {
                accumulator = Duration::ZERO;
            }
        }

        if game.score > score_before_update {
            audio.play(Sound::Point);
        }
        if phase_before_update == Phase::Playing && game.phase == Phase::Dying {
            audio.play(Sound::Hit);
            if game.death_cause() == Some(DeathCause::Pipe) {
                pending_sounds.push_back((frame_started + Duration::from_millis(500), Sound::Die));
            }
        }
        if phase_before_update == Phase::Dying && game.phase == Phase::GameOver {
            audio.play(Sound::Swoosh);
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

        let tick_progress = accumulator.as_secs_f64() / PHYSICS_STEP.as_secs_f64();
        terminal.terminal_mut().draw(|frame| {
            ui::draw_interpolated(
                frame,
                &game,
                high_score,
                new_best,
                ui_options,
                tick_progress,
            );
        })?;

        let poll_timeout = RENDER_FRAME_TIME.saturating_sub(frame_started.elapsed());
        if event::poll(poll_timeout)? {
            loop {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        match handle_key(key, area, fits, &mut game, &mut new_best) {
                            KeyAction::Quit => {
                                should_quit = true;
                                break;
                            }
                            KeyAction::Flap => audio.play(Sound::Wing),
                            KeyAction::Start => {
                                pending_sounds.clear();
                                audio.play(Sound::Wing);
                            }
                            KeyAction::CycleTheme => ui_options.cycle_theme(),
                            KeyAction::None => {}
                        }
                    }
                    Event::Mouse(mouse)
                        if handle_mouse(mouse, fits, &mut game) == KeyAction::Flap =>
                    {
                        audio.play(Sound::Wing);
                    }
                    _ => {}
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
) -> KeyAction {
    if key.code == KeyCode::Esc
        || matches!(key.code, KeyCode::Char('q' | 'Q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    {
        return KeyAction::Quit;
    }

    if key.code == KeyCode::Enter && key.kind == KeyEventKind::Press {
        if matches!(game.phase, Phase::Ready | Phase::GameOver)
            && ui::can_start_round(terminal_area)
        {
            if game.phase == Phase::GameOver || !terminal_fits {
                let (width, height) = ui::field_size(terminal_area);
                game.restart(width, height);
                *new_best = false;
            }
            if game.start() {
                return KeyAction::Start;
            }
        }
        return KeyAction::None;
    }

    if let KeyCode::Char(character) = key.code
        && character.eq_ignore_ascii_case(&'t')
        && key.kind == KeyEventKind::Press
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return KeyAction::CycleTheme;
    }

    if !terminal_fits {
        return KeyAction::None;
    }

    match key.code {
        KeyCode::Char(' ') | KeyCode::Up => {
            if game.flap() {
                return KeyAction::Flap;
            }
        }
        KeyCode::Char(character) if matches!(character.to_ascii_lowercase(), 'w' | 'k') => {
            if game.flap() {
                return KeyAction::Flap;
            }
        }
        KeyCode::Char(character)
            if key.kind == KeyEventKind::Press && character.eq_ignore_ascii_case(&'p') =>
        {
            game.toggle_pause();
        }
        _ => {}
    }

    KeyAction::None
}

fn handle_mouse(mouse: MouseEvent, terminal_fits: bool, game: &mut Game) -> KeyAction {
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) && terminal_fits && game.flap() {
        return KeyAction::Flap;
    }
    KeyAction::None
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

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn left_click_flaps_only_during_flight() {
        let mut game = Game::new(80, 20, 7);
        let left_down = mouse(MouseEventKind::Down(MouseButton::Left));

        assert_eq!(handle_mouse(left_down, true, &mut game), KeyAction::None);

        assert!(game.start());
        assert_eq!(handle_mouse(left_down, true, &mut game), KeyAction::Flap);
    }

    #[test]
    fn other_mouse_events_and_small_terminals_do_not_flap() {
        let mut game = Game::new(80, 20, 7);
        assert!(game.start());

        assert_eq!(
            handle_mouse(
                mouse(MouseEventKind::Down(MouseButton::Left)),
                false,
                &mut game
            ),
            KeyAction::None
        );
        assert_eq!(
            handle_mouse(
                mouse(MouseEventKind::Down(MouseButton::Right)),
                true,
                &mut game
            ),
            KeyAction::None
        );
        assert_eq!(
            handle_mouse(
                mouse(MouseEventKind::Up(MouseButton::Left)),
                true,
                &mut game
            ),
            KeyAction::None
        );
        assert_eq!(
            handle_mouse(mouse(MouseEventKind::Moved), true, &mut game),
            KeyAction::None
        );
    }

    #[test]
    fn quit_keys_exit_even_when_the_terminal_is_too_small() {
        let mut game = Game::new(80, 20, 7);
        let mut new_best = false;
        let area = Rect::new(0, 0, 20, 10);

        assert_eq!(
            handle_key(key(KeyCode::Esc), area, false, &mut game, &mut new_best),
            KeyAction::Quit
        );
        assert_eq!(
            handle_key(
                key(KeyCode::Char('q')),
                area,
                false,
                &mut game,
                &mut new_best
            ),
            KeyAction::Quit
        );
    }

    #[test]
    fn only_enter_starts_and_pause_key_toggles() {
        let mut game = Game::new(80, 20, 7);
        let mut new_best = false;
        let area = Rect::new(0, 0, 82, 26);

        handle_key(
            key(KeyCode::Char(' ')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        handle_key(key(KeyCode::Up), area, true, &mut game, &mut new_best);
        handle_key(
            key(KeyCode::Char('r')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        assert_eq!(game.phase, Phase::Ready);

        assert_eq!(
            handle_key(key(KeyCode::Enter), area, true, &mut game, &mut new_best),
            KeyAction::Start
        );
        assert_eq!(game.phase, Phase::Playing);

        handle_key(
            key(KeyCode::Char('p')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        assert_eq!(game.phase, Phase::Paused);

        handle_key(
            key(KeyCode::Char(' ')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        assert_eq!(game.phase, Phase::Paused);

        handle_key(
            key(KeyCode::Char('p')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        assert_eq!(game.phase, Phase::Playing);
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
    fn theme_key_works_in_every_phase_and_even_in_the_resize_screen() {
        for (phase, terminal_fits) in [
            (Phase::Ready, true),
            (Phase::Playing, true),
            (Phase::Paused, true),
            (Phase::Dying, true),
            (Phase::GameOver, true),
            (Phase::Ready, false),
        ] {
            let mut game = Game::new(80, 20, 7);
            game.phase = phase;
            game.score = 12;
            let mut new_best = true;
            let before = (game.phase, game.score, game.bird_y, game.bird_velocity);

            assert_eq!(
                handle_key(
                    key(KeyCode::Char('t')),
                    Rect::new(0, 0, 82, 26),
                    terminal_fits,
                    &mut game,
                    &mut new_best,
                ),
                KeyAction::CycleTheme
            );
            assert_eq!(
                (game.phase, game.score, game.bird_y, game.bird_velocity),
                before
            );
            assert!(new_best);

            assert_eq!(
                handle_key(
                    repeated_key(KeyCode::Char('t')),
                    Rect::new(0, 0, 82, 26),
                    terminal_fits,
                    &mut game,
                    &mut new_best,
                ),
                KeyAction::None
            );
        }

        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            let mut game = Game::new(80, 20, 7);
            let mut new_best = false;
            assert_eq!(
                handle_key(
                    KeyEvent::new(KeyCode::Char('t'), modifiers),
                    Rect::new(0, 0, 82, 26),
                    true,
                    &mut game,
                    &mut new_best,
                ),
                KeyAction::None
            );
        }

        let mut game = Game::new(80, 20, 7);
        let mut new_best = false;
        assert_eq!(
            handle_key(
                KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT),
                Rect::new(0, 0, 82, 26),
                true,
                &mut game,
                &mut new_best,
            ),
            KeyAction::CycleTheme
        );
    }

    #[test]
    fn enter_retry_can_adopt_a_smaller_but_still_playable_terminal() {
        let mut game = Game::new(100, 30, 7);
        game.phase = Phase::GameOver;
        let mut new_best = true;

        handle_key(
            key(KeyCode::Enter),
            Rect::new(0, 0, 80, 24),
            false,
            &mut game,
            &mut new_best,
        );

        assert_eq!((game.width, game.height), (20, 18));
        assert_eq!(game.phase, Phase::Playing);
        assert!(!new_best);
    }

    #[test]
    fn repeated_space_and_enter_cannot_restart_a_dead_round() {
        let mut game = Game::new(80, 20, 7);
        game.phase = Phase::GameOver;
        let original_pipes = game.pipes.clone();
        let mut new_best = true;
        let area = Rect::new(0, 0, 82, 26);

        handle_key(
            repeated_key(KeyCode::Char(' ')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        handle_key(
            repeated_key(KeyCode::Enter),
            area,
            true,
            &mut game,
            &mut new_best,
        );

        assert_eq!(game.phase, Phase::GameOver);
        assert_eq!(game.pipes, original_pipes);
        assert!(new_best);
    }

    #[test]
    fn enter_press_retries_and_r_never_restarts() {
        let mut game = Game::new(80, 20, 7);
        game.phase = Phase::GameOver;
        game.score = 12;
        let mut new_best = true;
        let area = Rect::new(0, 0, 82, 26);

        handle_key(
            key(KeyCode::Char('r')),
            area,
            true,
            &mut game,
            &mut new_best,
        );
        assert_eq!(game.phase, Phase::GameOver);
        assert_eq!(game.score, 12);

        handle_key(key(KeyCode::Enter), area, true, &mut game, &mut new_best);
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.score, 0);
        assert!(!new_best);
    }

    #[test]
    fn offscreen_flaps_are_ignored_without_a_wing_action() {
        let mut game = Game::new(80, 20, 7);
        game.start();
        game.bird_y = -1.0;
        game.bird_velocity = 2.0;
        let mut new_best = false;

        assert_eq!(
            handle_key(
                key(KeyCode::Char(' ')),
                Rect::new(0, 0, 82, 26),
                true,
                &mut game,
                &mut new_best,
            ),
            KeyAction::None
        );
        assert_eq!(game.bird_velocity, 2.0);
    }

    #[test]
    fn no_gameplay_key_can_skip_the_dying_animation() {
        let mut game = Game::new(80, 20, 7);
        game.phase = Phase::Dying;
        let mut new_best = false;
        let area = Rect::new(0, 0, 82, 26);

        for code in [
            KeyCode::Enter,
            KeyCode::Char(' '),
            KeyCode::Char('p'),
            KeyCode::Char('r'),
        ] {
            assert_eq!(
                handle_key(key(code), area, true, &mut game, &mut new_best),
                KeyAction::None
            );
            assert_eq!(game.phase, Phase::Dying);
        }
    }
}
