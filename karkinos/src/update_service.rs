//! S1 (auto-actualización): chequeo de releases y estado del botón de update.
//!
//! `UpdateService` sigue el patrón de `extensions_service.rs`: el pedido a
//! la API de GitHub corre en un hilo aparte y el resultado se consume por
//! frame con `try_recv`, así el dibujado nunca se bloquea esperando red.
//! En esta tarea el servicio solo detecta la actualización disponible (S1);
//! descargar, verificar e instalar es T4 (S2/S5).

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use karkinos_core::updater::{fetch_latest_release, is_newer, ReleaseInfo};

/// S1: cada cuánto se vuelve a consultar el último release publicado.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Estado del chequeo de actualizaciones.
#[derive(Default)]
pub struct UpdateService {
    /// Release más nuevo que el binario en uso, con el asset de esta
    /// plataforma. `None` = no se dibuja el botón (S1).
    available: Option<ReleaseInfo>,
    /// Pedido del usuario esperando que T4 lo ejecute.
    pending: bool,
    /// Chequeo en curso (hilo que corre `fetch_latest_release`).
    rx: Option<Receiver<Result<ReleaseInfo, String>>>,
    /// Instante del último chequeo disparado: marca cada 6 horas.
    last_check: Option<Instant>,
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
    /// T4 lo ejecute.
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

    /// Aplica el resultado de un chequeo. No toca red ni disco, así los
    /// tests inyectan un release fijo: el botón queda visible solo si la
    /// versión es más nueva Y el release publica el binario de esta
    /// plataforma.
    pub fn apply(&mut self, result: Result<ReleaseInfo, String>) {
        // Sin red, rate limit o error de la API: no se muestra nada ni se
        // interrumpe al usuario. Si ya había una actualización detectada se
        // mantiene: un fallo transitorio no la borra.
        if let Ok(release) = result {
            self.available = if Self::offers(&release) {
                Some(release)
            } else {
                None
            };
        }
    }

    /// `true` si ese release se puede ofrecer a este binario.
    fn offers(release: &ReleaseInfo) -> bool {
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
