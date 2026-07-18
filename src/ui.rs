//! Ratatui renderer for the game board and its surrounding HUD.

use ratatui::{
    Frame,
    buffer::{Buffer, Cell},
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::game::{
    BIRD_ART_HEIGHT, BIRD_ART_OFFSET_X, BIRD_ART_OFFSET_Y, BIRD_ART_WIDTH, BIRD_HEIGHT, BIRD_WIDTH,
    GRAVITY_PER_TICK, GROUND_Y, Game, MAX_FALL_VELOCITY, MAX_FIELD_HEIGHT, MAX_FIELD_WIDTH,
    MIN_FIELD_HEIGHT, MIN_FIELD_WIDTH, Medal, PIPE_SPEED_PER_TICK, PIPE_WIDTH, Phase,
    VIRTUAL_HEIGHT, VIRTUAL_WIDTH,
};
use crate::theme::{ResolvedTheme, ThemeState};

const HORIZONTAL_CHROME: u16 = 2;
const VERTICAL_CHROME: u16 = 6;
const HUD_MIN_WIDTH: u16 = 36;

// A typical terminal cell is about twice as tall as it is wide. Coupling the
// viewport to 9 columns for every 8 rows preserves the 288:512 game canvas in
// physical pixels instead of stretching it across a landscape terminal.
const PORTRAIT_COLUMNS: u32 = 9;
const PORTRAIT_ROWS: u32 = 8;

const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// Rendering switches controlled by command-line options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiOptions {
    /// Restrict artwork and borders to seven-bit ASCII.
    pub ascii: bool,
    /// Enable the color palette.
    pub color: bool,
    /// User-selected theme and its resolved light or dark appearance.
    pub theme: ThemeState,
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            ascii: false,
            color: true,
            theme: ThemeState::default(),
        }
    }
}

impl UiOptions {
    /// Advance System → Light → Dark → System.
    pub fn cycle_theme(&mut self) {
        self.theme.cycle();
    }
}

/// Return the largest supported play field that fits in `area`.
///
/// The returned dimensions are clamped to the simulation limits. On a terminal
/// smaller than the minimum, this deliberately returns the minimum dimensions;
/// [`fits`] can then be used to show the resize prompt instead of creating a
/// subtly different game.
pub fn field_size(area: Rect) -> (u16, u16) {
    let available_width = area
        .width
        .saturating_sub(HORIZONTAL_CHROME)
        .min(MAX_FIELD_WIDTH);
    let mut height = area
        .height
        .saturating_sub(VERTICAL_CHROME)
        .clamp(MIN_FIELD_HEIGHT, MAX_FIELD_HEIGHT);

    while height > MIN_FIELD_HEIGHT && portrait_width(height) > available_width {
        height -= 1;
    }

    (
        portrait_width(height).clamp(MIN_FIELD_WIDTH, MAX_FIELD_WIDTH),
        height,
    )
}

/// Whether `area` is large enough to host a newly-sized round.
pub fn can_start_round(area: Rect) -> bool {
    let (width, height) = field_size(area);
    area.width >= stage_width(width) && area.height >= height.saturating_add(VERTICAL_CHROME)
}

/// Return the terminal dimensions required to display `game` without clipping.
pub fn required_size(game: &Game) -> (u16, u16) {
    (
        stage_width(game.width),
        game.height.saturating_add(VERTICAL_CHROME),
    )
}

const fn portrait_width(height: u16) -> u16 {
    ((height as u32 * PORTRAIT_COLUMNS + PORTRAIT_ROWS / 2) / PORTRAIT_ROWS) as u16
}

const fn stage_width(field_width: u16) -> u16 {
    let field_with_border = field_width.saturating_add(HORIZONTAL_CHROME);
    if field_with_border > HUD_MIN_WIDTH {
        field_with_border
    } else {
        HUD_MIN_WIDTH
    }
}

/// Whether `area` can display the complete HUD, field, and controls.
pub fn fits(area: Rect, game: &Game) -> bool {
    let (needed_width, needed_height) = required_size(game);
    area.width >= needed_width && area.height >= needed_height
}

/// Draw one complete game frame at the latest fixed simulation state.
pub fn draw(frame: &mut Frame<'_>, game: &Game, best: u32, new_best: bool, options: UiOptions) {
    draw_interpolated(frame, game, best, new_best, options, 0.0);
}

/// Draw a game frame with moving objects sampled between fixed simulation ticks.
///
/// `tick_progress` is the fraction of the upcoming 60 Hz physics step which has
/// elapsed. It affects presentation only; scoring, collision detection, and all
/// other gameplay continue to use the fixed simulation state.
pub fn draw_interpolated(
    frame: &mut Frame<'_>,
    game: &Game,
    best: u32,
    new_best: bool,
    options: UiOptions,
    tick_progress: f64,
) {
    let area = frame.area();
    let palette = Palette::new(options.color, options.theme.resolved());
    let render_context = RenderContext {
        options,
        palette,
        tick_progress,
    };

    frame.render_widget(Block::default().style(palette.screen()), area);

    if !fits(area, game) {
        draw_too_small(frame, area, game, options, palette);
        return;
    }

    let (required_width, required_height) = required_size(game);
    let stage = centered(area, required_width, required_height);
    let header = Rect::new(stage.x, stage.y, stage.width, 3);
    let field_width = game.width.saturating_add(HORIZONTAL_CHROME);
    let field = Rect::new(
        stage
            .x
            .saturating_add(stage.width.saturating_sub(field_width) / 2),
        stage.y.saturating_add(3),
        field_width,
        game.height.saturating_add(2),
    );
    let footer = Rect::new(
        stage.x,
        field.y.saturating_add(field.height),
        stage.width,
        1,
    );

    draw_header(frame, header, game, best, options, palette);
    draw_field(frame, field, game, best, new_best, render_context);
    draw_footer(frame, footer, game.phase, options, palette);
}

#[derive(Clone, Copy)]
struct Palette {
    color: bool,
    theme: ResolvedTheme,
}

#[derive(Clone, Copy)]
struct RenderContext {
    options: UiOptions,
    palette: Palette,
    tick_progress: f64,
}

impl Palette {
    const SCREEN: Color = Color::Rgb(3, 7, 14);
    const PANEL: Color = Color::Rgb(8, 16, 29);
    const SKY_A: Color = Color::Rgb(5, 12, 24);
    const SKY_B: Color = Color::Rgb(6, 14, 27);
    const CYAN: Color = Color::Rgb(73, 226, 255);
    const MAGENTA: Color = Color::Rgb(255, 83, 203);
    const LIME: Color = Color::Rgb(137, 242, 83);
    const LIME_SHADOW: Color = Color::Rgb(42, 139, 68);
    const YELLOW: Color = Color::Rgb(255, 218, 72);
    const ORANGE: Color = Color::Rgb(255, 146, 64);
    const BRONZE: Color = Color::Rgb(205, 127, 50);
    const SILVER: Color = Color::Rgb(210, 220, 230);
    const PLATINUM: Color = Color::Rgb(118, 238, 220);
    const GROUND: Color = Color::Rgb(232, 215, 142);
    const GROUND_SHADOW: Color = Color::Rgb(153, 119, 65);
    const TEXT: Color = Color::Rgb(222, 237, 248);
    const DIM: Color = Color::Rgb(72, 95, 119);
    const DANGER: Color = Color::Rgb(255, 87, 111);
    const DAY_CLOUD: Color = Color::Rgb(239, 252, 247);

    const LIGHT_SCREEN: Color = Color::Rgb(218, 238, 246);
    const LIGHT_PANEL: Color = Color::Rgb(247, 252, 253);
    const LIGHT_SKY_A: Color = Color::Rgb(112, 197, 206);
    const LIGHT_SKY_B: Color = Color::Rgb(124, 203, 212);
    const LIGHT_CYAN: Color = Color::Rgb(0, 105, 128);
    const LIGHT_MAGENTA: Color = Color::Rgb(173, 42, 126);
    const LIGHT_LIME: Color = Color::Rgb(44, 110, 20);
    const LIGHT_LIME_SHADOW: Color = Color::Rgb(37, 105, 55);
    const LIGHT_YELLOW: Color = Color::Rgb(130, 80, 0);
    const LIGHT_ORANGE: Color = Color::Rgb(155, 65, 10);
    const LIGHT_SILVER: Color = Color::Rgb(104, 119, 132);
    const LIGHT_PLATINUM: Color = Color::Rgb(0, 126, 115);
    const LIGHT_TEXT: Color = Color::Rgb(20, 48, 61);
    const LIGHT_DIM: Color = Color::Rgb(62, 91, 105);
    const LIGHT_DANGER: Color = Color::Rgb(194, 38, 64);

    const fn new(color: bool, theme: ResolvedTheme) -> Self {
        Self { color, theme }
    }

    fn resolve(self, color: Color) -> Color {
        if self.theme == ResolvedTheme::Dark {
            return color;
        }

        if color == Self::SCREEN {
            Self::LIGHT_SCREEN
        } else if color == Self::PANEL {
            Self::LIGHT_PANEL
        } else if color == Self::SKY_A {
            Self::LIGHT_SKY_A
        } else if color == Self::SKY_B {
            Self::LIGHT_SKY_B
        } else if color == Self::CYAN {
            Self::LIGHT_CYAN
        } else if color == Self::MAGENTA {
            Self::LIGHT_MAGENTA
        } else if color == Self::LIME {
            Self::LIGHT_LIME
        } else if color == Self::LIME_SHADOW {
            Self::LIGHT_LIME_SHADOW
        } else if color == Self::YELLOW {
            Self::LIGHT_YELLOW
        } else if color == Self::ORANGE {
            Self::LIGHT_ORANGE
        } else if color == Self::SILVER {
            Self::LIGHT_SILVER
        } else if color == Self::PLATINUM {
            Self::LIGHT_PLATINUM
        } else if color == Self::TEXT {
            Self::LIGHT_TEXT
        } else if color == Self::DIM {
            Self::LIGHT_DIM
        } else if color == Self::DANGER {
            Self::LIGHT_DANGER
        } else {
            color
        }
    }

    fn fg(self, color: Color) -> Style {
        if self.color {
            Style::default().fg(self.resolve(color))
        } else {
            Style::default()
        }
    }

    fn fixed_fg(self, color: Color) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default()
        }
    }

    fn on(self, foreground: Color, background: Color) -> Style {
        if self.color {
            Style::default()
                .fg(self.resolve(foreground))
                .bg(self.resolve(background))
        } else {
            Style::default()
        }
    }

    fn fixed_on(self, foreground: Color, background: Color) -> Style {
        if self.color {
            Style::default().fg(foreground).bg(background)
        } else {
            Style::default()
        }
    }

    fn background(self, color: Color) -> Style {
        if self.color {
            Style::default().bg(self.resolve(color))
        } else {
            Style::default()
        }
    }

    fn screen(self) -> Style {
        self.background(Self::SCREEN)
    }

    fn panel(self) -> Style {
        self.background(Self::PANEL)
    }

    const fn is_light(self) -> bool {
        matches!(self.theme, ResolvedTheme::Light)
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn rounded_block<'a>(options: UiOptions) -> Block<'a> {
    let block = Block::default().borders(Borders::ALL);
    if options.ascii {
        block.border_set(ASCII_BORDER)
    } else {
        block.border_type(BorderType::Rounded)
    }
}

fn double_block<'a>(options: UiOptions) -> Block<'a> {
    let block = Block::default().borders(Borders::ALL);
    if options.ascii {
        block.border_set(ASCII_BORDER)
    } else {
        block.border_type(BorderType::Double)
    }
}

fn draw_header(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    best: u32,
    options: UiOptions,
    palette: Palette,
) {
    let title = Line::from(vec![
        Span::styled(
            " d",
            palette.fg(Palette::MAGENTA).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "bird ",
            palette.fg(Palette::CYAN).add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = rounded_block(options)
        .title(title)
        .border_style(palette.fg(Palette::CYAN))
        .style(palette.panel());

    let best = best.max(game.score);
    let separator = if options.ascii { " | " } else { " │ " };
    let line = Line::from(vec![
        Span::styled("SCORE ", palette.fg(Palette::CYAN)),
        Span::styled(
            format!("{:04}", game.score),
            palette.fg(Palette::TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(separator, palette.fg(Palette::DIM)),
        Span::styled("BEST ", palette.fg(Palette::MAGENTA)),
        Span::styled(
            format!("{best:04}"),
            palette.fg(Palette::TEXT).add_modifier(Modifier::BOLD),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(line)
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

fn draw_field(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    best: u32,
    new_best: bool,
    context: RenderContext,
) {
    let RenderContext {
        options,
        palette,
        tick_progress,
    } = context;
    let phase_label = match game.phase {
        Phase::Ready => " READY ",
        Phase::Playing => " FLYING ",
        Phase::Paused => " PAUSED ",
        Phase::Dying => " CRASHED ",
        Phase::GameOver => " GAME OVER ",
    };
    let phase_color = match game.phase {
        Phase::Ready | Phase::Playing => Palette::LIME,
        Phase::Paused => Palette::MAGENTA,
        Phase::Dying | Phase::GameOver => Palette::DANGER,
    };
    let field_block = double_block(options)
        .title(Line::styled(
            phase_label,
            palette.fg(phase_color).add_modifier(Modifier::BOLD),
        ))
        .border_style(palette.fg(Palette::DIM))
        .style(palette.background(Palette::SKY_A));
    frame.render_widget(field_block, area);

    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );

    draw_sky(frame, inner, game.elapsed, options, palette);
    draw_pipes(frame, inner, game, options, palette, tick_progress);
    draw_bird(frame, inner, game, options, palette, tick_progress);
    draw_ground(frame, inner, options, palette);

    match game.phase {
        Phase::Ready => draw_overlay(
            frame,
            inner,
            Overlay::Ready,
            game,
            best,
            new_best,
            options,
            palette,
        ),
        Phase::Paused => draw_overlay(
            frame,
            inner,
            Overlay::Paused,
            game,
            best,
            new_best,
            options,
            palette,
        ),
        Phase::GameOver => draw_overlay(
            frame,
            inner,
            Overlay::GameOver,
            game,
            best,
            new_best,
            options,
            palette,
        ),
        Phase::Playing | Phase::Dying => {}
    }
}

fn draw_sky(frame: &mut Frame<'_>, area: Rect, elapsed: f64, options: UiOptions, palette: Palette) {
    let drift = (elapsed * 2.0).floor().max(0.0) as u64;
    let buffer = frame.buffer_mut();

    for logical_y in 0..area.height {
        for logical_x in 0..area.width {
            let x = area.x.saturating_add(logical_x);
            let y = area.y.saturating_add(logical_y);
            let background = if logical_y % 2 == 0 {
                Palette::SKY_A
            } else {
                Palette::SKY_B
            };
            let shifted_x = u64::from(logical_x) + drift;
            let hash = shifted_x
                .wrapping_mul(73)
                .wrapping_add(u64::from(logical_y).wrapping_mul(151));
            let (symbol, foreground) = if palette.is_light() {
                let sun_x = area.width.saturating_sub(3);
                let cloud_hash = (shifted_x / 3)
                    .wrapping_mul(73)
                    .wrapping_add(u64::from(logical_y).wrapping_mul(151));
                if logical_y == 1 && logical_x == sun_x {
                    (if options.ascii { "*" } else { "●" }, Palette::YELLOW)
                } else if logical_y > 2
                    && logical_y < area.height.saturating_sub(4)
                    && cloud_hash % 97 == 0
                {
                    (if options.ascii { "." } else { "░" }, Palette::DAY_CLOUD)
                } else {
                    (" ", Palette::DAY_CLOUD)
                }
            } else if hash % 211 == 0 {
                ("+", Palette::TEXT)
            } else if hash % 79 == 0 {
                (".", Palette::DIM)
            } else {
                (" ", Palette::DIM)
            };

            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(symbol)
                    .set_style(palette.on(foreground, background));
            }
        }
    }
}

fn draw_pipes(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    options: UiOptions,
    palette: Palette,
    tick_progress: f64,
) {
    let buffer = frame.buffer_mut();
    let ground_top = project_floor(f64::from(GROUND_Y), VIRTUAL_HEIGHT, game.height)
        .clamp(0, i32::from(game.height));

    for pipe in &game.pipes {
        let visual_x = visual_pipe_x(pipe.x, game.phase, tick_progress);
        let pipe_left = project_exact(visual_x, VIRTUAL_WIDTH, game.width);
        let pipe_right = project_exact(visual_x + f64::from(PIPE_WIDTH), VIRTUAL_WIDTH, game.width);
        let gap_top = project_floor(f64::from(pipe.gap_top), VIRTUAL_HEIGHT, game.height);
        let gap_bottom = project_ceil(
            f64::from(pipe.gap_top + pipe.gap_height),
            VIRTUAL_HEIGHT,
            game.height,
        );
        // Preserve one visible lower-pipe cap when the virtual pipe starts in
        // the same coarse terminal row as the ground.
        let lower_pipe_top = visible_lower_pipe_top(gap_bottom, ground_top);

        if options.ascii {
            draw_ascii_pipe(
                buffer,
                area,
                game.width,
                ground_top,
                gap_top,
                lower_pipe_top,
                pipe_left,
                pipe_right,
                palette,
            );
            continue;
        }

        // A stable half-cell inset keeps wide shafts narrower than their caps.
        // Basing this on exact width avoids the old one-cell breathing as a
        // rounded edge crossed a character boundary.
        let shaft_inset = if pipe_right - pipe_left > 4.0 {
            0.5
        } else {
            0.0
        };

        for logical_y in 0..ground_top {
            if logical_y >= gap_top && logical_y < lower_pipe_top {
                continue;
            }

            let at_cap = logical_y + 1 == gap_top || logical_y == lower_pipe_top;
            let (left, right) = if at_cap {
                (pipe_left, pipe_right)
            } else {
                (pipe_left + shaft_inset, pipe_right - shaft_inset)
            };
            draw_unicode_pipe_row(buffer, area, game.width, logical_y, left, right, palette);
            if !at_cap && palette.color {
                draw_pipe_shadow_row(buffer, area, game.width, logical_y, left, right, palette);
            }
        }
    }
}

const LEFT_BLOCKS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
const LOWER_BLOCKS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

fn normalized_tick_progress(tick_progress: f64) -> f64 {
    if tick_progress.is_finite() {
        tick_progress.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn visual_pipe_x(pipe_x: f64, phase: Phase, tick_progress: f64) -> f64 {
    if phase != Phase::Playing {
        return pipe_x;
    }

    pipe_x - PIPE_SPEED_PER_TICK * normalized_tick_progress(tick_progress)
}

#[allow(clippy::too_many_arguments)]
fn draw_ascii_pipe(
    buffer: &mut Buffer,
    area: Rect,
    field_width: u16,
    ground_top: i32,
    gap_top: i32,
    lower_pipe_top: i32,
    exact_left: f64,
    exact_right: f64,
    palette: Palette,
) {
    let pipe_left = exact_left.floor() as i32;
    let pipe_right = exact_right.ceil() as i32;
    let shaft_inset = i32::from(pipe_right - pipe_left >= 5);
    let shaft_left = pipe_left + shaft_inset;
    let shaft_right = pipe_right - shaft_inset;

    for logical_x in pipe_left.max(0)..pipe_right.min(i32::from(field_width)) {
        for logical_y in 0..ground_top {
            if logical_y >= gap_top && logical_y < lower_pipe_top {
                continue;
            }

            let at_cap = logical_y + 1 == gap_top || logical_y == lower_pipe_top;
            let at_shaft_edge = !at_cap && (logical_x < shaft_left || logical_x >= shaft_right);
            let active_right = if at_cap { pipe_right } else { shaft_right };
            let at_shadow = logical_x + 1 == active_right;
            let symbol = if at_cap {
                "="
            } else if at_shaft_edge || at_shadow {
                "|"
            } else {
                "#"
            };
            let style = if at_shaft_edge || (at_shadow && !at_cap) {
                palette.fg(Palette::LIME_SHADOW)
            } else {
                palette.fg(Palette::LIME).add_modifier(Modifier::BOLD)
            };
            let x = area.x.saturating_add(logical_x as u16);
            let y = area.y.saturating_add(logical_y as u16);

            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(symbol).set_style(style);
            }
        }
    }
}

fn draw_unicode_pipe_row(
    buffer: &mut Buffer,
    area: Rect,
    field_width: u16,
    logical_y: i32,
    left: f64,
    right: f64,
    palette: Palette,
) {
    let first_cell = (left.floor() as i32).max(0);
    let last_cell = (right.ceil() as i32).min(i32::from(field_width));
    let background = if logical_y % 2 == 0 {
        Palette::SKY_A
    } else {
        Palette::SKY_B
    };

    for logical_x in first_cell..last_cell {
        let cell_left = f64::from(logical_x);
        let coverage_start = (left - cell_left).clamp(0.0, 1.0);
        let coverage_end = (right - cell_left).clamp(0.0, 1.0);
        if coverage_end <= coverage_start {
            continue;
        }

        let x = area.x.saturating_add(logical_x as u16);
        let y = area.y.saturating_add(logical_y as u16);

        if let Some(cell) = buffer.cell_mut((x, y)) {
            draw_horizontal_subcell(
                cell,
                coverage_start,
                coverage_end,
                "█",
                Palette::LIME,
                background,
                palette,
                true,
            );
        }
    }
}

fn draw_pipe_shadow_row(
    buffer: &mut Buffer,
    area: Rect,
    field_width: u16,
    logical_y: i32,
    pipe_left: f64,
    pipe_right: f64,
    palette: Palette,
) {
    const SHADOW_WIDTH: f64 = 1.0;

    let shadow_left = (pipe_right - SHADOW_WIDTH).max(pipe_left);
    let first_cell = (shadow_left.floor() as i32).max(0);
    let last_cell = (pipe_right.ceil() as i32).min(i32::from(field_width));
    let sky = if logical_y % 2 == 0 {
        Palette::SKY_A
    } else {
        Palette::SKY_B
    };

    for logical_x in first_cell..last_cell {
        let cell_left = f64::from(logical_x);
        let start_eighth = quantize_eighths((shadow_left - cell_left).clamp(0.0, 1.0));
        let end_eighth = quantize_eighths((pipe_right - cell_left).clamp(0.0, 1.0));
        if end_eighth <= start_eighth {
            continue;
        }

        let (symbol, style) = if start_eighth == 0 && end_eighth == 8 {
            ("█", palette.fg(Palette::LIME_SHADOW))
        } else if end_eighth == 8 {
            // The body occupies the left fraction and the shadow the right.
            (
                LEFT_BLOCKS[start_eighth],
                palette.on(Palette::LIME, Palette::LIME_SHADOW),
            )
        } else if start_eighth == 0 {
            // The shadow occupies the left fraction and sky the right.
            (
                LEFT_BLOCKS[end_eighth],
                palette.on(Palette::LIME_SHADOW, sky),
            )
        } else {
            // The pipe is always wider than its one-cell shadow, so this is only
            // a defensive fallback for an interval clipped at both boundaries.
            (
                LEFT_BLOCKS[end_eighth - start_eighth],
                palette.on(Palette::LIME_SHADOW, sky),
            )
        };
        let x = area.x.saturating_add(logical_x as u16);
        let y = area.y.saturating_add(logical_y as u16);

        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_style(style);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_horizontal_subcell(
    cell: &mut Cell,
    coverage_start: f64,
    coverage_end: f64,
    full_symbol: &'static str,
    fill: Color,
    background: Color,
    palette: Palette,
    bold: bool,
) {
    let start_eighth = quantize_eighths(coverage_start);
    let end_eighth = quantize_eighths(coverage_end);
    if end_eighth <= start_eighth {
        return;
    }

    let mut style = if start_eighth == 0 && end_eighth == 8 {
        cell.set_symbol(full_symbol);
        palette.fg(fill)
    } else if start_eighth == 0 {
        cell.set_symbol(LEFT_BLOCKS[end_eighth]);
        palette.on(fill, background)
    } else if end_eighth == 8 {
        // Unicode has left-aligned eighth blocks only. Swapping foreground and
        // background turns the empty left fraction into a right-aligned fill.
        cell.set_symbol(LEFT_BLOCKS[start_eighth]);
        if palette.color {
            palette.on(background, fill)
        } else {
            Style::default().add_modifier(Modifier::REVERSED)
        }
    } else {
        // Pipe spans are wider than a cell, so this is only a defensive fallback
        // for a heavily clipped interval.
        cell.set_symbol(LEFT_BLOCKS[end_eighth - start_eighth]);
        palette.on(fill, background)
    };

    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    cell.set_style(style);
}

fn quantize_eighths(value: f64) -> usize {
    (value.clamp(0.0, 1.0) * 8.0).round() as usize
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BirdPose {
    Up,
    Level,
    Down,
}

const BIRD_UP_ANGLE_CUTOFF: f64 = -5.0;
const BIRD_DOWN_ANGLE_CUTOFF: f64 = 30.0;

const ASCII_BIRD_UP: [[char; 6]; 2] = [
    [' ', '.', '-', 'o', '^', '^'],
    ['<', '=', '/', '/', ' ', ' '],
];
const ASCII_BIRD_LEVEL: [[char; 6]; 2] = [
    [' ', '.', '-', 'o', '>', '>'],
    ['<', '=', '/', '-', '>', ' '],
];
const ASCII_BIRD_DOWN: [[char; 6]; 2] = [
    [' ', '.', '-', 'o', '\\', ' '],
    ['<', '=', '/', '-', 'v', 'v'],
];

fn bird_pose(game: &Game) -> BirdPose {
    bird_pose_for(game.phase, game.bird_angle())
}

fn bird_pose_for(phase: Phase, angle: f64) -> BirdPose {
    if phase == Phase::Ready {
        BirdPose::Level
    } else if phase == Phase::GameOver || angle >= BIRD_DOWN_ANGLE_CUTOFF {
        BirdPose::Down
    } else if angle < BIRD_UP_ANGLE_CUTOFF {
        BirdPose::Up
    } else {
        BirdPose::Level
    }
}

const fn ascii_bird_artwork(pose: BirdPose) -> &'static [[char; 6]; 2] {
    match pose {
        BirdPose::Up => &ASCII_BIRD_UP,
        BirdPose::Level => &ASCII_BIRD_LEVEL,
        BirdPose::Down => &ASCII_BIRD_DOWN,
    }
}

const fn compact_ascii_bird(pose: BirdPose) -> [(char, Color); 2] {
    match pose {
        BirdPose::Up => [('o', Palette::TEXT), ('^', Palette::ORANGE)],
        BirdPose::Level => [('o', Palette::TEXT), ('>', Palette::ORANGE)],
        BirdPose::Down => [('o', Palette::TEXT), ('v', Palette::ORANGE)],
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BirdGeometry {
    body_left: i32,
    body_right: i32,
    top_eighth: i32,
    bottom_eighth: i32,
}

impl BirdGeometry {
    const fn body_width(self) -> i32 {
        self.body_right - self.body_left
    }

    const fn body_height_eighths(self) -> i32 {
        self.bottom_eighth - self.top_eighth
    }

    const fn tail_x(self) -> i32 {
        self.body_left - 1
    }

    const fn beak_x(self) -> i32 {
        self.body_right
    }

    const fn eye_x(self) -> i32 {
        self.body_right - 1
    }

    fn column_tilt_eighths(self, pose: BirdPose, logical_x: i32) -> i32 {
        debug_assert!(logical_x >= self.body_left && logical_x < self.body_right);

        let column = logical_x - self.body_left;
        // Terminal rows are roughly twice as tall as columns are wide. Moving
        // each adjacent body column by half a row therefore produces an
        // approximately 45-degree physical slope.
        let nose_up_offset = (self.body_width() - 1 - 2 * column) * 2;
        match pose {
            BirdPose::Up => nose_up_offset,
            BirdPose::Level => 0,
            BirdPose::Down => -nose_up_offset,
        }
    }

    fn column_top_eighth(self, pose: BirdPose, logical_x: i32) -> i32 {
        self.top_eighth + self.column_tilt_eighths(pose, logical_x)
    }

    fn column_bottom_eighth(self, pose: BirdPose, logical_x: i32) -> i32 {
        self.bottom_eighth + self.column_tilt_eighths(pose, logical_x)
    }

    fn detail_eighth(self, pose: BirdPose, logical_x: i32) -> i32 {
        // Every detail uses the same local anchor inside its body column.
        self.column_top_eighth(pose, logical_x) + self.body_height_eighths() * 3 / 8
    }

    fn detail_y(self, pose: BirdPose, logical_x: i32) -> i32 {
        self.detail_eighth(pose, logical_x).div_euclid(8)
    }

    #[cfg(test)]
    fn silhouette_top_eighth(self, pose: BirdPose) -> i32 {
        (self.body_left..self.body_right)
            .map(|logical_x| self.column_top_eighth(pose, logical_x))
            .min()
            .unwrap_or(self.top_eighth)
    }

    #[cfg(test)]
    fn silhouette_bottom_eighth(self, pose: BirdPose) -> i32 {
        (self.body_left..self.body_right)
            .map(|logical_x| self.column_bottom_eighth(pose, logical_x))
            .max()
            .unwrap_or(self.bottom_eighth)
    }
}

fn visual_bird_y(game: &Game, tick_progress: f64) -> f64 {
    if !matches!(game.phase, Phase::Playing | Phase::Dying) {
        return game.bird_y;
    }

    let next_velocity = (game.bird_velocity + GRAVITY_PER_TICK).min(MAX_FALL_VELOCITY);
    let ground_limit = f64::from(GROUND_Y - BIRD_HEIGHT);
    let next_y = (game.bird_y + next_velocity).trunc().min(ground_limit);
    let progress = normalized_tick_progress(tick_progress);

    game.bird_y + (next_y - game.bird_y) * progress
}

fn bird_geometry(game: &Game, visual_y: f64) -> BirdGeometry {
    let body_left = project_round(game.bird_x, VIRTUAL_WIDTH, game.width);
    let body_width = project_extent(BIRD_WIDTH, VIRTUAL_WIDTH, game.width).max(2);
    let body_height_eighths = project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, game.height) * 8;
    let center_eighth = (project_exact(
        visual_y + f64::from(BIRD_HEIGHT) / 2.0,
        VIRTUAL_HEIGHT,
        game.height,
    ) * 8.0)
        .round() as i32;
    let top_eighth = center_eighth - body_height_eighths / 2;

    BirdGeometry {
        body_left,
        body_right: body_left + body_width,
        top_eighth,
        bottom_eighth: top_eighth + body_height_eighths,
    }
}

fn draw_bird(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    options: UiOptions,
    palette: Palette,
    tick_progress: f64,
) {
    let pose = bird_pose(game);
    let visual_y = visual_bird_y(game, tick_progress);

    if options.ascii {
        draw_ascii_bird(frame, area, game, pose, palette, visual_y);
    } else {
        draw_unicode_bird(frame, area, game, pose, palette, visual_y);
    }
}

fn draw_ascii_bird(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    pose: BirdPose,
    palette: Palette,
    visual_y: f64,
) {
    // The source atlas stores each bird in a transparent 48x48 frame. Only a
    // 34x24 rectangle is opaque, so rendering the whole frame makes the bird
    // almost as wide as a pipe. These bounds reproduce the actual artwork.
    let visual_left = game.bird_x - f64::from(BIRD_ART_OFFSET_X);
    let visual_top = visual_y - f64::from(BIRD_ART_OFFSET_Y);
    let left = project_round(visual_left, VIRTUAL_WIDTH, game.width);
    let top = project_round(visual_top, VIRTUAL_HEIGHT, game.height);
    let sprite_width = project_extent(BIRD_ART_WIDTH, VIRTUAL_WIDTH, game.width);
    let sprite_height = project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, game.height);
    let right = left + sprite_width;
    let bottom = top + sprite_height;
    let buffer = frame.buffer_mut();

    if sprite_height == 1 {
        let compact = compact_ascii_bird(pose);
        let compact_width = sprite_width.min(compact.len() as i32);
        let compact_left = left + (sprite_width - compact_width) / 2;

        if top >= 0 && top < i32::from(game.height) {
            for logical_x in
                compact_left.max(0)..(compact_left + compact_width).min(i32::from(game.width))
            {
                let index = (logical_x - compact_left) as usize;
                let (symbol, color) = compact[index];
                let x = area.x.saturating_add(logical_x as u16);
                let y = area.y.saturating_add(top as u16);
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    let style = if symbol == 'o' {
                        palette.fg(color)
                    } else {
                        palette.fixed_fg(color)
                    };
                    cell.set_char(symbol)
                        .set_style(style.add_modifier(Modifier::BOLD));
                }
            }
        }
        return;
    }

    let artwork = ascii_bird_artwork(pose);
    for logical_y in top.max(0)..bottom.min(i32::from(game.height)) {
        let template_y = ((logical_y - top) * artwork.len() as i32 / sprite_height) as usize;
        for logical_x in left.max(0)..right.min(i32::from(game.width)) {
            let template_x = ((logical_x - left) * artwork[0].len() as i32 / sprite_width) as usize;
            let symbol =
                artwork[template_y.min(artwork.len() - 1)][template_x.min(artwork[0].len() - 1)];
            if symbol == ' ' {
                continue;
            }

            let color = if matches!(symbol, '>' | '^' | 'v') {
                Palette::ORANGE
            } else if symbol == 'o' {
                Palette::TEXT
            } else {
                Palette::YELLOW
            };
            let x = area.x.saturating_add(logical_x as u16);
            let y = area.y.saturating_add(logical_y as u16);
            if let Some(cell) = buffer.cell_mut((x, y)) {
                let style = if symbol == 'o' {
                    palette.fg(color)
                } else {
                    palette.fixed_fg(color)
                };
                cell.set_char(symbol)
                    .set_style(style.add_modifier(Modifier::BOLD));
            }
        }
    }
}

fn draw_unicode_bird(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    pose: BirdPose,
    palette: Palette,
    visual_y: f64,
) {
    let geometry = bird_geometry(game, visual_y);
    let buffer = frame.buffer_mut();

    // The solid body stays centered on the collision box. Tail and beak are
    // decorative, while every body column shares one pose-aware tilt model.
    for logical_x in geometry.body_left..geometry.body_right {
        let rounded_edge = geometry.body_width() >= 3
            && geometry.body_height_eighths() >= 10
            && matches!(logical_x, x if x == geometry.body_left || x + 1 == geometry.body_right);
        let edge_inset = i32::from(rounded_edge);
        draw_vertical_eighth_span(
            buffer,
            area,
            game.height,
            logical_x,
            geometry.column_top_eighth(pose, logical_x) + edge_inset,
            geometry.column_bottom_eighth(pose, logical_x) - edge_inset,
            Palette::YELLOW,
            palette,
        );
    }

    let tail_y = geometry.detail_y(pose, geometry.body_left);
    draw_bird_detail(
        buffer,
        area,
        game.width,
        geometry.tail_x(),
        tail_y,
        '◀',
        palette
            .fixed_fg(Palette::YELLOW)
            .add_modifier(Modifier::BOLD),
    );

    let eye_x = geometry.eye_x();
    let face_y = geometry.detail_y(pose, eye_x);
    draw_bird_detail(
        buffer,
        area,
        game.width,
        eye_x,
        face_y,
        '●',
        palette
            .fixed_on(Palette::TEXT, Palette::YELLOW)
            .add_modifier(Modifier::BOLD),
    );
    draw_bird_detail(
        buffer,
        area,
        game.width,
        geometry.beak_x(),
        face_y,
        '▶',
        palette
            .fixed_fg(Palette::ORANGE)
            .add_modifier(Modifier::BOLD),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_vertical_eighth_span(
    buffer: &mut Buffer,
    area: Rect,
    field_height: u16,
    logical_x: i32,
    top_eighth: i32,
    bottom_eighth: i32,
    fill: Color,
    palette: Palette,
) {
    if logical_x < 0 || logical_x >= i32::from(area.width) || bottom_eighth <= top_eighth {
        return;
    }

    let first_row = top_eighth.div_euclid(8).max(0);
    let last_row = ((bottom_eighth + 7).div_euclid(8)).min(i32::from(field_height));

    for logical_y in first_row..last_row {
        let row_top = logical_y * 8;
        let start_eighth = (top_eighth - row_top).clamp(0, 8) as usize;
        let end_eighth = (bottom_eighth - row_top).clamp(0, 8) as usize;
        if end_eighth <= start_eighth {
            continue;
        }

        let sky = if logical_y % 2 == 0 {
            Palette::SKY_A
        } else {
            Palette::SKY_B
        };
        let (symbol, mut style) = if start_eighth == 0 && end_eighth == 8 {
            ("█", palette.fixed_fg(fill))
        } else if end_eighth == 8 {
            (
                LOWER_BLOCKS[8 - start_eighth],
                palette.fixed_on(fill, palette.resolve(sky)),
            )
        } else if start_eighth == 0 {
            let style = if palette.color {
                palette.fixed_on(palette.resolve(sky), fill)
            } else {
                Style::default().add_modifier(Modifier::REVERSED)
            };
            (LOWER_BLOCKS[8 - end_eighth], style)
        } else {
            // Bird spans are at least one row high, so this is only a defensive
            // fallback after extreme viewport clipping.
            (
                LOWER_BLOCKS[end_eighth - start_eighth],
                palette.fixed_on(fill, palette.resolve(sky)),
            )
        };
        style = style.add_modifier(Modifier::BOLD);

        let x = area.x.saturating_add(logical_x as u16);
        let y = area.y.saturating_add(logical_y as u16);
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_style(style);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bird_detail(
    buffer: &mut Buffer,
    area: Rect,
    field_width: u16,
    logical_x: i32,
    logical_y: i32,
    symbol: char,
    style: Style,
) {
    if logical_x < 0
        || logical_x >= i32::from(field_width)
        || logical_y < 0
        || logical_y >= i32::from(area.height)
    {
        return;
    }

    let x = area.x.saturating_add(logical_x as u16);
    let y = area.y.saturating_add(logical_y as u16);
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_char(symbol).set_style(style);
    }
}

fn draw_ground(frame: &mut Frame<'_>, area: Rect, options: UiOptions, palette: Palette) {
    let ground_top = project_floor(f64::from(GROUND_Y), VIRTUAL_HEIGHT, area.height)
        .clamp(0, i32::from(area.height)) as u16;
    let buffer = frame.buffer_mut();

    for logical_y in ground_top..area.height {
        for logical_x in 0..area.width {
            let top_edge = logical_y == ground_top;
            let symbol = if top_edge {
                if options.ascii { "=" } else { "▀" }
            } else if (logical_x + logical_y) % 3 == 0 {
                if options.ascii { "." } else { "░" }
            } else {
                " "
            };
            let style = if top_edge {
                palette.on(Palette::LIME, Palette::GROUND)
            } else {
                palette.on(Palette::GROUND_SHADOW, Palette::GROUND)
            };
            let x = area.x.saturating_add(logical_x);
            let y = area.y.saturating_add(logical_y);
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(symbol).set_style(style);
            }
        }
    }
}

fn project_floor(value: f64, virtual_extent: u16, terminal_extent: u16) -> i32 {
    project_exact(value, virtual_extent, terminal_extent).floor() as i32
}

fn project_ceil(value: f64, virtual_extent: u16, terminal_extent: u16) -> i32 {
    project_exact(value, virtual_extent, terminal_extent).ceil() as i32
}

fn project_round(value: f64, virtual_extent: u16, terminal_extent: u16) -> i32 {
    project_exact(value, virtual_extent, terminal_extent).round() as i32
}

fn project_exact(value: f64, virtual_extent: u16, terminal_extent: u16) -> f64 {
    value * f64::from(terminal_extent) / f64::from(virtual_extent)
}

fn project_extent(value: u16, virtual_extent: u16, terminal_extent: u16) -> i32 {
    project_round(f64::from(value), virtual_extent, terminal_extent).max(1)
}

fn visible_lower_pipe_top(projected_pipe_top: i32, projected_ground_top: i32) -> i32 {
    projected_pipe_top.min((projected_ground_top - 1).max(0))
}

#[derive(Clone, Copy)]
enum Overlay {
    Ready,
    Paused,
    GameOver,
}

#[allow(clippy::too_many_arguments)]
fn draw_overlay(
    frame: &mut Frame<'_>,
    field: Rect,
    overlay: Overlay,
    game: &Game,
    best: u32,
    new_best: bool,
    options: UiOptions,
    palette: Palette,
) {
    let height = match overlay {
        Overlay::GameOver => 10,
        Overlay::Paused => 7,
        Overlay::Ready => 5,
    }
    .min(field.height);
    let width = 42.min(field.width);
    let area = if matches!(overlay, Overlay::Ready) {
        Rect::new(
            field
                .x
                .saturating_add(field.width.saturating_sub(width) / 2),
            field.y,
            width,
            height,
        )
    } else {
        centered(field, width, height)
    };

    let (title, title_color) = match overlay {
        Overlay::Ready => (" READY? ", Palette::LIME),
        Overlay::Paused => (" PAUSED ", Palette::MAGENTA),
        Overlay::GameOver => (" RUN OVER ", Palette::DANGER),
    };
    let block = rounded_block(options)
        .title(Line::styled(
            title,
            palette.fg(title_color).add_modifier(Modifier::BOLD),
        ))
        .border_style(palette.fg(title_color))
        .style(palette.panel());

    let compact = field.width < 24;
    let content = match overlay {
        Overlay::Ready => vec![
            Line::from(""),
            Line::styled(
                if compact {
                    "ENTER TO START"
                } else {
                    "PRESS ENTER TO START"
                },
                palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                if compact {
                    "SPACE FLAPS"
                } else {
                    "Space flaps during flight."
                },
                palette.fg(Palette::TEXT),
            ),
        ],
        Overlay::Paused => vec![
            Line::from(""),
            Line::styled(
                if compact {
                    "PAUSED"
                } else {
                    "FLIGHT SUSPENDED"
                },
                palette.fg(Palette::MAGENTA).add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                if compact {
                    "P TO RESUME"
                } else {
                    "Press P to resume"
                },
                palette.fg(Palette::TEXT),
            ),
        ],
        Overlay::GameOver => {
            let best = best.max(game.score);
            let result = if new_best {
                Line::styled(
                    "NEW BEST",
                    palette.fg(Palette::LIME).add_modifier(Modifier::BOLD),
                )
            } else {
                Line::styled("FINAL SCORE", palette.fg(Palette::DIM))
            };
            let medal = match game.medal() {
                Some(medal) => Line::styled(
                    format!("{} MEDAL", medal.label()),
                    palette.fg(medal_color(medal)).add_modifier(Modifier::BOLD),
                ),
                None => Line::styled("NO MEDAL", palette.fg(Palette::DIM)),
            };
            let score = if compact {
                Line::from(vec![
                    Span::styled(
                        format!("{:04}", game.score),
                        palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("/", palette.fg(Palette::DIM)),
                    Span::styled(
                        format!("{best:04}"),
                        palette.fg(Palette::MAGENTA).add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("{:04}", game.score),
                        palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  /  BEST ", palette.fg(Palette::DIM)),
                    Span::styled(
                        format!("{best:04}"),
                        palette.fg(Palette::MAGENTA).add_modifier(Modifier::BOLD),
                    ),
                ])
            };
            vec![
                Line::from(""),
                result,
                score,
                medal,
                Line::from(""),
                Line::styled(
                    if compact {
                        "ENTER TO RETRY"
                    } else {
                        "PRESS ENTER TO RETRY"
                    },
                    palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
                ),
            ]
        }
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(content)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
}

const fn medal_color(medal: Medal) -> Color {
    match medal {
        Medal::Bronze => Palette::BRONZE,
        Medal::Silver => Palette::SILVER,
        Medal::Gold => Palette::YELLOW,
        Medal::Platinum => Palette::PLATINUM,
    }
}

fn draw_footer(
    frame: &mut Frame<'_>,
    area: Rect,
    phase: Phase,
    options: UiOptions,
    palette: Palette,
) {
    let separator = "  ";
    let mut spans = match phase {
        Phase::Ready => vec![
            key_span("ENTER", Palette::CYAN, palette),
            Span::styled(" start", palette.fg(Palette::DIM)),
        ],
        Phase::Playing => vec![
            key_span("SPACE", Palette::CYAN, palette),
            Span::styled(" flap", palette.fg(Palette::DIM)),
            Span::styled(separator, palette.fg(Palette::DIM)),
            key_span("P", Palette::MAGENTA, palette),
            Span::styled(" pause", palette.fg(Palette::DIM)),
        ],
        Phase::Paused => vec![
            key_span("P", Palette::MAGENTA, palette),
            Span::styled(" resume", palette.fg(Palette::DIM)),
        ],
        Phase::Dying => vec![Span::styled("...", palette.fg(Palette::DANGER))],
        Phase::GameOver => vec![
            key_span("ENTER", Palette::CYAN, palette),
            Span::styled(" retry", palette.fg(Palette::DIM)),
        ],
    };
    spans.extend([
        Span::styled(separator, palette.fg(Palette::DIM)),
        key_span("T", Palette::YELLOW, palette),
        Span::styled(
            format!(" {}", options.theme.mode().label()),
            palette.fg(Palette::DIM),
        ),
        Span::styled(separator, palette.fg(Palette::DIM)),
        key_span("Q", Palette::DANGER, palette),
        Span::styled(" quit", palette.fg(Palette::DIM)),
    ]);
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

fn key_span(label: &'static str, color: Color, palette: Palette) -> Span<'static> {
    Span::styled(label, palette.fg(color).add_modifier(Modifier::BOLD))
}

fn draw_too_small(
    frame: &mut Frame<'_>,
    area: Rect,
    game: &Game,
    options: UiOptions,
    palette: Palette,
) {
    if area.is_empty() {
        return;
    }

    let (needed_width, needed_height) = required_size(game);
    let missing_width = needed_width.saturating_sub(area.width);
    let missing_height = needed_height.saturating_sub(area.height);
    let dialog = centered(area, 48.min(area.width), 9.min(area.height));
    let title = Line::styled(
        " RESIZE TERMINAL ",
        palette.fg(Palette::DANGER).add_modifier(Modifier::BOLD),
    );
    let block = rounded_block(options)
        .title(title)
        .border_style(palette.fg(Palette::DANGER))
        .style(palette.panel());
    let lines = vec![
        Line::from(""),
        Line::styled(
            "NOT ENOUGH SKY",
            palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
        ),
        Line::from(vec![
            Span::styled("CURRENT ", palette.fg(Palette::DIM)),
            Span::styled(
                format!("{}x{}", area.width, area.height),
                palette.fg(Palette::TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   NEEDED ", palette.fg(Palette::DIM)),
            Span::styled(
                format!("{needed_width}x{needed_height}"),
                palette.fg(Palette::CYAN).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::styled(
            format!("Grow by {missing_width} columns and {missing_height} rows"),
            palette.fg(Palette::TEXT),
        ),
        Line::from(""),
        Line::styled(
            if can_start_round(area) && matches!(game.phase, Phase::Ready | Phase::GameOver) {
                format!(
                    "ENTER fit/start  T {}  Q quit",
                    options.theme.mode().label()
                )
            } else {
                format!(
                    "Resize to continue  T {}  Q quit",
                    options.theme.mode().label()
                )
            },
            palette.fg(Palette::DIM),
        ),
    ];

    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        dialog,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Style},
    };

    use super::*;
    use crate::game::{BIRD_START_X, FIXED_STEP_SECONDS, Pipe};
    use crate::theme::ThemeMode;

    fn test_options(ascii: bool, mode: ThemeMode) -> UiOptions {
        UiOptions {
            ascii,
            color: true,
            theme: ThemeState::explicit(mode),
        }
    }

    fn game_with_pose(pose: BirdPose, width: u16, height: u16) -> Game {
        let mut game = Game::new(width, height, 7);
        match pose {
            BirdPose::Level => game.phase = Phase::Playing,
            BirdPose::Up => {
                assert!(game.start());
                game.update(FIXED_STEP_SECONDS);
            }
            BirdPose::Down => {
                assert!(game.start());
                for _ in 0..45 {
                    game.update(FIXED_STEP_SECONDS);
                }
            }
        }
        assert_eq!(bird_pose(&game), pose);
        game.bird_x = f64::from(BIRD_START_X);
        game.bird_y = 246.0;
        game
    }

    fn isolated_bird_rows(pose: BirdPose, ascii: bool, width: u16, height: u16) -> Vec<String> {
        let game = game_with_pose(pose, width, height);
        let options = test_options(ascii, ThemeMode::Dark);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                draw_bird(
                    frame,
                    Rect::new(0, 0, width, height),
                    &game,
                    options,
                    Palette::new(true, ResolvedTheme::Dark),
                    0.0,
                );
            })
            .expect("draw bird");

        let (left, top, right, bottom) = if ascii {
            let left = project_round(
                game.bird_x - f64::from(BIRD_ART_OFFSET_X),
                VIRTUAL_WIDTH,
                width,
            );
            let top = project_round(
                game.bird_y - f64::from(BIRD_ART_OFFSET_Y),
                VIRTUAL_HEIGHT,
                height,
            );
            (
                left,
                top,
                left + project_extent(BIRD_ART_WIDTH, VIRTUAL_WIDTH, width),
                top + project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, height),
            )
        } else {
            let geometry = bird_geometry(&game, game.bird_y);
            (
                geometry.tail_x(),
                geometry.silhouette_top_eighth(pose).div_euclid(8),
                geometry.beak_x() + 1,
                (geometry.silhouette_bottom_eighth(pose) + 7).div_euclid(8),
            )
        };
        let buffer = terminal.backend().buffer();

        (top..bottom)
            .map(|y| {
                (left..right)
                    .map(|x| buffer[(x as u16, y as u16)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn contrast_ratio(foreground: Color, background: Color) -> f64 {
        fn luminance(color: Color) -> f64 {
            fn linear(channel: u8) -> f64 {
                let channel = f64::from(channel) / 255.0;
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            }

            let Color::Rgb(red, green, blue) = color else {
                panic!("contrast tests require RGB colors");
            };
            0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
        }

        let lighter = luminance(foreground).max(luminance(background));
        let darker = luminance(foreground).min(luminance(background));
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn field_dimensions_preserve_the_portrait_canvas() {
        assert_eq!(field_size(Rect::new(0, 0, 80, 24)), (20, 18));
        assert_eq!(
            field_size(Rect::new(0, 0, 1, 1)),
            (MIN_FIELD_WIDTH, MIN_FIELD_HEIGHT)
        );
        assert_eq!(
            field_size(Rect::new(0, 0, u16::MAX, u16::MAX)),
            (MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT)
        );
        assert!(can_start_round(Rect::new(0, 0, 36, 20)));
        assert!(!can_start_round(Rect::new(0, 0, 35, 20)));
        assert!(!can_start_round(Rect::new(0, 0, 36, 19)));

        for area in [
            Rect::new(0, 0, 80, 24),
            Rect::new(0, 0, 100, 36),
            Rect::new(0, 0, 200, 100),
        ] {
            let (width, height) = field_size(area);
            let physical_aspect = f64::from(width) / (2.0 * f64::from(height));
            let original_aspect = f64::from(VIRTUAL_WIDTH) / f64::from(VIRTUAL_HEIGHT);
            assert!((physical_aspect - original_aspect).abs() < 0.02);
        }
    }

    #[test]
    fn hud_stays_readable_without_restretching_the_field() {
        let game = Game::new(20, 18, 7);
        assert_eq!(required_size(&game), (HUD_MIN_WIDTH, 24));
        assert!(fits(Rect::new(0, 0, HUD_MIN_WIDTH, 24), &game));
        assert!(!fits(Rect::new(0, 0, HUD_MIN_WIDTH - 1, 24), &game));
        assert!(!fits(Rect::new(0, 0, HUD_MIN_WIDTH, 23), &game));

        let stage = centered(Rect::new(0, 0, HUD_MIN_WIDTH, 24), HUD_MIN_WIDTH, 24);
        let field_width = game.width + HORIZONTAL_CHROME;
        let field_x = stage.x + (stage.width - field_width) / 2;
        assert_eq!(field_width, 22);
        assert_eq!(field_x, 7);
    }

    #[test]
    fn bird_pose_buckets_the_original_rotation_without_moving_the_hitbox() {
        assert_eq!(bird_pose_for(Phase::Ready, -20.0), BirdPose::Level);
        assert_eq!(bird_pose_for(Phase::Playing, -5.01), BirdPose::Up);
        assert_eq!(bird_pose_for(Phase::Playing, -5.0), BirdPose::Level);
        assert_eq!(bird_pose_for(Phase::Paused, 0.0), BirdPose::Level);
        assert_eq!(bird_pose_for(Phase::Dying, 29.99), BirdPose::Level);
        assert_eq!(bird_pose_for(Phase::Dying, 30.0), BirdPose::Down);
        assert_eq!(bird_pose_for(Phase::GameOver, -20.0), BirdPose::Down);
        assert_eq!(bird_pose_for(Phase::Playing, f64::NAN), BirdPose::Level);

        for pose in [BirdPose::Up, BirdPose::Level, BirdPose::Down] {
            let game = game_with_pose(pose, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT);
            assert_eq!(game.bird_x, f64::from(BIRD_START_X));
            assert_eq!(game.bird_y, 246.0);
            assert_eq!(project_extent(BIRD_ART_WIDTH, VIRTUAL_WIDTH, game.width), 5);
            assert_eq!(
                project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, game.height),
                2
            );

            let geometry = bird_geometry(&game, game.bird_y);
            assert_eq!(geometry.body_left, 13);
            assert_eq!(geometry.body_right, 16);
            assert_eq!(geometry.body_height_eighths(), 16);
            assert_eq!(geometry.tail_x(), 12);
            assert_eq!(geometry.beak_x(), 16);
        }
    }

    #[test]
    fn full_size_bird_has_distinct_up_level_and_down_silhouettes() {
        assert_eq!(
            isolated_bird_rows(BirdPose::Up, false, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT),
            ["   ▃ ", " ▃█●▶", "◀██▅ ", " ▅   "]
        );
        assert_eq!(
            isolated_bird_rows(BirdPose::Level, false, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT),
            ["◀▇█●▶", " ▁█▁ "]
        );
        assert_eq!(
            isolated_bird_rows(BirdPose::Down, false, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT),
            [" ▃   ", "◀██▃ ", " ▅█●▶", "   ▅ "]
        );

        assert_eq!(
            isolated_bird_rows(BirdPose::Up, true, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT),
            [" .-o^", "<=// "]
        );
        assert_eq!(
            isolated_bird_rows(BirdPose::Level, true, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT),
            [" .-o>", "<=/->"]
        );
        assert_eq!(
            isolated_bird_rows(BirdPose::Down, true, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT),
            [" .-o\\", "<=/-v"]
        );
    }

    #[test]
    fn tilted_body_and_eye_share_one_pose_geometry() {
        let expected_column_tops = [
            (BirdPose::Up, [156, 152, 148]),
            (BirdPose::Level, [152, 152, 152]),
            (BirdPose::Down, [148, 152, 156]),
        ];

        for (pose, expected_tops) in expected_column_tops {
            let game = game_with_pose(pose, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT);
            let geometry = bird_geometry(&game, game.bird_y);
            let actual_tops = [
                geometry.column_top_eighth(pose, geometry.body_left),
                geometry.column_top_eighth(pose, geometry.body_left + 1),
                geometry.column_top_eighth(pose, geometry.body_left + 2),
            ];
            assert_eq!(actual_tops, expected_tops);

            let eye_x = geometry.eye_x();
            assert_eq!(
                geometry.detail_eighth(pose, eye_x) - geometry.column_top_eighth(pose, eye_x),
                6,
                "eye moved within the {pose:?} body"
            );

            let backend = TestBackend::new(game.width, game.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    draw_bird(
                        frame,
                        Rect::new(0, 0, game.width, game.height),
                        &game,
                        test_options(false, ThemeMode::Dark),
                        Palette::new(true, ResolvedTheme::Dark),
                        0.0,
                    );
                })
                .expect("draw tilted bird");
            assert_eq!(
                terminal.backend().buffer()[(eye_x as u16, geometry.detail_y(pose, eye_x) as u16)]
                    .symbol(),
                "●"
            );
        }

        let mut moving = game_with_pose(BirdPose::Level, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT);
        moving.bird_velocity = MAX_FALL_VELOCITY;
        for tick_progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let geometry = bird_geometry(&moving, visual_bird_y(&moving, tick_progress));
            let eye_x = geometry.eye_x();
            assert_eq!(
                geometry.detail_eighth(BirdPose::Level, eye_x)
                    - geometry.column_top_eighth(BirdPose::Level, eye_x),
                6,
                "eye drifted at tick progress {tick_progress}"
            );
        }
    }

    #[test]
    fn small_bird_still_has_a_filled_body_and_shows_each_rotation() {
        for (pose, unicode, ascii) in [
            (BirdPose::Up, vec![" ▂●▶", "◀▂▆ "], "o^"),
            (BirdPose::Level, vec!["◀▄●▶", " ▄▄ "], "o>"),
            (BirdPose::Down, vec!["◀▆▂ ", " ▆●▶"], "ov"),
        ] {
            assert_eq!(isolated_bird_rows(pose, false, 20, 18), unicode);
            assert_eq!(isolated_bird_rows(pose, true, 20, 18), [ascii]);
        }

        for artwork in [ASCII_BIRD_UP, ASCII_BIRD_LEVEL, ASCII_BIRD_DOWN] {
            assert!(artwork.into_iter().flatten().all(|glyph| glyph.is_ascii()));
        }
    }

    #[test]
    fn light_theme_is_daytime_and_dark_theme_is_nighttime() {
        let width = 20;
        let height = 10;

        let light_backend = TestBackend::new(width, height);
        let mut light_terminal = Terminal::new(light_backend).expect("test terminal");
        light_terminal
            .draw(|frame| {
                draw_sky(
                    frame,
                    Rect::new(0, 0, width, height),
                    0.0,
                    test_options(false, ThemeMode::Light),
                    Palette::new(true, ResolvedTheme::Light),
                );
            })
            .expect("draw daylight");

        let dark_backend = TestBackend::new(width, height);
        let mut dark_terminal = Terminal::new(dark_backend).expect("test terminal");
        dark_terminal
            .draw(|frame| {
                draw_sky(
                    frame,
                    Rect::new(0, 0, width, height),
                    0.0,
                    test_options(false, ThemeMode::Dark),
                    Palette::new(true, ResolvedTheme::Dark),
                );
            })
            .expect("draw nighttime");

        let light = light_terminal.backend().buffer();
        let dark = dark_terminal.backend().buffer();
        assert_eq!(light[(17, 1)].symbol(), "●");
        assert_ne!(dark[(17, 1)].symbol(), "●");
        assert_eq!(light[(0, 0)].bg, Palette::LIGHT_SKY_A);
        assert_eq!(light[(0, 1)].bg, Palette::LIGHT_SKY_B);
        assert_eq!(dark[(0, 0)].bg, Palette::SKY_A);
        assert_eq!(dark[(0, 1)].bg, Palette::SKY_B);

        for theme in [ResolvedTheme::Light, ResolvedTheme::Dark] {
            let palette = Palette::new(false, theme);
            assert_eq!(palette.screen(), Style::default());
            assert_eq!(palette.panel(), Style::default());
            assert_eq!(palette.fg(Palette::TEXT), Style::default());
        }
    }

    #[test]
    fn bird_colors_do_not_change_between_day_and_night() {
        let game = game_with_pose(BirdPose::Level, MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT);

        let render = |mode, resolved| {
            let options = test_options(false, mode);
            let palette = Palette::new(true, resolved);
            let backend = TestBackend::new(game.width, game.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let area = Rect::new(0, 0, game.width, game.height);
                    draw_sky(frame, area, game.elapsed, options, palette);
                    draw_bird(frame, area, &game, options, palette, 0.0);
                })
                .expect("draw themed bird");
            terminal.backend().buffer().clone()
        };

        let dark = render(ThemeMode::Dark, ResolvedTheme::Dark);
        let light = render(ThemeMode::Light, ResolvedTheme::Light);
        let bird_cells = [(12, 19), (14, 19), (15, 19), (16, 19)];

        for position in bird_cells {
            assert_eq!(
                light[position].fg, dark[position].fg,
                "bird color changed at {position:?}"
            );
        }
        assert_eq!(light[(14, 19)].fg, Palette::YELLOW);
        assert_eq!(light[(15, 19)].fg, Palette::TEXT);
        assert_eq!(light[(15, 19)].bg, Palette::YELLOW);
        assert_eq!(light[(16, 19)].fg, Palette::ORANGE);
        assert_eq!(
            Palette::new(true, ResolvedTheme::Light).resolve(Palette::YELLOW),
            Palette::LIGHT_YELLOW,
            "only the bird should bypass the general light-theme text color"
        );
    }

    #[test]
    fn daylight_keeps_the_bird_pipes_and_prompts_high_contrast() {
        for foreground in [
            Palette::LIGHT_YELLOW,
            Palette::LIGHT_ORANGE,
            Palette::LIGHT_LIME,
        ] {
            for background in [Palette::LIGHT_SKY_A, Palette::LIGHT_SKY_B] {
                assert!(
                    contrast_ratio(foreground, background) >= 3.0,
                    "{foreground:?} washed out against {background:?}"
                );
            }
        }

        for foreground in [Palette::LIGHT_TEXT, Palette::LIGHT_YELLOW] {
            assert!(
                contrast_ratio(foreground, Palette::LIGHT_PANEL) >= 4.5,
                "{foreground:?} was hard to read on the daylight panel"
            );
        }
    }

    #[test]
    fn minimum_footer_exposes_each_theme_choice_without_clipping() {
        for (mode, label) in [
            (ThemeMode::System, "T SYS"),
            (ThemeMode::Light, "T LGT"),
            (ThemeMode::Dark, "T DRK"),
        ] {
            let backend = TestBackend::new(HUD_MIN_WIDTH, 1);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let options = test_options(false, mode);
                    draw_footer(
                        frame,
                        Rect::new(0, 0, HUD_MIN_WIDTH, 1),
                        Phase::Playing,
                        options,
                        Palette::new(true, options.theme.resolved()),
                    );
                })
                .expect("draw footer");

            let rendered: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(rendered.contains(label));
            assert!(rendered.contains("Q quit"));
        }
    }

    #[test]
    fn ascii_mode_contains_no_non_ascii_artwork() {
        let game = Game::new(MIN_FIELD_WIDTH, MIN_FIELD_HEIGHT, 7);
        let (width, height) = required_size(&game);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &game,
                    0,
                    false,
                    UiOptions {
                        ascii: true,
                        color: false,
                        theme: ThemeState::explicit(ThemeMode::Dark),
                    },
                );
            })
            .expect("draw frame");

        for cell in terminal.backend().buffer().content() {
            assert!(
                cell.symbol().is_ascii(),
                "ASCII mode rendered {:?}",
                cell.symbol()
            );
            assert_eq!(cell.fg, Color::Reset);
            assert_eq!(cell.bg, Color::Reset);
        }
    }

    #[test]
    fn compact_ready_card_keeps_the_source_sized_bird_visible() {
        let game = Game::new(MIN_FIELD_WIDTH, MIN_FIELD_HEIGHT, 7);
        let (width, height) = required_size(&game);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &game, 0, false, UiOptions::default()))
            .expect("draw frame");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("ENTER TO START"));
        assert!(rendered.contains('●'), "the ready card hid the bird's eye");
    }

    #[test]
    fn a_small_terminal_reports_current_and_required_dimensions() {
        let game = Game::new(MIN_FIELD_WIDTH, MIN_FIELD_HEIGHT, 7);
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &game, 0, false, UiOptions::default()))
            .expect("draw frame");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("CURRENT"));
        assert!(rendered.contains("40x10"));
        assert!(rendered.contains("36x20"));
    }

    #[test]
    fn a_smaller_but_playable_terminal_offers_enter_to_fit_a_new_round() {
        let game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| draw(frame, &game, 0, false, UiOptions::default()))
            .expect("draw frame");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("ENTER fit/start"));
        assert!(rendered.contains("T SYS"));
    }

    #[test]
    fn portrait_field_projects_narrow_pipes_and_a_readable_bird_body() {
        let width = 20;
        let height = 18;

        assert_eq!(project_extent(BIRD_ART_WIDTH, VIRTUAL_WIDTH, width), 2);
        assert_eq!(project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, height), 1);
        assert_eq!(
            project_floor(f64::from(GROUND_Y), VIRTUAL_HEIGHT, height),
            14
        );
        assert_eq!(project_ceil(f64::from(PIPE_WIDTH), VIRTUAL_WIDTH, width), 4);
        assert_eq!(
            project_ceil(180.0, VIRTUAL_HEIGHT, height)
                - project_floor(84.0, VIRTUAL_HEIGHT, height),
            5
        );
        assert!(
            project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, height)
                < project_ceil(180.0, VIRTUAL_HEIGHT, height)
                    - project_floor(84.0, VIRTUAL_HEIGHT, height)
        );

        let mut compact_game = Game::new(width, height, 7);
        compact_game.bird_y = 246.0;
        let compact_geometry = bird_geometry(&compact_game, compact_game.bird_y);
        assert_eq!(compact_geometry.body_width(), 2);
        assert_eq!(compact_geometry.body_height_eighths(), 8);

        assert_eq!(
            project_extent(BIRD_ART_WIDTH, VIRTUAL_WIDTH, MAX_FIELD_WIDTH),
            5
        );
        assert_eq!(
            project_extent(BIRD_ART_HEIGHT, VIRTUAL_HEIGHT, MAX_FIELD_HEIGHT),
            2
        );
        assert_eq!(
            project_round(244.0, VIRTUAL_HEIGHT, MAX_FIELD_HEIGHT)
                - project_round(197.0, VIRTUAL_HEIGHT, MAX_FIELD_HEIGHT),
            4,
            "one flap should visibly lift the two-row bird by two bird-heights"
        );
        assert_eq!(
            project_ceil(f64::from(PIPE_WIDTH), VIRTUAL_WIDTH, MAX_FIELD_WIDTH),
            9
        );
        assert_eq!(visible_lower_pipe_top(10, 10), 9);
        assert_eq!(visible_lower_pipe_top(7, 14), 7);
    }

    #[test]
    fn rendered_world_is_centered_and_uses_the_projected_geometry() {
        let mut game = Game::new(20, 18, 7);
        game.phase = Phase::Playing;
        game.pipes = vec![Pipe {
            x: 190.0,
            gap_top: 180,
            gap_height: 96,
            scored: false,
        }];
        game.bird_x = f64::from(BIRD_START_X);
        game.bird_y = 246.0;

        let (width, height) = required_size(&game);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &game, 0, false, UiOptions::default()))
            .expect("draw frame");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(7, 3)].symbol(), "╔");
        assert_eq!(buffer[(28, 3)].symbol(), "╗");

        let pipe_cells: Vec<(u16, &str)> = (0..width)
            .filter_map(|x| {
                let symbol = buffer[(x, 6)].symbol();
                matches!(symbol, "▎" | "█" | "▓" | "▊").then_some((x, symbol))
            })
            .collect();
        assert_eq!(pipe_cells, vec![(21, "▎"), (22, "█"), (23, "▊"), (24, "▊")]);
        assert_eq!(buffer[(13, 12)].symbol(), "◀");
        assert_eq!(buffer[(14, 12)].symbol(), "▄");
        assert_eq!(buffer[(15, 12)].symbol(), "●");
        assert_eq!(buffer[(16, 12)].symbol(), "▶");
        assert_eq!(buffer[(14, 13)].symbol(), "▄");
        assert_eq!(buffer[(15, 13)].symbol(), "▄");
    }

    #[test]
    fn bird_body_advances_between_fixed_physics_ticks() {
        let mut game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        assert!(game.start());
        game.bird_y = 246.0;
        game.bird_velocity = MAX_FALL_VELOCITY;
        let options = test_options(false, ThemeMode::Dark);
        let palette = Palette::new(true, ResolvedTheme::Dark);

        let render = |tick_progress| {
            let backend = TestBackend::new(game.width, game.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let area = Rect::new(0, 0, game.width, game.height);
                    draw_sky(frame, area, game.elapsed, options, palette);
                    draw_bird(frame, area, &game, options, palette, tick_progress);
                })
                .expect("draw interpolated bird");
            terminal.backend().buffer()[(13, 19)].clone()
        };

        let at_tick = render(0.0);
        let halfway = render(0.5);
        assert_eq!(at_tick.symbol(), "▇");
        assert_eq!(halfway.symbol(), "▄");
        assert_ne!(at_tick, halfway);
        assert_eq!(game.bird_y, 246.0, "rendering must not alter physics");

        assert_eq!(visual_bird_y(&game, 0.5), 250.0);
        assert_eq!(visual_bird_y(&game, 1.0), 254.0);
        assert_eq!(visual_bird_y(&game, f64::NAN), 246.0);

        let mut paused = game.clone();
        paused.phase = Phase::Paused;
        assert_eq!(visual_bird_y(&paused, 0.5), 246.0);

        let mut dying = game.clone();
        dying.phase = Phase::Dying;
        assert_eq!(visual_bird_y(&dying, 0.5), 250.0);
    }

    #[test]
    fn bird_body_keeps_constant_eighth_height_in_color_and_monochrome() {
        let mut game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        assert!(game.start());
        game.bird_y = 246.0;
        game.bird_velocity = MAX_FALL_VELOCITY;

        let render = |color, tick_progress| {
            let options = UiOptions {
                ascii: false,
                color,
                theme: ThemeState::explicit(ThemeMode::Dark),
            };
            let palette = Palette::new(color, ResolvedTheme::Dark);
            let backend = TestBackend::new(game.width, game.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let area = Rect::new(0, 0, game.width, game.height);
                    draw_sky(frame, area, game.elapsed, options, palette);
                    draw_bird(frame, area, &game, options, palette, tick_progress);
                })
                .expect("draw bird body");
            terminal.backend().buffer().clone()
        };

        for tick_progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let colored = render(true, tick_progress);
            let monochrome = render(false, tick_progress);
            let colored_symbols: Vec<&str> =
                colored.content().iter().map(|cell| cell.symbol()).collect();
            let monochrome_symbols: Vec<&str> = monochrome
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert_eq!(colored_symbols, monochrome_symbols);

            let geometry = bird_geometry(&game, visual_bird_y(&game, tick_progress));
            assert_eq!(geometry.body_height_eighths(), 16);
            let outer_body_eighths = (0..game.height)
                .map(|y| {
                    let cell = &colored[(geometry.body_left as u16, y)];
                    let block_eighths = LOWER_BLOCKS
                        .iter()
                        .position(|symbol| *symbol == cell.symbol())
                        .unwrap_or(0);
                    if cell.fg == Palette::YELLOW {
                        block_eighths
                    } else if cell.bg == Palette::YELLOW {
                        8 - block_eighths
                    } else {
                        0
                    }
                })
                .sum::<usize>();
            assert_eq!(
                outer_body_eighths, 14,
                "body height pulsed at tick progress {tick_progress}"
            );
        }

        assert!(monochrome_top_edge_is_reversed(
            &render(false, 0.0),
            (13, 20)
        ));
    }

    fn monochrome_top_edge_is_reversed(buffer: &Buffer, position: (u16, u16)) -> bool {
        buffer[position].modifier.contains(Modifier::REVERSED)
    }

    #[test]
    fn pipe_edges_advance_between_fixed_physics_ticks() {
        let mut game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        game.phase = Phase::Playing;
        game.pipes = vec![Pipe {
            x: 100.0,
            gap_top: 180,
            gap_height: 96,
            scored: false,
        }];
        let options = test_options(false, ThemeMode::Dark);
        let palette = Palette::new(true, options.theme.resolved());

        let render = |tick_progress| {
            let backend = TestBackend::new(game.width, game.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let area = Rect::new(0, 0, game.width, game.height);
                    draw_sky(frame, area, game.elapsed, options, palette);
                    draw_pipes(frame, area, &game, options, palette, tick_progress);
                })
                .expect("draw interpolated pipe");
            terminal.backend().buffer()[(16, 2)].clone()
        };

        let at_tick = render(0.0);
        let halfway = render(0.5);
        assert_eq!(at_tick.symbol(), "▏");
        assert_eq!(halfway.symbol(), "█");
        assert_ne!(at_tick, halfway);
        assert_eq!(game.pipes[0].x, 100.0, "rendering must not alter physics");

        assert_eq!(visual_pipe_x(100.0, Phase::Playing, 0.5), 99.0);
        assert_eq!(visual_pipe_x(100.0, Phase::Paused, 0.5), 100.0);
        assert_eq!(visual_pipe_x(100.0, Phase::Dying, 0.5), 100.0);
        assert_eq!(visual_pipe_x(100.0, Phase::Playing, f64::NAN), 100.0);
    }

    #[test]
    fn pipe_shadow_stays_one_cell_wide_through_motion() {
        let mut game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        game.phase = Phase::Playing;
        game.pipes = vec![Pipe {
            x: 100.0,
            gap_top: 180,
            gap_height: 96,
            scored: false,
        }];
        let options = test_options(false, ThemeMode::Dark);
        let palette = Palette::new(true, ResolvedTheme::Dark);

        for tick_progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let backend = TestBackend::new(game.width, game.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    let area = Rect::new(0, 0, game.width, game.height);
                    draw_sky(frame, area, game.elapsed, options, palette);
                    draw_pipes(frame, area, &game, options, palette, tick_progress);
                })
                .expect("draw interpolated pipe shadow");

            let buffer = terminal.backend().buffer();
            let shadow_eighths = (0..game.width)
                .map(|x| {
                    let cell = &buffer[(x, 2)];
                    let block_eighths = LEFT_BLOCKS
                        .iter()
                        .position(|symbol| *symbol == cell.symbol())
                        .unwrap_or(0);
                    if cell.fg == Palette::LIME_SHADOW {
                        block_eighths
                    } else if cell.fg == Palette::LIME && cell.bg == Palette::LIME_SHADOW {
                        8 - block_eighths
                    } else {
                        0
                    }
                })
                .sum::<usize>();

            assert_eq!(
                shadow_eighths, 8,
                "shadow width pulsed at tick progress {tick_progress}"
            );
        }
    }

    #[test]
    fn no_color_pipe_uses_one_fill_without_a_blinking_shadow_band() {
        let mut game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        game.phase = Phase::Playing;
        game.pipes = vec![Pipe {
            x: 0.0,
            gap_top: 180,
            gap_height: 96,
            scored: false,
        }];
        let options = UiOptions {
            ascii: false,
            color: false,
            theme: ThemeState::explicit(ThemeMode::Dark),
        };
        let (width, height) = required_size(&game);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &game, 0, false, options))
            .expect("draw monochrome pipe");

        let buffer = terminal.backend().buffer();
        let shaft: Vec<&str> = (1..=8).map(|x| buffer[(x, 6)].symbol()).collect();
        assert_eq!(shaft, vec!["▌", "█", "█", "█", "█", "█", "█", "▋"]);
        assert!(buffer.content().iter().all(|cell| cell.symbol() != "▓"));
    }

    #[test]
    fn large_pipe_has_a_wide_cap_and_visible_collision_edges() {
        let mut game = Game::new(MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT, 7);
        game.phase = Phase::Playing;
        game.pipes = vec![Pipe {
            x: 0.0,
            gap_top: 180,
            gap_height: 96,
            scored: false,
        }];

        let (width, height) = required_size(&game);
        let options = test_options(false, ThemeMode::Dark);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &game, 0, false, options))
            .expect("draw frame");

        let buffer = terminal.backend().buffer();
        let shaft: Vec<&str> = (1..=8).map(|x| buffer[(x, 6)].symbol()).collect();
        let cap: Vec<&str> = (1..=9).map(|x| buffer[(x, 17)].symbol()).collect();

        assert_eq!(shaft, vec!["▌", "█", "█", "█", "█", "█", "▋", "▋"]);
        assert_eq!(cap, vec!["█", "█", "█", "█", "█", "█", "█", "█", "▏"]);
        assert_eq!(buffer[(7, 6)].fg, Palette::LIME);
        assert_eq!(buffer[(7, 6)].bg, Palette::LIME_SHADOW);
        assert_eq!(buffer[(8, 6)].fg, Palette::LIME_SHADOW);
        assert_eq!(buffer[(8, 6)].bg, Palette::SKY_A);
    }

    #[test]
    fn result_card_shows_original_medal_and_enter_only_retry() {
        let mut game = Game::new(MIN_FIELD_WIDTH, MIN_FIELD_HEIGHT, 7);
        game.phase = Phase::GameOver;
        game.score = 30;
        let (width, height) = required_size(&game);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &game,
                    30,
                    false,
                    UiOptions {
                        ascii: true,
                        color: false,
                        theme: ThemeState::explicit(ThemeMode::Dark),
                    },
                );
            })
            .expect("draw frame");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("GOLD MEDAL"));
        assert!(rendered.contains("ENTER TO RETRY"));
        assert!(!rendered.contains("Space to fly again"));
        assert!(!rendered.contains("R reset"));
        assert!(!rendered.contains("LEVEL"));
        assert!(!rendered.contains("SPEED"));
    }
}
