# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust CLI/TUI (`ratatui` + `crossterm`) that parses a Batocera
`*.core.yml` file (e.g. a libretro core's option definitions) and
renders an interactive, text-only simulation of the resulting
EmulationStation options menu — so edits to these yml files can be
sanity-checked without rebuilding batocera-configgen or booting a
full Batocera image. It is a preview-only validator: it never writes
or persists anything.

Real-world `*.core.yml` fixtures for manual testing live in a sibling
checkout: `../batocera.linux/package/batocera/emulators/...` (137
files as of last count, spanning libretro and standalone emulator
packages, with real schema variance — see `src/model.rs` doc
comments for specifics like `group`/`submenu` aliasing and non-string
choice values).

## Commands

```bash
cargo build                 # debug build
cargo build --release       # release build (use this for manual TUI testing — snappier redraw)
cargo test                  # run all unit tests (pure parsing/menu-tree logic, no terminal needed)
cargo test <test_name>      # run a single test, e.g. `cargo test group_and_submenu_are_aliases`
cargo run -- <path.core.yml>  # launch the TUI against a yml file
```

There's no linter/formatter config beyond the Rust defaults; `cargo fmt`/`cargo clippy` are safe to run if desired but aren't currently enforced by CI.

Since this is a full-screen terminal app, `cargo run` won't behave usefully piped through a plain shell command — drive it interactively, or in a `tmux` session (`tmux new-session -d -s mv -x 120 -y 40 './target/release/menu-viewer <file>'`, then `tmux send-keys`/`capture-pane`) when verifying changes non-interactively.

## Architecture

Four modules, each with a single responsibility:

- **`src/model.rs`** — YAML parsing (`CoreYml`/`CustomFeature` structs) and the pure menu-tree builder (`build_menu`). This is where all the yml-schema-quirk handling lives, documented inline:
  - `submenu:` and `group:` are the same concept (`#[serde(alias = "group")]`); real files use either or both in the same file.
  - `choices` is parsed as `IndexMap<serde_yaml::Value, serde_yaml::Value>`, not `String`, because unquoted scalars in these files can resolve to numbers/bools rather than strings — and the tool deliberately *reproduces* that instead of normalizing it away, since that's exactly the class of authoring bug (forgetting to quote a label) it exists to catch. `yaml_scalar_to_string` is the display-stringification for this.
  - Exactly 2 choices → `Row::Toggle` (flips directly on Enter); 3+ → `Row::Choice` (opens a popup). 0 choices (preset/slider-style options) and `shared_features` both become `Row::Placeholder` (inert, dimmed, not interactive) — sliders and cross-file shared-feature definitions are both out of scope for this tool.
  - Row order matters and is preserved via `indexmap` (not derivable from a plain `serde_yaml::Value`/`BTreeMap`, which would sort alphabetically): ungrouped features in file order, then submenu groups in first-seen order, then `shared_features` appended last.
  - This module's `#[cfg(test)]` block is the primary regression net — it encodes the schema-variance assumptions above as fixtures (order preservation, alias handling, toggle-vs-choice threshold, placeholder fallbacks, scalar coercion). Extend it rather than hand-testing new edge cases only via the TUI.

- **`src/app.rs`** — all interactive state and key handling: a `Vec<MenuLevel>` stack for submenu drill-down/back-navigation, a `HashMap<key, selected_value>` for current selections (initialized to each feature's first choice), and an optional `Popup` for the choice-list overlay. `App::handle_key` is the single entry point for all keyboard behavior (Up/Down/Enter/Esc/Backspace/q); popup key handling is dispatched first and returns early, so popup-open state fully intercepts navigation until dismissed.

- **`src/ui.rs`** — pure rendering (`ratatui`) of whatever `App` currently reports: title/breadcrumb bar, scrollable row list with inline `[current value]`, bottom description pane for the focused row, and a centered popup overlay when one is open. No state lives here — it's a function of `&App`.

- **`src/main.rs`** — CLI arg parsing (`clap`), file load/parse with contextual error messages (`anyhow`), terminal raw-mode/alt-screen setup via a `TerminalGuard` (`Drop` impl restores the terminal on any exit path, including early returns), and the draw/input event loop.

Data flow is one-directional and non-cyclic: `main.rs` parses the yml once into an owned `Vec<Row>` tree (`model.rs`), then `App` (`app.rs`) borrows into that tree for the lifetime of the session — nothing mutates the parsed tree itself, only the selection map and navigation stack change during a run.
