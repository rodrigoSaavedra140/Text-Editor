    use super::*;
    // Nombres que la T5a movió a document_service.rs / karkinos-core:
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime};
    use crate::document_service::{mtime_is_stale, SaveProgress};
    use karkinos_core::{default_new_file_path, detect_line_ending, serialize_line_ending};

    // ---- S10: Zoom de fuente ----

    #[test]
    fn zoom_reseta_tamano_y_respeta_el_clamp() {
        const EPS: f32 = 0.001;
        // Zoom in avanza un paso...
        assert!((apply_zoom(14.0, true) - 15.0).abs() < EPS);
        // ...y zoom out retrocede.
        assert!((apply_zoom(14.0, false) - 13.0).abs() < EPS);
        // Clamp por abajo: en 8.0 ya no baja más.
        assert!((apply_zoom(8.0, false) - 8.0).abs() < EPS);
        // Clamp por arriba: en 32.0 ya no sube más.
        assert!((apply_zoom(32.0, true) - 32.0).abs() < EPS);
        // Fuera de rango por valores espurio: se recorta al entrar.
        assert!((apply_zoom(50.0, true) - 32.0).abs() < EPS);
        assert!((apply_zoom(1.0, false) - 8.0).abs() < EPS);
    }

    #[test]
    fn zoom_redondea_a_pasos_enteros() {
        const EPS: f32 = 0.001;
        assert!((apply_zoom(14.3, true) - 15.0).abs() < EPS);
        assert!((apply_zoom(14.3, false) - 13.0).abs() < EPS);
    }

    #[test]
    fn zoom_metodo_actualiza_font_size_y_avisa_en_status() {
        const EPS: f32 = 0.001;
        let mut app = EditorApp::default();
        let base = app.font_size;
        app.zoom(true);
        assert!((app.font_size - (base + 1.0)).abs() < EPS);
        assert!(app.doc.status.contains("Zoom"));

        app.zoom(false);
        assert!((app.font_size - base).abs() < EPS);

        // sincroniza persistence: el payload que se guarda y se lee
        // del Storage es exactamente el tamaño actual.
        app.font_size = 17.5;
        assert_eq!(font_size_to_string(app.font_size), "17.5");
        assert_eq!(font_size_from_string("17.5"), Some(17.5));
        assert_eq!(font_size_from_string("basura"), None);

        // S10: clamp también al cargar: un Storage corrupto machaca
        // la fuente inválida en lugar de crashear o dibujar gigante.
        app.font_size = font_size_from_string("999.0").unwrap_or_default();
        assert!((app.font_size - ZOOM_MAX).abs() < EPS);
    }

    #[test]
    fn set_font_size_sincroniza_editor_state_y_tabs_y_split() {
        const EPS: f32 = 0.001;
        let mut app = EditorApp::default();
        app.toggle_split(SplitDirection::Vertical);

        app.set_font_size(22.0);
        assert!((app.font_size - 22.0).abs() < EPS);
        assert!((app.editor_state.font_size - 22.0).abs() < EPS);
        assert!(
            (app.split.as_ref().unwrap().secondary.font_size - 22.0).abs() < EPS,
            "el split secundario debe recibir el nuevo font_size"
        );
        for tab in &app.doc.tabs {
            assert!((tab.editor_state.font_size - 22.0).abs() < EPS);
        }

        app.reset_zoom();
        assert!((app.font_size - crate::editor_widget::DEFAULT_FONT_SIZE).abs() < EPS);
        assert!((app.editor_state.font_size - crate::editor_widget::DEFAULT_FONT_SIZE).abs() < EPS);
    }

    #[test]
    fn execute_palette_command_zoom_actions() {
        let mut app = EditorApp::default();
        let initial = app.font_size;

        app.execute_palette_command("view:zoom_in");
        assert_eq!(app.font_size, initial + 1.0);

        app.execute_palette_command("view:zoom_out");
        assert_eq!(app.font_size, initial);

        app.execute_palette_command("view:reset_zoom");
        assert_eq!(app.font_size, crate::editor_widget::DEFAULT_FONT_SIZE);
    }



    // ---- S9: preservar saltos de línea originales ----

    #[test]
    fn detecta_ending_crlf_lf_y_archivo_nuevo() {
        assert_eq!(detect_line_ending("a\r\nb"), LineEnding::Crlf);
        assert_eq!(detect_line_ending("a\nb"), LineEnding::Lf);
        assert_eq!(detect_line_ending(""), LineEnding::Lf); // documento nuevo
        // SI un \r suelto existe sin su \n, no lo contamos como CRLF.
        assert_eq!(detect_line_ending("a\rb\nc"), LineEnding::Lf);
    }

    #[test]
    fn serialize_reintroduce_crlf_y_deja_lf_tal_cual() {
        assert_eq!(
            serialize_line_ending("a\nb\nc", LineEnding::Crlf),
            "a\r\nb\r\nc"
        );
        assert_eq!(serialize_line_ending("a\nb", LineEnding::Lf), "a\nb");
    }

    #[test]
    fn abrir_archivo_crlf_stack_buffer_limpio_y_ending_recordado()
    -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("karkinos_crlf_{}.txt", std::process::id()));
        std::fs::write(&path, "uno\r\ndos\r\n\r\ntres")?;
        let mut app = EditorApp::default();
        app.open_path(path.clone());
        // Dentro del buffer quedan líneas SIN \r escondido.
        assert_eq!(app.doc.text(), "uno\ndos\n\ntres");
        // Y el serializer trae de vuelta el archivo EXACTO original.
        assert_eq!(app.doc.current_text_with_ending(), "uno\r\ndos\r\n\r\ntres");
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn abrir_archivo_lf_preserva_lf() -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("karkinos_lf_{}.txt", std::process::id()));
        std::fs::write(&path, "a\nb")?;
        let mut app = EditorApp::default();
        app.open_path(path.clone());
        assert_eq!(app.doc.current_text_with_ending(), "a\nb");
        std::fs::remove_file(path)?;
        Ok(())
    }

    // ---- S1: cierre pedido con cambios sin guardar ----

    #[test]
    fn cerrar_guardando_sin_ruta_guarda_como_nuevo_txt_y_cierra()
    -> std::io::Result<()> {
        let dir = std::env::temp_dir()
            .join(format!("karkinos_close_default_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let _ = std::fs::remove_file(dir.join("untitled.txt"));

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.doc.dirty = true;
        // Sin ruta y sin archivo: save() ELIGE "untitled.txt" como
        // nombre default (bugfix 2026-10-29), así que el diálogo
        // ahora sí puede prometer el cierre: el guardado arranca
        // sobre un destino concreto en vez de abortar.
        app.doc.confirm_close_save();
        assert!(!app.doc.close_pending, "el diálogo sí puede cerrar: el save arrancó");
        assert!(app.doc.close_after_save);
        // El destino queda fijado a dir/untitled.txt antes del async.
        assert_eq!(app.doc.current_file, Some(dir.join("untitled.txt")));
        if let Some(progress) = &app.doc.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let _ = std::fs::remove_file(dir.join("untitled.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn note_save_done_refresca_mtime_y_cancela_cierre_en_error()
    -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("karkinos_save_{}.txt", std::process::id()));
        std::fs::write(&path, "contenido")?;
        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        // Memo viejo (como si el buffer hubiera venido de un archivo
        // distinto): tras un guardado ok tiene que quedar "no stale".
        app.doc.last_disk_mtime = None;
        let progress = Arc::new(Mutex::new(SaveProgress {
            written: 9,
            total: 9,
            done: true,
            error: None,
            saved_text_hash: 0,
            target: path.clone(),
            previous_file: None,
        }));
        app.doc.save_progress = Some(Arc::clone(&progress));
        app.doc.close_after_save = true;
        app.doc.note_save_done();
        assert!(app.doc.last_disk_mtime.is_some(), "post-guardado el memo tiene mtime nuevo");
        assert!(app.doc.close_after_save, "success: sigue el plan de cerrar");

        // Con error: cancelar el cierre automático y NO refrescar memo.
        progress.lock().unwrap().error = Some("disco lleno".to_string());
        app.doc.note_save_done();
        assert!(!app.doc.close_after_save);
        std::fs::remove_file(path)?;
        Ok(())
    }

    // ---- S1: archivo cambiado en el disco (reload externo) ----

    #[test]
    fn mtime_viejo_con_actual_diferente_es_stale() {
        let t = SystemTime::now();
        // No cambió: memo igual.
        assert!(!mtime_is_stale(Some(t), Some(t)));
        // Cambió en el disco (actual más nuevo).
        assert!(mtime_is_stale(Some(t), Some(t + Duration::from_secs(3))));
        // Borrado del disco: no hay mtime actual pero SÍ recordamos uno.
        assert!(mtime_is_stale(None, Some(t)));
        // Archivo en disco cuyo mtime todavía no memorizamos: está
        // "sin cambios por afuera" por DEFAULT — ofrecer recargar
        // sobre un buffer que nunca se comparó con disco sería
        // riesgo de datos.
        assert!(!mtime_is_stale(Some(t), None));
        // Documento nunca ligado a disco: recién ahí tampoco.
        assert!(!mtime_is_stale(None, None));
    }

    // ---- S3: undo/redo NO deben saltar al principio del documento ----

    #[test]
    fn undo_y_redo_conservan_cursor_y_scroll_en_su_lugar() {
        let mut app = EditorApp::default();
        app.set_text("uno\ndos\ntres");
        app.doc.snapshot_if_changed();

        // Simulo que el usuario tipeó más y la vista se movió abajo.
        app.set_text("uno\ndos\ntres\nCUATRO");
        app.doc.snapshot_if_changed();
        app.editor_state.cursor_line = 1;
        app.editor_state.cursor_col = 1;
        app.editor_state.scroll = 99.0;

        app.undo();
        // El texto vuelve al paso anterior...
        assert_eq!(app.doc.current_text(), "uno\ndos\ntres");
        // ...pero la vista NO se resetea a (0,0) / scroll 0.
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 1);
        assert!((app.editor_state.scroll - 99.0).abs() < f32::EPSILON);

        app.redo();
        assert_eq!(app.doc.current_text(), "uno\ndos\ntres\nCUATRO");
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 1);
        assert!((app.editor_state.scroll - 99.0).abs() < f32::EPSILON);
    }

    #[test]
    fn undo_restaura_cursor_clamp_fuera_del_buffer_reparado() {
        let mut app = EditorApp::default();
        app.set_text("sola una linea");
        app.doc.snapshot_if_changed();
        app.set_text("uno\ndos\ntres");
        app.doc.snapshot_if_changed();
        // Captura el estado del cursor fuera de rango (para ver como
        // aterrizaría sobre el buffer deshecho de UNA sola línea).
        app.editor_state.cursor_line = 2;
        app.editor_state.scroll = 40.0;

        app.undo();
        // El buffer deshecho tiene UNA línea: la 2 no existe y el
        // cursor aterriza al FIN de esa línea, no crashea ni va a 0.
        assert_eq!(app.doc.current_text(), "sola una linea");
        assert_eq!(app.editor_state.cursor_line, 0);
        assert_eq!(app.editor_state.cursor_col, "sola una linea".chars().count());
        assert!((app.editor_state.scroll - 40.0).abs() < f32::EPSILON);
    }


    // ---- Bugfix 2026-10-29: guardar archivo nuevo sin nombre ----

    #[test]
    fn nombre_default_nuevo_txt_y_sin_pisar_colisiones() {
        use std::collections::HashSet;
        use std::path::{Path, PathBuf};

        let dir = Path::new("/tmp/karkinos_test");
        fn mk_exists<'a>(
            existentes: &'a HashSet<&'static str>,
        ) -> impl Fn(&Path) -> bool + 'a {
            move |p: &Path| existentes.contains(p.file_name().and_then(|n| n.to_str()).unwrap_or(""))
        }

        // Caso feliz: carpeta libre → dir/untitled.txt
        assert_eq!(
            default_new_file_path(dir, &mk_exists(&HashSet::new())),
            PathBuf::from("/tmp/karkinos_test/untitled.txt")
        );

        // Colisión con untitled.txt → untitled (1).txt
        let mut existentes = HashSet::new();
        existentes.insert("untitled.txt");
        assert_eq!(
            default_new_file_path(dir, &mk_exists(&existentes)),
            PathBuf::from("/tmp/karkinos_test/untitled (1).txt")
        );

        // Colisión en cadena → cuenta mientras el nombre exista.
        existentes.insert("untitled (1).txt");
        existentes.insert("untitled (2).txt");
        assert_eq!(
            default_new_file_path(dir, &mk_exists(&existentes)),
            PathBuf::from("/tmp/karkinos_test/untitled (3).txt")
        );
    }

    #[test]
    fn save_con_input_vacio_guarda_como_nuevo_txt() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_save_default_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // limpio cualquier resto de corridas anteriores
        let _ = std::fs::remove_file(dir.join("untitled.txt"));

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.set_text("contenido sin nombre");
        app.doc.dirty = true;
        // current_file None y file_path_input vacío: el caso reportado.
        app.doc.file_path_input.clear();

        assert!(app.doc.save());
        // Elige dir/untitled.txt y muestra en el input el nombre elegido.
        assert_eq!(app.doc.current_file, Some(dir.join("untitled.txt")));
        assert_eq!(app.doc.file_path_input, "untitled.txt");
        // Espera acotado al guardado asincrónico hasta que el disco
        // tenga el contenido completo.
        use std::io::Read;
        let path = dir.join("untitled.txt");
        let mut contenido = String::new();
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            if let Ok(mut f) = std::fs::File::open(&path) {
                f.read_to_string(&mut contenido)?;
                if contenido.contains("contenido sin nombre") {
                    break;
                }
            }
        }
        assert!(contenido.contains("contenido sin nombre"), "el archivo necesita terminar el guardado async");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    // ---- T2 (S2/S3): dirty y current_file al cerrar un guardado ----

    /// Espera acotada a que el hilo de guardado marque `done`.
    fn wait_for_save_done(app: &EditorApp) {
        for _ in 0..400 {
            let done = app
                .doc
                .save_progress
                .as_ref()
                .map(|p| p.lock().unwrap().done)
                .unwrap_or(false);
            if done {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("el guardado asincrónico no terminó");
    }

    #[test]
    fn guardar_y_editar_durante_el_guardado_deja_dirty() -> std::io::Result<()> {
        let dir = std::env::temp_dir()
            .join(format!("karkinos_dirty_during_save_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("doc.txt");
        let _ = std::fs::remove_file(&path);

        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        app.set_text("a");
        assert!(app.doc.save());
        // El usuario teclea mientras el hilo de escritura sigue vivo.
        app.set_text("ab");
        wait_for_save_done(&app);

        assert!(app.doc.finish_save());
        assert!(app.doc.dirty, "lo que está en disco ya no coincide con el buffer");
        assert_eq!(std::fs::read_to_string(&path)?, "a");
        std::fs::remove_file(&path)?;
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn guardar_sin_editar_limpia_dirty() -> std::io::Result<()> {
        let dir = std::env::temp_dir()
            .join(format!("karkinos_clean_after_save_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("doc.txt");
        let _ = std::fs::remove_file(&path);

        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        app.set_text("a");
        app.doc.dirty = true;
        assert!(app.doc.save());
        wait_for_save_done(&app);

        assert!(app.doc.finish_save());
        assert!(!app.doc.dirty, "sin ediciones durante el guardado queda limpio");
        assert_eq!(app.doc.status, "Saved");
        std::fs::remove_file(&path)?;
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn guardado_fallido_restaura_current_file() {
        let mut app = EditorApp::default();
        app.doc.current_file = None;
        app.doc.file_path_input = "/nonexistent_dir_karkinos/x.txt".to_string();
        app.set_text("x");
        app.doc.dirty = true;
        assert!(app.doc.save());
        wait_for_save_done(&app);

        assert!(!app.doc.finish_save(), "la escritura falló");
        assert_eq!(app.doc.current_file, None, "no queda apuntando a una ruta que no se guardó");
        assert!(app.doc.status.starts_with("Save error"), "status: {}", app.doc.status);
        assert!(app.doc.dirty, "un guardado fallido no limpia dirty");
    }


    #[test]
    fn picker_se_queda_abierto_solo_si_el_click_es_adentro() {
        use egui::{pos2, Rect};

        let popup = Rect::from_min_max(pos2(100.0, 100.0), pos2(300.0, 400.0));
        let button = Rect::from_min_max(pos2(10.0, 10.0), pos2(80.0, 30.0));

        // Click adentro del popup (navegó una carpeta): sigue abierto.
        assert!(picker_should_stay_open(pos2(150.0, 200.0), Some(popup), Some(button)));
        // Click sobre el botón de toggle: él mismo maneja su estado.
        assert!(picker_should_stay_open(pos2(40.0, 20.0), Some(popup), Some(button)));
        // Click bien lejos (editor, otro lado de la ventana): cerrar.
        assert!(!picker_should_stay_open(pos2(700.0, 500.0), Some(popup), Some(button)));
        // Sin rects conocidos aún: cerrar (estado seguro).
        assert!(!picker_should_stay_open(pos2(150.0, 200.0), None, None));
    }

    // ---- S2: Buscar y reemplazar (Ctrl+F) ----

    #[test]
    fn buscar_coincidencias_case_sensitive_e_insensitive() {
        let mut app = EditorApp::default();
        app.set_text("Hola mundo\nhola otro mundo\nHOLA final");

        // Case-insensitive (default)
        app.find_query = "hola".to_string();
        app.find_case_sensitive = false;
        app.find_update_matches();
        assert_eq!(app.find_matches, vec![(0, 0), (1, 0), (2, 0)]);

        // Case-sensitive
        app.find_case_sensitive = true;
        app.find_update_matches();
        assert_eq!(app.find_matches, vec![(1, 0)]);

        // Sin query: lista vacía
        app.find_query = "".to_string();
        app.find_update_matches();
        assert!(app.find_matches.is_empty());
    }

    #[test]
    fn navegacion_siguiente_anterior_find_matches() {
        let mut app = EditorApp::default();
        app.set_text("uno\nuno\nuno");
        app.find_query = "uno".to_string();
        app.find_update_matches();
        assert_eq!(app.find_matches.len(), 3);
        assert_eq!(app.find_current, 0);

        app.find_next();
        assert_eq!(app.find_current, 1);
        app.find_next();
        assert_eq!(app.find_current, 2);
        // Wrap around
        app.find_next();
        assert_eq!(app.find_current, 0);

        // Prev wrap around
        app.find_prev();
        assert_eq!(app.find_current, 2);
        app.find_prev();
        assert_eq!(app.find_current, 1);
    }

    #[test]
    fn reemplazo_actual_y_reemplazo_todo_con_undo() {
        let mut app = EditorApp::default();
        app.set_text("gato y perro y gato");
        app.find_query = "gato".to_string();
        app.find_replace = "lobo".to_string();
        app.find_update_matches();
        assert_eq!(app.find_matches.len(), 2);

        // Reemplazar primer match
        app.find_replace_current();
        assert_eq!(app.doc.current_text(), "lobo y perro y gato");

        // Reemplazar todo lo que queda
        app.find_replace_all();
        assert_eq!(app.doc.current_text(), "lobo y perro y lobo");

        // Undo revierte el replace all
        app.undo();
        assert_eq!(app.doc.current_text(), "lobo y perro y gato");

        // Undo revierte el replace individual
        app.undo();
        assert_eq!(app.doc.current_text(), "gato y perro y gato");
    }

    // ---- S8: Botón Nuevo documento ----

    #[test]
    fn nuevo_documento_resetea_buffer_y_metadatos() {
        let mut app = EditorApp::default();
        app.set_text("texto previo");
        app.doc.current_file = Some(PathBuf::from("dummy.txt"));
        app.doc.file_path_input = "dummy.txt".to_string();
        app.doc.dirty = true;
        app.new_document();

        assert_eq!(app.doc.current_text(), "");
        assert_eq!(app.doc.text(), "");
        assert_eq!(app.doc.current_file, None);
        assert_eq!(app.doc.file_path_input, "");
        assert!(!app.doc.dirty);
        assert_eq!(app.doc.status, "New document");
    }

    #[test]
    fn try_nuevo_documento_con_dirty_pide_confirmacion() {
        let mut app = EditorApp::default();
        app.set_text("Some unsaved text");
        app.doc.dirty = true;
        app.try_new_document();
        assert!(app.doc.new_doc_pending);

        // Sin cambios: crea de una sin pedir confirmación
        let mut app_clean = EditorApp::default();
        app_clean.try_new_document();
        assert!(!app_clean.doc.new_doc_pending);

        // Buffer vacío aunque esté dirty: no pide confirmación
        let mut app_empty_dirty = EditorApp::default();
        app_empty_dirty.doc.dirty = true;
        app_empty_dirty.try_new_document();
        assert!(!app_empty_dirty.doc.new_doc_pending);
    }

    #[test]
    fn test_empty_file_exit_does_not_prompt_close() {
        let mut app = EditorApp::default();
        // Even if dirty is marked true, empty buffer means all dirty tabs are empty
        app.doc.dirty = true;
        assert!(app.doc.all_dirty_tabs_are_empty());

        // Closing an empty dirty tab closes without pending confirmation
        app.try_close_tab(0);
        assert!(app.doc.tab_close_pending.is_none());

        // With text, all_dirty_tabs_are_empty is false
        app.set_text("Actual content");
        app.doc.dirty = true;
        assert!(!app.doc.all_dirty_tabs_are_empty());
        assert_eq!(app.doc.first_dirty_tab_index(), Some(0));
    }

    // ---- S8 (Etapa 2): Pestañas / Múltiples documentos ----

    #[test]
    fn tabs_creacion_cambio_y_cierre() {
        let mut app = EditorApp::default();
        assert_eq!(app.doc.tabs.len(), 1);
        assert_eq!(app.doc.active_tab, 0);

        app.set_text("Texto en pestaña 1");
        app.doc.dirty = true;

        app.new_tab();
        assert_eq!(app.doc.tabs.len(), 2);
        assert_eq!(app.doc.active_tab, 1);
        assert_eq!(app.doc.current_text(), "");

        app.set_text("Texto en pestaña 2");
        app.switch_tab(0);
        assert_eq!(app.doc.active_tab, 0);
        assert_eq!(app.doc.current_text(), "Texto en pestaña 1");

        app.switch_tab(1);
        assert_eq!(app.doc.active_tab, 1);
        assert_eq!(app.doc.current_text(), "Texto en pestaña 2");

        app.close_tab(0);
        assert_eq!(app.doc.tabs.len(), 1);
        assert_eq!(app.doc.active_tab, 0);
        assert_eq!(app.doc.current_text(), "Texto en pestaña 2");
    }

    #[test]
    fn tabs_open_path_no_duplica_y_reutiliza_vacia() -> std::io::Result<()> {
        let dir = std::env::temp_dir();
        let path_a = dir.join(format!("karkinos_tab_a_{}.txt", std::process::id()));
        let path_b = dir.join(format!("karkinos_tab_b_{}.txt", std::process::id()));
        std::fs::write(&path_a, "archivo A")?;
        std::fs::write(&path_b, "archivo B")?;

        let mut app = EditorApp::default();
        // Abrir en pestaña inicial limpia debe reutilizarla
        app.open_path(path_a.clone());
        assert_eq!(app.doc.tabs.len(), 1);
        assert_eq!(app.doc.current_file, Some(path_a.clone()));

        // Abrir segundo archivo debe crear pestaña nueva
        app.open_path(path_b.clone());
        assert_eq!(app.doc.tabs.len(), 2);
        assert_eq!(app.doc.active_tab, 1);
        assert_eq!(app.doc.current_file, Some(path_b.clone()));

        // Abrir el archivo A nuevamente debe cambiar a la pestaña 0 sin duplicar
        app.open_path(path_a.clone());
        assert_eq!(app.doc.tabs.len(), 2);
        assert_eq!(app.doc.active_tab, 0);
        assert_eq!(app.doc.current_file, Some(path_a.clone()));

        std::fs::remove_file(path_a)?;
        std::fs::remove_file(path_b)?;
        Ok(())
    }

    // ---- T1: el `Rope` se comparte al sincronizar la pestaña activa ----

    #[test]
    fn sync_active_to_tab_conserva_el_texto_exacto() {
        let mut app = EditorApp::default();
        // Tildes, emoji y CRLF (que `set_text` normaliza a '\n').
        app.set_text("línea con tildes\r\nsegundo 😀 renglón\r\ntercero");
        app.sync_active_to_tab();

        let esperado = app.doc.text();
        let pestaña = &app.doc.tabs[app.doc.active_tab];
        assert_eq!(pestaña.text(), esperado);
        assert!(esperado.contains("tildes"));
        assert!(esperado.contains("😀"));
        assert!(!esperado.contains('\r'));
    }

    #[test]
    fn cambiar_de_pestana_y_volver_conserva_texto_y_cursor() {
        let mut app = EditorApp::default();
        app.set_text("primera pestaña\ncon dos líneas");
        app.doc.dirty = true;
        app.editor_state.cursor_line = 1;
        app.editor_state.cursor_col = 3;

        app.new_tab();
        app.set_text("segunda pestaña");
        assert_eq!(app.doc.tabs.len(), 2);

        app.switch_tab(0);
        assert_eq!(app.doc.current_text(), "primera pestaña\ncon dos líneas");
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 3);

        app.switch_tab(1);
        assert_eq!(app.doc.current_text(), "segunda pestaña");

        app.switch_tab(0);
        assert_eq!(app.doc.current_text(), "primera pestaña\ncon dos líneas");
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 3);
    }

    /// Rendimiento: correr a mano con
    /// `cargo test --release -p karkinos -- --ignored sync_active_to_tab_es_barato`.
    /// Antes de T1 cada llamada reconstruía el `Rope` completo desde un
    /// `String`; ahora es un clon O(1) del `Rope` compartido.
    #[test]
    #[ignore = "rendimiento: correr a mano con --release -- --ignored"]
    fn sync_active_to_tab_es_barato_con_documento_grande() {
        const LINEAS: usize = 200_000;
        const LLAMADAS: usize = 200;

        let mut texto = String::with_capacity(LINEAS * 20);
        for i in 0..LINEAS {
            texto.push_str("línea de prueba ");
            texto.push_str(&i.to_string());
            texto.push('\n');
        }

        let mut app = EditorApp::default();
        app.set_text(&texto);

        let inicio = std::time::Instant::now();
        for _ in 0..LLAMADAS {
            app.sync_active_to_tab();
        }
        let transcurrido = inicio.elapsed();
        eprintln!("{LLAMADAS} sync_active_to_tab sobre {LINEAS} líneas: {transcurrido:?}");

        // El umbral solo tiene sentido en release (en debug el motor de
        // tests no aplica las optimizaciones que se están midiendo).
        if !cfg!(debug_assertions) {
            assert!(
                transcurrido < Duration::from_millis(50),
                "{LLAMADAS} sincronizaciones tardaron {transcurrido:?}"
            );
        }
    }

    // ---- Guardado de archivo sin nombre con diálogo modal ----

    #[test]
    fn try_save_sin_nombre_abre_modal_y_guarda_con_nombre_o_default() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_try_save_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.set_text("codigo de prueba");
        app.doc.dirty = true;

        // 1. try_save() en archivo sin nombre debe abrir el modal y no guardar aún
        assert!(!app.doc.try_save());
        assert!(app.doc.save_name_pending);

        // 2. Confirmar con nombre "mi_programa.rs"
        app.doc.save_name_input = "mi_programa.rs".to_string();
        assert!(app.doc.confirm_save_name());
        assert!(!app.doc.save_name_pending);
        assert_eq!(app.doc.current_file, Some(dir.join("mi_programa.rs")));
        assert_eq!(app.doc.file_path_input, "mi_programa.rs");

        // Esperar a que termine el guardado asíncrono
        if let Some(progress) = &app.doc.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        // 3. Otro documento sin nombre: dejar en blanco debe asignar "untitled.txt"
        let mut app2 = EditorApp::default();
        app2.doc.browse_dir = dir.clone();
        app2.set_text("otro texto");
        app2.doc.dirty = true;
        assert!(!app2.doc.try_save());
        assert!(app2.doc.save_name_pending);

        // Dejar vacío
        app2.doc.save_name_input = "   ".to_string();
        assert!(app2.doc.confirm_save_name());
        assert!(!app2.doc.save_name_pending);
        assert_eq!(app2.doc.current_file, Some(dir.join("untitled.txt")));
        assert_eq!(app2.doc.file_path_input, "untitled.txt");

        if let Some(progress) = &app2.doc.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let _ = std::fs::remove_file(dir.join("mi_programa.rs"));
        let _ = std::fs::remove_file(dir.join("untitled.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn test_tabs_cycling_and_dirty_detection() {
        let mut app = EditorApp::default();
        app.new_tab();
        app.new_tab();
        assert_eq!(app.doc.tabs.len(), 3);
        assert_eq!(app.doc.active_tab, 2);

        // Ciclar adelante
        app.next_tab();
        assert_eq!(app.doc.active_tab, 0);
        app.next_tab();
        assert_eq!(app.doc.active_tab, 1);

        // Ciclar atrás
        app.prev_tab();
        assert_eq!(app.doc.active_tab, 0);
        app.prev_tab();
        assert_eq!(app.doc.active_tab, 2);

        // Detección de pestañas sucias
        assert_eq!(app.doc.dirty_tabs_count(), 0);
        assert_eq!(app.doc.first_dirty_tab_index(), None);

        // Marcar pestaña 1 como sucia
        app.doc.tabs[1].dirty = true;
        assert_eq!(app.doc.dirty_tabs_count(), 1);
        assert_eq!(app.doc.first_dirty_tab_index(), Some(1));

        // Si la activa (2) también está sucia, first_dirty_tab_index debe priorizar la activa
        app.doc.dirty = true;
        assert_eq!(app.doc.dirty_tabs_count(), 2);
        assert_eq!(app.doc.first_dirty_tab_index(), Some(2));
    }

    #[test]
    fn test_save_as_changes_target_file() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("karkinos_save_as_{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.set_text("codigo inicial");
        app.doc.current_file = Some(dir.join("archivo_original.txt"));
        app.doc.file_path_input = "archivo_original.txt".to_string();

        app.doc.save_as();
        assert!(app.doc.save_name_pending);
        assert_eq!(app.doc.save_name_input, "archivo_original.txt");

        app.doc.save_name_input = "archivo_copia.txt".to_string();
        assert!(app.doc.confirm_save_name());
        assert_eq!(app.doc.current_file, Some(dir.join("archivo_copia.txt")));

        if let Some(progress) = &app.doc.save_progress {
            while !progress.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        let _ = std::fs::remove_file(dir.join("archivo_copia.txt"));
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn test_editor_app_ast_worker_spans_and_folding_ranges() {
        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("main.rs"));
        app.set_text("fn calculate() -> i32 {\n    let a = 10;\n    a * 2\n}");

        // S3 (Roadmap v0.2.9): el parseo corre en FONDO: se espera la
        // respuesta del worker con presupuesto acotado (determinístico:
        // tree-sitter con 3 líneas parsea en <10ms).
        assert!(wait_for_ast(&mut app), "el worker debe terminar y la UI aplicar el resultado");
        assert!(
            app.editor_state.ast_spans.iter().any(|s| s.start_line == 0),
            "deben existir spans semánticos para la línea 0"
        );
        assert!(!app.editor_state.folding_ranges.is_empty());
        let has_func = app.editor_state.folding_ranges.iter().any(|r| r.start_line == 0 && r.end_line == 3);
        assert!(has_func, "should detect function folding range");
    }

    /// S3 (Roadmap v0.2.9): ayuda determinística de test — espera a que
    /// el worker de fondo responda (presupuesto ≤500ms).
    fn wait_for_ast(app: &mut EditorApp) -> bool {
        for _ in 0..100 {
            if app.poll_ast_results() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.poll_ast_results()
    }

    #[test]
    fn test_editor_app_lsp_diagnostics() {
        use karkinos_core::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut app = EditorApp::default();
        let path = PathBuf::from("main.rs");
        app.doc.current_file = Some(path);

        let client = LspClient::mock();
        client.set_diagnostics_for_file(
            "file://main.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 0, character: 3 },
                    end: LspPosition { line: 0, character: 7 },
                },
                severity: Some(DiagnosticSeverity::Error),
                code: Some("E0433".to_string()),
                source: Some("rustc".to_string()),
                message: "failed to resolve: use of undeclared crate".to_string(),
            }],
        );

        app.lsp.client = Some(client);
        app.update_lsp();

        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].severity, Some(DiagnosticSeverity::Error));
        assert!(app.doc.status.contains("failed to resolve"));
    }

    #[test]
    fn test_editor_app_plugin_execution() {
        let mut app = EditorApp::default();
        app.set_text("rust programming language");

        let handled = app.execute_plugin_command("uppercase_document");
        assert!(handled);
        assert_eq!(app.doc.current_text(), "RUST PROGRAMMING LANGUAGE");
        assert!(app.doc.status.contains("UPPERCASE"));

        let word_count_handled = app.execute_plugin_command("word_count");
        assert!(word_count_handled);
        assert!(app.doc.status.contains("3 words"));
    }

    #[test]
    fn test_tab_switching_preserves_lsp_singleton_and_ast() {
        use karkinos_core::lsp::{DiagnosticSeverity, LspClient, LspDiagnostic, LspPosition, LspRange};

        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("tab1.rs"));
        app.set_text("fn tab_one() {}");

        let client = LspClient::mock();
        client.set_diagnostics_for_file(
            "file://tab1.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 2 },
                },
                severity: Some(DiagnosticSeverity::Warning),
                code: None,
                source: Some("rustc".to_string()),
                message: "tab1 warning".to_string(),
            }],
        );
        client.set_diagnostics_for_file(
            "file://tab2.rs",
            vec![LspDiagnostic {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 2 },
                },
                severity: Some(DiagnosticSeverity::Error),
                code: None,
                source: Some("rustc".to_string()),
                message: "tab2 error".to_string(),
            }],
        );
        app.lsp.client = Some(client);
        app.update_lsp();
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab1 warning");

        // Create tab 2 with Rust code
        app.new_tab();
        app.doc.current_file = Some(PathBuf::from("tab2.rs"));
        app.set_text("fn tab_two() {}");
        app.update_lsp();
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab2 error");

        // Switching back to tab 1 must NOT drop LSP client and must load tab1 diagnostics
        app.switch_tab(0);
        assert!(app.lsp.client.is_some(), "LSP client singleton must remain alive");
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab1 warning");
        // S3 (Roadmap v0.2.9): al cambiar de pestaña se re-pide el AST
        // del contenido del tab activo (worker de fondo).
        assert!(wait_for_ast(&mut app), "AST del tab activo debe aplicarse tras el switch");

        // Switching to tab 2 must also preserve LSP client and load tab2 diagnostics
        app.switch_tab(1);
        assert!(app.lsp.client.is_some(), "LSP client singleton must remain alive");
        assert_eq!(app.editor_state.diagnostics.len(), 1);
        assert_eq!(app.editor_state.diagnostics[0].message, "tab2 error");
        assert!(wait_for_ast(&mut app), "AST del tab activo debe aplicarse tras el switch");
    }

    #[test]
    fn test_editor_app_cargo_runner_and_navigation() {
        use karkinos_core::cargo::CargoDiagnostic;

        let mut app = EditorApp::default();
        assert!(!app.cargo.panel_open);

        let diag = CargoDiagnostic {
            level: "error".to_string(),
            message: "mismatched types".to_string(),
            file: Some(PathBuf::from("test_target.rs")),
            line: 7,
            col: 4,
            rendered: "error[E0308]: mismatched types".to_string(),
        };

        app.jump_to_cargo_diagnostic(&diag);
        assert_eq!(app.editor_state.cursor_line, 7);
        assert_eq!(app.editor_state.cursor_col, 4);
        assert!(app.editor_state.needs_autoscroll);
        assert!(app.doc.status.contains("Jumped to error: line 8"));
    }

    #[test]
    fn test_editor_app_goto_definition_and_hover() {
        use karkinos_core::lsp::{LspClient, LspHover, LspLocation, LspPosition, LspRange};

        let mut app = EditorApp::default();
        let client = LspClient::mock();
        let target_file = std::env::temp_dir().join("karkinos_def_test.rs");
        std::fs::write(&target_file, "fn target() {}\n").unwrap();

        // Simulate incoming definition result
        client.set_mock_definition(LspLocation {
            uri: format!("file://{}", target_file.display()),
            range: LspRange {
                start: LspPosition { line: 0, character: 3 },
                end: LspPosition { line: 0, character: 9 },
            },
        });
        client.set_mock_hover(LspHover {
            contents: "fn target() -> ()".to_string(),
            range: None,
        });
        app.lsp.client = Some(client);

        // Poll LSP results
        app.poll_lsp_results();

        assert_eq!(app.editor_state.cursor_line, 0);
        assert_eq!(app.editor_state.cursor_col, 3);
        assert!(app.editor_state.needs_autoscroll);
        assert!(app.doc.status.contains("Docs: fn target() -> ()"));
        assert_eq!(app.editor_state.hover_text.as_deref(), Some("fn target() -> ()"));

        let _ = std::fs::remove_file(target_file);
    }

    #[test]
    fn test_current_language_autodetection_without_extension() {
        let mut app = EditorApp::default();
        app.doc.current_file = None;
        app.doc.file_path_input = String::new();

        // Initially empty buffer -> PlainText
        assert_eq!(app.current_language(), crate::syntax::Language::PlainText);

        // Buffer with Rust function -> Autodetects Rust
        app.set_text("pub fn main() {\n    println!(\"hello\");\n}");
        assert_eq!(app.current_language(), crate::syntax::Language::Rust);

        // Buffer with JSON -> Autodetects Json
        app.set_text("{\n  \"editor\": \"karkinos\"\n}");
        assert_eq!(app.current_language(), crate::syntax::Language::Json);

        // Buffer with HTML -> Autodetects Html
        app.set_text("<!DOCTYPE html>\n<html><body></body></html>");
        assert_eq!(app.current_language(), crate::syntax::Language::Html);
    }

    #[test]
    fn test_editor_app_code_actions_and_inlay_hints() {
        use karkinos_core::lsp::{
            InlayHintKind, LspClient, LspCodeAction, LspInlayHint, LspPosition, LspRange,
            LspTextEdit, LspWorkspaceEdit,
        };
        use std::collections::HashMap;

        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("main.rs"));
        app.set_text("fn main() {\n    let m = HashMap::new();\n}\n");

        let client = LspClient::mock();
        let mut changes = HashMap::new();
        changes.insert(
            "file://main.rs".to_string(),
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: 0, character: 0 },
                    end: LspPosition { line: 0, character: 0 },
                },
                new_text: "use std::collections::HashMap;\n".to_string(),
            }],
        );

        client.set_mock_code_actions(vec![LspCodeAction {
            title: "Import `HashMap`".to_string(),
            kind: Some("quickfix".to_string()),
            edit: Some(LspWorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
            }),
            is_preferred: Some(true),
        }]);

        client.set_mock_inlay_hints(vec![LspInlayHint {
            position: LspPosition { line: 1, character: 9 },
            label: ": HashMap<_, _>".to_string(),
            kind: Some(InlayHintKind::Type),
            padding_left: Some(true),
            padding_right: None,
        }]);

        app.lsp.client = Some(client);
        app.poll_lsp_results();

        assert!(app.lsp.code_actions_menu_open);
        assert_eq!(app.lsp.code_actions.len(), 1);
        assert_eq!(app.lsp.code_actions[0].title, "Import `HashMap`");
        assert_eq!(app.editor_state.inlay_hints.len(), 1);
        assert_eq!(app.editor_state.inlay_hints[0].label, ": HashMap<_, _>");

        // Apply code action
        app.apply_code_action(0);
        assert!(!app.lsp.code_actions_menu_open);
        let text = app.doc.current_text();
        assert!(text.starts_with("use std::collections::HashMap;\nfn main()"));
        assert!(app.doc.dirty);
        assert!(app.doc.status.contains("Applied: Import `HashMap`"));
    }

    #[test]
    fn test_editor_app_workspace_rename() {
        use karkinos_core::lsp::{
            LspClient, LspPosition, LspRange, LspTextEdit, LspWorkspaceEdit,
        };
        use std::collections::HashMap;

        let mut app = EditorApp::default();
        app.doc.current_file = Some(PathBuf::from("lib.rs"));
        app.set_text("pub fn calculate() -> i32 { 42 }\n");

        let client = LspClient::mock();
        let mut changes = HashMap::new();
        changes.insert(
            "file://lib.rs".to_string(),
            vec![LspTextEdit {
                range: LspRange {
                    start: LspPosition { line: 0, character: 7 },
                    end: LspPosition { line: 0, character: 16 },
                },
                new_text: "compute_result".to_string(),
            }],
        );

        client.set_mock_rename(LspWorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
        });

        app.lsp.client = Some(client);
        app.poll_lsp_results();

        let text = app.doc.current_text();
        assert_eq!(text, "pub fn compute_result() -> i32 { 42 }\n");
        assert!(app.doc.dirty);
        assert!(app.doc.status.contains("Applied edits across 1 file(s)"));
    }

    #[test]
    fn edit_lsp_y_edit_de_teclado_mantienen_el_mismo_texto() {
        use karkinos_core::lsp::{LspPosition, LspRange, LspTextEdit};

        let mut app = EditorApp::default();
        app.set_text("fn main() {\n    let x = 1;\n}\n");

        // Un edit del LSP reemplaza el "1" por "42" sobre el Rope.
        app.apply_text_edits_to_active(&[LspTextEdit {
            range: LspRange {
                start: LspPosition { line: 1, character: 12 },
                end: LspPosition { line: 1, character: 13 },
            },
            new_text: "42".to_string(),
        }]);

        // Después "tipeamos" con el mismo helper del widget: escribe
        // sobre el mismo buffer, sin reconstruir ninguna copia aparte.
        let end_col = app.doc.line(1).chars().count();
        crate::editor_widget::insert_text(app.doc.buffer_mut(), (1, end_col), "!");

        assert_eq!(app.doc.text(), "fn main() {\n    let x = 42;!\n}\n");
        assert_eq!(app.doc.line_count(), 4);
        assert_eq!(app.doc.line(1), "    let x = 42;!");
    }

    #[test]
    fn test_editor_app_breakpoints_and_dap_debugging() {
        use karkinos_core::dap::{DapClient, DapStackFrame, DapStopReason, DapVariable};

        let mut app = EditorApp::default();
        let file_path = PathBuf::from("src/main.rs");
        app.doc.current_file = Some(file_path.clone());
        app.set_text("fn main() {\n    let val = 10;\n    println!(\"{}\", val);\n}\n");

        // Toggle breakpoint at line 1
        app.toggle_breakpoint(1);
        assert!(app.dap.breakpoints.contains(&(file_path.clone(), 1)));
        assert!(app.editor_state.breakpoints.contains(&1));
        assert!(app.doc.status.contains("Breakpoint set at line 2"));

        // Toggle breakpoint again -> removes it
        app.toggle_breakpoint(1);
        assert!(!app.dap.breakpoints.contains(&(file_path.clone(), 1)));
        assert!(!app.editor_state.breakpoints.contains(&1));
        assert!(app.doc.status.contains("Breakpoint removed at line 2"));

        // Set breakpoint again
        app.toggle_breakpoint(1);

        // Start debugging with mock DAP client
        let dap = DapClient::mock();
        dap.set_mock_stopped(
            DapStopReason::Breakpoint,
            DapStackFrame {
                id: 1,
                name: "main()".to_string(),
                source_path: Some(file_path.clone()),
                line: 1,
                column: 4,
            },
            vec![DapVariable {
                name: "val".to_string(),
                value: "10".to_string(),
                type_name: Some("i32".to_string()),
            }],
        );

        app.dap.client = Some(dap);
        app.poll_dap_results();

        assert_eq!(app.dap.stopped_line, Some((file_path.clone(), 1)));
        assert_eq!(app.editor_state.stopped_line, Some(1));
        assert_eq!(app.editor_state.cursor_line, 1);
        assert!(app.doc.status.contains("Stopped at line 2"));

        // Test step over
        app.step_over();
        assert!(app.doc.status.contains("Debugger: Step over"));

        // Test stop debugging
        app.stop_debugging();
        assert!(app.dap.client.is_none());
        assert!(app.dap.stopped_line.is_none());
        assert!(app.editor_state.stopped_line.is_none());
        assert!(app.doc.status.contains("Debugger: Stopped"));
    }

    // F5 (Roadmap v0.2.31): split panes.

    #[test]
    fn toggle_split_abre_cambia_eje_y_cierra() {
        let mut app = EditorApp::default();
        assert!(app.split.is_none(), "por defecto hay un solo panel");

        app.toggle_split(SplitDirection::Vertical);
        let split = app.split.as_ref().expect("split vertical");
        assert_eq!(split.direction, SplitDirection::Vertical);
        assert_eq!(split.ratio, 0.5);

        // Mismo eje: cierra.
        app.toggle_split(SplitDirection::Vertical);
        assert!(app.split.is_none());

        // Otro eje: cambia la dirección sin salir del split.
        app.toggle_split(SplitDirection::Vertical);
        app.toggle_split(SplitDirection::Horizontal);
        assert_eq!(
            app.split.as_ref().expect("split horizontal").direction,
            SplitDirection::Horizontal
        );
    }

    #[test]
    fn set_split_ratio_clampa_en_rango_util() {
        let mut app = EditorApp::default();
        app.toggle_split(SplitDirection::Vertical);
        app.set_split_ratio(0.01);
        assert_eq!(app.split.as_ref().unwrap().ratio, 0.15);
        app.set_split_ratio(0.99);
        assert_eq!(app.split.as_ref().unwrap().ratio, 0.85);
        app.set_split_ratio(0.4);
        assert_eq!(app.split.as_ref().unwrap().ratio, 0.4);
    }

    // F6 (Roadmap v0.2.32): búsqueda global.

    #[test]
    fn trigger_workspace_search_con_regex_invalida_reporta_error() {
        let mut app = EditorApp::default();
        app.workspace_search.input = "([a-z".to_string();
        app.workspace_search.use_regex = true;
        app.trigger_workspace_search();

        assert!(app.workspace_search.error.is_some(), "la regex inválida se informa");
        assert!(!app.workspace_search.searching, "no se lanza un recorrido inútil");
        assert!(app.workspace_search.results.is_empty());
    }

    #[test]
    fn trigger_workspace_search_vacia_no_busca() {
        let mut app = EditorApp::default();
        app.workspace_search.input = "   ".to_string();
        app.trigger_workspace_search();
        assert!(!app.workspace_search.searching);
        assert!(app.workspace_search.error.is_none());
    }

    // v0.2.43: reemplazo global en el workspace.

    fn replace_test_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("karkinos_replace_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn execute_workspace_replace_escribe_disco_y_sincroniza_pestanas() {
        let dir = replace_test_dir("sync");
        let a = dir.join("a.rs");
        let b = dir.join("b.txt");
        std::fs::write(&a, "let foo = foo;\n").expect("a");
        std::fs::write(&b, "foo bar\n").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.open_path(a.clone());
        app.workspace_search.input = "foo".to_string();
        app.workspace_search.replace_input = "baz".to_string();

        let summary = app.execute_workspace_replace().expect("replace");

        assert_eq!(summary.files_modified, 2);
        assert_eq!(summary.replacements_count, 3);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "let baz = baz;\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "baz bar\n");

        // El buffer activo y la pestaña abierta reflejan el disco, sin dirty.
        assert_eq!(app.doc.current_text(), "let baz = baz;\n");
        assert_eq!(app.doc.text(), "let baz = baz;\n");
        assert!(!app.doc.dirty);
        let tab = &app.doc.tabs[app.doc.active_tab];
        assert_eq!(tab.text(), "let baz = baz;\n");
        assert!(!tab.dirty);
        assert_eq!(
            tab.last_disk_mtime,
            std::fs::metadata(&a).ok().and_then(|m| m.modified().ok())
        );

        let feedback = app
            .workspace_search
            .replace_summary
            .as_deref()
            .unwrap_or_default();
        assert!(
            feedback.contains("3 occurrences across 2 files"),
            "summary: {feedback}"
        );
        assert!(app.doc.status.contains("3 occurrences across 2 files"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn execute_workspace_replace_respeta_el_filtro_de_archivos() {
        let dir = replace_test_dir("filter");
        let rust = dir.join("only.rs");
        let text = dir.join("skip.txt");
        std::fs::write(&rust, "foo\n").expect("rust");
        std::fs::write(&text, "foo\n").expect("text");

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.workspace_search.input = "foo".to_string();
        app.workspace_search.replace_input = "bar".to_string();
        app.workspace_search.file_filter = " *.rs ".to_string();

        let summary = app.execute_workspace_replace().expect("replace");

        assert_eq!(summary.files_modified, 1);
        assert_eq!(summary.replacements_count, 1);
        assert_eq!(std::fs::read_to_string(&rust).unwrap(), "bar\n");
        assert_eq!(std::fs::read_to_string(&text).unwrap(), "foo\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn execute_workspace_replace_preserva_ediciones_sin_guardar_en_pestanas_intactas() {
        let dir = replace_test_dir("dirty_tabs");
        let a = dir.join("a.rs");
        let b = dir.join("b.rs");
        std::fs::write(&a, "alpha\n").expect("a");
        std::fs::write(&b, "foo\n").expect("b");

        let mut app = EditorApp::default();
        app.doc.browse_dir = dir.clone();
        app.open_path(a.clone());
        app.set_text("alpha edited\n");
        app.doc.dirty = true;
        app.open_path(b.clone());
        app.workspace_search.input = "foo".to_string();
        app.workspace_search.replace_input = "bar".to_string();

        app.execute_workspace_replace().expect("replace");

        // a.rs no tenía coincidencias: su pestaña conserva edits y dirty.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha\n");
        let tab_a = app
            .doc
            .tabs
            .iter()
            .find(|tab| tab.current_file.as_deref() == Some(a.as_path()))
            .expect("tab a");
        assert_eq!(tab_a.text(), "alpha edited\n");
        assert!(tab_a.dirty, "una pestaña intacta no pierde ediciones");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn palette_comandos_de_reemplazo_abren_y_alternan_el_campo() {
        let mut app = EditorApp::default();

        let replace = PALETTE_COMMANDS
            .iter()
            .find(|command| command.id == "search:replace_in_workspace")
            .expect("comando replace_in_workspace");
        assert_eq!(replace.title, "Search: Replace in Files (Ctrl+Shift+H)");
        assert_eq!(replace.category, "Search");
        assert_eq!(replace.shortcut, Some("Ctrl+Shift+H"));
        assert!(
            PALETTE_COMMANDS.iter().any(|command| {
                command.id == "search:toggle_replace" && command.category == "Search"
            })
        );

        app.execute_palette_command("search:replace_in_workspace");
        assert_eq!(app.sidebar_tab, SidebarTab::Search);
        assert!(app.workspace_search.open);
        assert!(app.workspace_search.show_replace);
        assert!(app.workspace_search.replace_focus_requested);

        app.execute_palette_command("search:toggle_replace");
        assert!(!app.workspace_search.show_replace);
        app.execute_palette_command("search:toggle_replace");
        assert!(app.workspace_search.show_replace);
    }

    #[test]
    fn jump_to_search_match_posiciona_cursor_y_seleccion() {        let path = std::env::temp_dir().join(format!(
            "karkinos_jump_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn uno() {}\nfn dos() {}\nfn tres() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.open_path(path.clone());
        app.jump_to_search_match(
            &path,
            &karkinos_core::SearchMatch {
                line_number: 2,
                col_start: 3,
                col_end: 6,
                line_content: "fn dos() {}".to_string(),
            },
        );

        assert_eq!(app.editor_state.cursor_line, 1, "línea 2 es índice 1");
        assert_eq!(app.editor_state.cursor_col, 3);
        assert_eq!(app.editor_state.sel_anchor, Some((1, 6)));

        std::fs::remove_file(path).ok();
    }

    // Barra lateral (v0.2.33): explorador.

    #[test]
    fn explorer_toggle_y_refresh_cargan_archivos_del_proyecto() {
        let root = std::env::temp_dir().join(format!(
            "karkinos_explorer_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(root.join("src")).expect("crear src");
        std::fs::create_dir_all(root.join("target")).expect("crear target");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("archivo");
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        assert!(!app.explorer.open);

        app.toggle_explorer();
        assert!(app.explorer.open, "el explorador queda abierto");
        let names: Vec<String> = app
            .explorer
            .files
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(names.contains(&"src/main.rs".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("target/")), "{names:?}");

        app.toggle_explorer();
        assert!(!app.explorer.open, "el segundo toggle lo cierra");

        std::fs::remove_dir_all(&root).ok();
    }

    // v0.2.41: operaciones de archivo desde el explorador.

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
    fn picker_crear_archivo_lo_abre_y_cierra_el_popup() {
        let root = explorer_temp_root("picker_create_file");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.picker_open = true;
        app.picker_new_kind = Some(PickerNewItemKind::File);
        app.picker_new_name = "desde_picker.rs".to_string();

        let path = app.picker_create_item().expect("crear archivo");

        assert_eq!(path, root.join("desde_picker.rs"));
        assert!(path.is_file(), "el archivo existe en disco");
        assert_eq!(
            app.doc.current_file,
            Some(path.clone()),
            "el archivo se abre de inmediato"
        );
        assert!(!app.picker_open, "el popup se cierra al abrir el archivo");
        assert_eq!(app.picker_new_kind, None);
        assert!(app.picker_new_name.is_empty());
        assert_eq!(app.doc.status, "Created desde_picker.rs");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn picker_crear_carpeta_refresca_listado_y_mantiene_popup() {
        let root = explorer_temp_root("picker_create_folder");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.picker_open = true;
        // Calienta el caché antes de crear: el listado tiene que refrescarse.
        let (dirs, _) = app.doc.cached_list_dir();
        assert!(dirs.is_empty(), "{dirs:?}");

        app.picker_new_kind = Some(PickerNewItemKind::Folder);
        app.picker_new_name = "nueva_carpeta".to_string();

        let path = app.picker_create_item().expect("crear carpeta");

        assert_eq!(path, root.join("nueva_carpeta"));
        assert!(path.is_dir());
        assert!(app.picker_open, "el selector sigue abierto para navegar");
        assert_eq!(app.picker_new_kind, None);
        assert!(app.picker_new_name.is_empty());
        assert_eq!(app.doc.current_file, None, "una carpeta no se abre");
        let (dirs, _) = app.doc.cached_list_dir();
        assert!(dirs.contains(&"nueva_carpeta".to_string()), "{dirs:?}");
        assert_eq!(app.doc.status, "Created nueva_carpeta");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn picker_crear_item_rechaza_nombres_invalidos_y_existentes() {
        let root = explorer_temp_root("picker_create_invalid");
        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.picker_open = true;
        app.picker_new_kind = Some(PickerNewItemKind::File);

        app.picker_new_name = "   ".to_string();
        assert!(app.picker_create_item().is_err(), "nombre vacío");
        assert!(
            app.picker_new_kind.is_some(),
            "el input sigue abierto para corregir"
        );

        app.picker_new_name = "../escapa".to_string();
        assert!(app.picker_create_item().is_err(), "no sale de la raíz");
        assert!(!root.parent().unwrap().join("escapa").exists());

        std::fs::write(root.join("existente.rs"), "ya existe").expect("archivo");
        app.picker_new_name = "existente.rs".to_string();
        assert!(app.picker_create_item().is_err(), "no pisa archivos");

        // Sin ítem pendiente, el método no toca el disco.
        app.picker_new_kind = None;
        app.picker_new_name = "otro.rs".to_string();
        assert!(app.picker_create_item().is_err());
        assert!(!root.join("otro.rs").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    fn symbol(name: &str, kind: karkinos_core::syntax_ast::SymbolKind, line: usize) -> karkinos_core::syntax_ast::DocumentSymbol {
        karkinos_core::syntax_ast::DocumentSymbol {
            name: name.to_string(),
            kind,
            line,
            character: 3,
        }
    }

    #[test]
    fn symbol_picker_filtra_por_nombre_sin_mayusculas() {
        let mut app = EditorApp::default();
        app.editor_state.symbols = vec![
            symbol("main", karkinos_core::syntax_ast::SymbolKind::Function, 0),
            symbol("process", karkinos_core::syntax_ast::SymbolKind::Method, 5),
        ];
        app.open_symbol_picker();
        assert!(app.symbol_picker.open);
        assert_eq!(app.filtered_symbols().len(), 2, "sin filtro muestra todo");

        app.symbol_picker.query = "PROC".to_string();
        let filtered = app.filtered_symbols();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "process");
    }

    #[test]
    fn jump_to_symbol_posiciona_cursor_y_cierra_selector() {
        let path = std::env::temp_dir().join(format!(
            "karkinos_symbol_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn uno() {}\nfn dos() {}\nfn tres() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.open_path(path.clone());
        app.open_symbol_picker();
        app.jump_to_symbol(&symbol("dos", karkinos_core::syntax_ast::SymbolKind::Function, 1));

        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 3);
        assert!(!app.symbol_picker.open, "el salto cierra el selector");

        std::fs::remove_file(path).ok();
    }

    // F8 (Roadmap v0.2.35): blame inline.

    #[test]
    fn inline_blame_se_limpia_cuando_el_cursor_se_mueve() {
        let path = std::env::temp_dir().join(format!(
            "karkinos_blame_move_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn main() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        app.editor_state.blame_text = Some("Ada, 2 days ago".to_string());
        app.git_status.last_cursor = (0, 0);
        app.editor_state.cursor_line = 3;

        app.update_inline_blame();
        assert!(app.editor_state.blame_text.is_none(), "al moverse se limpia");
        assert!(app.git_status.cursor_settled_at.is_some());

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn inline_blame_aparece_tras_el_dwell_con_cache() {
        let path = std::env::temp_dir().join(format!(
            "karkinos_blame_{}.rs",
            std::process::id()
        ));
        std::fs::write(&path, "fn main() {}\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.current_file = Some(path.clone());
        app.editor_state.cursor_line = 0;
        app.editor_state.cursor_col = 0;
        app.git_status.last_cursor = (0, 0);
        app.git_status.cursor_settled_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(400));

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut lines = std::collections::HashMap::new();
        lines.insert(
            1usize,
            karkinos_core::git_status::BlameLine {
                author: "Ada".to_string(),
                timestamp: now - 86_400,
                summary: "feat: x".to_string(),
            },
        );
        app.git_status
            .blame_cache
            .insert(path.clone(), std::sync::Arc::new(lines));

        app.update_inline_blame();
        let text = app
            .editor_state
            .blame_text
            .clone()
            .expect("con la cache y el dwell cumplido hay ghost text");
        assert!(text.starts_with("Ada, "), "{text}");
        assert!(text.contains("feat: x"), "{text}");

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn blame_dwell_no_repinta_tras_superar_300ms() {
        let mut app = EditorApp::default();

        // Sin cursor asentado no hay repaint pendiente (reposo total).
        assert!(app.blame_repaint_remaining().is_none());

        // Recién movido el cursor: falta cumplir el dwell, pide un repaint.
        app.git_status.cursor_settled_at = Some(std::time::Instant::now());
        assert!(app.blame_repaint_remaining().is_some());

        // Cumplido el dwell (más de 300 ms): no pide ningún repaint adicional.
        app.git_status.cursor_settled_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(400));
        assert!(
            app.blame_repaint_remaining().is_none(),
            "tras el dwell el editor debe entrar en reposo"
        );
    }

    // UI-6 (v0.2.38): árbol del explorador.

    #[test]
    fn file_tree_groups_directories_first_and_sorted() {
        let root = std::path::Path::new("/proj");
        let paths = vec![
            std::path::PathBuf::from("/proj/src/main.rs"),
            std::path::PathBuf::from("/proj/src/app.rs"),
            std::path::PathBuf::from("/proj/Cargo.toml"),
            std::path::PathBuf::from("/proj/crates/core/lib.rs"),
        ];
        let tree = FileTreeNode::build_from_paths(root, &paths);
        assert_eq!(tree.len(), 3);
        assert!(matches!(&tree[0], FileTreeNode::Directory { name, .. } if name == "crates"));
        assert!(matches!(&tree[1], FileTreeNode::Directory { name, .. } if name == "src"));
        assert!(matches!(&tree[2], FileTreeNode::File { name, .. } if name == "Cargo.toml"));

        match &tree[1] {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 2);
                assert!(
                    matches!(&children[0], FileTreeNode::File { name, .. } if name == "app.rs")
                );
                assert!(
                    matches!(&children[1], FileTreeNode::File { name, .. } if name == "main.rs")
                );
            }
            _ => panic!("src tiene que ser una carpeta"),
        }
        if let FileTreeNode::File { extension, .. } = &tree[2] {
            assert_eq!(extension, "toml");
        }
    }

    #[test]
    fn file_tree_collects_all_directories_for_collapse_all() {
        let root = std::path::Path::new("/p");
        let paths = vec![
            std::path::PathBuf::from("/p/a/b/c.rs"),
            std::path::PathBuf::from("/p/d/e.rs"),
        ];
        let tree = FileTreeNode::build_from_paths(root, &paths);
        let mut dirs = Vec::new();
        FileTreeNode::collect_directories(&tree, &mut dirs);
        assert!(dirs.contains(&std::path::PathBuf::from("/p/a")));
        assert!(dirs.contains(&std::path::PathBuf::from("/p/a/b")));
        assert!(dirs.contains(&std::path::PathBuf::from("/p/d")));
        assert_eq!(dirs.len(), 3);
    }

    #[test]
    fn file_tree_includes_empty_directories() {
        let root = std::path::Path::new("/proj");
        let files = vec![std::path::PathBuf::from("/proj/src/main.rs")];
        let dirs = vec![
            std::path::PathBuf::from("/proj/src"),
            std::path::PathBuf::from("/proj/assets/images"),
            std::path::PathBuf::from("/proj/empty"),
        ];

        let tree = FileTreeNode::build_from_paths_and_dirs(root, &files, &dirs);
        let names: Vec<&str> = tree.iter().map(|n| n.name()).collect();
        assert_eq!(names, vec!["assets", "empty", "src"], "{names:?}");

        let empty = tree
            .iter()
            .find(|n| n.name() == "empty")
            .expect("la carpeta vacía aparece en el árbol");
        match empty {
            FileTreeNode::Directory { children, path, .. } => {
                assert!(children.is_empty(), "sin hijos");
                assert_eq!(path, &std::path::PathBuf::from("/proj/empty"));
            }
            _ => panic!("empty tiene que ser una carpeta"),
        }

        let assets = tree
            .iter()
            .find(|n| n.name() == "assets")
            .expect("assets aparece");
        match assets {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].name(), "images");
                match &children[0] {
                    FileTreeNode::Directory { children, .. } => {
                        assert!(children.is_empty(), "images queda vacía")
                    }
                    _ => panic!("images tiene que ser una carpeta"),
                }
            }
            _ => panic!("assets tiene que ser una carpeta"),
        }

        let src = tree.iter().find(|n| n.name() == "src").expect("src aparece");
        match src {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 1);
                assert!(
                    matches!(&children[0], FileTreeNode::File { name, .. } if name == "main.rs")
                );
            }
            _ => panic!("src tiene que ser una carpeta"),
        }
    }

    #[test]
    fn refresh_explorer_incluye_carpetas_vacias_en_el_arbol() {
        let root = explorer_temp_root("empty_folders");
        std::fs::create_dir_all(root.join("vacia/nested")).expect("crear vacía");
        std::fs::create_dir_all(root.join("src")).expect("crear src");
        std::fs::create_dir_all(root.join("target")).expect("crear target");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("archivo");
        std::fs::write(root.join("target/junk.rs"), "fn junk() {}").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();

        let rel_dirs: Vec<String> = app
            .explorer
            .directories
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(rel_dirs.contains(&"vacia".to_string()), "{rel_dirs:?}");
        assert!(
            rel_dirs.contains(&"vacia/nested".to_string()),
            "{rel_dirs:?}"
        );
        assert!(
            !rel_dirs.iter().any(|d| d.starts_with("target")),
            "{rel_dirs:?}"
        );

        let tree = FileTreeNode::build_from_paths_and_dirs(
            &root,
            &app.explorer.files,
            &app.explorer.directories,
        );
        let vacia = tree
            .iter()
            .find(|n| n.name() == "vacia")
            .expect("la carpeta vacía aparece en el árbol");
        match vacia {
            FileTreeNode::Directory { children, .. } => {
                assert_eq!(children.len(), 1, "nested conserva la jerarquía");
                assert_eq!(children[0].name(), "nested");
                match &children[0] {
                    FileTreeNode::Directory { children, .. } => {
                        assert!(children.is_empty(), "la carpeta hoja no tiene hijos")
                    }
                    _ => panic!("nested tiene que ser una carpeta"),
                }
            }
            _ => panic!("vacia tiene que ser una carpeta"),
        }

        std::fs::remove_dir_all(&root).ok();
    }

    // UI-7 (v0.2.38): minimap toggleable.

    #[test]
    fn toggle_minimap_flips_primary_and_split() {
        let mut app = EditorApp::default();
        assert!(!app.editor_state.show_minimap, "arranca apagado");

        app.toggle_split(SplitDirection::Vertical);
        assert!(!app.split.as_ref().unwrap().secondary.show_minimap);

        app.toggle_minimap();
        assert!(app.editor_state.show_minimap);
        assert!(
            app.split.as_ref().unwrap().secondary.show_minimap,
            "el panel secundario acompaña"
        );

        app.toggle_minimap();
        assert!(!app.editor_state.show_minimap);
        assert!(!app.split.as_ref().unwrap().secondary.show_minimap);
    }

    #[test]
    fn toggle_smear_cursor_flips_primary_tabs_and_split() {
        let mut app = EditorApp::default();
        assert!(app.editor_state.smear_enabled, "arranca encendido por defecto");

        app.toggle_split(SplitDirection::Vertical);
        assert!(app.split.as_ref().unwrap().secondary.smear_enabled);

        app.toggle_smear_cursor();
        assert!(!app.editor_state.smear_enabled);
        assert!(!app.split.as_ref().unwrap().secondary.smear_enabled);
        assert!(app.doc.status.contains("Disabled"));

        app.toggle_smear_cursor();
        assert!(app.editor_state.smear_enabled);
        assert!(app.split.as_ref().unwrap().secondary.smear_enabled);
        assert!(app.doc.status.contains("Enabled"));
    }

    // Barra lateral unificada (v0.2.39): pestañas compartidas y alternancia limpia.

    #[test]
    fn sidebar_tab_switching_unifies_panel() {
        let mut app = EditorApp::default();
        assert_eq!(app.sidebar_tab, SidebarTab::None);
        assert!(!app.explorer.open);
        assert!(!app.workspace_search.open);

        // Abrir Explorer
        app.toggle_explorer();
        assert_eq!(app.sidebar_tab, SidebarTab::Explorer);
        assert!(app.explorer.open);
        assert!(!app.workspace_search.open);

        // Cambiar a Search (se abre en la misma ventana, cierra Explorer)
        app.open_workspace_search();
        assert_eq!(app.sidebar_tab, SidebarTab::Search);
        assert!(app.workspace_search.open);
        assert!(!app.explorer.open);

        // Cambiar a Symbols
        app.open_symbol_picker();
        assert_eq!(app.sidebar_tab, SidebarTab::Symbols);
        assert!(!app.workspace_search.open);

        // Volver a clickear Symbols lo cierra
        app.open_symbol_picker();
        assert_eq!(app.sidebar_tab, SidebarTab::None);

        // v0.2.42: Source Control entra en la misma alternancia.
        app.open_source_control();
        assert_eq!(app.sidebar_tab, SidebarTab::SourceControl);
        assert!(!app.explorer.open);
        assert!(!app.workspace_search.open);
        assert!(!app.symbol_picker.open);

        app.open_source_control();
        assert_eq!(app.sidebar_tab, SidebarTab::None);

        // v0.2.56: Extensions entra en la misma alternancia unificada.
        app.open_extensions();
        assert_eq!(app.sidebar_tab, SidebarTab::Extensions);
        assert!(!app.explorer.open);
        assert!(!app.workspace_search.open);
        assert!(!app.symbol_picker.open);

        app.open_extensions();
        assert_eq!(app.sidebar_tab, SidebarTab::None);
    }

    // v0.2.40: Command Palette (Ctrl+Shift+P / F1).

    #[test]
    fn palette_catalogo_tiene_ids_unicos_y_los_comandos_esperados() {
        let mut ids: Vec<&str> = PALETTE_COMMANDS.iter().map(|command| command.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "cada id del catálogo es único");
        assert_eq!(total, 39, "el catálogo de v0.2.59 tiene 39 comandos");

        for expected in [
            "layout:split_vertical",
            "layout:split_horizontal",
            "layout:close_split",
            "editor:toggle_minimap",
            "editor:toggle_smear_cursor",
            "sidebar:explorer",
            "sidebar:search",
            "search:replace_in_workspace",
            "search:toggle_replace",
            "sidebar:symbols",
            "sidebar:extensions",
            "terminal:toggle",
            "cargo:check",
            "cargo:test",
            "cargo:clippy",
            "cargo:run",
            "debug:toggle_breakpoint",
            "debug:start",
            "debug:step_over",
            "debug:step_into",
            "debug:stop",
            "theme:dark",
            "theme:monokai",
            "theme:dracula",
            "theme:onedark",
            "editor:format",
            "file:save",
            "file:new",
            "file:open_folder",
            "file:rename_active",
            "file:duplicate_active",
            "view:zoom_in",
            "view:zoom_out",
            "view:reset_zoom",
            "git:source_control",
            "git:refresh",
            "git:stage_all",
            "git:view_diff",
            "git:close_diff",
        ] {
            assert!(
                PALETTE_COMMANDS.iter().any(|command| command.id == expected),
                "falta el comando {expected}"
            );
        }
    }

    #[test]
    fn palette_filtra_por_categoria_y_titulo_sin_mayusculas() {
        let mut app = EditorApp::default();
        assert_eq!(app.filtered_palette_commands().len(), PALETTE_COMMANDS.len());

        app.command_palette.query = "THEME".to_string();
        let themes: Vec<&str> = app
            .filtered_palette_commands()
            .iter()
            .map(|command| command.id)
            .collect();
        assert_eq!(
            themes,
            ["theme:dark", "theme:monokai", "theme:dracula", "theme:onedark"]
        );

        app.command_palette.query = "minimap".to_string();
        let minimap: Vec<&str> = app
            .filtered_palette_commands()
            .iter()
            .map(|command| command.id)
            .collect();
        assert_eq!(minimap, ["editor:toggle_minimap"]);

        app.command_palette.query = "  file:  ".to_string();
        let files: Vec<&str> = app
            .filtered_palette_commands()
            .iter()
            .map(|command| command.id)
            .collect();
        assert_eq!(
            files,
            [
                "file:save",
                "file:new",
                "file:open_folder",
                "file:rename_active",
                "file:duplicate_active"
            ],
            "el trim también aplica"
        );

        app.command_palette.query = "sin-coincidencias".to_string();
        assert!(app.filtered_palette_commands().is_empty());
    }

    #[test]
    fn palette_abre_limpia_query_y_no_borra_lo_escrito_si_ya_esta_abierto() {
        let mut app = EditorApp::default();
        assert!(!app.command_palette.open);

        app.open_command_palette();
        assert!(app.command_palette.open);
        assert!(app.command_palette.query.is_empty());
        assert_eq!(app.command_palette.selected, 0);

        app.command_palette.query = "cargo".to_string();
        app.command_palette.selected = 2;
        app.open_command_palette();
        assert_eq!(app.command_palette.query, "cargo", "F1 de nuevo no borra el query");
        assert_eq!(app.command_palette.selected, 2);
    }

    #[test]
    fn palette_seleccion_clampa_y_enter_ejecuta_el_seleccionado() {
        let mut app = EditorApp::default();
        app.open_command_palette();
        app.command_palette.query = "theme".to_string();

        app.move_palette_selection(-1);
        assert_eq!(app.command_palette.selected, 0, "no baja de 0");

        app.move_palette_selection(1);
        app.move_palette_selection(100);
        assert_eq!(app.command_palette.selected, 3, "no pasa del último");

        app.execute_selected_palette_command();
        assert_eq!(
            app.theme_mode,
            crate::theme::ThemeMode::OneDark,
            "Enter ejecuta el comando seleccionado"
        );
        assert!(!app.command_palette.open, "ejecutar cierra el modal");
        assert!(app.command_palette.query.is_empty());
        assert_eq!(app.command_palette.selected, 0);
    }

    #[test]
    fn palette_ejecuta_layout_y_cierra_o_ignora_ids_desconocidos() {
        let mut app = EditorApp::default();
        app.open_command_palette();

        app.execute_palette_command("layout:split_vertical");
        assert_eq!(
            app.split.as_ref().expect("split abierto").direction,
            SplitDirection::Vertical
        );
        assert!(!app.command_palette.open);

        app.open_command_palette();
        app.execute_palette_command("layout:close_split");
        assert!(app.split.is_none());

        app.open_command_palette();
        app.execute_palette_command("no:existe");
        assert!(!app.command_palette.open, "un id desconocido no deja la UI colgada");
        assert!(app.split.is_none());
    }

    #[test]
    fn format_document_rechaza_lenguajes_no_rust_sin_tocar_el_buffer() {
        let mut app = EditorApp::default();
        app.doc.file_path_input = "notas.txt".to_string();
        app.set_text("texto plano\n");

        app.format_document();

        assert_eq!(app.doc.current_text(), "texto plano\n");
        assert!(
            app.doc.status.starts_with("Format: rustfmt"),
            "status: {}",
            app.doc.status
        );
        assert!(!app.doc.dirty);
    }

    #[test]
    fn format_aplica_texto_como_edicion_undoable_y_preserva_cursor() {
        let mut app = EditorApp::default();
        app.set_text("fn  main( ){}\n");
        app.editor_state.cursor_line = 0;
        app.editor_state.cursor_col = 8;

        app.apply_formatted_text("fn main() {}\n");

        assert_eq!(app.doc.current_text(), "fn main() {}\n");
        assert!(app.doc.dirty, "el formateo marca el buffer como sucio");
        assert_eq!(app.editor_state.cursor_line, 0);
        assert_eq!(app.editor_state.cursor_col, 8, "el cursor no salta al inicio");

        app.undo();
        assert_eq!(
            app.doc.current_text(),
            "fn  main( ){}\n",
            "el formateo se puede deshacer"
        );
    }

    // v0.2.42: Source Control (git status, stage y commit).

    /// v0.2.44: watcher de disco, pestaña limpia.
    #[test]
    fn poll_external_changes_reloads_clean_tab_from_disk() {
        let root = std::env::temp_dir().join(format!(
            "karkinos_watch_clean_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("carpeta temporal");
        let path = root.join("watched.txt");
        std::fs::write(&path, "primera\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(path.clone());
        assert_eq!(app.doc.text(), "primera\n");
        assert!(!app.doc.dirty);

        std::fs::write(&path, "segunda linea\n").expect("reescritura");
        app.doc.tabs[app.doc.active_tab].last_disk_mtime = Some(std::time::SystemTime::UNIX_EPOCH);
        app.last_disk_poll = None;
        app.poll_external_changes();

        assert_eq!(app.doc.text(), "segunda linea\n");
        assert_eq!(app.doc.text(), "segunda linea\n");
        assert!(!app.doc.dirty, "una recarga limpia no marca dirty");
        assert!(app.doc.tabs[app.doc.active_tab].last_disk_mtime.is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    /// v0.2.44: watcher de disco, pestaña con ediciones sin guardar.
    #[test]
    fn poll_external_changes_preserves_dirty_tab() {
        let root = std::env::temp_dir().join(format!(
            "karkinos_watch_dirty_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("carpeta temporal");
        let path = root.join("edited.txt");
        std::fs::write(&path, "en disco\n").expect("archivo");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.open_path(path.clone());
        app.set_text("edicion local\n");
        app.doc.dirty = true;

        std::fs::write(&path, "cambio externo\n").expect("reescritura");
        app.doc.tabs[app.doc.active_tab].last_disk_mtime = Some(std::time::SystemTime::UNIX_EPOCH);
        app.last_disk_poll = None;
        app.poll_external_changes();

        assert_eq!(app.doc.text(), "edicion local\n", "no pisa ediciones");
        assert!(app.doc.dirty, "sigue dirty");
        assert!(
            app.doc.status.contains("File modified on disk: edited.txt"),
            "status = {}",
            app.doc.status
        );

        std::fs::remove_dir_all(&root).ok();
    }

    fn cli_temp_dir(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "karkinos_cli_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&root).expect("crear carpeta temporal");
        root
    }

    #[test]
    fn init_from_cli_path_con_directorio_existente() {
        let root = cli_temp_dir("dir");
        let mut app = EditorApp::default();
        app.init_from_cli_path(&root);

        assert_eq!(
            app.doc.browse_dir, root,
            "browse_dir apunta a la carpeta pasada"
        );
        assert_eq!(
            app.explorer.loaded_for.as_deref(),
            Some(root.as_path()),
            "el explorador se refrescó"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn init_from_cli_path_con_archivo_existente() {
        let root = cli_temp_dir("file");
        let file = root.join("hola.txt");
        std::fs::write(&file, "contenido").expect("crear archivo");

        let mut app = EditorApp::default();
        app.init_from_cli_path(&file);

        assert_eq!(app.doc.browse_dir, root, "browse_dir apunta al padre");
        assert_eq!(
            app.explorer.loaded_for.as_deref(),
            Some(root.as_path()),
            "el explorador se refrescó"
        );
        assert_eq!(
            app.doc.current_file.as_deref(),
            Some(file.as_path()),
            "el archivo quedó abierto en la pestaña activa"
        );
        assert_eq!(app.doc.text(), "contenido");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn init_from_cli_path_con_archivo_nuevo() {
        let root = cli_temp_dir("new");
        let file = root.join("nuevo.rs");

        let mut app = EditorApp::default();
        app.init_from_cli_path(&file);

        assert_eq!(
            app.doc.browse_dir, root,
            "browse_dir apunta al padre existente"
        );
        assert_eq!(
            app.explorer.loaded_for.as_deref(),
            Some(root.as_path()),
            "el explorador se refrescó"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    // ---- v0.2.48 (Tarea B): IPC single-instance ----

    #[test]
    fn poll_ipc_events_abre_archivo_de_comando_entrante() {
        let root = cli_temp_dir("ipc_events");
        let file = root.join("abrir.rs");
        std::fs::write(&file, "fn main() {}").expect("crear archivo");

        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = EditorApp {
            ipc_server: Some(karkinos_core::ipc::IpcServer::from_receiver(rx)),
            ..Default::default()
        };

        tx.send(karkinos_core::ipc::IpcCommand::Open(file.clone()))
            .expect("enviar comando");

        let ctx = egui::Context::default();
        app.poll_ipc_events(&ctx);

        assert_eq!(
            app.doc.current_file.as_deref(),
            Some(file.as_path()),
            "el comando abre el archivo en la pestaña activa"
        );
        assert_eq!(app.doc.text(), "fn main() {}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn poll_ipc_events_sin_servidor_no_hace_nada() {
        let mut app = EditorApp::default();

        let ctx = egui::Context::default();
        // No debe paniquear ni mutar el estado del documento.
        let file_path_input = app.doc.file_path_input.clone();
        app.poll_ipc_events(&ctx);
        assert_eq!(app.doc.file_path_input, file_path_input);
        assert!(app.doc.current_file.is_none());
    }

    // ---- Roadmap v0.2.45 (Tarea C): Open Folder ----

    #[test]
    fn open_workspace_folder_cambia_browse_dir_y_notifica_status() {
        let root = cli_temp_dir("open_folder");
        let other = cli_temp_dir("open_folder_target");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();

        // Dejar estado previo que debe quedar limpio tras el cambio.
        app.workspace_search.results = vec![karkinos_core::FileMatches {
            path: std::path::PathBuf::from("stale.rs"),
            relative_display: "stale.rs".to_string(),
            matches: Vec::new(),
        }];
        app.git_diff_modal = Some(GitDiffViewState {
            file_path: std::path::PathBuf::from("stale.rs"),
            rel_name: "stale.rs".to_string(),
            diff_text: "old diff".to_string(),
            parsed_lines: Vec::new(),
            is_staged: false,
        });

        app.open_workspace_folder(&other);

        assert_eq!(app.doc.browse_dir, other, "browse_dir apunta a la nueva carpeta");
        assert_eq!(
            app.explorer.loaded_for.as_deref(),
            Some(other.as_path()),
            "el explorador se refrescó"
        );
        assert!(app.workspace_search.results.is_empty(), "la búsqueda global quedó limpia");
        assert!(app.git_diff_modal.is_none(), "el visor de diff se cerró");
        assert_eq!(
            app.doc.status,
            format!("Opened folder: {}", other.display()),
            "el status notifica el cambio"
        );

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[test]
    fn prompt_open_folder_abre_modal_y_cierra_otros_modales() {
        let mut app = EditorApp {
            picker_open: true,
            ..Default::default()
        };
        app.command_palette.open = true;
        app.command_palette.query = "file".to_string();

        app.prompt_open_folder();

        assert!(app.open_folder_modal.is_some(), "el modal queda abierto");
        assert!(!app.picker_open, "el selector de carpetas se cierra");
        assert!(!app.command_palette.open, "el palette se cierra");
        assert!(app.command_palette.query.is_empty(), "el query del palette se limpia");
    }

    #[test]
    fn confirm_open_folder_con_directorio_existente_cambia_y_cierra() {
        let root = cli_temp_dir("confirm_dir");
        let target = cli_temp_dir("confirm_dir_target");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();
        app.open_folder_modal = Some(target.display().to_string());

        app.confirm_open_folder();

        assert_eq!(app.doc.browse_dir, target, "browse_dir cambió");
        assert!(app.open_folder_modal.is_none(), "el modal se cerró");

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn confirm_open_folder_con_ruta_invalida_mantiene_modal_y_avisa() {
        let root = cli_temp_dir("confirm_invalid");
        let missing = root.join("no_existe");

        let mut app = EditorApp::default();
        app.doc.browse_dir = root.clone();
        app.refresh_explorer();
        app.open_folder_modal = Some(missing.display().to_string());

        app.confirm_open_folder();

        assert_eq!(app.doc.browse_dir, root, "browse_dir no cambió");
        assert!(app.open_folder_modal.is_some(), "el modal sigue abierto para corregir");
        assert!(app.doc.status.contains("Not a folder"), "avisa en el status");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn confirm_open_folder_expande_tilde_al_home() {
        let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) else {
            return; // sin home en este entorno no hay nada que expandir
        };
        let home = std::path::PathBuf::from(home);
        let dir_name = format!(
            "karkinos_tilde_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let target = home.join(&dir_name);
        std::fs::create_dir_all(&target).expect("crear carpeta temporal en el home");

        let mut app = EditorApp {
            open_folder_modal: Some(format!("~/{}", dir_name)),
            ..Default::default()
        };

        app.confirm_open_folder();

        assert_eq!(
            app.doc.browse_dir, target,
            "browse_dir apunta a la carpeta expandida"
        );
        assert!(app.open_folder_modal.is_none(), "el modal se cerró");

        std::fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn execute_palette_command_abre_el_modal_de_open_folder() {
        let mut app = EditorApp::default();
        assert!(app.open_folder_modal.is_none());

        app.execute_palette_command("file:open_folder");

        assert!(app.open_folder_modal.is_some(), "el comando abre el modal");
        assert!(!app.command_palette.open, "el palette se cierra al ejecutar");
    }

    // ---- v0.2.51: journaling & crash recovery ----

    fn recovery_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_recovery_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("crear carpeta de recovery temporal");
        dir
    }

    fn recovery_entry(title: &str, content: &str) -> RecoveryEntry {
        RecoveryEntry {
            original_path: None,
            title: title.to_string(),
            content: content.to_string(),
            cursor_line: 0,
            cursor_col: 0,
            timestamp_secs: 1_700_000_000,
        }
    }

    #[test]
    fn recovery_inicio_detecta_journals_y_abre_el_modal() {
        let dir = recovery_test_dir("detect");
        let filename = journal_filename(Some(std::path::Path::new("/tmp/main.rs")), 7);
        save_recovery_file(&dir, &filename, &recovery_entry("main.rs", "fn main() {}\n"))
            .expect("journal escrito");

        let mut app = EditorApp::default();
        app.init_recovery_from_dir(dir.clone());

        assert_eq!(app.recovery_pending.len(), 1);
        assert!(app.show_recovery_modal);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn poll_recovery_autosave_espera_el_debounce_y_escribe_el_journal() {
        let dir = recovery_test_dir("autosave");
        let mut app = EditorApp::default();
        app.init_recovery_from_dir(dir.clone());
        app.set_text("hola mundo");
        app.doc.dirty = true;

        let t0 = Instant::now();
        app.poll_recovery_autosave_at(t0);
        assert!(
            scan_recovery_dir(&dir).is_empty(),
            "no escribe antes de 1.5 s de inactividad"
        );

        app.poll_recovery_autosave_at(t0 + Duration::from_secs(2));
        let entries = scan_recovery_dir(&dir);
        assert_eq!(entries.len(), 1, "un journal por pestaña dirty");
        assert_eq!(entries[0].1.content, "hola mundo");
        assert_eq!(entries[0].1.original_path, None);
        assert_eq!(entries[0].1.title, "Untitled 1");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn poll_recovery_autosave_no_escribe_si_el_modal_esta_abierto() {
        let dir = recovery_test_dir("modal_guard");
        let mut app = EditorApp::default();
        app.init_recovery_from_dir(dir.clone());
        app.show_recovery_modal = true;
        app.set_text("pendiente");
        app.doc.dirty = true;

        let t0 = Instant::now();
        app.poll_recovery_autosave_at(t0 + Duration::from_secs(10));

        assert!(scan_recovery_dir(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn restore_all_recovery_abre_pestanas_dirty_y_limpia_el_directorio() {
        let dir = recovery_test_dir("restore");
        let target = dir.join("recuperado.rs");
        let mut saved = recovery_entry("recuperado.rs", "fn main() {\n    println!(\"x\");\n}\n");
        saved.original_path = Some(target.clone());
        saved.cursor_line = 1;
        saved.cursor_col = 4;
        save_recovery_file(&dir, &journal_filename(Some(&target), 1), &saved)
            .expect("journal con ruta");

        let mut untitled = recovery_entry("Untitled 9", "abc\ndef");
        untitled.cursor_line = 1;
        untitled.cursor_col = 2;
        save_recovery_file(&dir, &journal_filename(None, 9), &untitled)
            .expect("journal untitled");

        let mut app = EditorApp::default();
        app.init_recovery_from_dir(dir.clone());
        let tabs_before = app.doc.tabs.len();

        app.restore_all_recovery();

        assert!(!app.show_recovery_modal);
        assert!(app.recovery_pending.is_empty());
        assert_eq!(app.doc.tabs.len(), tabs_before + 2);
        assert!(app.doc.dirty, "el buffer recuperado queda dirty");
        assert_eq!(app.doc.current_text(), "abc\ndef");
        assert_eq!(app.editor_state.cursor_line, 1);
        assert_eq!(app.editor_state.cursor_col, 2);
        assert!(
            app.doc
                .tabs
                .iter()
                .any(|tab| tab.current_file.as_ref() == Some(&target)),
            "la ruta original se restaura en su pestaña"
        );
        assert!(scan_recovery_dir(&dir).is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discard_all_recovery_limpia_journals_sin_tocar_disco() {
        let dir = recovery_test_dir("discard");
        let target = dir.join("intacto.txt");
        std::fs::write(&target, "contenido en disco\n").expect("archivo de prueba");
        let mut entry = recovery_entry("intacto.txt", "no guardado");
        entry.original_path = Some(target.clone());
        save_recovery_file(&dir, &journal_filename(Some(&target), 2), &entry)
            .expect("journal escrito");

        let mut app = EditorApp::default();
        app.init_recovery_from_dir(dir.clone());

        app.discard_all_recovery();

        assert!(!app.show_recovery_modal);
        assert!(app.recovery_pending.is_empty());
        assert!(scan_recovery_dir(&dir).is_empty(), "journals borrados");
        assert_eq!(
            std::fs::read_to_string(&target).expect("el archivo sigue"),
            "contenido en disco\n"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cleanup_recovery_borra_journal_al_guardar_y_cerrar_pestana() {
        let dir = recovery_test_dir("cleanup");
        let mut app = EditorApp::default();
        app.init_recovery_from_dir(dir.clone());
        app.set_text("sin guardar");
        app.doc.dirty = true;

        let tab_id = app.doc.tabs[app.doc.active_tab].id;
        let target = dir.join("guardado.txt");
        app.doc.current_file = Some(target.clone());
        save_recovery_file(&dir, &journal_filename(None, tab_id), &recovery_entry("Untitled", "x"))
            .expect("journal untitled");
        save_recovery_file(
            &dir,
            &journal_filename(Some(&target), tab_id),
            &recovery_entry("guardado.txt", "x"),
        )
        .expect("journal por ruta");

        app.cleanup_recovery_for_active_tab();
        assert!(scan_recovery_dir(&dir).is_empty(), "el guardado limpia ambos journals");

        save_recovery_file(
            &dir,
            &journal_filename(Some(&target), tab_id),
            &recovery_entry("guardado.txt", "x"),
        )
        .expect("journal por ruta");
        app.close_tab(0);
        assert!(scan_recovery_dir(&dir).is_empty(), "cerrar la pestaña limpia su journal");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn format_recovery_age_at_escala_segundos_minutos_horas_y_dias() {
        let now = 1_700_000_000;
        assert_eq!(format_recovery_age_at(now, now), "hace unos segundos");
        assert_eq!(format_recovery_age_at(now - 59, now), "hace unos segundos");
        assert_eq!(format_recovery_age_at(now - 120, now), "hace 2 min");
        assert_eq!(format_recovery_age_at(now - 7_200, now), "hace 2 h");
        assert_eq!(format_recovery_age_at(now - 172_800, now), "hace 2 días");
    }