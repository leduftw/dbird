//! Deterministic simulation of the original game's fixed mechanics.
//!
//! Gameplay is evaluated on the APK's 288x512 virtual canvas. `Game::width`
//! and `Game::height` describe only the adaptive terminal viewport; changing
//! them never changes the course, physics, or collision geometry.

/// Smallest supported terminal play-field width, in cells.
pub const MIN_FIELD_WIDTH: u16 = 16;
/// Smallest supported terminal play-field height, in cells.
pub const MIN_FIELD_HEIGHT: u16 = 14;
/// Largest supported terminal play-field width, in cells.
pub const MAX_FIELD_WIDTH: u16 = 45;
/// Largest supported terminal play-field height, in cells.
pub const MAX_FIELD_HEIGHT: u16 = 40;

/// Width of the original virtual canvas, in pixels.
pub const VIRTUAL_WIDTH: u16 = 288;
/// Height of the original virtual canvas, in pixels.
pub const VIRTUAL_HEIGHT: u16 = 512;
/// Top edge of the ground on the virtual canvas.
pub const GROUND_Y: u16 = 400;

/// Initial left edge of the bird's collision box.
pub const BIRD_START_X: u16 = 80;
/// Initial top edge of the bird's collision box.
pub const BIRD_START_Y: u16 = 246;
/// Width of the bird's collision box.
pub const BIRD_WIDTH: u16 = 20;
/// Height of the bird's collision box.
pub const BIRD_HEIGHT: u16 = 20;
/// Width of the non-transparent bird artwork inside its 48x48 atlas frame.
pub const BIRD_ART_WIDTH: u16 = 34;
/// Height of the non-transparent bird artwork inside its 48x48 atlas frame.
pub const BIRD_ART_HEIGHT: u16 = 24;
/// Distance from the artwork's left edge to the hitbox's left edge.
pub const BIRD_ART_OFFSET_X: u16 = 8;
/// Distance from the artwork's top edge to the hitbox's top edge.
pub const BIRD_ART_OFFSET_Y: u16 = 2;

/// Width of each pipe on the virtual canvas.
pub const PIPE_WIDTH: u16 = 52;
/// Fixed vertical clearance between the top and bottom pipes.
pub const PIPE_GAP_HEIGHT: u16 = 96;
/// Fixed distance between consecutive pipe leading edges.
pub const PIPE_PITCH: u16 = 157;
/// Fixed empty horizontal space between consecutive pipes.
pub const PIPE_EMPTY_SPACE: u16 = PIPE_PITCH - PIPE_WIDTH;
/// Number of active-play ticks before the APK reveals its hidden first pipe.
pub const PIPE_WARMUP_TICKS: u64 = 68;
/// Leading edge of the first pipe when the hidden-pipe warmup ends.
pub const FIRST_PIPE_X: u16 = 414;
/// First active-play tick on which any part of the first pipe is on-screen.
pub const FIRST_PIPE_VISIBLE_TICK: u64 = 132;
/// Active-play tick on which the first pipe reaches the bird and awards a point.
pub const FIRST_PIPE_SCORE_TICK: u64 = 235;

/// Duration of one discrete simulation tick.
pub const FIXED_STEP_SECONDS: f64 = 1.0 / 60.0;
/// Vertical velocity applied when a round starts or the bird flaps.
pub const FLAP_VELOCITY: f64 = -5.0;
/// Downward velocity added on every simulation tick.
pub const GRAVITY_PER_TICK: f64 = 0.3;
/// Maximum downward velocity, in virtual pixels per tick.
pub const MAX_FALL_VELOCITY: f64 = 8.0;
/// Most upward visual rotation used by the original bird sprite, in degrees.
pub const MIN_BIRD_ANGLE: f64 = -20.0;
/// Most downward visual rotation used by the original bird sprite, in degrees.
pub const MAX_BIRD_ANGLE: f64 = 90.0;
/// Angular velocity applied by a flap, in degrees per simulation tick.
pub const FLAP_ANGULAR_VELOCITY: f64 = -10.0;
/// Downward angular acceleration applied on every simulation tick.
pub const BIRD_ANGULAR_ACCELERATION: f64 = 0.4;
/// Horizontal movement of every pipe on each simulation tick.
pub const PIPE_SPEED_PER_TICK: f64 = 2.0;
/// Number of fixed ticks between a fatal collision and the result card.
pub const DEATH_TICKS: u16 = 60;

const PIPE_PAIR_COUNT: usize = 4;
const MIN_BOTTOM_PIPE_TOP: u16 = 180;
const MAX_BOTTOM_PIPE_TOP: u16 = 359;
const STEP_EPSILON: f64 = FIXED_STEP_SECONDS * 1e-9;

/// The current state of a game round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    Playing,
    Paused,
    /// Fatal animation: pipes are frozen while the bird falls.
    Dying,
    GameOver,
}

/// The collision which ended the current round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeathCause {
    Ground,
    Pipe,
}

/// A medal awarded for the completed round's score.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Medal {
    Bronze,
    Silver,
    Gold,
    Platinum,
}

impl Medal {
    /// Return the medal earned by `score`, or `None` below ten points.
    pub const fn for_score(score: u32) -> Option<Self> {
        match score {
            0..=9 => None,
            10..=19 => Some(Self::Bronze),
            20..=29 => Some(Self::Silver),
            30..=39 => Some(Self::Gold),
            _ => Some(Self::Platinum),
        }
    }

    /// Uppercase label suitable for the terminal result card.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bronze => "BRONZE",
            Self::Silver => "SILVER",
            Self::Gold => "GOLD",
            Self::Platinum => "PLATINUM",
        }
    }
}

/// A single top-and-bottom pipe pair in virtual-canvas coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipe {
    /// Horizontal position of the pipe's leading (left) edge.
    pub x: f64,
    /// Top edge of the fixed-size passable opening.
    pub gap_top: u16,
    /// Height of the passable opening. Always [`PIPE_GAP_HEIGHT`] for generated pipes.
    pub gap_height: u16,
    /// Whether this pipe has already awarded its point.
    pub scored: bool,
}

/// Complete simulation state for one game.
#[derive(Clone, Debug)]
pub struct Game {
    /// Adaptive terminal play-field width, in cells.
    pub width: u16,
    /// Adaptive terminal play-field height, in cells.
    pub height: u16,
    /// Left edge of the bird hitbox on the virtual canvas.
    pub bird_x: f64,
    /// Top edge of the bird hitbox on the virtual canvas.
    pub bird_y: f64,
    /// Bird velocity in virtual pixels per simulation tick.
    pub bird_velocity: f64,
    bird_angle: f64,
    bird_angle_velocity: f64,
    pub pipes: Vec<Pipe>,
    pub score: u32,
    pub phase: Phase,
    /// Time actually simulated during the current round.
    pub elapsed: f64,

    initial_seed: u64,
    round: u64,
    rng_state: u64,
    accumulator: f64,
    playing_ticks: u64,
    pipes_active: bool,
    death_cause: Option<DeathCause>,
    death_ticks: u16,
}

impl Game {
    /// Construct a ready-to-play round. Unsupported viewport sizes are clamped.
    pub fn new(width: u16, height: u16, seed: u64) -> Self {
        let mut game = Self {
            width: width.clamp(MIN_FIELD_WIDTH, MAX_FIELD_WIDTH),
            height: height.clamp(MIN_FIELD_HEIGHT, MAX_FIELD_HEIGHT),
            bird_x: 0.0,
            bird_y: 0.0,
            bird_velocity: 0.0,
            bird_angle: 0.0,
            bird_angle_velocity: 0.0,
            pipes: Vec::new(),
            score: 0,
            phase: Phase::Ready,
            elapsed: 0.0,
            initial_seed: seed,
            round: 0,
            rng_state: seed,
            accumulator: 0.0,
            playing_ticks: 0,
            pipes_active: false,
            death_cause: None,
            death_ticks: 0,
        };
        game.reset_round();
        game
    }

    /// Start a ready round with the APK's initial upward launch.
    pub fn start(&mut self) -> bool {
        if self.phase == Phase::Ready {
            self.phase = Phase::Playing;
            self.bird_velocity = FLAP_VELOCITY;
            self.bird_angle_velocity = FLAP_ANGULAR_VELOCITY;
            true
        } else {
            false
        }
    }

    /// Flap only during active play.
    ///
    /// A flap is ignored while the collision box is above the canvas. Gravity
    /// continues to act there, so the bird eventually falls back into view.
    pub fn flap(&mut self) -> bool {
        if self.phase == Phase::Playing && self.bird_y >= 0.0 {
            self.bird_velocity = FLAP_VELOCITY;
            self.bird_angle_velocity = FLAP_ANGULAR_VELOCITY;
            true
        } else {
            false
        }
    }

    /// Advance the game by wall-clock seconds.
    ///
    /// Calls are accumulated and evaluated at 60 Hz, so the discrete APK
    /// mechanics do not depend on the renderer's frame rate.
    pub fn update(&mut self, dt_secs: f64) {
        if !matches!(self.phase, Phase::Playing | Phase::Dying)
            || !dt_secs.is_finite()
            || dt_secs <= 0.0
        {
            return;
        }

        self.accumulator += dt_secs;
        while self.accumulator + STEP_EPSILON >= FIXED_STEP_SECONDS {
            self.accumulator -= FIXED_STEP_SECONDS;
            if self.accumulator.abs() < STEP_EPSILON {
                self.accumulator = 0.0;
            }
            match self.phase {
                Phase::Playing => self.step_playing(),
                Phase::Dying => self.step_dying(),
                Phase::Ready | Phase::Paused | Phase::GameOver => break,
            }

            if !matches!(self.phase, Phase::Playing | Phase::Dying) {
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
            Phase::Ready | Phase::Dying | Phase::GameOver => {}
        }
    }

    /// Pause a running round.
    pub fn pause(&mut self) {
        if self.phase == Phase::Playing {
            self.phase = Phase::Paused;
        }
    }

    /// Resume a paused round without applying a flap.
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

    /// Fixed horizontal pipe speed, in virtual pixels per simulation tick.
    pub const fn speed(&self) -> f64 {
        PIPE_SPEED_PER_TICK
    }

    /// Fixed vertical pipe clearance, in virtual pixels.
    pub const fn gap_height(&self) -> u16 {
        PIPE_GAP_HEIGHT
    }

    /// Current visual rotation of the bird, in degrees.
    pub const fn bird_angle(&self) -> f64 {
        self.bird_angle
    }

    /// Medal currently earned by the round's score.
    pub const fn medal(&self) -> Option<Medal> {
        Medal::for_score(self.score)
    }

    /// Fatal collision for a dying or completed round.
    pub const fn death_cause(&self) -> Option<DeathCause> {
        self.death_cause
    }

    fn reset_round(&mut self) {
        self.bird_x = f64::from(BIRD_START_X);
        self.bird_y = f64::from(BIRD_START_Y);
        self.bird_velocity = 0.0;
        self.bird_angle = 0.0;
        self.bird_angle_velocity = 0.0;
        self.pipes.clear();
        self.score = 0;
        self.phase = Phase::Ready;
        self.elapsed = 0.0;
        self.rng_state = self
            .initial_seed
            .wrapping_add(self.round.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        self.accumulator = 0.0;
        self.playing_ticks = 0;
        self.pipes_active = false;
        self.death_cause = None;
        self.death_ticks = 0;
    }

    fn step_playing(&mut self) {
        self.elapsed += FIXED_STEP_SECONDS;
        self.advance_bird();
        self.playing_ticks = self.playing_ticks.saturating_add(1);

        if !self.pipes_active && self.playing_ticks == PIPE_WARMUP_TICKS {
            self.activate_pipes();
        } else if self.pipes_active {
            for pipe in &mut self.pipes {
                pipe.x -= PIPE_SPEED_PER_TICK;
            }

            self.award_passed_pipes();
            self.remove_old_pipes();
            self.refill_pipes();
        }

        if let Some(cause) = self.collision_cause() {
            self.begin_dying(cause);
        }
    }

    fn step_dying(&mut self) {
        self.elapsed += FIXED_STEP_SECONDS;
        self.advance_bird();
        self.clamp_to_ground();
        self.death_ticks = self.death_ticks.saturating_add(1);

        if self.death_ticks >= DEATH_TICKS {
            self.phase = Phase::GameOver;
        }
    }

    fn advance_bird(&mut self) {
        self.bird_velocity = (self.bird_velocity + GRAVITY_PER_TICK).min(MAX_FALL_VELOCITY);

        // The APK stores position as an integer and uses a compound assignment
        // from a floating-point velocity. Java truncates that result toward zero.
        self.bird_y = (self.bird_y + self.bird_velocity).trunc();

        // Rotation has its own angular momentum in the original game. A flap
        // kicks it upward, then this acceleration gradually turns the nose down.
        self.bird_angle += self.bird_angle_velocity;
        self.bird_angle_velocity += BIRD_ANGULAR_ACCELERATION;
        self.bird_angle = self.bird_angle.clamp(MIN_BIRD_ANGLE, MAX_BIRD_ANGLE);
    }

    fn activate_pipes(&mut self) {
        self.pipes.clear();
        for index in 0..PIPE_PAIR_COUNT {
            let x = FIRST_PIPE_X as usize + index * PIPE_PITCH as usize;
            self.push_pipe(x as f64);
        }
        self.pipes_active = true;
    }

    fn begin_dying(&mut self, cause: DeathCause) {
        self.death_cause = Some(cause);
        self.death_ticks = 0;
        self.phase = Phase::Dying;

        if cause == DeathCause::Ground {
            self.clamp_to_ground();
        }
    }

    fn clamp_to_ground(&mut self) {
        let ground_limit = f64::from(GROUND_Y - BIRD_HEIGHT);
        if self.bird_y >= ground_limit {
            self.bird_y = ground_limit;
            self.bird_velocity = 0.0;
        }
    }

    fn award_passed_pipes(&mut self) {
        for pipe in &mut self.pipes {
            if !pipe.scored && pipe.x <= self.bird_x {
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
        let mut next_x = self.pipes.last().map_or(f64::from(FIRST_PIPE_X), |pipe| {
            pipe.x + f64::from(PIPE_PITCH)
        });

        while self.pipes.len() < PIPE_PAIR_COUNT {
            self.push_pipe(next_x);
            next_x += f64::from(PIPE_PITCH);
        }
    }

    fn push_pipe(&mut self, x: f64) {
        let bottom_pipe_top = self.next_bottom_pipe_top();
        self.pipes.push(Pipe {
            x,
            gap_top: bottom_pipe_top - PIPE_GAP_HEIGHT,
            gap_height: PIPE_GAP_HEIGHT,
            scored: false,
        });
    }

    fn next_bottom_pipe_top(&mut self) -> u16 {
        let span = u64::from(MAX_BOTTOM_PIPE_TOP - MIN_BOTTOM_PIPE_TOP) + 1;
        MIN_BOTTOM_PIPE_TOP + (self.next_random() % span) as u16
    }

    // SplitMix64: tiny, reproducible, and sufficient for deterministic courses.
    fn next_random(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.rng_state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn collision_cause(&self) -> Option<DeathCause> {
        let bird_top = self.bird_y as i32;
        let bird_bottom = bird_top + i32::from(BIRD_HEIGHT);

        if bird_top >= i32::from(GROUND_Y - BIRD_HEIGHT) {
            return Some(DeathCause::Ground);
        }

        // There is no ceiling. A completely hidden bird cannot hit a visible pipe,
        // and hidden warmup pipes do not participate in collision detection.
        if bird_bottom <= 0 || !self.pipes_active {
            return None;
        }

        let bird_left = self.bird_x as i32;
        let bird_right = bird_left + i32::from(BIRD_WIDTH);

        self.pipes.iter().find_map(|pipe| {
            let pipe_left = pipe.x as i32;
            let pipe_right = pipe_left + i32::from(PIPE_WIDTH);
            let overlaps_horizontally = bird_left <= pipe_right && bird_right >= pipe_left;
            let gap_top = i32::from(pipe.gap_top);
            let gap_bottom = gap_top + i32::from(pipe.gap_height);
            let touches_or_crosses_pipe = bird_top <= gap_top || bird_bottom >= gap_bottom;

            (overlaps_horizontally && touches_or_crosses_pipe).then_some(DeathCause::Pipe)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: f64 = FIXED_STEP_SECONDS;

    fn playing_game() -> Game {
        let mut game = Game::new(70, 20, 42);
        game.start();
        game
    }

    fn active_game() -> Game {
        let mut game = playing_game();
        game.playing_ticks = PIPE_WARMUP_TICKS;
        game.activate_pipes();
        game
    }

    fn safe_pipe(x: f64) -> Pipe {
        Pipe {
            x,
            gap_top: 180,
            gap_height: PIPE_GAP_HEIGHT,
            scored: false,
        }
    }

    fn stable_tick(game: &mut Game) {
        game.bird_y = f64::from(BIRD_START_Y);
        game.bird_velocity = -GRAVITY_PER_TICK;
        game.update(TICK);
    }

    fn finish_warmup(game: &mut Game) {
        while game.playing_ticks < PIPE_WARMUP_TICKS {
            stable_tick(game);
        }
    }

    #[test]
    fn viewport_is_clamped_without_scaling_virtual_geometry() {
        let game = Game::new(1, u16::MAX, 7);
        assert_eq!(game.width, MIN_FIELD_WIDTH);
        assert_eq!(game.height, MAX_FIELD_HEIGHT);
        assert_eq!(game.bird_x, f64::from(BIRD_START_X));
        assert_eq!(game.bird_y, f64::from(BIRD_START_Y));
        assert_eq!(game.bird_angle(), 0.0);
        assert_eq!(game.phase, Phase::Ready);
        assert_eq!(game.score, 0);
        assert_eq!(game.elapsed, 0.0);
        assert!(game.pipes.is_empty());
        assert_eq!(game.death_cause(), None);
        assert_eq!(BIRD_ART_WIDTH, 34);
        assert_eq!(BIRD_ART_HEIGHT, 24);
        assert_eq!(BIRD_ART_OFFSET_X, 8);
        assert_eq!(BIRD_ART_OFFSET_Y, 2);
        assert_eq!(BIRD_ART_WIDTH - BIRD_WIDTH - BIRD_ART_OFFSET_X, 6);
        assert_eq!(BIRD_ART_HEIGHT - BIRD_HEIGHT - BIRD_ART_OFFSET_Y, 2);
    }

    #[test]
    fn enter_facing_start_api_is_separate_from_flapping() {
        let mut game = Game::new(70, 20, 7);
        let starting_y = game.bird_y;

        assert!(!game.flap());
        game.update(1.0);
        assert_eq!(game.phase, Phase::Ready);
        assert_eq!(game.bird_y, starting_y);

        assert!(game.start());
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.bird_velocity, FLAP_VELOCITY);
        assert_eq!(game.bird_angle(), 0.0);

        game.phase = Phase::GameOver;
        assert!(!game.flap());
        assert!(!game.start());
        assert_eq!(game.phase, Phase::GameOver);
        assert_eq!(game.bird_velocity, FLAP_VELOCITY);

        game.phase = Phase::Dying;
        assert!(!game.flap());
        assert!(!game.start());
        game.pause();
        game.toggle_pause();
        assert_eq!(game.phase, Phase::Dying);
    }

    #[test]
    fn discrete_tick_matches_apk_gravity_and_integer_truncation() {
        let mut game = playing_game();

        game.update(TICK);
        assert!((game.bird_velocity - (-4.7)).abs() < 1e-10);
        assert_eq!(game.bird_y, 241.0);
        assert_eq!(game.bird_angle(), -10.0);

        game.update(TICK);
        assert!((game.bird_velocity - (-4.4)).abs() < 1e-10);
        assert_eq!(game.bird_y, 236.0);
        assert!((game.bird_angle() - (-19.6)).abs() < 1e-10);

        game.bird_velocity = 7.9;
        game.update(TICK);
        assert_eq!(game.bird_velocity, MAX_FALL_VELOCITY);
    }

    #[test]
    fn bird_rotation_keeps_the_original_flap_inertia_and_angle_limits() {
        let mut game = playing_game();

        game.update(TICK * 3.0);
        assert_eq!(game.bird_angle(), MIN_BIRD_ANGLE);

        // A flap resets angular velocity, not the current angle. This makes a
        // falling bird sweep back upward instead of snapping between poses.
        game.bird_angle = 60.0;
        assert!(game.flap());
        assert_eq!(game.bird_angle(), 60.0);
        game.update(TICK);
        assert_eq!(game.bird_angle(), 50.0);

        for _ in 0..100 {
            game.update(TICK);
        }
        assert_eq!(game.bird_angle(), MAX_BIRD_ANGLE);
    }

    #[test]
    fn one_flap_rises_the_source_locked_forty_seven_pixels() {
        let mut game = playing_game();
        let mut apex = game.bird_y;

        for _ in 0..30 {
            game.update(TICK);
            apex = apex.min(game.bird_y);
        }

        assert_eq!(apex, 199.0);
        assert_eq!(f64::from(BIRD_START_Y) - apex, 47.0);
    }

    #[test]
    fn fixed_step_is_independent_of_frame_partitioning() {
        let mut whole = playing_game();
        let mut partitioned = whole.clone();

        whole.update(TICK * 12.0);
        for _ in 0..12 {
            partitioned.update(TICK);
        }

        assert_eq!(whole.bird_y, partitioned.bird_y);
        assert!((whole.bird_velocity - partitioned.bird_velocity).abs() < 1e-10);
        assert!((whole.bird_angle() - partitioned.bird_angle()).abs() < 1e-10);
        assert!((whole.elapsed - partitioned.elapsed).abs() < 1e-10);
        assert_eq!(whole.pipes, partitioned.pipes);
    }

    #[test]
    fn pause_freezes_simulation_and_flap_does_not_resume_it() {
        let mut game = playing_game();
        game.update(TICK * 3.0);
        game.pause();
        let frozen = game.clone();

        game.update(20.0);
        game.flap();
        assert_eq!(game.phase, Phase::Paused);
        assert_eq!(game.bird_y, frozen.bird_y);
        assert_eq!(game.bird_velocity, frozen.bird_velocity);
        assert_eq!(game.bird_angle(), frozen.bird_angle());
        assert_eq!(game.pipes, frozen.pipes);
        assert_eq!(game.elapsed, frozen.elapsed);

        game.toggle_pause();
        assert_eq!(game.phase, Phase::Playing);
        game.update(TICK);
        assert!(game.elapsed > frozen.elapsed);
    }

    #[test]
    fn bird_can_leave_through_ceiling_and_gravity_brings_it_back() {
        let mut game = playing_game();
        game.pipes.clear();
        game.bird_y = -35.0;
        game.bird_velocity = -1.0;

        game.flap();
        assert_eq!(
            game.bird_velocity, -1.0,
            "flaps above the canvas are ignored"
        );

        for _ in 0..90 {
            game.update(TICK);
            assert_eq!(game.phase, Phase::Playing);
            if game.bird_y >= 0.0 {
                break;
            }
        }

        assert!(game.bird_y >= 0.0, "gravity should return the bird to view");
        game.flap();
        assert_eq!(game.bird_velocity, FLAP_VELOCITY);
    }

    #[test]
    fn ground_collision_enters_dying_and_stays_clamped_for_sixty_ticks() {
        let mut game = playing_game();
        game.bird_y = f64::from(GROUND_Y - BIRD_HEIGHT - 7);
        game.bird_velocity = MAX_FALL_VELOCITY;

        game.update(TICK);

        assert_eq!(game.bird_y, f64::from(GROUND_Y - BIRD_HEIGHT));
        assert_eq!(game.phase, Phase::Dying);
        assert_eq!(game.death_cause(), Some(DeathCause::Ground));

        game.update(TICK * f64::from(DEATH_TICKS - 1));
        assert_eq!(game.phase, Phase::Dying);
        assert_eq!(game.bird_y, f64::from(GROUND_Y - BIRD_HEIGHT));
        assert_eq!(game.death_ticks, DEATH_TICKS - 1);

        game.update(TICK);
        assert_eq!(game.phase, Phase::GameOver);
        assert_eq!(game.bird_y, f64::from(GROUND_Y - BIRD_HEIGHT));
        assert_eq!(game.death_cause(), Some(DeathCause::Ground));
    }

    #[test]
    fn pipe_collision_freezes_pipes_while_bird_falls() {
        let mut collision = active_game();
        collision.bird_y = 100.0;
        collision.bird_velocity = 0.0;
        collision.pipes = vec![Pipe {
            x: collision.bird_x + 1.0 + PIPE_SPEED_PER_TICK,
            gap_top: 150,
            gap_height: PIPE_GAP_HEIGHT,
            scored: false,
        }];
        collision.update(TICK);
        assert_eq!(collision.phase, Phase::Dying);
        assert_eq!(collision.death_cause(), Some(DeathCause::Pipe));

        let frozen_pipes = collision.pipes.clone();
        let collision_y = collision.bird_y;
        collision.update(TICK * 20.0);
        assert_eq!(collision.phase, Phase::Dying);
        assert_eq!(collision.pipes, frozen_pipes);
        assert!(collision.bird_y > collision_y);

        collision.update(TICK * 40.0);
        assert_eq!(collision.phase, Phase::GameOver);
        assert_eq!(collision.bird_y, f64::from(GROUND_Y - BIRD_HEIGHT));
        assert_eq!(collision.pipes, frozen_pipes);
    }

    #[test]
    fn pipe_collision_includes_rectangle_and_gap_boundaries() {
        let cases = [
            // Bird's right edge touches the pipe's left edge.
            (100.0 + PIPE_SPEED_PER_TICK, 100.0, 150),
            // Bird's left edge touches the pipe's right edge.
            (28.0 + PIPE_SPEED_PER_TICK, 100.0, 150),
            // Bird's top edge touches the upper gap boundary.
            (81.0 + PIPE_SPEED_PER_TICK, 180.0, 180),
            // Bird's bottom edge touches the lower gap boundary.
            (81.0 + PIPE_SPEED_PER_TICK, 256.0, 180),
        ];

        for (pipe_x, bird_y, gap_top) in cases {
            let mut game = active_game();
            game.bird_y = bird_y;
            game.bird_velocity = -GRAVITY_PER_TICK;
            game.pipes = vec![Pipe {
                x: pipe_x,
                gap_top,
                gap_height: PIPE_GAP_HEIGHT,
                scored: false,
            }];

            game.update(TICK);

            assert_eq!(game.phase, Phase::Dying, "x={pipe_x}, y={bird_y}");
            assert_eq!(game.death_cause(), Some(DeathCause::Pipe));
        }

        let mut safe = active_game();
        safe.bird_y = 181.0;
        safe.bird_velocity = -GRAVITY_PER_TICK;
        safe.pipes = vec![safe_pipe(safe.bird_x + 1.0 + PIPE_SPEED_PER_TICK)];
        safe.update(TICK);
        assert_eq!(safe.phase, Phase::Playing);
    }

    #[test]
    fn hidden_pipe_warmup_lasts_exactly_sixty_eight_playing_ticks() {
        let mut game = playing_game();

        for tick in 1..PIPE_WARMUP_TICKS {
            stable_tick(&mut game);
            assert!(game.pipes.is_empty(), "pipe appeared on warmup tick {tick}");
            assert_eq!(game.score, 0);
        }

        stable_tick(&mut game);
        assert_eq!(game.playing_ticks, PIPE_WARMUP_TICKS);
        assert_eq!(game.pipes.len(), PIPE_PAIR_COUNT);
        assert_eq!(game.pipes[0].x, f64::from(FIRST_PIPE_X));
        assert_eq!(game.score, 0);
    }

    #[test]
    fn first_pipe_screen_entry_and_score_ticks_are_source_locked() {
        let mut game = playing_game();
        finish_warmup(&mut game);
        let safe_y = game.pipes[0].gap_top + 1;

        for movement_tick in 1..=167 {
            game.bird_y = f64::from(safe_y);
            game.bird_velocity = -GRAVITY_PER_TICK;
            game.update(TICK);

            match movement_tick {
                63 => {
                    assert_eq!(game.playing_ticks, FIRST_PIPE_VISIBLE_TICK - 1);
                    assert_eq!(game.pipes[0].x, f64::from(VIRTUAL_WIDTH));
                }
                64 => {
                    assert_eq!(game.playing_ticks, FIRST_PIPE_VISIBLE_TICK);
                    assert_eq!(
                        game.pipes[0].x,
                        f64::from(VIRTUAL_WIDTH) - PIPE_SPEED_PER_TICK
                    );
                }
                166 => {
                    assert_eq!(game.pipes[0].x, game.bird_x + PIPE_SPEED_PER_TICK);
                    assert_eq!(game.score, 0);
                }
                167 => {
                    assert_eq!(game.playing_ticks, FIRST_PIPE_SCORE_TICK);
                    assert_eq!(game.pipes[0].x, game.bird_x);
                    assert_eq!(game.score, 1);
                }
                _ => {}
            }
            assert_eq!(game.phase, Phase::Playing);
        }
    }

    #[test]
    fn generated_course_has_fixed_apk_geometry_and_independent_heights() {
        let mut saw_large_adjacent_shift = false;

        for seed in 0..100 {
            let mut game = Game::new(70, 20, seed);
            game.start();
            finish_warmup(&mut game);
            assert_eq!(game.pipes.len(), PIPE_PAIR_COUNT);
            assert_eq!(game.pipes[0].x, f64::from(FIRST_PIPE_X));

            for pipe in &game.pipes {
                assert!((84..=263).contains(&pipe.gap_top));
                assert_eq!(pipe.gap_height, PIPE_GAP_HEIGHT);
            }
            for pair in game.pipes.windows(2) {
                assert_eq!(pair[1].x - pair[0].x, f64::from(PIPE_PITCH));
                saw_large_adjacent_shift |= pair[0].gap_top.abs_diff(pair[1].gap_top) > 30;
            }
        }

        assert!(
            saw_large_adjacent_shift,
            "pipe heights must not be adjacency-limited"
        );
        assert_eq!(PIPE_PITCH - PIPE_WIDTH, PIPE_EMPTY_SPACE);
    }

    #[test]
    fn pipe_speed_gap_and_pitch_never_change_with_score() {
        let mut game = active_game();
        let initial_x = game.pipes[0].x;
        game.score = u32::MAX;
        game.bird_y = f64::from(game.pipes[0].gap_top + 1);
        game.bird_velocity = -GRAVITY_PER_TICK;

        game.update(TICK);

        assert_eq!(initial_x - game.pipes[0].x, PIPE_SPEED_PER_TICK);
        assert_eq!(game.speed(), PIPE_SPEED_PER_TICK);
        assert_eq!(game.gap_height(), PIPE_GAP_HEIGHT);
        assert_eq!(game.pipes[1].x - game.pipes[0].x, f64::from(PIPE_PITCH));
    }

    #[test]
    fn pipe_scores_once_when_its_leading_edge_reaches_bird_x() {
        let mut game = active_game();
        game.bird_y = 181.0;
        game.bird_velocity = -GRAVITY_PER_TICK;
        game.pipes = vec![safe_pipe(game.bird_x + PIPE_SPEED_PER_TICK)];

        game.update(TICK);
        assert_eq!(game.score, 1);
        assert!(game.pipes[0].scored);

        for _ in 0..10 {
            game.bird_y = 181.0;
            game.bird_velocity = -GRAVITY_PER_TICK;
            game.update(TICK);
        }
        assert_eq!(game.score, 1);
    }

    #[test]
    fn same_seed_produces_same_course_and_restart_advances_it() {
        let mut original = Game::new(100, 30, 0xfeed_beef);
        let mut first = Game::new(100, 30, 0xfeed_beef);
        let mut second = first.clone();
        let mut different = Game::new(100, 30, 0xfeed_beee);

        for game in [&mut original, &mut first, &mut second, &mut different] {
            game.start();
            finish_warmup(game);
        }

        assert_eq!(first.pipes, second.pipes);
        assert_ne!(first.pipes, different.pipes);

        first.restart(u16::MAX, 0);
        second.restart(u16::MAX, 0);

        assert!(first.pipes.is_empty());
        assert_eq!(first.death_cause(), None);
        first.start();
        second.start();
        finish_warmup(&mut first);
        finish_warmup(&mut second);

        assert_eq!(first.width, MAX_FIELD_WIDTH);
        assert_eq!(first.height, MIN_FIELD_HEIGHT);
        assert_eq!(first.phase, Phase::Playing);
        assert_eq!(first.score, 0);
        assert!((first.elapsed - TICK * PIPE_WARMUP_TICKS as f64).abs() < STEP_EPSILON);
        assert_eq!(first.bird_x, f64::from(BIRD_START_X));
        assert_eq!(first.bird_y, f64::from(BIRD_START_Y));
        assert_eq!(first.pipes, second.pipes);
        assert_ne!(first.pipes, original.pipes);
    }

    #[test]
    fn medal_thresholds_match_the_original_game() {
        for score in 0..10 {
            assert_eq!(Medal::for_score(score), None);
        }
        assert_eq!(Medal::for_score(10), Some(Medal::Bronze));
        assert_eq!(Medal::for_score(19), Some(Medal::Bronze));
        assert_eq!(Medal::for_score(20), Some(Medal::Silver));
        assert_eq!(Medal::for_score(29), Some(Medal::Silver));
        assert_eq!(Medal::for_score(30), Some(Medal::Gold));
        assert_eq!(Medal::for_score(39), Some(Medal::Gold));
        assert_eq!(Medal::for_score(40), Some(Medal::Platinum));
        assert_eq!(Medal::for_score(100), Some(Medal::Platinum));
        assert_eq!(Medal::Gold.label(), "GOLD");

        let mut game = Game::new(70, 20, 1);
        game.score = 20;
        assert_eq!(game.medal(), Some(Medal::Silver));
    }
}
