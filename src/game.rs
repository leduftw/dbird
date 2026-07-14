//! Deterministic, fixed-step game simulation.
//!
//! The renderer is deliberately kept out of this module: positions are in
//! terminal cells and the game advances only in fixed simulation ticks.

/// Smallest supported play-field width, in terminal cells.
pub const MIN_FIELD_WIDTH: u16 = 54;
/// Smallest supported play-field height, in terminal cells.
pub const MIN_FIELD_HEIGHT: u16 = 14;
/// Largest supported play-field width, in terminal cells.
pub const MAX_FIELD_WIDTH: u16 = 100;
/// Largest supported play-field height, in terminal cells.
pub const MAX_FIELD_HEIGHT: u16 = 30;

/// Width of a pipe in terminal cells.
pub const PIPE_WIDTH: u16 = 5;
/// Width of the bird sprite in terminal cells.
pub const BIRD_WIDTH: u16 = 3;
/// Height of the bird sprite in terminal cells.
pub const BIRD_HEIGHT: u16 = 1;

const FIXED_STEP_SECONDS: f64 = 1.0 / 120.0;
const GRAVITY: f64 = 30.0;
const FLAP_VELOCITY: f64 = -10.5;
const MAX_FALL_VELOCITY: f64 = 16.0;
const BASE_PIPE_SPEED: f64 = 12.0;
const MAX_PIPE_SPEED: f64 = 18.0;
const SPEED_PER_LEVEL: f64 = 0.75;
const MAX_LEVEL: u32 = 10;
const SCORE_PER_LEVEL: u32 = 5;
const MIN_GAP_HEIGHT: u16 = 4;
const PIPE_SPACING: f64 = 30.0;
const FIRST_PIPE_DISTANCE: f64 = 30.0;
const VERTICAL_MARGIN: u16 = 2;
const MAX_GAP_SHIFT: u16 = 5;

/// The current state of a game round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    Playing,
    Paused,
    GameOver,
}

/// A single top-and-bottom pipe pair.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipe {
    /// Horizontal position of the pipe's leading (left) edge.
    pub x: f64,
    /// First row of the passable opening.
    pub gap_top: u16,
    /// Height of the passable opening.
    pub gap_height: u16,
    /// Whether this pipe has already awarded its point.
    pub scored: bool,
}

/// Complete simulation state for one game.
#[derive(Clone, Debug)]
pub struct Game {
    pub width: u16,
    pub height: u16,
    pub bird_x: f64,
    pub bird_y: f64,
    pub bird_velocity: f64,
    pub pipes: Vec<Pipe>,
    pub score: u32,
    pub phase: Phase,
    /// Time actually simulated during the current round.
    pub elapsed: f64,

    initial_seed: u64,
    round: u64,
    rng_state: u64,
    accumulator: f64,
}

impl Game {
    /// Construct a ready-to-play round. Unsupported dimensions are clamped.
    pub fn new(width: u16, height: u16, seed: u64) -> Self {
        let mut game = Self {
            width: width.clamp(MIN_FIELD_WIDTH, MAX_FIELD_WIDTH),
            height: height.clamp(MIN_FIELD_HEIGHT, MAX_FIELD_HEIGHT),
            bird_x: 0.0,
            bird_y: 0.0,
            bird_velocity: 0.0,
            pipes: Vec::new(),
            score: 0,
            phase: Phase::Ready,
            elapsed: 0.0,
            initial_seed: seed,
            round: 0,
            rng_state: seed,
            accumulator: 0.0,
        };
        game.reset_round();
        game
    }

    /// Flap the bird. The first flap also starts a ready round.
    pub fn flap(&mut self) {
        match self.phase {
            Phase::Ready => {
                self.phase = Phase::Playing;
                self.bird_velocity = FLAP_VELOCITY;
            }
            Phase::Playing => self.bird_velocity = FLAP_VELOCITY,
            Phase::Paused | Phase::GameOver => {}
        }
    }

    /// Advance the game by wall-clock seconds.
    ///
    /// Calls are accumulated and evaluated at 120 Hz, so game physics do not
    /// depend on the renderer's frame rate.
    pub fn update(&mut self, dt_secs: f64) {
        if self.phase != Phase::Playing || !dt_secs.is_finite() || dt_secs <= 0.0 {
            return;
        }

        self.accumulator += dt_secs;
        while self.accumulator + f64::EPSILON >= FIXED_STEP_SECONDS {
            self.accumulator -= FIXED_STEP_SECONDS;
            self.step();

            if self.phase != Phase::Playing {
                self.accumulator = 0.0;
                break;
            }
        }
    }

    /// Toggle between the playing and paused phases.
    pub fn toggle_pause(&mut self) {
        match self.phase {
            Phase::Playing => self.pause(),
            Phase::Paused => self.resume(),
            Phase::Ready | Phase::GameOver => {}
        }
    }

    /// Pause a running round.
    pub fn pause(&mut self) {
        if self.phase == Phase::Playing {
            self.phase = Phase::Paused;
        }
    }

    /// Resume a paused round.
    pub fn resume(&mut self) {
        if self.phase == Phase::Paused {
            self.phase = Phase::Playing;
        }
    }

    /// Start a clean ready round with the next deterministic course.
    pub fn restart(&mut self, width: u16, height: u16) {
        self.width = width.clamp(MIN_FIELD_WIDTH, MAX_FIELD_WIDTH);
        self.height = height.clamp(MIN_FIELD_HEIGHT, MAX_FIELD_HEIGHT);
        self.round = self.round.wrapping_add(1);
        self.reset_round();
    }

    /// Current difficulty level (1 through 10).
    pub fn level(&self) -> u32 {
        (self.score / SCORE_PER_LEVEL + 1).min(MAX_LEVEL)
    }

    /// Current horizontal pipe speed, in terminal cells per second.
    pub fn speed(&self) -> f64 {
        (BASE_PIPE_SPEED + f64::from(self.level() - 1) * SPEED_PER_LEVEL).min(MAX_PIPE_SPEED)
    }

    /// Opening height assigned to newly-created pipes at this difficulty.
    pub fn gap_height(&self) -> u16 {
        let initial = (self.height / 3).clamp(6, 10);
        let reduction = ((self.level() - 1) * 2 / 3) as u16;
        initial.saturating_sub(reduction).max(MIN_GAP_HEIGHT)
    }

    fn reset_round(&mut self) {
        self.bird_x = f64::from(self.width) * 0.25;
        self.bird_y = (f64::from(self.height) - f64::from(BIRD_HEIGHT)) * 0.5;
        self.bird_velocity = 0.0;
        self.pipes.clear();
        self.score = 0;
        self.phase = Phase::Ready;
        self.elapsed = 0.0;
        self.rng_state = self
            .initial_seed
            .wrapping_add(self.round.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        self.accumulator = 0.0;

        let mut x = self.bird_x + FIRST_PIPE_DISTANCE;
        let fill_until = f64::from(self.width) + PIPE_SPACING * 2.0;
        while x <= fill_until {
            self.push_pipe(x);
            x += PIPE_SPACING;
        }
    }

    fn step(&mut self) {
        self.elapsed += FIXED_STEP_SECONDS;
        self.bird_velocity =
            (self.bird_velocity + GRAVITY * FIXED_STEP_SECONDS).min(MAX_FALL_VELOCITY);
        self.bird_y += self.bird_velocity * FIXED_STEP_SECONDS;

        let pipe_delta = self.speed() * FIXED_STEP_SECONDS;
        for pipe in &mut self.pipes {
            pipe.x -= pipe_delta;
        }

        self.award_passed_pipes();
        self.remove_old_pipes();
        self.refill_pipes();

        if self.collides() {
            self.phase = Phase::GameOver;
        }
    }

    fn award_passed_pipes(&mut self) {
        let bird_left = self.bird_x.round() as i32;
        for pipe in &mut self.pipes {
            let pipe_right = pipe.x.round() as i32 + i32::from(PIPE_WIDTH);
            if !pipe.scored && pipe_right <= bird_left {
                pipe.scored = true;
                self.score = self.score.saturating_add(1);
            }
        }
    }

    fn remove_old_pipes(&mut self) {
        self.pipes
            .retain(|pipe| pipe.x + f64::from(PIPE_WIDTH) >= 0.0);
    }

    fn refill_pipes(&mut self) {
        let fill_until = f64::from(self.width) + PIPE_SPACING * 2.0;
        let mut next_x = self
            .pipes
            .last()
            .map_or(f64::from(self.width), |pipe| pipe.x + PIPE_SPACING);

        while next_x <= fill_until {
            self.push_pipe(next_x);
            next_x += PIPE_SPACING;
        }
    }

    fn push_pipe(&mut self, x: f64) {
        let gap_height = self.gap_height();
        let gap_top = self.next_gap_top(gap_height);
        self.pipes.push(Pipe {
            x,
            gap_top,
            gap_height,
            scored: false,
        });
    }

    fn next_gap_top(&mut self, gap_height: u16) -> u16 {
        let min_top = VERTICAL_MARGIN;
        let max_top = self
            .height
            .saturating_sub(VERTICAL_MARGIN + gap_height)
            .max(min_top);
        let span = u64::from(max_top - min_top) + 1;
        let sampled = min_top + (self.next_random() % span) as u16;

        // Nearby openings keep every seeded course demanding but navigable.
        if let Some(previous) = self.pipes.last() {
            let low = previous.gap_top.saturating_sub(MAX_GAP_SHIFT).max(min_top);
            let high = previous.gap_top.saturating_add(MAX_GAP_SHIFT).min(max_top);
            sampled.clamp(low, high)
        } else {
            sampled
        }
    }

    // SplitMix64: tiny, reproducible, and sufficient for level generation.
    fn next_random(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.rng_state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn collides(&self) -> bool {
        let bird_top = self.bird_y.round() as i32;
        let bird_bottom = bird_top + i32::from(BIRD_HEIGHT);
        if bird_top <= 0 || bird_bottom >= i32::from(self.height) {
            return true;
        }

        let bird_left = self.bird_x.round() as i32;
        let bird_right = bird_left + i32::from(BIRD_WIDTH);

        self.pipes.iter().any(|pipe| {
            let pipe_left = pipe.x.round() as i32;
            let pipe_right = pipe_left + i32::from(PIPE_WIDTH);
            let overlaps_horizontally = bird_left < pipe_right && bird_right > pipe_left;
            let gap_top = i32::from(pipe.gap_top);
            let gap_bottom = gap_top + i32::from(pipe.gap_height);
            let outside_gap = bird_top < gap_top || bird_bottom > gap_bottom;

            overlaps_horizontally && outside_gap
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: f64 = FIXED_STEP_SECONDS;

    fn playing_game() -> Game {
        let mut game = Game::new(70, 20, 42);
        game.flap();
        game
    }

    #[test]
    fn dimensions_are_clamped_and_round_starts_ready() {
        let game = Game::new(1, u16::MAX, 7);
        assert_eq!(game.width, MIN_FIELD_WIDTH);
        assert_eq!(game.height, MAX_FIELD_HEIGHT);
        assert_eq!(game.phase, Phase::Ready);
        assert_eq!(game.score, 0);
        assert_eq!(game.elapsed, 0.0);
        assert_eq!(BIRD_WIDTH, 3);
        assert_eq!(BIRD_HEIGHT, 1);
    }

    #[test]
    fn first_flap_starts_and_gravity_changes_velocity() {
        let mut game = Game::new(70, 20, 7);
        let starting_y = game.bird_y;

        game.update(1.0);
        assert_eq!(game.bird_y, starting_y, "ready rounds must remain frozen");

        game.flap();
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.bird_velocity, FLAP_VELOCITY);

        game.update(TICK);
        assert!((game.bird_velocity - (FLAP_VELOCITY + GRAVITY * TICK)).abs() < 1e-10);
        assert!(game.bird_y < starting_y);
    }

    #[test]
    fn fixed_step_is_independent_of_frame_partitioning() {
        let mut whole = playing_game();
        let mut partitioned = whole.clone();

        whole.update(TICK * 12.0);
        for _ in 0..12 {
            partitioned.update(TICK);
        }

        assert!((whole.bird_y - partitioned.bird_y).abs() < 1e-10);
        assert!((whole.bird_velocity - partitioned.bird_velocity).abs() < 1e-10);
        assert!((whole.elapsed - partitioned.elapsed).abs() < 1e-10);
        assert_eq!(whole.pipes, partitioned.pipes);
    }

    #[test]
    fn pause_freezes_every_part_of_the_simulation() {
        let mut game = playing_game();
        game.update(TICK * 3.0);
        game.pause();
        let frozen = game.clone();

        game.update(20.0);
        game.flap();
        assert_eq!(game.phase, Phase::Paused);
        assert_eq!(game.bird_y, frozen.bird_y);
        assert_eq!(game.bird_velocity, frozen.bird_velocity);
        assert_eq!(game.pipes, frozen.pipes);
        assert_eq!(game.score, frozen.score);
        assert_eq!(game.elapsed, frozen.elapsed);

        game.toggle_pause();
        assert_eq!(game.phase, Phase::Playing);
        game.update(TICK);
        assert!(game.elapsed > frozen.elapsed);
    }

    #[test]
    fn ceiling_and_floor_end_the_round() {
        let mut ceiling = playing_game();
        ceiling.bird_y = 0.0;
        ceiling.update(TICK);
        assert_eq!(ceiling.phase, Phase::GameOver);

        let mut floor = playing_game();
        floor.bird_y = f64::from(floor.height - BIRD_HEIGHT) - 0.01;
        floor.bird_velocity = MAX_FALL_VELOCITY;
        floor.update(TICK);
        assert_eq!(floor.phase, Phase::GameOver);
    }

    #[test]
    fn touching_a_pipe_outside_its_gap_is_a_collision() {
        let mut game = playing_game();
        game.bird_y = 3.0;
        game.bird_velocity = 0.0;
        game.pipes = vec![Pipe {
            x: game.bird_x + 1.0,
            gap_top: 8,
            gap_height: 6,
            scored: false,
        }];

        game.update(TICK);
        assert_eq!(game.phase, Phase::GameOver);
    }

    #[test]
    fn bird_fully_inside_gap_passes_without_collision() {
        let mut game = playing_game();
        game.bird_y = 9.0;
        game.bird_velocity = 0.0;
        game.pipes = vec![Pipe {
            x: game.bird_x + 1.0,
            gap_top: 8,
            gap_height: 6,
            scored: false,
        }];

        game.update(TICK);
        assert_eq!(game.phase, Phase::Playing);
    }

    #[test]
    fn collisions_match_the_cells_drawn_in_the_terminal() {
        let mut game = playing_game();
        game.bird_y = 10.51;
        game.bird_velocity = 0.0;
        game.pipes = vec![Pipe {
            x: game.bird_x + 0.49,
            gap_top: 11,
            gap_height: 4,
            scored: false,
        }];

        game.update(TICK);

        assert_eq!(game.bird_y.round() as u16, 11);
        assert_eq!(game.phase, Phase::Playing);
    }

    #[test]
    fn generated_gaps_stay_safe_and_near_the_previous_gap() {
        for height in MIN_FIELD_HEIGHT..=MAX_FIELD_HEIGHT {
            for seed in 0..50 {
                let game = Game::new(70, height, seed);
                for pipe in &game.pipes {
                    assert!(pipe.gap_top >= VERTICAL_MARGIN);
                    assert!(
                        pipe.gap_top + pipe.gap_height <= height.saturating_sub(VERTICAL_MARGIN)
                    );
                }
                for pair in game.pipes.windows(2) {
                    assert!(pair[0].gap_top.abs_diff(pair[1].gap_top) <= MAX_GAP_SHIFT);
                    assert!((pair[1].x - pair[0].x - PIPE_SPACING).abs() < 1e-10);
                }
            }
        }
    }

    #[test]
    fn pipe_scores_once_after_its_trailing_edge_passes() {
        let mut game = playing_game();
        game.bird_y = 9.0;
        game.bird_velocity = 0.0;
        game.pipes = vec![Pipe {
            x: game.bird_x - f64::from(PIPE_WIDTH) + 0.05,
            gap_top: 5,
            gap_height: 10,
            scored: false,
        }];

        game.update(TICK);
        assert_eq!(game.score, 1);
        assert!(game.pipes[0].scored);

        game.update(TICK * 10.0);
        assert_eq!(game.score, 1);
    }

    #[test]
    fn a_simple_flap_cadence_can_clear_the_first_seeded_pipe() {
        let mut game = Game::new(98, 24, 42);
        game.flap();

        for tick in 1..=420 {
            if matches!(tick, 108 | 192 | 276 | 360) {
                game.flap();
            }
            game.update(TICK);
            assert_ne!(
                game.phase,
                Phase::GameOver,
                "cadence crashed at tick {tick}, y={}, score={}",
                game.bird_y,
                game.score
            );
        }

        assert!(game.score >= 1, "the first pipe should have been cleared");
    }

    #[test]
    fn same_seed_produces_same_course() {
        let first = Game::new(100, 30, 0xfeed_beef);
        let second = Game::new(100, 30, 0xfeed_beef);
        let different = Game::new(100, 30, 0xfeed_beee);

        assert_eq!(first.pipes, second.pipes);
        assert_ne!(
            first
                .pipes
                .iter()
                .map(|pipe| pipe.gap_top)
                .collect::<Vec<_>>(),
            different
                .pipes
                .iter()
                .map(|pipe| pipe.gap_top)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn difficulty_increases_and_caps() {
        let mut game = Game::new(100, 30, 1);
        assert_eq!(game.level(), 1);
        assert_eq!(game.speed(), BASE_PIPE_SPEED);
        assert_eq!(game.gap_height(), 10);

        game.score = 25;
        assert_eq!(game.level(), 6);
        assert!(game.speed() > BASE_PIPE_SPEED);
        assert!(game.gap_height() < 10);

        game.score = u32::MAX;
        assert_eq!(game.level(), MAX_LEVEL);
        assert_eq!(game.speed(), MAX_PIPE_SPEED);
        assert_eq!(game.gap_height(), MIN_GAP_HEIGHT);
    }

    #[test]
    fn restart_is_clean_and_advances_the_seeded_course() {
        let original_course = Game::new(MAX_FIELD_WIDTH, MIN_FIELD_HEIGHT, 123);
        let mut game = Game::new(MAX_FIELD_WIDTH, MIN_FIELD_HEIGHT, 123);
        let mut matching_game = game.clone();
        game.flap();
        game.update(TICK * 20.0);
        game.score = 99;
        game.phase = Phase::GameOver;

        game.restart(u16::MAX, 0);
        matching_game.restart(u16::MAX, 0);

        assert_eq!(game.width, MAX_FIELD_WIDTH);
        assert_eq!(game.height, MIN_FIELD_HEIGHT);
        assert_eq!(game.phase, Phase::Ready);
        assert_eq!(game.score, 0);
        assert_eq!(game.elapsed, 0.0);
        assert_eq!(game.bird_velocity, 0.0);
        assert_eq!(game.bird_x, original_course.bird_x);
        assert_eq!(game.bird_y, original_course.bird_y);
        assert_eq!(game.pipes, matching_game.pipes);
        assert_ne!(game.pipes, original_course.pipes);
    }
}
