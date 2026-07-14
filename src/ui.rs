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
    Game, MAX_FIELD_HEIGHT, MAX_FIELD_WIDTH, MIN_FIELD_HEIGHT, MIN_FIELD_WIDTH, PIPE_WIDTH, Phase,
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
    let compact = game.width < 66;
    let line = if compact {
        Line::from(vec![
            Span::styled("S ", palette.fg(Palette::CYAN)),
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
            Span::styled(separator, palette.fg(Palette::DIM)),
            Span::styled(
                format!("L{}  {:.1}x", game.level(), game.speed() / 12.0),
                palette.fg(Palette::LIME),
            ),
        ])
    } else {
        Line::from(vec![
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
            Span::styled(separator, palette.fg(Palette::DIM)),
            Span::styled(
                format!("LEVEL {:02}", game.level()),
                palette.fg(Palette::CYAN),
            ),
            Span::styled(separator, palette.fg(Palette::DIM)),
            Span::styled(
                format!("SPEED {:.1}x", game.speed() / 12.0),
                palette.fg(Palette::LIME),
            ),
        ])
    };

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
        Phase::GameOver => " GAME OVER ",
    };
    let phase_color = match game.phase {
        Phase::Ready | Phase::Playing => Palette::LIME,
        Phase::Paused => Palette::MAGENTA,
        Phase::GameOver => Palette::DANGER,
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
        Phase::Playing => {}
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

    for pipe in &game.pipes {
        let pipe_x = pipe.x.round() as i32;
        let gap_bottom = pipe.gap_top.saturating_add(pipe.gap_height);

        for offset in 0..PIPE_WIDTH {
            let logical_x = pipe_x.saturating_add(i32::from(offset));
            if logical_x < 0 || logical_x >= i32::from(game.width) {
                continue;
            }

            for logical_y in 0..game.height {
                if logical_y >= pipe.gap_top && logical_y < gap_bottom {
                    continue;
                }

                let at_cap = logical_y.saturating_add(1) == pipe.gap_top || logical_y == gap_bottom;
                let at_shadow = offset.saturating_add(1) == PIPE_WIDTH;
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
                let y = area.y.saturating_add(logical_y);

                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_symbol(symbol).set_style(style);
                }
            }
        }
    }
}

fn draw_bird(frame: &mut Frame<'_>, area: Rect, game: &Game, options: UiOptions, palette: Palette) {
    let bird_x = (game.bird_x.round() as i32).clamp(0, i32::from(game.width.saturating_sub(1)));
    let bird_y = (game.bird_y.round() as i32).clamp(0, i32::from(game.height.saturating_sub(1)));
    let wing = if game.bird_velocity < -1.0 {
        if options.ascii { "/" } else { "▜" }
    } else if game.bird_velocity > 1.0 {
        if options.ascii { "\\" } else { "▟" }
    } else if options.ascii {
        "-"
    } else {
        "▐"
    };
    let cells = [
        (wing, palette.fg(Palette::YELLOW)),
        (
            "O",
            palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
        ),
        (
            ">",
            palette.fg(Palette::ORANGE).add_modifier(Modifier::BOLD),
        ),
    ];
    let buffer = frame.buffer_mut();

    for (offset, (symbol, style)) in cells.into_iter().enumerate() {
        let logical_x = bird_x.saturating_add(offset as i32);
        if logical_x < 0 || logical_x >= i32::from(game.width) {
            continue;
        }
        let x = area.x.saturating_add(logical_x as u16);
        let y = area.y.saturating_add(bird_y as u16);
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_style(style);
        }
    }
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
        Overlay::GameOver => 9,
        Overlay::Ready | Overlay::Paused => 7,
    }
    .min(field.height);
    let width = 42.min(field.width);
    let area = centered(field, width, height);

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
                "PRESS SPACE TO FLAP",
                palette.fg(Palette::YELLOW).add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                "Thread every gap. Stay airborne.",
                palette.fg(Palette::TEXT),
            ),
        ],
        Overlay::Paused => vec![
            Line::from(""),
            Line::styled(
                "FLIGHT SUSPENDED",
                palette.fg(Palette::MAGENTA).add_modifier(Modifier::BOLD),
            ),
            Line::styled("P or Space to resume", palette.fg(Palette::TEXT)),
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
                Line::from(""),
                Line::styled("Space to fly again  |  R reset", palette.fg(Palette::TEXT)),
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
            key_span("SPACE", Palette::CYAN, palette),
            Span::styled(" take off", palette.fg(Palette::DIM)),
        ],
        Phase::Playing => vec![
            key_span("SPACE", Palette::CYAN, palette),
            Span::styled(" flap", palette.fg(Palette::DIM)),
            Span::styled(separator, palette.fg(Palette::DIM)),
            key_span("P", Palette::MAGENTA, palette),
            Span::styled(" pause", palette.fg(Palette::DIM)),
        ],
        Phase::Paused => vec![
            key_span("SPACE", Palette::CYAN, palette),
            Span::styled(" resume/flap", palette.fg(Palette::DIM)),
            Span::styled(separator, palette.fg(Palette::DIM)),
            key_span("P", Palette::MAGENTA, palette),
            Span::styled(" resume", palette.fg(Palette::DIM)),
        ],
        Phase::GameOver => vec![
            key_span("SPACE", Palette::CYAN, palette),
            Span::styled(" retry", palette.fg(Palette::DIM)),
        ],
    };
    spans.extend([
        Span::styled(separator, palette.fg(Palette::DIM)),
        key_span("R", Palette::LIME, palette),
        Span::styled(" restart", palette.fg(Palette::DIM)),
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
            if can_start_round(area) {
                "R fit a new round  |  Q quit"
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
    fn a_smaller_but_playable_terminal_offers_to_fit_a_new_round() {
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
        assert!(rendered.contains("R fit a new round"));
    }
}
