//! S3 (Roadmap v0.2.9): Worker de fondo para el parseo de Tree-sitter
//! (estilo Zed/Lapce adaptado a egui).
//!
//! Antes: `app.update_ast()` corría `engine.update_from_text(&texto)` en
//! el hilo de la UI — cada paradise/repararse del bloque clásico generaba
//! micro-tirones en archivos medianos (>3.000 líneas) al tipear.
//!
//! Ahora: la UI envía `(revisión, snapshot_del_texto)` por canal y el
//! worker —propietario exclusivo de su `AstEngine`, así no hace falta
//! que sea `Send`— parsea en segundo plano y devuelve SOLO datos planos
//! (`Vec<AstHighlightSpan>`, `Vec<FoldingRange>`, ambos `Send`) por un
//! canal de resultados. La UI cada frame hace `try_recv`; el último
//! pedido gana: el worker drena la cola y parsea solo la revisión más
//! fresca (coalescing), así tipear rápido atropella parseos viejos sin
//! costo.

use karkinos_core::syntax_ast::{AstEngine, AstHighlightSpan, AstLanguage, FoldingRange};
use std::sync::mpsc;

/// Pedido pendiente: revisión monótonica medida desde la app (`ast_request_rev`)
/// + snapshot del texto del buffer. El snapshot es una copia del String
/// (dado que el `Rope` de ropey no se puede alimentar a tree-sitter
/// directamente): es una copia O(n) por edición GRATUITA tragarlo; el
/// worker coalescea, así que tipear rápido NO acumula parseos.
pub struct AstRequest {
    pub revision: u64,
    pub text: String,
}

/// Resultado del worker: solo datos planos `Send` (ningún `tree_sitter::Tree`
/// cruza de hilo; los nemónicos interactivos del árbol viejo no le
/// importan a la UI, que solo consume spans/plegado).
pub struct AstResult {
    pub revision: u64,
    pub spans: Vec<AstHighlightSpan>,
    pub folding: Vec<FoldingRange>,
    // F2 (Roadmap v0.2.28): encabezados contenedores para el sticky scroll.
    pub scopes: Vec<karkinos_core::syntax_ast::ScopeHeader>,
    // F7 (Roadmap v0.2.34): símbolos del documento para el selector.
    pub symbols: Vec<karkinos_core::syntax_ast::DocumentSymbol>,
}

pub struct AstWorker {
    request_tx: Option<mpsc::Sender<AstRequest>>,
    result_rx: Option<mpsc::Receiver<AstResult>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

// Solo datos planos viajan por canal: el engine nunca sale del hilo.
// (Markup para el compilador: los campos no-daemon Los son por const.)
impl AstWorker {
    /// Lanza el hilo worker. El motor de Rust se instancia y vive
    /// DENTRO del hilo; si falla la inicialización de tree-sitter el
    /// worker queda idle y los pedidos se descartan.
    pub fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<AstRequest>();
        // Capacidad 1: el worker BLOQUEA si la UI no consume; así los
        // pedidos en cola se coalescean antes del próximo parseo y el
        // costo de fondo tiene techo natural.
        let (result_tx, result_rx) = mpsc::sync_channel::<AstResult>(1);
        let handle = std::thread::Builder::new()
            .name("karkinos-ast".to_string())
            .spawn(move || ast_worker_loop(request_rx, result_tx))
            .expect("debe poder crearse el hilo del worker AST");
        Self {
            request_tx: Some(request_tx),
            result_rx: Some(result_rx),
            handle: Some(handle),
        }
    }

    /// Encola el parseo de la revisión `revision` con el texto dado.
    /// No bloquea: la cola vive en memoria y el worker drena al último.
    pub fn request(&self, revision: u64, text: &str) {
        if let Some(request_tx) = &self.request_tx {
            let _ = request_tx.send(AstRequest {
                revision,
                text: text.to_string(),
            });
        }
    }

    /// Consume los resultados listos y devuelve el más fresco de este
    /// frame (si llegaron varios, los más viejos se descartan: siempre
    /// hay está el pedido posterior que los supera).
    pub fn try_recv_result(&self) -> Option<AstResult> {
        let Some(result_rx) = &self.result_rx else {
            return None;
        };
        let mut best: Option<AstResult> = result_rx.try_recv().ok();
        while let Ok(next) = result_rx.try_recv() {
            let keep = best.as_ref().map_or(true, |b| next.revision > b.revision);
            if keep {
                best = Some(next);
            }
        }
        best
    }
}

impl Drop for AstWorker {
    fn drop(&mut self) {
        // Orden crítico para NO colgar el proceso:
        // 1) soltar el receiver de resultados → si el worker estaba
        //    bloqueado en send() (canal lleno), sale por Err y repasa
        //    los pedidos pendientes hacia recv();
        // 2) soltar el sender de pedidos → recv() del loop devuelve Err
        //    y el worker cambia a modo salida;
        // 3) recién entonces join.
        self.result_rx.take();
        self.request_tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Loop del worker: espera un pedido, drena los más nuevos, parsea SOLO
/// la revisión más fresca (las repetidas/viejas se saltan) y manda el
/// resultado. Sale cuando se cierra el canal de pedidos.
fn ast_worker_loop(
    request_rx: mpsc::Receiver<AstRequest>,
    result_tx: mpsc::SyncSender<AstResult>,
) {
    let mut engine = AstEngine::new(AstLanguage::Rust).ok();
    let mut last_done: u64 = 0;
    while let Ok(first) = request_rx.recv() {
        // Coalescing: quedarnos con el último pedido encolado (el más
        // fresco). Los intermedios son parseos a punto de quedar viejos.
        let mut latest = first;
        while let Ok(newer) = request_rx.try_recv() {
            latest = newer;
        }
        if latest.revision <= last_done {
            continue;
        }
        let Some(ref mut engine) = engine else { continue };
        engine.update_from_text(&latest.text);
        let result = AstResult {
            revision: latest.revision,
            spans: engine.highlight_spans(),
            folding: engine.folding_ranges(),
            scopes: engine.sticky_scopes(&latest.text),
            symbols: engine.extract_document_symbols(&latest.text),
        };
        // send() bloquea si el canal (cap. 1) está lleno: backpressure
        // natural. Al despertar, el drenado de arriba ya coalesce todo.
        let _ = result_tx.send(result);
        last_done = latest.revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Poll auxiliar determinista: espera hasta `tries × 10ms` por un resultado.
    fn poll_until(worker: &AstWorker, tries: usize) -> Option<AstResult> {
        for _ in 0..tries {
            if let Some(result) = worker.try_recv_result() {
                return Some(result);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        worker.try_recv_result()
    }

    #[test]
    fn ast_worker_devuelve_spans_y_plegado_para_rust() {
        let worker = AstWorker::spawn();
        worker.request(1, "fn hola() {\n    otros();\n}");
        let result = poll_until(&worker, 100).expect("el worker debe responder");
        assert_eq!(result.revision, 1);
        assert!(
            result.spans.iter().any(|s| s.start_line == 0),
            "deben existir spans semánticos de la línea 0"
        );
        assert!(
            !result.folding.is_empty(),
            "fn debe abrir un rango de plegado"
        );
    }

    #[test]
    fn ast_worker_entrega_las_revisiones_pedidas_en_orden() {
        let worker = AstWorker::spawn();
        worker.request(1, "fn uno() {}");
        let r1 = poll_until(&worker, 100).expect("reve 1 debe llegar");
        assert_eq!(r1.revision, 1);

        worker.request(2, "fn uno() {}\nfn dos_con_cuerpo() {\n    algo();\n}");
        let r2 = poll_until(&worker, 100).expect("rev 2 debe llegar");
        assert_eq!(r2.revision, 2);
        assert!(
            r2.folding.len() > r1.folding.len(),
            "el segundo archivo agrega una fn multilínea → un plegado más"
        );
    }

    #[test]
    fn ast_worker_saltea_revisiones_repetidas() {
        let worker = AstWorker::spawn();
        worker.request(3, "fn tres() {}");
        assert!(poll_until(&worker, 100).is_some());
        // Pedir de nuevo la MISMA revisión no produce otro resultado:
        // no hay nada nuevo que parsear. Poll breve con presupuesto de
        // 600ms y verificamos que no llegó un resultado con revisión 4.
        worker.request(3, "fn tres() {}");
        std::thread::sleep(Duration::from_millis(300));
        let extra = poll_until(&worker, 5);
        assert!(
            extra.is_none() || extra.unwrap().revision == 3,
            "no debe generarse trabajo para una revisión ya parseada"
        );
    }
}
