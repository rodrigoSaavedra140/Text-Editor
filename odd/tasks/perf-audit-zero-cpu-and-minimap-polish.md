# Roadmap v0.2.47: Zero-Alloc & Zero-CPU Polish (Blame Idle, Minimap Sampling, I/O Debounce & Clean Code)

## Contexto & Objetivos
Auditoría integral del rendimiento, ciclo de pintado y calidad del código en Karkinos para garantizar 120+ FPS y verdadero reposo al 0.0% CPU:

1. **Eliminación del Repaint Loop continuo en reposo (Bug de Git Blame)**:
   - En `ui.rs`, `if self.editor_state.blame_text.is_none() && self.doc.current_file.is_some()` pedía un repintado cada 300 ms infinitamente cuando una línea o archivo no tiene commit (archivo nuevo o fuera de Git), impidiendo que el editor entre en reposo.
   - Solución: solo pedir repaint si el cursor se movió hace menos de 300 ms (`cursor_settled_at.elapsed() < 300ms`), calculando el tiempo remanente exacto. Una vez cumplido el dwell, no se pide ningún repaint adicional.

2. **Eliminación de I/O de disco en cada frame (`refresh_git_branch`)**:
   - `refresh_git_branch()` se invocaba en todos los frames del loop `update()`. Si el workspace no es un repositorio Git, `read_branch` devuelve `None`, forzando lecturas a `.git/HEAD` 60-120 veces por segundo.
   - Solución: invocar `refresh_git_branch()` únicamente en `poll_external_changes()` (cadencia de 1.5s) y al abrir/cambiar de carpeta.

3. **Corrección de truncado en el Minimap lateral (Archivos largos)**:
   - En `editor_widget.rs`, el paso vertical del minimap estaba forzado con `step.max(1.0)` y un `break` si `y > minimap_rect.bottom()`, lo que provocaba que archivos con más líneas que la altura de la ventana (ej. > 600 líneas) no mostraran las líneas inferiores en el minimap.
   - Solución: cálculo proporcional continuo donde el minimap cubre del 0% al 100% de las líneas del archivo, mapeando densidades o píxeles a líneas (`(py * total / height)`), renderizando con fidelidad completa documentos de cualquier longitud.

4. **Limpieza de Código y Cero Warnings de Clippy**:
   - Reordenar `util.rs` y `ui.rs` para mover todas las funciones de producción antes de los bloques `mod tests`.
   - Corregir `collapsible_if` en `ui.rs`.
   - Corregir `manual_pattern_char_comparison` y `unnecessary_filter_map` en `karkinos-core`.
   - Limpiar clones superfluos por frame en la UI.

---

## Tarea A: Corrección de CPU Idle y Debounce de I/O

### Archivos Afectados
- `karkinos/src/ui.rs`
- `karkinos/src/app.rs`

### Especificación Técnica
1. En `karkinos/src/ui.rs`:
   - Corregir el repaint del blame:
     ```rust
     if let Some(at) = self.git_status.cursor_settled_at {
         let elapsed = at.elapsed();
         if elapsed < std::time::Duration::from_millis(300) {
             let remaining = std::time::Duration::from_millis(300) - elapsed;
             ctx.request_repaint_after(remaining);
         }
     }
     ```
   - Eliminar `self.refresh_git_branch();` de la llamada por frame en `update()`.
2. En `karkinos/src/app.rs`:
   - En `poll_external_changes(&mut self)`:
     Asegurar que `self.refresh_git_branch()` se llame dentro del bloque periódico.
3. Tests unitarios en `karkinos/src/app.rs`:
   - Verificar que `cursor_settled_at` y el blame dwell no causen repintados tras superar los 300 ms.

---

## Tarea B: Minimap Proporcional sin Truncado

### Archivos Afectados
- `karkinos/src/editor_widget.rs`

### Especificación Técnica
1. En `editor_widget.rs` (renderizado de minimap):
   - Eliminar `step.max(1.0)` y el corte abrupto `break`.
   - Si `lines.len() <= mm_height`:
     - Renderizar cada línea en su posición `minimap_rect.top() + i * step`.
   - Si `lines.len() > mm_height`:
     - Iterar por cada pixel vertical visible del minimap (`0..mm_height as usize`):
     - Mapear el píxel a la línea correspondiente `line_idx = i * lines.len() / mm_height as usize`.
     - Dibujar la barra de sintaxis/longitud correspondiente a esa línea.
2. Tests unitarios:
   - Archivo de 2,000 líneas renderiza el minimap sin truncar las líneas inferiores.
   - El viewport del minimap mantiene el rango proporcional exacto.

---

## Tarea C: Limpieza Integral de Clippy y Refactor Estructural

### Archivos Afectados
- `karkinos/src/util.rs`
- `karkinos/src/ui.rs`
- `crates/karkinos-core/src/lsp.rs`
- `crates/karkinos-core/src/dap.rs`
- `crates/karkinos-core/src/syntax_ast.rs`
- `Cargo.toml` (bump a `v0.2.47`)
- `docs/index.html`

### Especificación Técnica
1. Mover todas las funciones de `util.rs` y `ui.rs` por encima de sus módulos `mod tests`.
2. Resolver todos los `collapsible_if` señalados por clippy en `ui.rs`.
3. Resolver advertencias en `lsp.rs` y `dap.rs`.
4. Sincronizar versión `0.2.47` en Cargo.toml y docs.
5. `cargo clippy --workspace --all-targets` debe finalizar con 0 warnings.
6. `cargo test --workspace` al 100% verde.
