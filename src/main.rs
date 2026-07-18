mod app;
mod model;
mod ui;

use anyhow::{Context, Result};
use app::App;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use model::{build_menu, CoreYml};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::path::PathBuf;

/// Preview a Batocera core-options yml as an interactive text menu.
#[derive(Parser)]
struct Cli {
    /// Path to a *.core.yml file
    yml_path: PathBuf,
}

/// Ensures the terminal is restored on exit, including early returns.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let contents = std::fs::read_to_string(&cli.yml_path)
        .with_context(|| format!("failed to read {}", cli.yml_path.display()))?;
    let core: CoreYml = serde_yaml::from_str(&contents)
        .with_context(|| format!("failed to parse {} as a core yml", cli.yml_path.display()))?;
    let root = build_menu(&core);

    enable_raw_mode().context("failed to enable raw terminal mode")?;
    execute!(stdout(), EnterAlternateScreen).context("failed to enter alternate screen")?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend).context("failed to init terminal")?;

    let mut app = App::new(&root);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key.code);
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
