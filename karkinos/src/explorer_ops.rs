//! S9 (Roadmap v0.2.72): operaciones del explorador de archivos.
//!
//! Crear, borrar, renombrar y duplicar ítems del explorador. Se mueven
//! tal cual desde `app.rs` como un `impl` más del mismo `EditorApp`
//! (Rust permite repartir los métodos inherentes en varios archivos),
//! sin cambiar comportamiento.

use std::path::{Path, PathBuf};

use karkinos_core::recovery::{journal_filename, remove_recovery_file};

use crate::app::{is_valid_explorer_item_name, EditorApp, ExplorerNewItemKind, SidebarTab};

impl EditorApp {
    /// v0.2.41: crea el ítem pendiente del diálogo inline del explorador.
    /// El nombre debe ser un único componente de ruta, así nunca se escribe
    /// fuera de `target_dir`. Un archivo creado se abre de inmediato.
    pub fn create_explorer_item(&mut self) -> Result<PathBuf, String> {
        let Some(kind) = self.explorer.creating_item else {
            return Err("No item is being created".to_string());
        };

        let name = self.explorer.new_item_name.trim().to_string();
        if !is_valid_explorer_item_name(&name) {
            let msg = "Invalid name: use a single file or folder name".to_string();
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let target_dir = self
            .explorer
            .new_item_target_dir
            .clone()
            .unwrap_or_else(|| self.doc.browse_dir.clone());
        let path = target_dir.join(&name);
        if path.exists() {
            let msg = format!("Already exists: {}", path.display());
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let created = match kind {
            ExplorerNewItemKind::File => std::fs::File::create(&path).map(|_| ()),
            ExplorerNewItemKind::Folder => std::fs::create_dir(&path),
        };
        if let Err(e) = created {
            let msg = format!("Could not create {}: {}", name, e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        self.explorer.cancel_new_item();
        self.explorer.loaded_for = None;
        self.refresh_explorer();

        if kind == ExplorerNewItemKind::File {
            self.open_path(path.clone());
        }
        self.doc.status = format!("Created {}", name);
        Ok(path)
    }

    /// v0.2.41: elimina un archivo del disco y cierra su pestaña si estaba
    /// abierto. Las carpetas se rechazan: el explorador solo ofrece el botón
    /// de borrado en filas de archivo.
    pub fn delete_explorer_file(&mut self, path: &Path) -> Result<(), String> {
        if path.is_dir() {
            let msg = format!("Not a file: {}", path.display());
            self.doc.status = msg.clone();
            return Err(msg);
        }

        if let Err(e) = std::fs::remove_file(path) {
            let msg = format!("Could not delete {}: {}", path.display(), e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        self.sync_active_to_tab();
        if let Some(idx) = self
            .doc
            .tabs
            .iter()
            .position(|t| t.current_file.as_deref() == Some(path))
        {
            self.close_tab(idx);
        }

        self.explorer.item_to_delete = None;
        self.explorer.loaded_for = None;
        self.refresh_explorer();
        self.doc.status = format!("Deleted {}", name);
        Ok(())
    }

    /// v0.2.49: abre el renombrado inline para `path` (archivo o carpeta).
    /// Guarda la ruta original y pre-carga el nombre actual como valor del input.
    pub fn begin_rename_explorer_item(&mut self, path: &Path) {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("item")
            .to_string();
        self.explorer.renaming_item = Some((path.to_path_buf(), name));
        self.explorer.rename_focus = true;
    }

    /// v0.2.49: cancela el renombrado inline sin tocar el disco.
    pub fn cancel_rename_explorer_item(&mut self) {
        self.explorer.renaming_item = None;
    }

    /// v0.2.49: ejecuta el renombrado pendiente (`renaming_item`) en disco y
    /// sincroniza las pestañas abiertas afectadas. Devuelve la ruta nueva o un
    /// error legible; en caso de error el diálogo queda abierto para corregir.
    pub fn confirm_rename_explorer_item(&mut self) -> Result<PathBuf, String> {
        let Some((old_path, new_name)) = self.explorer.renaming_item.clone() else {
            return Err("No item is being renamed".to_string());
        };

        let trimmed = new_name.trim().to_string();
        if !is_valid_explorer_item_name(&trimmed) {
            let msg = "Invalid name: use a single file or folder name".to_string();
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let parent = old_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.doc.browse_dir.clone());
        let new_path = parent.join(&trimmed);

        if new_path == old_path {
            self.explorer.renaming_item = None;
            self.doc.status = "Rename cancelled".to_string();
            return Ok(old_path);
        }
        if new_path.exists() {
            let msg = "Target already exists".to_string();
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let is_dir = old_path.is_dir();
        if let Err(e) = std::fs::rename(&old_path, &new_path) {
            let msg = format!("Could not rename {}: {}", old_path.display(), e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let old_name = old_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("item")
            .to_string();

        // La pestaña activa vive duplicada en `doc` y en `tabs[active_tab]`;
        // sincronizamos primero para que el espejo parta del estado actual.
        self.sync_active_to_tab();

        let mut renamed_tabs: Vec<(usize, PathBuf, PathBuf)> = Vec::new();

        if is_dir {
            for tab in &mut self.doc.tabs {
                if let Some(current) = tab.current_file.as_mut() {
                    if let Ok(rest) = current.strip_prefix(&old_path) {
                        let old_tab_path = current.clone();
                        let updated = new_path.join(rest);
                        *current = updated.clone();
                        renamed_tabs.push((tab.id, old_tab_path, updated));
                    }
                }
            }
            if let Some(current) = self.doc.current_file.as_mut() {
                if let Ok(rest) = current.strip_prefix(&old_path) {
                    *current = new_path.join(rest);
                }
            }
        } else {
            for tab in &mut self.doc.tabs {
                if tab.current_file.as_deref() == Some(old_path.as_path()) {
                    let old_tab_path = old_path.clone();
                    tab.current_file = Some(new_path.clone());
                    tab.file_path_input = trimmed.clone();
                    renamed_tabs.push((tab.id, old_tab_path, new_path.clone()));
                }
            }
            if self.doc.current_file.as_deref() == Some(old_path.as_path()) {
                self.doc.current_file = Some(new_path.clone());
                self.doc.file_path_input = trimmed.clone();
            }
        }

        // v0.2.54: sincronizar journals de recuperación para pestañas renombradas
        for (tab_id, old_tab_path, _) in renamed_tabs {
            let old_filename = journal_filename(Some(&old_tab_path), tab_id);
            let _ = remove_recovery_file(&self.recovery_dir, &old_filename);
            self.recovery_journaled.remove(&tab_id);
            self.recovery_changed_at.insert(tab_id, std::time::Instant::now());
        }

        self.explorer.renaming_item = None;
        self.explorer.loaded_for = None;
        self.refresh_explorer();
        self.refresh_git_status();
        self.refresh_git_branch();
        self.doc.status = format!("Renamed: {old_name} -> {trimmed}");
        Ok(new_path)
    }

    /// v0.2.49: duplica `path` como `<stem>_copy.<ext>` (o con sufijo `_N` si
    /// el nombre ya existe) y abre la copia en una pestaña nueva.
    pub fn explorer_duplicate_item(&mut self, path: &Path) -> Result<PathBuf, String> {
        if !path.is_file() {
            let msg = format!("Not a file: {}", path.display());
            self.doc.status = msg.clone();
            return Err(msg);
        }

        let parent = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.doc.browse_dir.clone());
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("copy")
            .to_string();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        let mut n = 0usize;
        let dest = loop {
            let name = if n == 0 {
                format!("{stem}_copy")
            } else {
                format!("{stem}_copy_{n}")
            };
            let name = if ext.is_empty() { name } else { format!("{name}.{ext}") };
            let candidate = parent.join(&name);
            if !candidate.exists() {
                break candidate;
            }
            n += 1;
        };

        if let Err(e) = std::fs::copy(path, &dest) {
            let msg = format!("Could not duplicate {}: {}", path.display(), e);
            self.doc.status = msg.clone();
            return Err(msg);
        }

        self.explorer.loaded_for = None;
        self.refresh_explorer();
        self.open_path(dest.clone());
        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        self.doc.status = format!("Duplicated {name}");
        Ok(dest)
    }

    /// v0.2.49: abre el renombrado inline del archivo activo y muestra el
    /// explorador para que el campo inline quede visible.
    pub fn begin_rename_active_file(&mut self) {
        if let Some(path) = self.doc.current_file.clone() {
            self.begin_rename_explorer_item(&path);
            self.sidebar_tab = SidebarTab::Explorer;
            self.explorer.open = true;
            self.workspace_search.open = false;
            self.refresh_explorer();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{EditorApp, ExplorerNewItemKind};
    fn explorer_temp_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "karkinos_explorer_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("crear raíz temporal");
        root
    }

    #[test]
    fn explorer_crear_archivo_lo_escribe_en_disco_y_lo_abre() {
        let root = explorer_temp_root("create_file");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.explorer.open = true;

        app.explorer.begin_new_item(ExplorerNewItemKind::File, None);
        app.explorer.new_item_name = "nuevo.rs".to_string();

        let path = app.create_explorer_item().expect("crear archivo");

        assert_eq!(path, root.join("nuevo.rs"));
        assert!(path.is_file(), "el archivo existe en disco");
        assert_eq!(
            app.doc.current_file,
            Some(path.clone()),
            "el archivo se abre de inmediato"
        );
        assert!(
            app.explorer.files.contains(&path),
            "el explorador se refresca con el archivo nuevo"
        );
        assert_eq!(app.explorer.creating_item, None);
        assert!(app.explorer.new_item_name.is_empty());
        assert_eq!(app.doc.status, "Created nuevo.rs");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_crear_carpeta_en_disco_y_rechaza_nombres_invalidos() {
        let root = explorer_temp_root("create_folder");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.explorer
            .begin_new_item(ExplorerNewItemKind::Folder, Some(root.clone()));
        app.explorer.new_item_name = "nueva_carpeta".to_string();

        let path = app.create_explorer_item().expect("crear carpeta");

        assert_eq!(path, root.join("nueva_carpeta"));
        assert!(path.is_dir());
        assert_eq!(
            app.doc.current_file, None,
            "una carpeta no se abre en el editor"
        );
        assert_eq!(app.explorer.creating_item, None);

        app.explorer.begin_new_item(ExplorerNewItemKind::File, None);
        app.explorer.new_item_name = "   ".to_string();
        assert!(app.create_explorer_item().is_err(), "nombre vacío");
        assert!(
            app.explorer.creating_item.is_some(),
            "el diálogo sigue abierto para corregir"
        );

        app.explorer.new_item_name = "../escapa".to_string();
        assert!(app.create_explorer_item().is_err(), "no sale de la raíz");
        assert!(!root.parent().unwrap().join("escapa").exists());

        app.explorer.new_item_name = "nuevo.rs".to_string();
        std::fs::write(root.join("nuevo.rs"), "ya existe").expect("archivo existente");
        assert!(
            app.create_explorer_item().is_err(),
            "no pisa archivos existentes"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_borrar_archivo_cierra_su_pestana_y_refresca() {
        let root = explorer_temp_root("delete_file");
        let a = root.join("a.rs");
        let b = root.join("b.rs");
        std::fs::write(&a, "fn a() {}").expect("a");
        std::fs::write(&b, "fn b() {}").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();
        app.open_path(a.clone());
        app.open_path(b.clone());
        assert_eq!(app.doc.current_file, Some(b.clone()));
        assert_eq!(app.doc.tabs.len(), 2);

        app.delete_explorer_file(&a).expect("borrar a.rs");

        assert!(!a.exists(), "a.rs desaparece del disco");
        assert!(b.exists(), "b.rs queda intacto");
        assert_eq!(
            app.doc.current_file,
            Some(b.clone()),
            "la pestaña activa no cambia"
        );
        assert!(app
            .doc
            .tabs
            .iter()
            .all(|t| t.current_file.as_deref() != Some(a.as_path())));
        assert!(!app.explorer.files.contains(&a));
        assert_eq!(app.doc.status, "Deleted a.rs");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_borrar_archivo_activo_resetea_la_pestana() {
        let root = explorer_temp_root("delete_active");
        let a = root.join("unico.rs");
        std::fs::write(&a, "fn unico() {}").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(a.clone());
        assert_eq!(app.doc.tabs.len(), 1);

        app.delete_explorer_file(&a).expect("borrar archivo activo");

        assert!(!a.exists());
        assert_eq!(app.doc.current_file, None);
        assert_eq!(app.doc.tabs.len(), 1, "la única pestaña se resetea en blanco");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_borrar_archivo_inexistente_y_carpeta_devuelven_error() {
        let root = explorer_temp_root("delete_missing");
        let missing = root.join("no_existe.rs");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        assert!(app.delete_explorer_file(&missing).is_err());
        assert!(
            app.delete_explorer_file(&root).is_err(),
            "rechaza carpetas, solo borra archivos"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    // v0.2.49: renombrado y duplicado desde el explorador.

    #[test]
    fn explorer_renombrar_archivo_sincroniza_pestana_y_titulo() {
        let root = explorer_temp_root("rename_file");
        let a = root.join("a.rs");
        std::fs::write(&a, "fn a() {}").expect("a");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(a.clone());
        assert_eq!(app.doc.current_file, Some(a.clone()));

        app.begin_rename_explorer_item(&a);
        app.explorer.renaming_item.as_mut().unwrap().1 = "b.rs".to_string();
        let new_path = app.confirm_rename_explorer_item().expect("renombrar");

        let b = root.join("b.rs");
        assert_eq!(new_path, b);
        assert!(!a.exists(), "a.rs desaparece del disco");
        assert!(b.exists(), "b.rs aparece en disco");
        assert_eq!(app.doc.current_file, Some(b.clone()));
        assert_eq!(app.doc.file_path_input, "b.rs", "el título de la pestaña activa");
        assert!(app
            .doc
            .tabs
            .iter()
            .all(|t| t.current_file.as_deref() != Some(a.as_path())));
        assert_eq!(app.doc.tabs[0].current_file, Some(b.clone()));
        assert_eq!(app.doc.status, "Renamed: a.rs -> b.rs");
        assert!(app.explorer.renaming_item.is_none());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_renombrar_carpeta_actualiza_pestanas_anidadas() {
        let root = explorer_temp_root("rename_folder");
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("crear src");
        let main = src.join("main.rs");
        std::fs::write(&main, "fn main() {}").expect("main");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(main.clone());
        assert_eq!(app.doc.current_file, Some(main.clone()));

        app.begin_rename_explorer_item(&src);
        app.explorer.renaming_item.as_mut().unwrap().1 = "src2".to_string();
        let new_dir = app.confirm_rename_explorer_item().expect("renombrar carpeta");

        let src2 = root.join("src2");
        assert_eq!(new_dir, src2);
        assert!(!src.exists(), "src desaparece del disco");
        assert!(src2.exists(), "src2 aparece en disco");
        let moved_main = src2.join("main.rs");
        assert!(moved_main.exists(), "main.rs se mueve dentro de src2");
        assert_eq!(
            app.doc.current_file,
            Some(moved_main.clone()),
            "la pestaña activa reescribe su prefijo"
        );
        assert_eq!(app.doc.tabs[0].current_file, Some(moved_main));
        assert_eq!(app.doc.status, "Renamed: src -> src2");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_renombrar_a_nombre_existente_o_invalido_es_rechazado() {
        let root = explorer_temp_root("rename_reject");
        let a = root.join("a.rs");
        let b = root.join("b.rs");
        std::fs::write(&a, "fn a() {}").expect("a");
        std::fs::write(&b, "fn b() {}").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        app.begin_rename_explorer_item(&a);
        app.explorer.renaming_item.as_mut().unwrap().1 = "   ".to_string();
        assert!(app.confirm_rename_explorer_item().is_err(), "nombre vacío");
        assert!(a.exists(), "a.rs queda intacto");

        app.begin_rename_explorer_item(&a);
        app.explorer.renaming_item.as_mut().unwrap().1 = "../escape".to_string();
        assert!(app.confirm_rename_explorer_item().is_err(), "no sale de la raíz");
        assert!(a.exists());
        assert!(!root.parent().unwrap().join("escape").exists());

        app.begin_rename_explorer_item(&a);
        app.explorer.renaming_item.as_mut().unwrap().1 = "b.rs".to_string();
        assert!(app.confirm_rename_explorer_item().is_err(), "no pisa archivos existentes");
        assert!(a.exists());
        assert!(b.exists());
        assert!(
            app.explorer.renaming_item.is_some(),
            "el diálogo sigue abierto para corregir"
        );

        app.cancel_rename_explorer_item();
        assert!(app.explorer.renaming_item.is_none());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn explorer_duplicar_genera_copia_libre_y_la_abre() {
        let root = explorer_temp_root("duplicate");
        let a = root.join("a.rs");
        std::fs::write(&a, "fn a() {}").expect("a");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();

        let dest = app.explorer_duplicate_item(&a).expect("duplicar");
        assert_eq!(dest, root.join("a_copy.rs"));
        assert!(dest.is_file());
        assert_eq!(
            std::fs::read_to_string(&dest).expect("leer copia"),
            "fn a() {}",
            "la copia es fiel"
        );
        assert_eq!(app.doc.current_file, Some(dest.clone()), "la copia se abre");

        let dest2 = app.explorer_duplicate_item(&a).expect("duplicar de nuevo");
        assert_eq!(dest2, root.join("a_copy_1.rs"), "elige un nombre libre");
        assert!(dest2.is_file());
        assert_eq!(app.doc.current_file, Some(dest2.clone()));

        let carpeta = root.join("sub");
        std::fs::create_dir_all(&carpeta).expect("carpeta");
        assert!(
            app.explorer_duplicate_item(&carpeta).is_err(),
            "rechaza carpetas, solo duplica archivos"
        );

        std::fs::remove_dir_all(&root).ok();
    }



}
