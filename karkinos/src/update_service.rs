//! S1 (auto-actualización): chequeo de releases, descarga y estado del
//! botón de update.
//!
//! `UpdateService` sigue el patrón de `extensions_service.rs`: todo lo que
//! puede tardar (pedido a la API de GitHub, descarga del binario) corre en
//! un hilo aparte y el resultado se consume por frame con `try_recv`, así
//! el dibujado nunca se bloquea esperando red. S1 detecta la actualización
//! disponible; T4 (S2/S5) descarga, verifica con `SHA256SUMS` e instala.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use karkinos_core::updater::{
    fetch_latest_release, install_release, is_newer, platform_asset_name, ReleaseInfo,
};

/// S1: cada cuánto se vuelve a consultar el último release publicado.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// S5: nombre del archivo de checksums que publica cada release.
const SUMS_ASSET_NAME: &str = "SHA256SUMS";

/// Resultado de la descarga + instalación de T4. Uno de los dos se convierte
/// en el diálogo modal al terminar (S2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// El binario nuevo ya está en disco: falta reiniciar para estrenarlo.
    Installed { version: String },
    /// No se tocó nada y el mensaje explica por qué (S5).
    Failed { message: String },
}

/// Texto del diálogo que pregunta si reiniciar (S2). Si hay pestañas con
/// cambios sin guardar lo avisa ANTES de reiniciar; un reinicio se lleva
/// puesto lo no guardado.
pub fn restart_prompt(version: &str, dirty_tabs: usize) -> String {
    let mut prompt = format!("Karkinos {} is installed. Restart now?", version);
    if dirty_tabs == 1 {
        prompt.push_str("\n\nYou have 1 tab with unsaved changes.");
    } else if dirty_tabs > 1 {
        prompt.push_str(&format!(
            "\n\nYou have {} tabs with unsaved changes.",
            dirty_tabs
        ));
    }
    prompt
}

/// Estado del chequeo y de la instalación de actualizaciones.
#[derive(Default)]
pub struct UpdateService {
    /// Release más nuevo que el binario en uso, con el asset de esta
    /// plataforma. `None` = no se dibuja el botón (S1).
    available: Option<ReleaseInfo>,
    /// Pedido del usuario esperando que `begin_install` lo ejecute.
    pending: bool,
    /// Descarga + instalación en curso (el botón muestra un spinner y no
    /// se puede volver a apretar).
    installing: bool,
    /// Versión ya instalada en esta sesión: el chequeo de 6 h la vuelve a
    /// ver como "más nueva" (la versión compilada no cambió hasta
    /// reiniciar), así que se recuerda para no reofrecer el mismo binario.
    installed: Option<String>,
    /// Resultado de la última instalación, pendiente de que el usuario
    /// cierre el diálogo ("Restart" / "Later" / "OK").
    result: Option<InstallOutcome>,
    /// Chequeo en curso (hilo que corre `fetch_latest_release`).
    rx: Option<Receiver<Result<ReleaseInfo, String>>>,
    /// Instalación en curso (hilo que corre `install_release`).
    install_rx: Option<Receiver<Result<(), String>>>,
    /// Instante del último chequeo disparado: marca cada 6 horas.
    last_check: Option<Instant>,
}

/// Lo que el hilo de instalación necesita saber, ya validado: sin
/// `SHA256SUMS` publicado no hay nada que descargar (S5).
struct InstallPlan {
    asset_name: String,
    asset_url: String,
    sums_url: String,
}

impl UpdateService {
    /// Versión del release disponible (tal cual el `tag_name`, p. ej.
    /// `v0.2.85`), o `None` si no hay nada nuevo.
    pub fn available_version(&self) -> Option<&str> {
        self.available.as_ref().map(|release| release.version.as_str())
    }

    /// `true` cuando corresponde dibujar el botón de actualización.
    pub fn button_visible(&self) -> bool {
        self.available.is_some()
    }

    /// `true` cuando el usuario ya apretó el botón y el pedido espera que
    /// `begin_install` lo tome.
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// Registra el click del botón. Sin actualización disponible no hay
    /// nada que dejar pendiente.
    pub fn start(&mut self) {
        if self.available.is_some() {
            self.pending = true;
        }
    }

    /// `true` mientras se descarga e instala: el botón muestra un spinner
    /// y queda deshabilitado (S2).
    pub fn installing(&self) -> bool {
        self.installing
    }

    /// `true` si un click puede arrancar la descarga ahora mismo.
    pub fn can_start_install(&self) -> bool {
        self.pending && !self.installing && self.available.is_some()
    }

    /// Resultado de la última instalación, para el diálogo modal.
    pub fn result(&self) -> Option<&InstallOutcome> {
        self.result.as_ref()
    }

    /// Cierra el diálogo del resultado sin descartar la actualización: si
    /// falló, el botón sigue ahí para reintentar.
    pub fn dismiss_result(&mut self) {
        self.result = None;
        self.pending = false;
    }

    /// Arranca la descarga + instalación en un hilo aparte. Devuelve `true`
    /// cuando hubo algo que procesar (para pedir un repintado).
    ///
    /// Un plan inválido (release sin `SHA256SUMS`) se resuelve acá mismo,
    /// sin red: es el camino que usan los tests.
    pub fn begin_install(&mut self) -> bool {
        if !self.can_start_install() {
            return false;
        }
        self.pending = false;

        let plan = match self.install_plan() {
            Ok(plan) => plan,
            Err(message) => {
                self.apply_install_result(Err(message));
                return true;
            }
        };

        self.installing = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.install_rx = Some(rx);
        std::thread::spawn(move || {
            // `owned_by_package` sigue en `false` en T4: el chequeo real de
            // pacman (y la rama que abre la terminal con yay) es T5.
            let result = std::env::current_exe()
                .map_err(|e| format!("Cannot locate the running Karkinos executable: {}", e))
                .and_then(|exe| {
                    install_release(
                        &exe,
                        &plan.asset_url,
                        &plan.sums_url,
                        &plan.asset_name,
                        false,
                    )
                });
            let _ = tx.send(result);
        });
        true
    }

    /// Lo que hay que bajar, o el error que impide instalar (S5: sin
    /// `SHA256SUMS` no se toca nada).
    fn install_plan(&self) -> Result<InstallPlan, String> {
        let Some(release) = self.available.as_ref() else {
            return Err("no update is available".to_string());
        };
        let Some(asset_name) = platform_asset_name() else {
            return Err("this platform has no published binary".to_string());
        };
        let Some(asset_url) = release.platform_asset_url() else {
            return Err(format!("the release has no {} asset", asset_name));
        };
        let Some(sums_url) = release.asset_url(SUMS_ASSET_NAME) else {
            return Err(format!(
                "Karkinos {} does not publish SHA256SUMS: nothing was installed",
                release.version
            ));
        };
        Ok(InstallPlan {
            asset_name: asset_name.to_string(),
            asset_url: asset_url.to_string(),
            sums_url: sums_url.to_string(),
        })
    }

    /// Consume el resultado de la instalación si ya llegó.
    pub fn poll_install(&mut self) -> bool {
        let Some(rx) = self.install_rx.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.apply_install_result(result);
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.apply_install_result(Err("the update download stopped unexpectedly".to_string()));
                true
            }
        }
    }

    /// Aplica el resultado de la instalación. `Ok` deja la versión como
    /// "ya instalada" y oculta el botón; `Err` mantiene la actualización
    /// disponible para reintentar y deja el mensaje para el diálogo.
    pub fn apply_install_result(&mut self, result: Result<(), String>) {
        self.installing = false;
        self.install_rx = None;
        match result {
            Ok(()) => {
                let version = self
                    .available
                    .take()
                    .map(|release| release.version)
                    .unwrap_or_default();
                self.installed = Some(version.clone());
                self.result = Some(InstallOutcome::Installed { version });
            }
            Err(message) => {
                self.result = Some(InstallOutcome::Failed { message });
            }
        }
    }

    /// Aplica el resultado de un chequeo. No toca red ni disco, así los
    /// tests inyectan un release fijo: el botón queda visible solo si la
    /// versión es más nueva Y el release publica el binario de esta
    /// plataforma.
    pub fn apply(&mut self, result: Result<ReleaseInfo, String>) {
        // Sin red, rate limit o error de la API: no se muestra nada ni se
        // interrumpe al usuario. Si ya había una actualización detectada se
        // mantiene: un fallo transitorio no la borra.
        if let Ok(release) = result {
            self.available = if self.offers(&release) {
                Some(release)
            } else {
                None
            };
        }
    }

    /// `true` si ese release se puede ofrecer a este binario.
    fn offers(&self, release: &ReleaseInfo) -> bool {
        // Una versión que acabamos de instalar no se vuelve a ofrecer: hasta
        // reiniciar seguimos comparando contra la versión compilada vieja.
        if self.installed.as_deref() == Some(release.version.as_str()) {
            return false;
        }
        is_newer(&release.version, env!("CARGO_PKG_VERSION"))
            && release.platform_asset_url().is_some()
    }

    /// Dispara el chequeo si toca (al iniciar y luego cada 6 horas). Nunca
    /// bloquea: el pedido HTTP corre en un hilo aparte.
    pub fn check_due(&mut self, now: Instant) {
        if self.rx.is_some() {
            return;
        }
        if let Some(last) = self.last_check {
            if now.saturating_duration_since(last) < CHECK_INTERVAL {
                return;
            }
        }
        self.last_check = Some(now);

        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(fetch_latest_release());
        });
    }

    /// Consume el resultado del chequeo si ya llegó. Devuelve `true` solo
    /// cuando hubo algo que aplicar (para pedir un repintado).
    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.rx.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.rx = None;
                self.apply(result);
                true
            }
            Err(TryRecvError::Empty) => false,
            // El hilo terminó sin mandar nada (no debería pasar): se libera
            // el canal para que el próximo chequeo pueda arrancar.
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                false
            }
        }
    }

    /// Tiempo hasta el próximo chequeo programado, para despertar el bucle
    /// de egui sin repintar de más.
    pub fn next_check_in(&self, now: Instant) -> Option<Duration> {
        let last = self.last_check?;
        Some(CHECK_INTERVAL.saturating_sub(now.saturating_duration_since(last)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karkinos_core::updater::ReleaseAsset;

    /// Release bien formado para los tests: el binario de esta plataforma
    /// más `SHA256SUMS`. Las URLs son `example.invalid` a propósito: ningún
    /// test baja nada (los que llegan a la red usan un plan inválido).
    fn installable_release(version: &str) -> ReleaseInfo {
        let asset = platform_asset_name().expect("esta plataforma tiene binario");
        ReleaseInfo {
            version: version.to_string(),
            assets: vec![
                ReleaseAsset {
                    name: asset.to_string(),
                    download_url: "https://example.invalid/karkinos".to_string(),
                },
                ReleaseAsset {
                    name: "SHA256SUMS".to_string(),
                    download_url: "https://example.invalid/SHA256SUMS".to_string(),
                },
            ],
        }
    }

    #[test]
    fn restart_prompt_dice_la_version_y_avisa_si_hay_cambios_sin_guardar() {
        assert_eq!(
            restart_prompt("v0.2.85", 0),
            "Karkinos v0.2.85 is installed. Restart now?"
        );

        let one = restart_prompt("v0.2.85", 1);
        assert!(
            one.starts_with("Karkinos v0.2.85 is installed. Restart now?"),
            "fue: {one}"
        );
        assert!(one.contains("1 tab with unsaved changes"), "fue: {one}");

        let many = restart_prompt("v0.2.85", 3);
        assert!(many.contains("3 tabs with unsaved changes"), "fue: {many}");
    }

    #[test]
    fn begin_install_solo_cuando_hay_pedido_y_no_hay_descarga_en_curso() {
        if platform_asset_name().is_none() {
            return;
        }
        let mut service = UpdateService::default();
        // Sin actualización disponible no hay nada que descargar.
        assert!(!service.can_start_install());
        assert!(!service.begin_install());
        assert!(!service.installing());

        service.apply(Ok(installable_release("v99.0.0")));
        assert!(
            !service.can_start_install(),
            "sin apretar el botón todavía no hay pedido"
        );
        service.start();
        assert!(service.can_start_install());

        // Mientras descarga el botón no se puede volver a apretar.
        service.installing = true;
        assert!(!service.can_start_install());
        assert!(
            !service.begin_install(),
            "un segundo click no arranca otra descarga"
        );
    }

    #[test]
    fn un_release_sin_sha256sums_falla_sin_tocar_la_red() {
        let Some(asset) = platform_asset_name() else {
            return;
        };
        let mut service = UpdateService::default();
        // Release con el binario pero SIN `SHA256SUMS`: S5 exige no tocar
        // nada y mostrar el error antes de bajar el binario.
        service.apply(Ok(ReleaseInfo {
            version: "v99.0.0".to_string(),
            assets: vec![ReleaseAsset {
                name: asset.to_string(),
                download_url: "https://example.invalid/bin".to_string(),
            }],
        }));
        service.start();

        assert!(service.begin_install(), "el pedido se procesa");
        assert!(!service.installing(), "no hay descarga que esperar");
        match service.result() {
            Some(InstallOutcome::Failed { message }) => {
                assert!(message.contains("SHA256SUMS"), "fue: {message}");
            }
            other => panic!("se esperaba un error visible, fue {other:?}"),
        }
        assert!(
            service.button_visible(),
            "el error no borra la actualización disponible"
        );
    }

    #[test]
    fn una_instalacion_fallida_muestra_el_error_y_deja_reintentar() {
        if platform_asset_name().is_none() {
            return;
        }
        let mut service = UpdateService::default();
        service.apply(Ok(installable_release("v99.0.0")));
        service.start();
        service.installing = true;

        service.apply_install_result(Err("checksum mismatch for karkinos-linux-x86_64".to_string()));

        assert!(!service.installing(), "la descarga terminó");
        assert_eq!(
            service.result(),
            Some(&InstallOutcome::Failed {
                message: "checksum mismatch for karkinos-linux-x86_64".to_string()
            })
        );
        assert!(service.button_visible(), "el botón sigue ahí para reintentar");
        assert_eq!(service.available_version(), Some("v99.0.0"));

        // "OK" cierra el diálogo sin descartar la actualización.
        service.dismiss_result();
        assert!(service.result().is_none());
        assert!(!service.pending());
    }

    #[test]
    fn tras_instalar_no_se_reofrece_la_misma_version() {
        if platform_asset_name().is_none() {
            return;
        }
        let mut service = UpdateService::default();
        service.apply(Ok(installable_release("v99.0.0")));
        assert!(service.button_visible());
        service.installing = true;

        service.apply_install_result(Ok(()));

        assert_eq!(
            service.result(),
            Some(&InstallOutcome::Installed {
                version: "v99.0.0".to_string()
            })
        );
        assert!(
            !service.button_visible(),
            "la versión ya está instalada: el botón se oculta"
        );

        // Aunque el chequeo de 6 h vuelva a ver el mismo release (hasta
        // reiniciar seguimos reportando la versión compilada vieja), no se
        // ofrece otra vez el mismo binario.
        service.apply(Ok(installable_release("v99.0.0")));
        assert!(!service.button_visible());

        // Una versión distinta sí se ofrece.
        service.apply(Ok(installable_release("v99.1.0")));
        assert!(service.button_visible());
    }

    #[test]
    fn next_check_in_espera_seis_horas() {
        let mut service = UpdateService::default();
        let now = Instant::now();

        // Sin chequeo previo no hay despertador que programar.
        assert_eq!(service.next_check_in(now), None);

        // Recién chequeado: el próximo pedido es en 6 horas, y pasado ese
        // plazo no queda espera pendiente (se dispara en el acto).
        service.last_check = Some(now);
        assert_eq!(
            service.next_check_in(now),
            Some(CHECK_INTERVAL),
            "recién chequeado: el próximo es en 6 horas"
        );
        assert!(service
            .next_check_in(now + CHECK_INTERVAL)
            .unwrap()
            .is_zero());
    }

    #[test]
    fn apply_no_borra_una_actualizacion_ya_detectada_por_un_error() {
        let Some(asset) = karkinos_core::updater::platform_asset_name() else {
            return;
        };
        let mut service = UpdateService::default();
        service.apply(Ok(ReleaseInfo {
            version: "v99.0.0".to_string(),
            assets: vec![karkinos_core::updater::ReleaseAsset {
                name: asset.to_string(),
                download_url: "https://example.invalid/bin".to_string(),
            }],
        }));
        assert!(service.button_visible());

        service.apply(Err("network down".to_string()));
        assert!(
            service.button_visible(),
            "un fallo transitorio de red no oculta la actualización ya conocida"
        );
        assert_eq!(service.available_version(), Some("v99.0.0"));
    }
}
