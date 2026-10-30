# Feature: Tree-sitter Semantic AST Engine & Code Folding (Phase 3)

**Objective**:
Equip Karkinos with an industrial syntax engine powered by `tree-sitter`:
1. Incremental AST parsing on top of `karkinos-core` without blocking the main editing loop.
2. Semantic node classification (functions, types, keywords, strings, comments, constants).
3. Code folding ranges calculation (multi-line functions, structs, impl blocks, match statements).
4. Structural code node inspection under cursor.
5. Zero regressions with existing lexical highlighting fallback.

**Branch**: `main`
**Test runner**: `cargo test --workspace`

## Specs

- **S1** — `AstEngine` in `karkinos-core`:
  - Scaffolding of `AstEngine` using `tree-sitter` and `tree-sitter-rust`.
  - Methods for parsing, edits from `TextBuffer`, and syntax trees.
- **S2** — Semantic Highlight Spans & Folding Ranges:
  - Extraction of multi-line block folding ranges `(start_line, end_line)`.
  - Classification of AST nodes into semantic tokens (`Keyword`, `FunctionDef`, `FunctionCall`, `Type`, `String`, `Comment`, etc.).
- **S3** — Document & GUI integration:
  - `Document` integration with `AstEngine`.
  - GUI gutter and syntax styling support for AST-derived semantic tokens and folding markers.
- **S4** — Test verification & performance:
  - Unit tests verifying parsing, folding detection, and node queries.
  - Zero warnings and 100% test pass across workspace (92 tests passing).

## Tasks

| ID | Specs | Route | Status | Commit |
|----|-------|-------|--------|--------|
| T1 | S1 | inline | completado | |
| T2 | S2 | inline | completado | |
| T3 | S3 | inline | completado | |
| T4 | S4 | inline | completado | |

## Log

- **L1** (2026-10-30): Initialized Phase 3 Tree-sitter AST engine task.
- **L2** (2026-10-30): Implemented `AstEngine`, `FoldingRange`, and `AstHighlightSpan` in `karkinos-core`.
- **L3** (2026-10-30): Integrated AST engine into `Document`, `EditorApp`, and `EditorState`.
- **L4** (2026-10-30): Added gutter folding markers and AST-enriched semantic tokenizer in GUI.
- **L5** (2026-10-30): Verified workspace with 92 passing unit tests and 0 compiler warnings.
