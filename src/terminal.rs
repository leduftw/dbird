use std::io::{self, Stdout, stdout};
use std::panic;

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Owns the raw-mode terminal and restores it even when a caller returns early.
pub struct TerminalSession {
    terminal: Tui,
    restored: bool,
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;

        let mut output = stdout();
        if let Err(error) = execute!(
            output,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        ) {
            let _ = execute!(output, Show, LeaveAlternateScreen);
            let _ = disable_raw_mode();
            return Err(error);
        }

        let backend = CrosstermBackend::new(output);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let mut output = stdout();
                let _ = execute!(output, Show, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            restored: false,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut Tui {
        &mut self.terminal
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        let raw_result = disable_raw_mode();
        let screen_result = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let cursor_result = self.terminal.show_cursor();

        let result = raw_result.and(screen_result).and(cursor_result);
        self.restored = result.is_ok();
        result
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Restores the user's shell before delegating to Rust's normal panic report.
pub fn install_panic_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let mut output = stdout();
        let _ = execute!(output, Show, LeaveAlternateScreen);
        default_hook(info);
    }));
}
