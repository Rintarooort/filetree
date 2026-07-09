mod app;
mod file_ops;
mod file_tree;
mod git_status;
mod input;
mod ui;

use std::env;
use std::fs;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use app::App;

/// 前回起動したルートを保存するファイル: $XDG_CONFIG_HOME/filetree/last_root
fn state_file() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("filetree").join("last_root")
}

fn load_last_root() -> Option<PathBuf> {
    let contents = fs::read_to_string(state_file()).ok()?;
    let p = PathBuf::from(contents.trim());
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

fn save_last_root(path: &Path) {
    let sf = state_file();
    if let Some(dir) = sf.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&sf, path.to_string_lossy().as_bytes());
}

/// cwd が「意味のある」ディレクトリか (ルート / や $HOME はプロジェクトとして扱わない)
fn is_meaningful_cwd(dir: &Path) -> bool {
    if dir == Path::new("/") {
        return false;
    }
    if let Ok(home) = env::var("HOME") {
        if dir == Path::new(&home) {
            return false;
        }
    }
    true
}

fn main() -> Result<()> {
    // ルート決定:
    //   1. 明示的な引数があればそれを最優先
    //   2. cwd が意味のあるディレクトリ (プロジェクト等) ならそれを使う
    //   3. cwd が / や $HOME のときだけ、前回起動したルートにフォールバック
    let path = match env::args().nth(1) {
        Some(arg) => PathBuf::from(arg),
        None => {
            let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if is_meaningful_cwd(&cwd) {
                cwd
            } else {
                load_last_root().unwrap_or(cwd)
            }
        }
    };

    let path = path.canonicalize().unwrap_or(path);

    // 次回のフォールバック用に、今回のルートを保存しておく
    save_last_root(&path);

    // Read default command from environment variable
    let default_command = env::var("FILETREE_DEFAULT_CMD").ok();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new(&path, default_command)?;
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    // Flush terminal to clear any buffered input
    terminal.flush()?;

    // Clear any pending events in the input buffer
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    let mut visible_height = 20usize;

    loop {
        terminal.draw(|f| {
            app.tree_area_height = f.area().height.saturating_sub(5) as usize;
            visible_height = ui::draw(f, app);
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    input::handle_key_event(app, key, visible_height);
                }
                Event::Mouse(mouse) => {
                    input::handle_mouse_event(app, mouse);
                }
                Event::Paste(text) => {
                    app.handle_drop(&text);
                }
                _ => {}
            }
        }

        // Check drop buffer timeout
        app.check_drop_buffer();

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
