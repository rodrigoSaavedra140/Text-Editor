use std::time::{Duration, Instant};

use eframe::egui;

/// Tamaño legible del texto actual en el buffer (bytes UTF-8) — es el
/// tamaño "en vivo", incluye cambios sin guardar todavía, no el
/// tamaño del archivo en disco.
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} bytes", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Pide el cierre prolijo de la ventana Y, en paralelo, lanza un hilo
/// del sistema operativo que va a matar el proceso a la fuerza al
/// segundo — sin importar si egui sigue llamando a update() o no
/// después de pedir el cierre (WSLg a veces deja de mandar frames en
/// cuanto la ventana empieza a cerrarse, lo que dejaba una
/// salvaguarda dentro del loop de egui sin poder ejecutarse nunca).
pub fn request_close(ctx: &egui::Context, closing_since: &mut Option<Instant>) {
    if closing_since.is_some() {
        return; // ya se pidió el cierre, no lancemos un hilo por cada frame
    }
    *closing_since = Some(Instant::now());
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(1));
        std::process::exit(0);
    });
}

/// Detecta si estamos corriendo dentro de WSL (y no en Linux nativo).
/// WSL define variables de entorno propias, y su kernel se identifica
/// a sí mismo con "microsoft" en /proc/version — revisamos las dos
/// señales por si alguna versión de WSL no define la otra.
#[cfg(target_os = "linux")]
pub fn is_wsl() -> bool {
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        return true;
    }
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let v = version.to_lowercase();
        if v.contains("microsoft") || v.contains("wsl") {
            return true;
        }
    }
    false
}

// En Windows nativo o macOS esto no aplica — LIBGL_ALWAYS_SOFTWARE es
// específico de Mesa/Linux, así que directamente no hace nada ahí.
#[cfg(not(target_os = "linux"))]
pub fn is_wsl() -> bool {
    false
}