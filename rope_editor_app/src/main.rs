use std::env;
use std::io;
use std::path::Path;

use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};

use rope_editor::editor::Editor;

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    let mut editor = Editor::new();
    if let Some(path_str) = args.get(1) {
        let path = Path::new(path_str);
        if editor.open_file(path).is_err() {
            // El archivo no existe todavía: seguimos con buffer vacío
            // pero lo asociamos al path para que "Save" (Ctrl+S) lo cree.
            editor.set_new_file_path(path);
        }
    }

    editor.run();

    execute!(io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    Ok(())
}
