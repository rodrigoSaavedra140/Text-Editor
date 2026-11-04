use std::time::{Duration, Instant};

use eframe::egui;

/// S4: parsea la entrada de "Ir a línea N" (Ctrl+G). Devuelve el
/// índice 0-based de destino o None si la entrada no es un número:
/// el llamador no mueve el cursor en ese caso, solo avisa.
pub fn parse_goto_line(input: &str, total_lines: usize) -> Option<usize> {
    let n: usize = input.trim().parse().ok()?;
    if n == 0 {
        return Some(0); // "0" y "1" son la misma primera línea
    }
    Some((n - 1).min(total_lines.max(1) - 1))
}

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

/// Directorio home del usuario actual: `$HOME`, o `USERPROFILE` en
/// Windows. `None` si ninguna de las dos variables está definida.
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(std::path::PathBuf::from)
}

/// Expande una ruta de usuario que empieza con `~` (`~`, `~/...` o
/// `~\...` en Windows) usando el home del usuario. `std::path::Path`
/// no expande la tilde por su cuenta, por eso el modal "Open Folder" y
/// la invocación por CLI fallaban con "Not a folder".
///
/// - Si `raw` empieza con `~`, se resuelve contra el home concatenando
///   el resto de la ruta.
/// - Si el resultado sigue siendo relativo, se concatena contra `base`.
/// - Si es absoluto, se devuelve tal cual.
pub fn expand_user_path(raw: &str, base: &std::path::Path) -> std::path::PathBuf {
    let raw = raw.trim();

    let mut path = if let Some(rest) = raw.strip_prefix('~') {
        // `~`, `~/...` y `~\...`: descartamos el separador que sigue a la
        // tilde para que `join` no trate el resto como absoluto (en
        // `home.join("/resto")` el home se descartaría).
        let rest = rest.trim_start_matches(['/', '\\']);
        match home_dir() {
            Some(home) => home.join(rest),
            None => std::path::PathBuf::from(raw),
        }
    } else {
        std::path::PathBuf::from(raw)
    };

    if !path.is_absolute() {
        path = base.join(path);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_goto_line_numera_desde_1_y_clampea() {
        assert_eq!(parse_goto_line("3", 10), Some(2));
        assert_eq!(parse_goto_line("  7  ", 10), Some(6));
        assert_eq!(parse_goto_line("1", 10), Some(0));
        // 0 = primera línea también (más simple que rechazar).
        assert_eq!(parse_goto_line("0", 10), Some(0));
        // Fuera de rango: se acota a la última línea.
        assert_eq!(parse_goto_line("999", 10), Some(9));
        // Documento con una sola línea: todo cae adentro.
        assert_eq!(parse_goto_line("5", 1), Some(0));
        // Basura: sin movimiento.
        assert_eq!(parse_goto_line("abc", 10), None);
        assert_eq!(parse_goto_line("-4", 10), None);
        assert_eq!(parse_goto_line("", 10), None);
    }

    #[test]
    fn expand_user_path_expande_tilde_al_home() {
        let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) else {
            return; // sin home en este entorno no hay nada que expandir
        };
        let home = std::path::PathBuf::from(home);
        let base = std::path::Path::new("/tmp/karkinos_base");

        assert_eq!(expand_user_path("~/test", base), home.join("test"));
        // `~` solo también apunta al home.
        assert_eq!(expand_user_path("~", base), home);
    }

    #[test]
    fn expand_user_path_mantiene_rutas_absolutas() {
        let base = std::path::Path::new("/tmp/karkinos_base");
        assert_eq!(
            expand_user_path("/etc/hosts", base),
            std::path::PathBuf::from("/etc/hosts")
        );
    }

    #[test]
    fn expand_user_path_resuelve_relativas_contra_base() {
        let base = std::path::Path::new("/tmp/karkinos_base");
        assert_eq!(
            expand_user_path("src/main.rs", base),
            base.join("src/main.rs")
        );
    }
}
