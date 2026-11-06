//! S9 (Roadmap v0.2.72): operaciones de Git del Source Control.
//!
//! Rama, estado, diff, stage y commit. Se mueven tal cual desde
//! `app.rs` como un `impl` más del mismo `EditorApp`, sin cambiar
//! comportamiento.

use std::path::{Path, PathBuf};

use crate::app::{EditorApp, GitDiffViewState};

impl EditorApp {
    /// F8 (Roadmap v0.2.35): lee la rama del repo una sola vez (desde
    /// `.git/HEAD`, sin subprocesos).
    pub fn refresh_git_branch(&mut self) {
        if self.git_status.branch.is_none() {
            self.git_status.branch = karkinos_core::read_branch(&self.doc.browse_dir);
        }
    }

    /// v0.2.42: raíz del repo que contiene el workspace actual.
    pub(crate) fn git_repo_root(&self) -> PathBuf {
        crate::git_diff_worker::repo_root_for(&self.doc.browse_dir)
            .unwrap_or_else(|| self.doc.browse_dir.clone())
    }

    /// v0.2.42: recarga los cambios pendientes de `git status --porcelain`.
    pub fn refresh_git_status(&mut self) {
        let root = self.git_repo_root();
        self.git_ui.is_loading = true;
        self.git_ui.last_error = None;

        match karkinos_core::git_status::query_git_status(&root) {
            Ok(entries) => {
                self.git_ui.entries = entries;
            }
            Err(error) => {
                self.git_ui.entries.clear();
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git status failed: {error}");
            }
        }

        self.git_ui.is_loading = false;
        self.git_status.branch = karkinos_core::read_branch(&root);
    }

    /// v0.2.44: abre el visor de diff de `path` (working tree o staged).
    /// La ruta relativa se calcula contra `doc.browse_dir`, que es la raíz
    /// contra la que también consulta el Source Control.
    pub fn open_git_diff(&mut self, path: &Path, staged: bool) {
        let root = self.doc.browse_dir.clone();
        let rel_path = path
            .strip_prefix(&root)
            .map(|relative| relative.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf());
        let diff_text =
            match karkinos_core::git_status::query_git_diff(&root, Some(&rel_path), staged) {
                Ok(diff) => diff,
                Err(error) => {
                    self.doc.status = format!("Git diff failed: {error}");
                    return;
                }
            };

        let rel_name = rel_path.to_string_lossy().replace('\\', "/");
        let parsed_lines = karkinos_core::git_status::parse_unified_diff(&diff_text);
        self.git_diff_modal = Some(GitDiffViewState {
            file_path: path.to_path_buf(),
            rel_name,
            diff_text,
            parsed_lines,
            is_staged: staged,
        });
    }

    /// v0.2.44: cierra el visor de diff.
    pub fn close_git_diff(&mut self) {
        self.git_diff_modal = None;
    }

    /// v0.2.42: crea un commit con el mensaje de la caja de Source Control.
    pub fn git_commit(&mut self) -> Result<String, String> {
        let root = self.git_repo_root();
        let message = self.git_ui.commit_message.clone();

        match karkinos_core::git_status::execute_git_commit(&root, &message) {
            Ok(output) => {
                self.git_ui.commit_message.clear();
                self.git_ui.last_error = None;
                self.refresh_git_status();
                self.doc.status = "Commit created".to_string();
                Ok(output)
            }
            Err(error) => {
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git commit failed: {error}");
                Err(error)
            }
        }
    }

    /// v0.2.42: agrega todos los cambios al índice (`git add -A`).
    pub fn git_stage_all(&mut self) -> Result<(), String> {
        let root = self.git_repo_root();

        match karkinos_core::git_status::execute_git_stage_all(&root) {
            Ok(()) => {
                self.git_ui.last_error = None;
                self.refresh_git_status();
                self.doc.status = "Staged all changes".to_string();
                Ok(())
            }
            Err(error) => {
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git stage failed: {error}");
                Err(error)
            }
        }
    }

    /// v0.2.42: agrega un único archivo al índice.
    pub fn git_stage_path(&mut self, path: &Path) -> Result<(), String> {
        let root = self.git_repo_root();

        match karkinos_core::git_status::execute_git_stage_path(&root, path) {
            Ok(()) => {
                self.git_ui.last_error = None;
                self.refresh_git_status();
                Ok(())
            }
            Err(error) => {
                self.git_ui.last_error = Some(error.clone());
                self.doc.status = format!("Git stage failed: {error}");
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{EditorApp, SidebarTab};
    fn git_temp_repo(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "karkinos_git_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("crear repo temporal");

        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .expect("ejecutar git");
            assert!(
                output.status.success(),
                "git {args:?} falló: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "karkinos@example.com"]);
        git(&["config", "user.name", "Karkinos Test"]);
        root
    }

    #[test]
    fn git_ui_refresh_stage_all_and_commit_roundtrip() {
        use karkinos_core::git_status::GitStatusKind;

        let root = git_temp_repo("roundtrip");
        std::fs::write(root.join("nuevo.txt"), "hola\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.refresh_git_status();
        assert!(app.git_ui.last_error.is_none(), "{:?}", app.git_ui.last_error);
        assert!(app.git_status.branch.is_some(), "la rama se lee del repo");
        assert_eq!(app.git_ui.entries.len(), 1);
        assert_eq!(app.git_ui.entries[0].rel_path, "nuevo.txt");
        assert_eq!(app.git_ui.entries[0].kind, GitStatusKind::Untracked);
        assert!(!app.git_ui.entries[0].staged);

        app.git_stage_all().expect("stage all");
        assert_eq!(app.git_ui.entries.len(), 1);
        assert_eq!(app.git_ui.entries[0].kind, GitStatusKind::Added);
        assert!(app.git_ui.entries[0].staged);

        app.git_ui.commit_message = "add nuevo.txt".to_string();
        let output = app.git_commit().expect("commit");
        assert!(output.contains("add nuevo.txt"), "{output}");
        assert!(app.git_ui.commit_message.is_empty(), "el mensaje se limpia");
        assert!(app.git_ui.entries.is_empty(), "no quedan cambios pendientes");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_ui_commit_rejects_blank_message_and_keeps_it() {
        let root = git_temp_repo("blank_message");
        std::fs::write(root.join("a.txt"), "a\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.git_ui.commit_message = "   ".to_string();

        let error = app.git_commit().expect_err("mensaje vacío");
        assert_eq!(error, "Commit message cannot be empty");
        assert_eq!(app.git_ui.commit_message, "   ", "no se pierde lo escrito");
        assert!(app.git_ui.last_error.is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_ui_stage_single_file_leaves_the_rest_untracked() {
        use karkinos_core::git_status::GitStatusKind;

        let root = git_temp_repo("stage_one");
        std::fs::write(root.join("uno.txt"), "uno\n").expect("archivo");
        std::fs::write(root.join("dos.txt"), "dos\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_git_status();
        assert_eq!(app.git_ui.entries.len(), 2);

        let path = app
            .git_ui
            .entries
            .iter()
            .find(|entry| entry.rel_path == "uno.txt")
            .expect("uno.txt en la lista")
            .path
            .clone();
        app.git_stage_path(&path).expect("stage de un archivo");

        assert_eq!(app.git_ui.entries.len(), 2);
        let uno = app
            .git_ui
            .entries
            .iter()
            .find(|entry| entry.rel_path == "uno.txt")
            .expect("uno.txt sigue listado");
        assert_eq!(uno.kind, GitStatusKind::Added);
        assert!(uno.staged);
        let dos = app
            .git_ui
            .entries
            .iter()
            .find(|entry| entry.rel_path == "dos.txt")
            .expect("dos.txt sigue listado");
        assert_eq!(dos.kind, GitStatusKind::Untracked);
        assert!(!dos.staged);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn palette_git_commands_open_refresh_and_stage() {
        let root = git_temp_repo("palette");
        std::fs::write(root.join("palette.txt"), "contenido\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.execute_palette_command("git:source_control");
        assert_eq!(app.sidebar_tab, SidebarTab::SourceControl);
        assert_eq!(app.git_ui.entries.len(), 1);

        app.execute_palette_command("git:stage_all");
        assert!(app.git_ui.entries[0].staged);

        app.execute_palette_command("git:refresh");
        assert_eq!(app.sidebar_tab, SidebarTab::SourceControl, "refresh no cierra");
        assert!(app.git_ui.entries[0].staged);

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: apertura y cierre del visor de diff.
    #[test]
    fn open_and_close_git_diff_modal() {
        use karkinos_core::git_status::DiffLineKind;

        let root = git_temp_repo("diff_view");
        let path = root.join("view.txt");
        std::fs::write(&path, "uno\n").expect("archivo inicial");
        for args in [vec!["add", "view.txt"], vec!["commit", "-m", "inicial"]] {
            let output = std::process::Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .expect("ejecutar git");
            assert!(
                output.status.success(),
                "git {args:?} falló: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::write(&path, "uno\ndos\n").expect("archivo modificado");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.open_git_diff(&path, false);
        let view = app.git_diff_modal.as_ref().expect("modal abierto");
        assert_eq!(view.rel_name, "view.txt");
        assert!(!view.is_staged);
        assert!(view.diff_text.contains("+dos"));
        assert!(view
            .parsed_lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Added && line.text == "+dos"));

        app.close_git_diff();
        assert!(app.git_diff_modal.is_none(), "close limpia el modal");

        let stage = std::process::Command::new("git")
            .args(["add", "view.txt"])
            .current_dir(&root)
            .output()
            .expect("ejecutar git");
        assert!(stage.status.success());

        app.open_git_diff(&path, true);
        let view = app.git_diff_modal.as_ref().expect("modal staged");
        assert!(view.is_staged);
        assert!(view.diff_text.contains("+dos"));

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: comandos del visor en el catálogo del Command Palette.
    #[test]
    fn palette_git_diff_commands_open_and_close_modal() {
        let root = git_temp_repo("palette_diff");
        let path = root.join("active.txt");
        std::fs::write(&path, "linea\n").expect("archivo");
        for args in [vec!["add", "active.txt"], vec!["commit", "-m", "inicial"]] {
            let output = std::process::Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .expect("ejecutar git");
            assert!(output.status.success(), "git falló");
        }
        std::fs::write(&path, "linea\nnueva\n").expect("modificado");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(path);

        app.execute_palette_command("git:view_diff");
        assert!(app.git_diff_modal.is_some(), "view_diff abre el modal");

        app.execute_palette_command("git:close_diff");
        assert!(app.git_diff_modal.is_none(), "close_diff lo cierra");

        std::fs::remove_dir_all(&root).ok();
    }

    // ---- Roadmap v0.2.45 (Tarea B): init_from_cli_path ----


}
