# Roadmap v0.2.50: Smart Delimiters & Auto-Closing Pairs (`()`, `[]`, `{}`, `""`)

## Contexto & Objetivos
Todo editor moderno (Zed, Sublime, VS Code, Helix) asiste al desarrollador al tipear delimitadores estructurales (`(`, `[`, `{`, `"`, `'`). Hasta ahora, Karkinos requería tipear manualmente la apertura y el cierre de cada delimitador, lo que genera fricción y errores de sintaxis comunes.

### Capacidades a Implementar
1. **Auto-cierre al tipear apertura**: Al ingresar `(`, `[`, `{`, `"`, `'`:
   - Inserta automáticamente la pareja de cierre y ubica el cursor en el centro.
2. **Auto-envoltura de selección (Selection Wrap)**:
   - Si hay texto seleccionado y se presiona un delimitador de apertura, el texto seleccionado se envuelve entre los delimitadores en lugar de ser reemplazado (ej. `foo` -> `(foo)`).
3. **Overtyping (Salto sobre el cierre)**:
   - Si el cursor está justo antes de `)`, `]`, `}`, `"`, `'` y el usuario tipea ese carácter de cierre, el editor avanza el cursor sin duplicar el carácter.
4. **Borrado en par con Backspace**:
   - Al presionar Backspace cuando el cursor está entre un par vacío (`(|)`, `[|]`, `{|}`), se eliminan ambos caracteres simultáneamente.
5. **Apertura de bloque con Enter entre llaves (`{|}`)**:
   - Al presionar Enter entre `{` y `}`, el editor formatea el bloque con indentación expandida en 3 líneas, dejando el cursor en el renglón interior.

---

## Tarea A: Motor de Delimitadores en `crates/karkinos-core/src/text_ops.rs`

### Archivos Afectados
- `crates/karkinos-core/src/text_ops.rs`

### Especificación Técnica
1. Implementar funciones puras en `text_ops.rs`:
   ```rust
   pub fn closing_delimiter(open: char) -> Option<char> {
       match open {
           '(' => Some(')'),
           '[' => Some(']'),
           '{' => Some('}'),
           '"' => Some('"'),
           '\'' => Some('\''),
           '`' => Some('`'),
           _ => None,
       }
   }

   pub fn is_closing_delimiter(c: char) -> bool {
       matches!(c, ')' | ']' | '}' | '"' | '\'' | '`')
   }

   pub fn is_empty_pair_around(line: &str, col: usize) -> bool {
       if col == 0 { return false; }
       let chars: Vec<char> = line.chars().collect();
       if col > chars.len() { return false; }
       if let (Some(&before), Some(&after)) = (chars.get(col - 1), chars.get(col)) {
           closing_delimiter(before) == Some(after)
       } else {
           false
       }
   }
   ```
2. Tests unitarios en `crates/karkinos-core/src/text_ops.rs`:
   - `closing_delimiter` mapea paréntesis, corchetes, llaves y comillas.
   - `is_empty_pair_around` detecta pares adyacentes vacíos `()`, `[]`, `{}`.

---

## Tarea B: Integración en el Editor Widget (`karkinos/src/editor_widget.rs`)

### Archivos Afectados
- `karkinos/src/editor_widget.rs`

### Especificación Técnica
1. En `Event::Text(t)`:
   - Si `t` contiene un solo carácter `c`:
     - **Caso Selection Wrap**: Si hay selección (`selection_range(state).is_some()`) y `closing_delimiter(c)` devuelve `Some(close)`:
       - Extrae el texto de la selección.
       - Reemplaza la selección por `format!("{}{}{}", c, sel_text, close)`.
       - Ajusta el cursor y la selección.
     - **Caso Overtyping**: Si no hay selección y `is_closing_delimiter(c)`:
       - Si el carácter siguiente en la línea actual es exactamente `c`:
         - Avanza `state.cursor_col += 1`.
         - Salta la inserción redundante.
     - **Caso Auto-cierre**: Si no hay selección y `closing_delimiter(c)` devuelve `Some(close)`:
       - Si el carácter a la derecha es espacio, fin de línea o delimitador de cierre (no una letra/número):
         - Inserta `format!("{}{}", c, close)`.
         - Coloca el cursor en `(state.cursor_line, state.cursor_col + 1)`.
2. En `Key::Backspace`:
   - Si no hay selección y `is_empty_pair_around(&lines[state.cursor_line], state.cursor_col)`:
     - Borra tanto el carácter anterior como el posterior (rango `(col - 1, col + 1)`).
3. En `Key::Enter`:
   - Si el cursor está justo entre `{` y `}` en la misma línea:
     - Determina la indentación base de la línea.
     - Inserta: `\n` + base + 4 espacios + `\n` + base.
     - Posiciona el cursor en la línea intermedia con la indentación de 4 espacios.
4. Tests unitarios en `editor_widget.rs`:
   - Auto-cierre de `(` inserta `()` y coloca el cursor en medio.
   - Overtyping de `)` avanza sin duplicar.
   - Backspace en `()` borra ambos caracteres.
   - Enter en `{}` genera bloque con indentación.

---

## Tarea C: Versión v0.2.50 y Validación

### Archivos Afectados
- `karkinos/Cargo.toml` (bump a `0.2.50`)
- `crates/karkinos-core/Cargo.toml` (bump a `0.2.50`)
- `scripts/PKGBUILD`
- `docs/index.html`

### Especificación Técnica
1. Sincronizar versión `0.2.50` en Cargo.toml, Cargo.lock, PKGBUILD y docs/index.html.
2. `cargo check --workspace` y `cargo clippy --workspace --all-targets` con 0 warnings.
3. `cargo test --workspace` al 100% verde.
