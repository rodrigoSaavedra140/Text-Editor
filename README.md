
classDiagram
    %% ============================================================
    %%  ESTRUCTURAS DE DATOS (Núcleo Rope) — diseño persistente (Rc)
    %% ============================================================

    class Node {
        <<abstract>>
        #weight: usize
        +weight() usize
        +is_leaf() bool
        +len() usize
    }
    note for Node "weight = longitud propia en un Leaf;\nlongitud del subárbol izquierdo en un Internal.\nTodos los métodos son de solo lectura:\nlas ediciones se hacen vía Rope."

    class Leaf {
        -text: String
    }

    class Internal {
        -left: Rc~Node~
        -right: Rc~Node~
    }

    class Rope {
        -root: Rc~Node~
        +new(text: str) Rope
        +insert(pos: usize, text: str) Rope
        +delete(start: usize, end: usize) Rope
        +char_at(index: usize) Option~char~
        +concat(other: Rope) Rope
        +split_at(pos: usize) Tuple~Rope,Rope~
        +rebalance() Rope
        +len() usize
        +is_empty() bool
        +to_string() String
        +line_at(index: usize) usize
        +cursor_at(offset: usize) Cursor
    }
    note for Rope "Persistente: devuelve un Rope NUEVO y comparten (Rc)\nlos subárboles no modificados.\nEstructura de árbol binario balanceado.\nInserción/eliminación: O(log n)"

    class Cursor {
        -offset: usize
        +move_left(n: usize) void
        +move_right(n: usize) void
        +position() usize
    }

    %% ============================================================
    %%  COMPONENTES DEL EDITOR
    %% ============================================================

    class Editor {
        -buffer: Buffer
        -viewport: Viewport
        -renderer: Renderer
        -input_handler: InputHandler
        -mode: EditorMode
        -clipboard: Clipboard
        -undo_stack: UndoStack
        -config: Config
        +open_file(path: Path) Result~void,Error~
        +save_file() Result~void,Error~
        +handle_input(key: KeyEvent) void
        +undo() void
        +redo() void
    }
    note for Editor "Patrón de diseño (MVC simplificado):\nEditor orquesta, Buffer maneja el estado,\nRenderer la presentación,\nInputHandler traduce eventos."

    class Buffer {
        -rope: Rope
        -file_path: Option~PathBuf~
        -dirty: bool
        -modified_at: SystemTime
        +insert(pos: usize, text: str) void
        +delete(start: usize, end: usize) void
        +line(line_num: usize) String
        +line_count() usize
        +is_modified() bool
    }
    note for Buffer "insert/delete llaman a Rope y REEMPLAZAN self.rope.\nMutación a nivel de Buffer,\npersistencia a nivel de Rope."

    class Viewport {
        -top_line: usize
        -left_col: usize
        -width: usize
        -height: usize
        +scroll(delta: isize) void
        +resize(w: usize, h: usize) void
        +visible_lines() Range~usize~
    }

    class Renderer {
        -backend: Box~dyn Backend~
        +render(buffer: Buffer, viewport: Viewport) void
        +draw_status_bar(editor: Editor) void
    }

    class Backend {
        <<interface>>
        +draw_text(x: u16, y: u16, text: str, style: Style) void
        +clear() void
        +flush() void
        +size() Tuple~u16,u16~
    }

    class TerminalBackend {
        -stdout: Stdout
    }

    class Style {
        <<struct>>
        +fg: Color
        +bg: Color
        +bold: bool
    }

    class Color {
        <<enumeration>>
        RESET
        BLACK
        RED
        GREEN
        YELLOW
        BLUE
        WHITE
    }

    class Theme {
        -normal: Style
        -cursor: Style
        -status_bar: Style
    }

    class EditorMode {
        <<enumeration>>
        NORMAL
        INSERT
        VISUAL
        COMMAND
    }

    class InputHandler {
        +read_key() KeyEvent
        +map_key(event: KeyEvent) Command
    }

    class KeyEvent {
        <<struct>>
        +code: KeyCode
        +modifiers: Modifiers
    }

    class KeyCode {
        <<enumeration>>
        CHAR
        ENTER
        ESC
        BACKSPACE
        ARROW
    }

    class Modifiers {
        <<struct>>
        +ctrl: bool
        +alt: bool
        +shift: bool
    }

    class Direction {
        <<enumeration>>
        UP
        DOWN
        LEFT
        RIGHT
    }

    class Command {
        <<enumeration>>
        InsertChar
        DeleteChar
        MoveCursor
        Save
        Quit
        Undo
        Redo
        SwitchMode
    }

    class UndoStack {
        -stack: Vec~Edit~
        -redo_stack: Vec~Edit~
        +push(edit: Edit) void
        +undo() Option~Edit~
        +redo() Option~Edit~
    }

    class Edit {
        <<struct>>
        +kind: EditKind
        +position: usize
        +before: Rope
        +after: Rope
    }
    note for Edit "Guarda directamente el Rope antes/después\n(structural sharing de Rc)."

    class EditKind {
        <<enumeration>>
        INSERT
        DELETE
        REPLACE
    }

    class Clipboard {
        -content: String
        +copy(text: str) void
        +paste() str
    }

    class Config {
        -tab_size: usize
        -line_numbers: bool
        -theme: Theme
    }

    %% ============================================================
    %%  RELACIONES
    %% ============================================================

    Node <|-- Leaf
    Node <|-- Internal

    Internal o-- Node : left
    Internal o-- Node : right
    Rope o-- Node : root
    Rope ..> Cursor : crea
    Edit o-- Rope : before
    Edit o-- Rope : after

    Buffer *-- Rope
    Editor *-- Buffer
    Editor *-- Viewport
    Editor *-- Renderer
    Editor *-- InputHandler
    Editor *-- UndoStack
    Editor *-- Clipboard
    Editor *-- Config
    Editor --> EditorMode : estado actual

    Renderer o-- Backend
    Backend <|.. TerminalBackend
    Backend ..> Style : usa
    Config *-- Theme
    Theme *-- Style

    InputHandler ..> KeyEvent : produce
    InputHandler ..> Command : produce
    Editor ..> Command : ejecuta
    KeyEvent *-- KeyCode
    KeyEvent *-- Modifiers
    KeyCode ..> Direction

    UndoStack *-- Edit
    Edit --> EditKind
