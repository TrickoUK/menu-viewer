mod app;
mod model;
mod tree;
mod ui;

use anyhow::{bail, Context, Result};
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
use std::path::{Path, PathBuf};

/// Preview a Batocera core-options yml as an interactive text menu.
#[derive(Parser)]
struct Cli {
    /// Path to a *.core.yml file (auto-detected if omitted and exactly one
    /// .yml file exists in the current directory)
    yml_path: Option<PathBuf>,

    /// Dump the menu as a tree to the terminal and exit, instead of
    /// launching the interactive TUI
    #[arg(long)]
    tree: bool,
}

/// Auto-detect a single `.yml` file in `dir` when no path was given on the
/// command line. Non-recursive, and matches only `.yml` (not `.yaml`) to
/// mirror the `*.core.yml` convention these files actually use.
fn find_single_yml_in(dir: &Path) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("yml"))
        .collect();
    candidates.sort();

    match candidates.len() {
        0 => bail!("no .yml file given and none found in the current directory"),
        1 => Ok(candidates.remove(0)),
        _ => {
            let names: Vec<String> = candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            bail!(
                "no .yml file given and multiple were found in the current directory: {} — pass one explicitly",
                names.join(", ")
            )
        }
    }
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

    let yml_path = match cli.yml_path {
        Some(p) => p,
        None => find_single_yml_in(Path::new("."))?,
    };

    let contents = std::fs::read_to_string(&yml_path)
        .with_context(|| format!("failed to read {}", yml_path.display()))?;
    let core: CoreYml = serde_yaml::from_str(&contents)
        .with_context(|| format!("failed to parse {} as a core yml", yml_path.display()))?;
    let root = build_menu(&core);

    if cli.tree {
        tree::print_tree(&root);
        return Ok(());
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A fresh, empty temp directory, removed on drop. Isolated per test
    /// (process id + monotonic counter) so parallel `cargo test` runs don't
    /// collide.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "menu-viewer-test-{}-{n}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn touch(&self, name: &str) {
            std::fs::write(self.0.join(name), "").unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn errors_when_no_yml_files_present() {
        let dir = TempDir::new();
        dir.touch("readme.txt");
        assert!(find_single_yml_in(&dir.0).is_err());
    }

    #[test]
    fn finds_the_single_yml_file() {
        let dir = TempDir::new();
        dir.touch("notes.txt");
        dir.touch("snes.core.yml");
        let found = find_single_yml_in(&dir.0).unwrap();
        assert_eq!(found.file_name().unwrap(), "snes.core.yml");
    }

    #[test]
    fn errors_when_multiple_yml_files_present() {
        let dir = TempDir::new();
        dir.touch("a.core.yml");
        dir.touch("b.core.yml");
        let err = find_single_yml_in(&dir.0).unwrap_err().to_string();
        assert!(err.contains("a.core.yml"));
        assert!(err.contains("b.core.yml"));
    }

    #[test]
    fn ignores_non_yml_extensions() {
        let dir = TempDir::new();
        dir.touch("core.yaml");
        dir.touch("core.yml");
        let found = find_single_yml_in(&dir.0).unwrap();
        assert_eq!(found.file_name().unwrap(), "core.yml");
    }
}
