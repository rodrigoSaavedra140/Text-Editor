//! S1 (Auto-actualización in-app): lógica pura del chequeo de releases.
//!
//! Acá vive todo lo que se puede probar sin red ni disco: comparar
//! versiones, leer la respuesta de la API de GitHub y decidir qué asset
//! corresponde a esta plataforma. La capa fina (`fetch_latest_release`)
//! queda al final del módulo; la UI y la descarga viven en el crate
//! `karkinos`.

use std::time::Duration;

use serde::Deserialize;

/// Endpoint del último release publicado del repositorio.
const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/rodrigoSaavedra140/Text-Editor/releases/latest";
/// Tiempo máximo de espera del chequeo (el hilo de fondo no se cuelga).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
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

/// Agente HTTP que solo habla `https`, también al seguir redirecciones.
/// Repite el de `open_vsx.rs` porque aquel es privado de su módulo.
fn https_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().https_only(true).build()
}

/// Capa fina: consulta el último release publicado. Corre en un hilo de
/// la UI; un error (sin red, rate limit, JSON raro) es solo un `Err` que
/// el servicio descarta en silencio (S1).
pub fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    let body = https_agent()
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
