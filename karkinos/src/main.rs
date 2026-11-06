#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ast_worker;
mod cargo_service;
mod dap_service;
mod document_service;
mod editor_widget;
mod explorer_ops;
mod extensions_service;
mod fonts;
mod git_diff_worker;
mod git_ops;
mod git_status_worker;
mod lsp_service;
mod quick_open;
mod syntax;
mod terminal_service;
mod theme;
mod title_bar;
mod ui;
mod ui_components;
mod ui_explorer;
mod ui_extensions;
mod ui_source_control;
mod util;

pub use karkinos_core::buffer;
pub use karkinos_core::undo;

use eframe::egui;

use app::EditorApp;
use util::is_wsl;

/// Logo de Karkinos, embebido en el binario en tiempo de compilación
/// (no se lee de disco en tiempo de ejecución — así el ícono y el
/// fondo minimalista funcionan siempre, sin depender de que el
/// archivo esté presente donde sea que se ejecute el programa).
pub const LOGO_BYTES: &[u8] = include_bytes!("../assets/logo.png");

fn main() -> eframe::Result<()> {
    // Roadmap v0.2.45 (Tarea A): restaura la consola si un panic la deja
    // corrupta (modo raw, cursor oculto o alternate screen buffer). Las
    // secuencias ANSI revierten atributos, muestran el cursor y salen del
    // buffer alternativo antes de volcar el error por stderr.
    std::panic::set_hook(Box::new(|info| {
        eprint!("\x1b[0m\x1b[?25h\x1b[?1049l");
        eprintln!("{info}");
    }));

    // Roadmap v0.2.45 (Tarea A): argumentos de línea de comandos.
    // `--version`/`-v` y `--help`/`-h` imprimen y salen con código 0; un
    // path posicional se guarda para abrirlo en la pestaña inicial.
    let mut target_path: Option<std::path::PathBuf> = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-v" | "--version" => {
                println!("karkinos {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            _ => {
                target_path = Some(std::path::PathBuf::from(arg));
            }
        }
    }

    // v0.2.48 (Tarea B): coordinación de instancia única. Si ya hay una
    // primaria escuchando, le reenviamos el path pedido y salimos al
    // instante, sin abrir una segunda ventana (y su GPU/CPU extra). Si no
    // hay primaria (o no traíamos path), nos convertimos en la primaria y
    // levantamos el servidor IPC que atenderá los próximos `karkinos <path>`.
    // Coordinación de instancia única: verifica siempre si ya hay una instancia primaria activa.
    // Si viene con path, le reenviamos el path y sale. Si no trae path, le pide foco/activación y sale.
    match karkinos_core::ipc::try_send_to_primary(target_path.as_deref()) {
        Ok(true) => {
            if target_path.is_some() {
                println!("Karkinos: opened in existing window");
            } else {
                println!("Karkinos: activated existing window");
            }
            return Ok(());
        }
        Ok(false) => {}
        Err(e) => eprintln!("Karkinos: IPC check failed: {e}"),
    }

    let ipc_server = karkinos_core::ipc::IpcServer::spawn().ok();

    // WSLg usa una GPU virtual cuyo contexto OpenGL a veces falla en
    // silencio (la ventana nunca llega a abrirse, sin ningún error).
    // Forzamos renderizado por software SOLO si detectamos WSL — en
    // Linux nativo con una GPU de verdad, esto solo haría que ande
    // más lento sin necesidad.
    if is_wsl() {
        std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([900.0, 650.0]) // tamaño de referencia antes de maximizar
        .with_maximized(true)
        .with_resizable(true)
        .with_decorations(false); // barra de título propia, ver custom_title_bar en ui.rs

    // Ícono de la barra de título / taskbar con transparencia alfa para
    // que no quede un recuadro negro en el dock o panel del sistema.
    if let Some(icon) = crate::ui::load_window_icon() {
        viewport = viewport.with_icon(icon);
    }

    // S2 (v0.2.81): Karkinos siempre es oscuro; no seguir el tema del
    // sistema para que egui no arranque con su paleta clara.
    let options = eframe::NativeOptions {
        viewport,
        follow_system_theme: false,
        default_theme: eframe::Theme::Dark,
        ..Default::default()
    };
    eframe::run_native("Karkinos", options, Box::new(move |cc| {
        // Configura Geist Mono como la fuente principal para todo el texto monoespaciado (editor, terminal, logs)
        crate::fonts::install_fonts(&cc.egui_ctx);

        // Desactivamos el zoom global con teclado de egui para evitar que Ctrl+=/-
        // escale la ventana completa y corrompa el atlas de fuentes del editor.
        // El zoom del editor se controla directamente sobre el tamaño de fuente.
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
        });

        // S10: el zoom elegido la sesión anterior vuela al arrancar
        // desde el Storage de la app (app-storage.json). Sin Storage
        // (o valor corrupto) queda el default de 14 px.
        let mut app = EditorApp::new();
        // v0.2.48 (Tarea B): la instancia primaria recibe el servidor IPC
        // (o `None` si el bind falló o este proceso era secundario).
        app.ipc_server = ipc_server;
        // Roadmap v0.2.45 (Tarea A): pasar el path posicional del CLI al
        // setup de EditorApp. La apertura completa (archivo nuevo, Git,
        // etc.) la resuelve `init_from_cli_path` (Tarea B).
        if let Some(path) = target_path {
            app.init_from_cli_path(&path);
        }
        if let Some(storage) = &cc.storage {
            if let Some(size) = storage.get_string("editor_font_size") {
                if let Some(parsed) = crate::app::font_size_from_string(&size) {
                    app.font_size = parsed;
                    app.editor_state.set_font_size(parsed);
                }
            }
            if let Some(theme_str) = storage.get_string("editor_theme") {
                app.theme_mode = crate::theme::ThemeMode::from_storage_str(&theme_str);
            }
        }
        // S2 (v0.2.81): los widgets propios de egui (menús, popups,
        // ventanas, botones) usan la paleta del tema activo en vez de la
        // interna de egui. `apply_visuals` conserva además el sistema de
        // diseño de la UI-1 (esquinas de 6/10 px y sombra de elevación).
        crate::theme::apply_visuals(&cc.egui_ctx, &app.theme_mode.theme());
        Box::new(app)
    }))
}

/// Imprime el uso básico y las opciones; el llamador sale con código 0.
fn print_usage() {
    println!("karkinos [PATH]");
    println!();
    println!("Usage: karkinos [PATH]");
    println!("Opens PATH (a file or folder) on startup, or the current directory if omitted.");
    println!();
    println!("Options:");
    println!("  -h, --help     Print this help message and exit");
    println!("  -v, --version  Print version information and exit");
}