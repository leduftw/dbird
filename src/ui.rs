//! Ratatui renderer for the game board and its surrounding HUD.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::game::{
    BIRD_VISUAL_OFFSET, BIRD_VISUAL_SIZE, GROUND_Y, Game, MAX_FIELD_HEIGHT, MAX_FIELD_WIDTH,
    MIN_FIELD_HEIGHT, MIN_FIELD_WIDTH, Medal, PIPE_WIDTH, Phase, VIRTUAL_HEIGHT, VIRTUAL_WIDTH,
};

const HORIZONTAL_CHROME: u16 = 2;
const VERTICAL_CHROME: u16 = 6;

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
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            ascii: false,
            color: true,
        }
    }
}

/// Return the largest supported play field that fits in `area`.
///
/// The returned dimensions are clamped to the simulation limits. On a terminal
/// smaller than the minimum, this deliberately returns the minimum dimensions;
/// [`fits`] can then be used to show the resize prompt instead of creating a
/// subtly different game.
pub fn field_size(area: Rect) -> (u16, u16) {
    (
        area.width
            .saturating_sub(HORIZONTAL_CHROME)
            .clamp(MIN_FIELD_WIDTH, MAX_FIELD_WIDTH),
        area.height
            .saturating_sub(VERTICAL_CHROME)
            .clamp(MIN_FIELD_HEIGHT, MAX_FIELD_HEIGHT),
    )
}

/// Whether `area` is large enough to host a newly-sized round.
pub fn can_start_round(area: Rect) -> bool {
    let (width, height) = field_size(area);
    area.width >= width.saturating_add(HORIZONTAL_CHROME)
        && area.height >= height.saturating_add(VERTICAL_CHROME)
}

/// Return the terminal dimensions required to display `game` without clipping.
pub fn required_size(game: &Game) -> (u16, u16) {
    (
        game.width.saturating_add(HORIZONTAL_CHROME),
        game.height.saturating_add(VERTICAL_CHROME),
    )
}

/// Whether `area` can display the complete HUD, field, and controls.
pub fn fits(area: Rect, game: &Game) -> bool {
    let (needed_width, needed_height) = required_size(game);
    area.width >= needed_width && area.height >= needed_height
}

/// Draw one complete game frame.
pub fn draw(frame: &mut Frame<'_>, game: &Game, best: u32, new_best: bool, options: UiOptions) {
    let area = frame.area();
    let palette = Palette::new(options.color);

    frame.render_widget(Block::default().style(palette.screen()), area);

    if !fits(area, game) {
        draw_too_small(frame, area, game, options, palette);
        return;
    }

    let (required_width, required_height) = required_size(game);
    let stage = centered(area, required_width, required_height);
    let header = Rect::new(stage.x, stage.y, stage.width, 3);
    let field = Rect::new(
        stage.x,
        stage.y.saturating_add(3),
        stage.width,
        game.height.saturating_add(2),
    );
    let footer = Rect::new(
        stage.x,
        field.y.saturating_add(field.height),
        stage.width,
        1,
    );

    draw_header(frame, header, game, best, options, palette);
    draw_field(frame, field, game, best, new_best, options, palette);
    draw_footer(frame, footer, game.phase, options, palette);
}

#[derive(Clone, Copy)]
struct Palette {
    color: bool,
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

    const fn new(color: bool) -> Self {
        Self { color }
    }

    fn fg(self, color: Color) -> Style {
        if self.color {
            Style::default().fg(color)
        } else {
            Style::default()
        }
    }

    fn on(self, foreground: Color, background: Color) -> Style {
        if self.color {
            Style::default().fg(foreground).bg(background)
        } else {
            Style::default()
        }
    }

    fn background(self, color: Color) -> Style {
        if self.color {
            Style::default().bg(color)
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
    options: UiOptions,
    palette: Palette,
) {
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

    draw_sky(frame, inner, game.elapsed, palette);
    draw_pipes(frame, inner, game, options, palette);
    draw_bird(frame, inner, game, options, palette);
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

fn draw_sky(frame: &mut Frame<'_>, area: Rect, elapsed: f64, palette: Palette) {
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
            let hash = (u64::from(logical_x) + drift)
                .wrapping_mul(73)
                .wrapping_add(u64::from(logical_y).wrapping_mul(151));
            let (symbol, foreground) = if hash % 211 == 0 {
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
) {
    let (body, shadow, cap) = if options.ascii {
        ("#", "|", "=")
    } else {
        ("█", "▓", "█")
    };
    let buffer = frame.buffer_mut();

    let ground_top = project_floor(f64::from(GROUND_Y), VIRTUAL_HEIGHT, game.height)
        .clamp(0, i32::from(game.height));

    for pipe in &game.pipes {
        let pipe_left = project_floor(pipe.x, VIRTUAL_WIDTH, game.width);
        let pipe_right = project_ceil(pipe.x + f64::from(PIPE_WIDTH), VIRTUAL_WIDTH, game.width);
        let gap_top = project_floor(f64::from(pipe.gap_top), VIRTUAL_HEIGHT, game.height);
        let gap_bottom = project_ceil(
            f64::from(pipe.gap_top + pipe.gap_height),
            VIRTUAL_HEIGHT,
            game.height,
        );
        // Preserve one visible lower-pipe cap when the virtual pipe starts in
        // the same coarse terminal row as the ground.
        let lower_pipe_top = visible_lower_pipe_top(gap_bottom, ground_top);

        for logical_x in pipe_left.max(0)..pipe_right.min(i32::from(game.width)) {
            for logical_y in 0..ground_top {
                if logical_y >= gap_top && logical_y < lower_pipe_top {
                    continue;
                }

                let at_cap = logical_y + 1 == gap_top || logical_y == lower_pipe_top;
                let at_shadow = logical_x + 1 == pipe_right;
                let symbol = if at_cap {
                    cap
                } else if at_shadow {
                    shadow
                } else {
                    body
                };
                let style = if at_shadow && !at_cap {
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
}

fn draw_bird(frame: &mut Frame<'_>, area: Rect, game: &Game, options: UiOptions, palette: Palette) {
    const UNICODE_BIRD: [[char; 16]; 2] = [
        [
            ' ', ' ', ' ', ' ', '╭', '─', '─', '─', '─', '●', '╮', ' ', '▶', '▶', ' ', ' ',
        ],
        [
            '╭', '━', '━', '━', '╯', ' ', ' ', ' ', ' ', '╰', '─', '─', '▶', '▶', ' ', ' ',
        ],
    ];
    const ASCII_BIRD: [[char; 16]; 2] = [
        [
            ' ', ' ', ' ', ' ', '.', '-', '-', '-', '-', 'o', '.', ' ', '>', '>', ' ', ' ',
        ],
        [
            '<', '=', '=', '=', '/', ' ', ' ', ' ', ' ', '\\', '-', '-', '>', '>', ' ', ' ',
        ],
    ];

    let visual_left = game.bird_x - f64::from(BIRD_VISUAL_OFFSET);
    let visual_top = game.bird_y - f64::from(BIRD_VISUAL_OFFSET);
    let left = project_floor(visual_left, VIRTUAL_WIDTH, game.width);
    let right = project_ceil(
        visual_left + f64::from(BIRD_VISUAL_SIZE),
        VIRTUAL_WIDTH,
        game.width,
    );
    let top = project_floor(visual_top, VIRTUAL_HEIGHT, game.height);
    let bottom = project_ceil(
        visual_top + f64::from(BIRD_VISUAL_SIZE),
        VIRTUAL_HEIGHT,
        game.height,
    );
    let sprite_width = (right - left).max(1);
    let sprite_height = (bottom - top).max(1);
    let artwork = if options.ascii {
        &ASCII_BIRD
    } else {
        &UNICODE_BIRD
    };
    let buffer = frame.buffer_mut();

    for logical_y in top.max(0)..bottom.min(i32::from(game.height)) {
        let template_y = ((logical_y - top) * artwork.len() as i32 / sprite_height) as usize;
        for logical_x in left.max(0)..right.min(i32::from(game.width)) {
            let template_x = ((logical_x - left) * artwork[0].len() as i32 / sprite_width) as usize;
            let symbol =
                artwork[template_y.min(artwork.len() - 1)][template_x.min(artwork[0].len() - 1)];
            if symbol == ' ' {
                continue;
            }

            let color = if matches!(symbol, '>' | '▶') {
                Palette::ORANGE
            } else if matches!(symbol, 'o' | '●') {
                Palette::TEXT
            } else {
                Palette::YELLOW
            };
            let x = area.x.saturating_add(logical_x as u16);
            let y = area.y.saturating_add(logical_y as u16);
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_char(symbol)
                    .set_style(palette.fg(color).add_modifier(Modifier::BOLD));
            }
        }
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
    (value * f64::from(terminal_extent) / f64::from(virtual_extent)).floor() as i32
}

fn project_ceil(value: f64, virtual_extent: u16, terminal_extent: u16) -> i32 {
    (value * f64::from(terminal_extent) / f64::from(virtual_extent)).ceil() as i32
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

    let content = match overlay {
        Overlay::Ready => vec![
            Line::from(""),
            Line::styled(
                "PRESS ENTER TO START",
                palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
            ),
            Line::styled("Space flaps during flight.", palette.fg(Palette::TEXT)),
        ],
        Overlay::Paused => vec![
            Line::from(""),
            Line::styled(
                "FLIGHT SUSPENDED",
                palette.fg(Palette::MAGENTA).add_modifier(Modifier::BOLD),
            ),
            Line::styled("Press P to resume", palette.fg(Palette::TEXT)),
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
            vec![
                Line::from(""),
                result,
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
                ]),
                medal,
                Line::from(""),
                Line::styled(
                    "PRESS ENTER TO RETRY",
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
    let separator = if options.ascii { "  |  " } else { "  │  " };
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
                "ENTER fit and start  |  Q quit"
            } else {
                "Resize to continue  |  Q quit"
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
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};

    use super::*;

    #[test]
    fn field_dimensions_reserve_chrome_and_clamp_to_game_limits() {
        assert_eq!(field_size(Rect::new(0, 0, 80, 24)), (78, 18));
        assert_eq!(
            field_size(Rect::new(0, 0, 1, 1)),
            (MIN_FIELD_WIDTH, MIN_FIELD_HEIGHT)
        );
        assert_eq!(
            field_size(Rect::new(0, 0, u16::MAX, u16::MAX)),
            (MAX_FIELD_WIDTH, MAX_FIELD_HEIGHT)
        );
        assert!(can_start_round(Rect::new(0, 0, 56, 20)));
        assert!(!can_start_round(Rect::new(0, 0, 55, 20)));
        assert!(!can_start_round(Rect::new(0, 0, 56, 19)));
    }

    #[test]
    fn required_size_and_fit_use_the_same_chrome_budget() {
        let game = Game::new(70, 20, 7);
        assert_eq!(required_size(&game), (72, 26));
        assert!(fits(Rect::new(0, 0, 72, 26), &game));
        assert!(!fits(Rect::new(0, 0, 71, 26), &game));
        assert!(!fits(Rect::new(0, 0, 72, 25), &game));
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
    fn ready_card_keeps_the_enlarged_bird_visible() {
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
        assert!(rendered.contains("PRESS ENTER TO START"));
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
        assert!(rendered.contains("56x20"));
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
        assert!(rendered.contains("ENTER fit and start"));
    }

    #[test]
    fn standard_terminal_projects_original_visual_proportions() {
        let width = 78;
        let height = 18;
        let bird_left = project_floor(
            f64::from(crate::game::BIRD_START_X - BIRD_VISUAL_OFFSET),
            VIRTUAL_WIDTH,
            width,
        );
        let bird_right = project_ceil(
            f64::from(crate::game::BIRD_START_X - BIRD_VISUAL_OFFSET + BIRD_VISUAL_SIZE),
            VIRTUAL_WIDTH,
            width,
        );

        assert_eq!(bird_right - bird_left, 14);
        assert_eq!(
            project_floor(f64::from(GROUND_Y), VIRTUAL_HEIGHT, height),
            14
        );
        assert_eq!(
            project_ceil(f64::from(PIPE_WIDTH), VIRTUAL_WIDTH, width),
            15
        );
        assert_eq!(
            project_ceil(180.0, VIRTUAL_HEIGHT, height)
                - project_floor(84.0, VIRTUAL_HEIGHT, height),
            5
        );
        assert_eq!(visible_lower_pipe_top(10, 10), 9);
        assert_eq!(visible_lower_pipe_top(7, 14), 7);
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
        assert!(rendered.contains("PRESS ENTER TO RETRY"));
        assert!(!rendered.contains("Space to fly again"));
        assert!(!rendered.contains("R reset"));
        assert!(!rendered.contains("LEVEL"));
        assert!(!rendered.contains("SPEED"));
    }
}
