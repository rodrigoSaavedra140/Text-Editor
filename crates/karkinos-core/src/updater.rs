//! S1 (Auto-actualización in-app): lógica pura del chequeo de releases.
//!
//! Acá vive todo lo que se puede probar sin red ni disco: comparar
//! versiones, leer la respuesta de la API de GitHub y decidir qué asset
//! corresponde a esta plataforma. La capa fina (`fetch_latest_release`)
//! queda al final del módulo; la UI y la descarga viven en el crate
//! `karkinos`.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Endpoint del último release publicado del repositorio.
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/rodrigoSaavedra140/Text-Editor/releases/latest";
/// Tiempo máximo de espera del chequeo (el hilo de fondo no se cuelga).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Tope de bytes de la descarga del binario (200 MB): un Karkinos ronda
/// los 25 MB, así que este límite no rechaza releases legítimos pero evita
/// que una respuesta inesperada llene el disco.
const MAX_BINARY_BYTES: u64 = 200 * 1024 * 1024;
/// Tope de bytes de `SHA256SUMS` (unas pocas líneas).
const MAX_SUMS_BYTES: u64 = 1024 * 1024;
/// Tiempo máximo de espera de una descarga (el binario pesa decenas de MB).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// Sufijo del binario viejo durante un reemplazo en Windows.
const BACKUP_SUFFIX: &str = ".old";
/// Sufijo del binario nuevo, hermano del destino (mismo filesystem, para
/// que el `rename` final sea atómico).
const STAGING_SUFFIX: &str = ".new";
/// Un archivo adjunto del release, con el nombre con el que se publicó y
/// la URL directa de descarga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
}

/// Datos del último release que la app necesita para decidir si hay
/// actualización y qué bajar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// `tag_name` tal cual lo publica GitHub (normalmente `vX.Y.Z`).
    pub version: String,
    pub assets: Vec<ReleaseAsset>,
}

impl ReleaseInfo {
    /// URL de descarga del asset con ese nombre exacto, si está publicado.
    pub fn asset_url(&self, name: &str) -> Option<&str> {
        self.assets
            .iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.download_url.as_str())
    }

    /// URL del binario de esta plataforma, o `None` si el release no lo
    /// publicó (o la plataforma no tiene binario).
    pub fn platform_asset_url(&self) -> Option<&str> {
        platform_asset_name().and_then(|name| self.asset_url(name))
    }
}

/// Convierte `vX.Y.Z` (o `X.Y.Z`) en sus tres componentes numéricos.
/// Devuelve `None` para cualquier otra forma: sin los tres números no hay
/// comparación segura posible (pre-releases y sufijos quedan afuera).
pub fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let body = version.strip_prefix('v').unwrap_or(version);
    let mut parts = body.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    // Una cuarta parte (`0.2.84.1`) o un sufijo inválido no es `X.Y.Z`.
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// `true` solo si `latest` es estrictamente más nueva que `current`.
/// Con versiones inválidas devuelve `false`: ante la duda no se ofrece
/// actualizar (S1: sin datos confiables no aparece nada).
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// Nombre del binario suelto que corresponde a esta plataforma. Las
/// plataformas sin binario publicado devuelven `None`.
pub fn platform_asset_name() -> Option<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some("karkinos-linux-x86_64")
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some("karkinos-windows-x86_64.exe")
    }

    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        None
    }
}

/// Subconjunto de `GET .../releases/latest` que nos importa. Los campos
/// son `Option` para poder rechazar el payload con un error claro en vez
/// de inventar una versión vacía.
#[derive(Deserialize)]
struct ReleasePayload {
    tag_name: Option<String>,
    #[serde(default)]
    assets: Vec<AssetPayload>,
}

#[derive(Deserialize)]
struct AssetPayload {
    name: Option<String>,
    browser_download_url: Option<String>,
}

/// Lee la respuesta de la API de GitHub (`tag_name` + `assets[].name` +
/// `assets[].browser_download_url`). Un JSON inválido o sin `tag_name` es
/// un error: sin versión no se puede decidir si hay actualización.
pub fn parse_latest_release(json: &str) -> Result<ReleaseInfo, String> {
    let payload: ReleasePayload = serde_json::from_str(json)
        .map_err(|e| format!("Failed to parse the GitHub release payload: {}", e))?;

    let version = payload.tag_name.unwrap_or_default().trim().to_string();
    if version.is_empty() {
        return Err("GitHub release payload has no tag_name".to_string());
    }

    // Un asset sin nombre o sin URL no sirve para descargar nada: se ignora
    // en vez de dejar una entrada a medio armar.
    let assets = payload
        .assets
        .into_iter()
        .filter_map(|asset| {
            let name = asset.name?;
            let download_url = asset.browser_download_url?;
            if name.is_empty() || download_url.is_empty() {
                return None;
            }
            Some(ReleaseAsset { name, download_url })
        })
        .collect();

    Ok(ReleaseInfo { version, assets })
}

/// Capa fina: consulta el último release publicado. Corre en un hilo de
/// la UI; un error (sin red, rate limit, JSON raro) es solo un `Err` que
/// el servicio descarta en silencio (S1).
pub fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    // El agente https (solo TLS, también al seguir redirecciones) es el
    // mismo que usa Open VSX: una sola implementación en el crate.
    let body = crate::open_vsx::https_agent()
        .get(LATEST_RELEASE_URL)
        // GitHub exige un User-Agent en todas sus peticiones.
        .set("User-Agent", &format!("karkinos/{}", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .timeout(REQUEST_TIMEOUT)
        .call()
        .map_err(|e| format!("GitHub release request failed: {}", e))?
        .into_string()
        .map_err(|e| format!("Invalid UTF-8 response: {}", e))?;

    parse_latest_release(&body)
}

// ---------------------------------------------------------------------
// S5: integridad del release (`SHA256SUMS`)
// ---------------------------------------------------------------------

/// Lee un archivo `SHA256SUMS` en el formato de `sha256sum`
/// (`<hash hex en minúsculas><espacios><nombre>`, con un `*` opcional del
/// modo binario antes del nombre) y devuelve nombre → hash. Las líneas que
/// no cumplen el formato se descartan: un archivo sucio no debe impedir
/// verificar los demás archivos.
pub fn parse_sha256sums(text: &str) -> HashMap<String, String> {
    let mut sums = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Solo el primer espacio separa: el hash no tiene espacios y el
        // resto es el nombre (que sí puede tenerlos).
        let Some((hash, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let hash = hash.trim().to_ascii_lowercase();
        // `sha256sum` siempre escribe 64 hex.
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let rest = rest.trim_start();
        let name = rest.strip_prefix('*').unwrap_or(rest).trim();
        if name.is_empty() {
            continue;
        }
        sums.insert(name.to_string(), hash);
    }
    sums
}

/// Hash publicado para `asset_name`, o `None` si `SHA256SUMS` no trae su
/// línea.
pub fn expected_sha256(sums: &HashMap<String, String>, asset_name: &str) -> Option<String> {
    sums.get(asset_name).cloned()
}

/// SHA-256 de `bytes` en el mismo formato que `sha256sum`: 64 hex en
/// minúsculas.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Verifica que `bytes` sea exactamente el archivo que `SHA256SUMS` publica
/// para `asset_name`. Sin línea o con hash distinto no se instala nada
/// (S5) y el error dice cuál de los dos casos es.
pub fn verify(bytes: &[u8], sums_text: &str, asset_name: &str) -> Result<(), String> {
    let sums = parse_sha256sums(sums_text);
    let Some(expected) = expected_sha256(&sums, asset_name) else {
        return Err(format!("checksum missing for {}", asset_name));
    };
    if sha256_hex(bytes) != expected {
        return Err(format!("checksum mismatch for {}", asset_name));
    }
    Ok(())
}

// ---------------------------------------------------------------------
// S2: tipo de instalación y reemplazo del ejecutable
// ---------------------------------------------------------------------

/// Cómo se puede actualizar esta instalación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// Binario portable: Karkinos se reemplaza a sí mismo (S2).
    Portable,
    /// Lo instaló el gestor de paquetes (T5): la app no toca el archivo.
    Package,
}

/// Decide si esta instalación es portable o la administra el gestor de
/// paquetes. En esta tarea el dato llega ya resuelto; el chequeo real de
/// `pacman -Qo` es T5 (S4).
pub fn install_kind(_exe_path: &Path, owned_by_package: bool) -> InstallKind {
    if owned_by_package {
        InstallKind::Package
    } else {
        InstallKind::Portable
    }
}

/// Mensaje único para "no se pudo escribir el binario": dice el
/// directorio y qué puede hacer el usuario (S2).
fn write_error(dir: &Path) -> String {
    format!(
        "cannot write {}: reinstall Karkinos or update it with your package manager",
        dir.display()
    )
}

/// `path` con un sufijo agregado al nombre del archivo
/// (`karkinos.exe` → `karkinos.exe.old`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Borra el `<exe>.old` que dejó una actualización anterior. En Windows el
/// binario viejo sigue bloqueado mientras vive el proceso que lo ejecutaba,
/// así que solo se puede limpiar en el arranque siguiente. Devuelve `true`
/// si había algo que borrar.
pub fn cleanup_stale_backup(exe_path: &Path) -> bool {
    std::fs::remove_file(with_suffix(exe_path, BACKUP_SUFFIX)).is_ok()
}

/// Reemplazo del ejecutable en Unix: el binario nuevo se escribe al lado
/// (mismo filesystem), se marca ejecutable y se `rename` sobre el destino.
/// El `rename` es atómico y válido aunque el destino esté en ejecución: el
/// kernel conserva el inodo viejo hasta que el proceso termina.
#[cfg(unix)]
pub fn replace_executable(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let dir = destination.parent().unwrap_or_else(|| Path::new("."));
    let staging = with_suffix(destination, STAGING_SUFFIX);

    std::fs::write(&staging, bytes).map_err(|_| write_error(dir))?;
    // Sin permiso de ejecución el archivo no sirve como binario.
    if std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).is_err() {
        let _ = std::fs::remove_file(&staging);
        return Err(write_error(dir));
    }
    if std::fs::rename(&staging, destination).is_err() {
        let _ = std::fs::remove_file(&staging);
        return Err(write_error(dir));
    }
    Ok(())
}

/// Reemplazo del ejecutable en Windows: un `.exe` en ejecución no se puede
/// sobrescribir pero SÍ renombrar, así que el actual se aparta a
/// `karkinos.exe.old`, se escribe el nuevo con el nombre original y, si la
/// escritura falla, el `.old` vuelve a su lugar (rollback). El `.old`
/// sobrante lo limpia [`cleanup_stale_backup`] en el arranque siguiente.
#[cfg(windows)]
pub fn replace_executable(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    install_with_backup(destination, bytes)
}

/// Plataformas sin binario publicado: no hay forma segura de reemplazarse
/// a sí mismas.
#[cfg(not(any(unix, windows)))]
pub fn replace_executable(_destination: &Path, _bytes: &[u8]) -> Result<(), String> {
    Err("in-place updates are not supported on this platform".to_string())
}

/// Escritura con respaldo: aparta el destino, escribe el nuevo y hace
/// rollback si la escritura falla. Es la estrategia de Windows, pero vive
/// fuera de `#[cfg(windows)]` para poder probar el rollback en cualquier
/// plataforma.
pub fn install_with_backup(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    install_with_backup_using(destination, bytes, |path, data| std::fs::write(path, data))
}

/// Igual que [`install_with_backup`] con la escritura inyectable: el test
/// fuerza el fallo (sin permisos raros ni procesos) para ejercitar el
/// rollback, que de otra forma no se puede provocar.
fn install_with_backup_using(
    destination: &Path,
    bytes: &[u8],
    write: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
) -> Result<(), String> {
    let dir = destination.parent().unwrap_or_else(|| Path::new("."));
    let backup = with_suffix(destination, BACKUP_SUFFIX);
    // Sin destino previo no hay nada que apartar.
    let had_previous = std::fs::rename(destination, &backup).is_ok();

    if write(destination, bytes).is_ok() {
        // En Linux el `.old` se borra en el acto; en Windows todavía está
        // bloqueado y lo limpia el próximo arranque.
        let _ = std::fs::remove_file(&backup);
        return Ok(());
    }

    // Rollback: lo que haya quedado a medio escribir se descarta y el
    // binario viejo vuelve a su nombre original.
    if had_previous {
        let _ = std::fs::remove_file(destination);
        let _ = std::fs::rename(&backup, destination);
    }
    Err(write_error(dir))
}

/// Verifica y reemplaza, en ese orden: si el checksum no coincide el
/// destino queda intacto byte a byte (S5). Recibe el destino explícito;
/// los tests lo apuntan a un directorio temporal, nunca al ejecutable real.
pub fn install_verified(
    destination: &Path,
    bytes: &[u8],
    sums_text: &str,
    asset_name: &str,
) -> Result<(), String> {
    verify(bytes, sums_text, asset_name)?;
    replace_executable(destination, bytes)
}

/// Baja una URL https completa con un tope de bytes. Un byte de más del
/// límite permite distinguir "justo en el tope" de "se pasó".
fn download_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err(format!("Refusing to download from {}", url));
    }
    let response = crate::open_vsx::https_agent()
        .get(url)
        .timeout(DOWNLOAD_TIMEOUT)
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let mut data = Vec::new();
    response
        .into_reader()
        .take(max_bytes + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    if data.len() as u64 > max_bytes {
        return Err(format!("Download exceeds the {} byte limit", max_bytes));
    }
    Ok(data)
}

/// Capa fina de instalación (S2 + S5), pensada para correr en un hilo
/// aparte: baja `SHA256SUMS` y el binario, verifica y reemplaza
/// `exe_path`. Cualquier fallo deja el ejecutable como estaba.
pub fn install_release(
    exe_path: &Path,
    asset_url: &str,
    sums_url: &str,
    asset_name: &str,
    owned_by_package: bool,
) -> Result<(), String> {
    if install_kind(exe_path, owned_by_package) == InstallKind::Package {
        return Err(
            "Karkinos is managed by your package manager: update it with your package manager"
                .to_string(),
        );
    }

    let sums = download_bytes(sums_url, MAX_SUMS_BYTES)?;
    let sums_text =
        String::from_utf8(sums).map_err(|_| "SHA256SUMS is not valid UTF-8".to_string())?;
    let bytes = download_bytes(asset_url, MAX_BINARY_BYTES)?;

    install_verified(exe_path, &bytes, &sums_text, asset_name)
}

// ---------------------------------------------------------------------
// S2: reinicio tras actualizar + instancia única
// ---------------------------------------------------------------------

/// Flag interno del reinicio tras una actualización. Lleva el pid del
/// proceso que se está cerrando.
pub const AFTER_UPDATE_FLAG: &str = "--after-update";

/// Qué tiene que hacer este proceso con el arranque y la instancia única.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum LaunchMode {
    /// Arranque normal: si hay una instancia primaria, se le reenvían los
    /// argumentos y este proceso sale.
    Forward,
    /// Arranque después de una actualización: la instancia vieja está
    /// cerrándose. Este proceso NO reenvía nada (la vieja se quedaría con
    /// los argumentos del reinicio y moriría igual); espera a que suelte el
    /// socket de instancia única y toma la primaria.
    TakeOver { previous_pid: Option<u32> },
}

/// Decide el arranque a partir de los argumentos (sin el nombre del
/// ejecutable). La sola presencia del flag basta para no reenviar: un pid
/// ilegible solo quita el dato de diagnóstico.
pub fn launch_mode(args: &[String]) -> LaunchMode {
    match args.iter().position(|arg| arg == AFTER_UPDATE_FLAG) {
        Some(index) => LaunchMode::TakeOver {
            previous_pid: args.get(index + 1).and_then(|pid| pid.parse().ok()),
        },
        None => LaunchMode::Forward,
    }
}

/// Argumentos para relanzar el binario recién instalado con la misma línea
/// de comandos, agregando (o reemplazando) el flag interno con el pid del
/// proceso actual. Reemplazar evita acumular flags si se actualiza varias
/// veces seguidas.
pub fn restart_args(args: &[String], previous_pid: u32) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.len() + 2);
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == AFTER_UPDATE_FLAG {
            skip_next = true;
            continue;
        }
        forwarded.push(arg.clone());
    }
    forwarded.push(AFTER_UPDATE_FLAG.to_string());
    forwarded.push(previous_pid.to_string());
    forwarded
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- S1: parse_version ----

    #[test]
    fn parse_version_acepta_con_y_sin_v() {
        assert_eq!(parse_version("v0.2.84"), Some((0, 2, 84)));
        assert_eq!(parse_version("0.2.84"), Some((0, 2, 84)));
        assert_eq!(parse_version("v1.10.3"), Some((1, 10, 3)));
        assert_eq!(parse_version("10.0.0"), Some((10, 0, 0)));
    }

    #[test]
    fn parse_version_rechaza_lo_que_no_es_x_y_z() {
        for invalid in [
            "",
            "v",
            "0",
            "0.2",
            "0.2.84.1",
            "0.2.x",
            "v0.2.84-rc1",
            " 0.2.84",
            "0.2.84 ",
            "vv0.2.84",
            "-1.2.3",
            "0.2.18446744073709551616",
        ] {
            assert_eq!(
                parse_version(invalid),
                None,
                "{invalid:?} no es una versión X.Y.Z"
            );
        }
    }

    // ---- S1: is_newer ----

    #[test]
    fn is_newer_compara_componente_por_componente() {
        assert!(is_newer("v0.2.85", "v0.2.84"));
        assert!(is_newer("0.3.0", "0.2.84"));
        assert!(is_newer("v1.0.0", "v0.99.99"));
        assert!(is_newer("v0.2.10", "v0.2.9"));
        assert!(!is_newer("v0.2.83", "v0.2.84"));
        assert!(!is_newer("v0.1.99", "v0.2.0"));
        assert!(!is_newer("v0.9.9", "v1.0.0"));
        assert!(!is_newer("v0.2.84", "v0.2.84"));
        assert!(!is_newer("0.2.84", "v0.2.84"));
    }

    #[test]
    fn is_newer_es_false_si_alguna_version_es_invalida() {
        assert!(!is_newer("basura", "v0.2.84"));
        assert!(!is_newer("v0.2.85", "basura"));
        assert!(!is_newer("", ""));
    }

    // ---- S1: parse_latest_release ----

    const RELEASE_JSON: &str = r#"{
        "url": "https://api.github.com/repos/rodrigoSaavedra140/Text-Editor/releases/1",
        "tag_name": "v0.2.85",
        "name": "Karkinos v0.2.85",
        "prerelease": false,
        "assets": [
            {
                "name": "karkinos-linux-x86_64",
                "size": 24000000,
                "browser_download_url": "https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.85/karkinos-linux-x86_64"
            },
            {
                "name": "karkinos-windows-x86_64.exe",
                "size": 25000000,
                "browser_download_url": "https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.85/karkinos-windows-x86_64.exe"
            },
            {
                "name": "SHA256SUMS",
                "size": 300,
                "browser_download_url": "https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.85/SHA256SUMS"
            }
        ]
    }"#;

    #[test]
    fn parse_latest_release_lee_el_tag_y_los_assets() {
        let release = parse_latest_release(RELEASE_JSON).expect("el JSON de ejemplo debe parsear");

        assert_eq!(release.version, "v0.2.85");
        assert_eq!(release.assets.len(), 3);
        assert_eq!(release.assets[0].name, "karkinos-linux-x86_64");
        assert_eq!(
            release.assets[0].download_url,
            "https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.85/karkinos-linux-x86_64"
        );
        assert_eq!(release.assets[2].name, "SHA256SUMS");
        assert_eq!(
            release.asset_url("karkinos-windows-x86_64.exe"),
            Some("https://github.com/rodrigoSaavedra140/Text-Editor/releases/download/v0.2.85/karkinos-windows-x86_64.exe")
        );
        assert_eq!(release.asset_url("no-existe"), None);
    }

    #[test]
    fn parse_latest_release_rechaza_json_invalido_o_sin_tag() {
        assert!(parse_latest_release("no es json").is_err());
        assert!(
            parse_latest_release(r#"{"assets": []}"#).is_err(),
            "sin tag_name no hay versión que comparar"
        );
        assert!(
            parse_latest_release(r#"{"tag_name": "", "assets": []}"#).is_err(),
            "un tag vacío tampoco sirve"
        );
    }

    // ---- S2/S5: helpers de instalación ----

    /// Directorio temporal único por test: la instalación se prueba
    /// siempre sobre un destino explícito, nunca contra el ejecutable real
    /// ni dentro del repositorio.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "karkinos_updater_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("crear carpeta temporal");
        dir
    }

    // ---- S5: sha256_hex / parse_sha256sums / verify ----

    /// Vectores conocidos de SHA-256 de `abc` y de la cadena vacía.
    const SHA_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn sha256_hex_coincide_con_vectores_conocidos() {
        assert_eq!(sha256_hex(b"abc"), SHA_ABC);
        assert_eq!(sha256_hex(b""), SHA_EMPTY);
        // `sha256sum` imprime 64 hex en minúsculas: el formato del release.
        assert_eq!(sha256_hex(b"Karkinos"), sha256_hex(b"Karkinos").to_lowercase());
        assert_eq!(sha256_hex(b"Karkinos").len(), 64);
    }

    #[test]
    fn parse_sha256sums_acepta_dos_espacios_y_modo_binario() {
        let text = format!(
            "{SHA_ABC}  karkinos-linux-x86_64\n{SHA_EMPTY} *karkinos-windows-x86_64.exe\n"
        );
        let sums = parse_sha256sums(&text);

        assert_eq!(
            sums.get("karkinos-linux-x86_64"),
            Some(&SHA_ABC.to_string()),
            "el formato de `sha256sum` separa con dos espacios"
        );
        assert_eq!(
            sums.get("karkinos-windows-x86_64.exe"),
            Some(&SHA_EMPTY.to_string()),
            "el `*` del modo binario no forma parte del nombre"
        );
        assert_eq!(sums.len(), 2);

        assert_eq!(
            expected_sha256(&sums, "karkinos-linux-x86_64"),
            Some(SHA_ABC.to_string())
        );
        assert_eq!(expected_sha256(&sums, "SHA256SUMS"), None);
    }

    #[test]
    fn parse_sha256sums_ignora_lineas_invalidas() {
        let text = format!(
            "{SHA_ABC}  bueno\n\n# comentario\nno-es-un-hash  otro\nabc123  corto\n{SHA_ABC}  \n   \n"
        );
        let sums = parse_sha256sums(&text);

        assert_eq!(sums.len(), 1, "solo la línea bien formada entra al mapa");
        assert_eq!(sums.get("bueno"), Some(&SHA_ABC.to_string()));
        assert!(parse_sha256sums("").is_empty());
    }

    #[test]
    fn verify_acepta_el_hash_correcto_y_rechaza_mismatch_o_falta() {
        let sums = format!("{SHA_ABC}  karkinos-linux-x86_64\n");

        assert_eq!(verify(b"abc", &sums, "karkinos-linux-x86_64"), Ok(()));
        assert_eq!(
            verify(b"otro contenido", &sums, "karkinos-linux-x86_64"),
            Err("checksum mismatch for karkinos-linux-x86_64".to_string())
        );
        assert_eq!(
            verify(b"abc", &sums, "karkinos-windows-x86_64.exe"),
            Err("checksum missing for karkinos-windows-x86_64.exe".to_string())
        );
        // Sin `SHA256SUMS` (texto vacío) tampoco se instala nada.
        assert_eq!(
            verify(b"abc", "", "karkinos-linux-x86_64"),
            Err("checksum missing for karkinos-linux-x86_64".to_string())
        );
    }

    // ---- S2/S5: reemplazo del ejecutable (destino explícito, sin
    // `current_exe()` ni procesos) ----

    #[test]
    fn install_verified_reemplaza_el_destino_sin_dejar_temporales() {
        let dir = temp_dir("install_ok");
        let exe = dir.join("karkinos");
        std::fs::write(&exe, b"viejo").expect("binario viejo");
        let sums = format!("{SHA_ABC}  karkinos-linux-x86_64\n");

        install_verified(&exe, b"abc", &sums, "karkinos-linux-x86_64").expect("instalar");

        assert_eq!(std::fs::read(&exe).expect("leer destino"), b"abc");
        assert!(
            !dir.join("karkinos.new").exists(),
            "el temporal .new no sobrevive a la instalación"
        );
        assert!(
            !dir.join("karkinos.old").exists(),
            "sin backup sobrante en este caso"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn install_verified_deja_el_binario_en_0755() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("install_mode");
        let exe = dir.join("karkinos");
        std::fs::write(&exe, b"viejo").expect("binario viejo");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644))
            .expect("dejar el destino sin permiso de ejecución");
        let sums = format!("{SHA_ABC}  karkinos-linux-x86_64\n");

        install_verified(&exe, b"abc", &sums, "karkinos-linux-x86_64").expect("instalar");

        let mode = std::fs::metadata(&exe).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "el binario instalado queda ejecutable");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn install_verified_con_checksum_malo_deja_el_destino_intacto() {
        let dir = temp_dir("install_bad_checksum");
        let exe = dir.join("karkinos");
        std::fs::write(&exe, b"original intacto").expect("binario viejo");
        // El hash publicado corresponde a otro contenido.
        let sums = format!("{SHA_EMPTY}  karkinos-linux-x86_64\n");

        let err = install_verified(&exe, b"abc", &sums, "karkinos-linux-x86_64")
            .expect_err("el checksum no coincide");

        assert_eq!(err, "checksum mismatch for karkinos-linux-x86_64");
        assert_eq!(
            std::fs::read(&exe).expect("leer destino"),
            b"original intacto",
            "sin verificación no se toca ni un byte"
        );
        assert!(!dir.join("karkinos.new").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn install_verified_fuera_de_un_directorio_escribible_da_un_error_claro() {
        let dir = temp_dir("install_unwritable");
        let missing = dir.join("no-existe");
        let exe = missing.join("karkinos");
        let sums = format!("{SHA_ABC}  karkinos-linux-x86_64\n");

        let err = install_verified(&exe, b"abc", &sums, "karkinos-linux-x86_64")
            .expect_err("no hay dónde escribir");

        assert_eq!(
            err,
            format!(
                "cannot write {}: reinstall Karkinos or update it with your package manager",
                missing.display()
            )
        );
        assert!(!exe.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn install_with_backup_escribe_el_binario_nuevo() {
        let dir = temp_dir("backup_ok");
        let exe = dir.join("karkinos.exe");
        std::fs::write(&exe, b"viejo").expect("binario viejo");

        install_with_backup(&exe, b"nuevo").expect("reemplazar");

        assert_eq!(std::fs::read(&exe).expect("leer destino"), b"nuevo");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn install_with_backup_hace_rollback_si_falla_la_escritura() {
        let dir = temp_dir("backup_rollback");
        let exe = dir.join("karkinos.exe");
        std::fs::write(&exe, b"viejo").expect("binario viejo");

        // Escritura forzada a fallar: es el único camino al rollback (en un
        // disco sano la escritura del binario no falla).
        let err = install_with_backup_using(&exe, b"nuevo", |_path, _bytes| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "disco lleno",
            ))
        })
        .expect_err("la escritura falla");

        assert!(
            err.starts_with("cannot write "),
            "el error explica el directorio, fue: {err}"
        );
        assert_eq!(
            std::fs::read(&exe).expect("leer destino"),
            b"viejo",
            "el binario viejo vuelve a su nombre original"
        );
        assert!(
            !dir.join("karkinos.exe.old").exists(),
            "el backup se consumió en el rollback"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cleanup_stale_backup_borra_solo_el_archivo_viejo() {
        let dir = temp_dir("cleanup_old");
        let exe = dir.join("karkinos.exe");
        std::fs::write(&exe, b"actual").expect("binario actual");
        let old = dir.join("karkinos.exe.old");
        std::fs::write(&old, b"viejo").expect("backup sobrante");

        assert!(cleanup_stale_backup(&exe), "el .old sobrante se borra al iniciar");
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(&exe).expect("leer binario"),
            b"actual",
            "el ejecutable en uso no se toca"
        );
        assert!(
            !cleanup_stale_backup(&exe),
            "sin .old no hay nada que borrar"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- S2: tipo de instalación ----

    #[test]
    fn install_kind_es_paquete_solo_si_lo_administra_el_gestor() {
        let exe = std::path::Path::new("/usr/bin/karkinos");
        assert_eq!(install_kind(exe, false), InstallKind::Portable);
        assert_eq!(
            install_kind(exe, true),
            InstallKind::Package,
            "el chequeo real de pacman llega en T5"
        );
    }

    // ---- S2: reinicio tras actualizar + instancia única ----

    #[test]
    fn launch_mode_no_reenvia_argumentos_despues_de_una_actualizacion() {
        assert_eq!(launch_mode(&[]), LaunchMode::Forward);
        assert_eq!(launch_mode(&["/proj".to_string()]), LaunchMode::Forward);
        assert_eq!(
            launch_mode(&[AFTER_UPDATE_FLAG.to_string(), "1234".to_string()]),
            LaunchMode::TakeOver {
                previous_pid: Some(1234)
            }
        );
        // Un pid ilegible no cambia la decisión: el proceso nuevo nunca
        // reenvía sus argumentos a la instancia vieja.
        assert_eq!(
            launch_mode(&[AFTER_UPDATE_FLAG.to_string(), "basura".to_string()]),
            LaunchMode::TakeOver { previous_pid: None }
        );
    }

    #[test]
    fn restart_args_conserva_los_argumentos_y_reemplaza_el_flag() {
        assert_eq!(
            restart_args(&[], 42),
            vec![AFTER_UPDATE_FLAG.to_string(), "42".to_string()]
        );
        assert_eq!(
            restart_args(&["/proj".to_string(), "--flag".to_string()], 42),
            vec![
                "/proj".to_string(),
                "--flag".to_string(),
                AFTER_UPDATE_FLAG.to_string(),
                "42".to_string()
            ]
        );
        // Reiniciar varias veces no acumula flags con pids viejos.
        assert_eq!(
            restart_args(
                &[
                    "/proj".to_string(),
                    AFTER_UPDATE_FLAG.to_string(),
                    "7".to_string()
                ],
                42
            ),
            vec![
                "/proj".to_string(),
                AFTER_UPDATE_FLAG.to_string(),
                "42".to_string()
            ]
        );
    }

    // ---- S1: platform_asset_name ----

    #[test]
    fn platform_asset_name_corresponde_a_la_plataforma_del_test() {
        let actual = platform_asset_name();

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(actual, Some("karkinos-linux-x86_64"));

        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(actual, Some("karkinos-windows-x86_64.exe"));

        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64")
        )))]
        assert_eq!(actual, None, "sin binario publicado no hay actualización");
    }
}
