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
comments for specifics like the independent `group`/`submenu`
semantics (a feature may set either, both, or neither — they're not
aliases) and non-string choice values).

## Commands

```bash
cargo build                 # debug build
cargo build --release       # release build (use this for manual TUI testing — snappier redraw)
cargo test                  # run all unit tests (pure parsing/menu-tree logic, no terminal needed)
cargo test <test_name>      # run a single test, e.g. `cargo test group_and_submenu_combine_into_nested_submenu_under_header`
cargo run -- <path.core.yml>  # launch the TUI against a yml file
cargo run -- <path.core.yml> --tree  # dump the menu tree to stdout and exit (no TUI)
cargo run --                # yml_path is optional: auto-detects a single *.yml in the cwd
```

There's no linter/formatter config beyond the Rust defaults; `cargo fmt`/`cargo clippy` are safe to run if desired but aren't currently enforced by CI.

Since this is a full-screen terminal app, `cargo run` won't behave usefully piped through a plain shell command — drive it interactively, or in a `tmux` session (`tmux new-session -d -s mv -x 120 -y 40 './target/release/menu-viewer <file>'`, then `tmux send-keys`/`capture-pane`) when verifying changes non-interactively.

## Architecture

Five modules, each with a single responsibility:

- **`src/model.rs`** — YAML parsing (`CoreYml`/`CustomFeature` structs) and the pure menu-tree builder (`build_menu`). This is where all the yml-schema-quirk handling lives, documented inline:
  - `group:` and `submenu:` are independent fields, not aliases (per EmulationStation's actual `GuiMenu::addFeatures`): `group` produces an inline, non-interactive `Row::GroupHeader` section label clustering the rows that follow it *within the same screen*; `submenu` produces a real drill-down `Row::Submenu` into a separate screen. A feature may set either, both, or neither — real files do all four. Group headers only ever appear at the top level; `build_menu`'s `bucket_by_submenu` helper is applied once for ungrouped features and once per group (scoped to that group's features), so the same submenu name can legitimately produce separate `Row::Submenu`s under different groups.
  - `choices` is parsed as `IndexMap<serde_yaml::Value, serde_yaml::Value>`, not `String`, because unquoted scalars in these files can resolve to numbers/bools rather than strings — and the tool deliberately *reproduces* that instead of normalizing it away, since that's exactly the class of authoring bug (forgetting to quote a label) it exists to catch. `yaml_scalar_to_string` is the display-stringification for this.
  - Exactly 2 choices → `Row::Toggle` (flips directly on Enter); 3+ → `Row::Choice` (opens a popup). 0 choices (preset/slider-style options) and `shared_features` both become `Row::Placeholder` (inert, dimmed, not interactive) — sliders and cross-file shared-feature definitions are both out of scope for this tool. `Row::GroupHeader` is a fifth, distinct row kind: also inert and landable but rendered bold (not dimmed), since it's a real structural element rather than an out-of-scope one.
  - Row order matters and is preserved via `indexmap` (not derivable from a plain `serde_yaml::Value`/`BTreeMap`, which would sort alphabetically): ungrouped-and-unsubmenu'd features in file order, then submenu-bucketed ungrouped features, then each group (first-seen order) as a `GroupHeader` followed by that group's own submenu-bucketed rows, then `shared_features` appended last.
  - This module's `#[cfg(test)]` block is the primary regression net — it encodes the schema-variance assumptions above as fixtures (order preservation, group/submenu independence and combination, toggle-vs-choice threshold, placeholder fallbacks, scalar coercion). Extend it rather than hand-testing new edge cases only via the TUI.

- **`src/app.rs`** — all interactive state and key handling: a `Vec<MenuLevel>` stack for submenu drill-down/back-navigation, a `HashMap<key, selected_value>` for current selections (initialized to each feature's first choice), and an optional `Popup` for the choice-list overlay. `App::handle_key` is the single entry point for all keyboard behavior (Up/Down/Enter/Esc/Backspace/q); popup key handling is dispatched first and returns early, so popup-open state fully intercepts navigation until dismissed.

- **`src/ui.rs`** — pure rendering (`ratatui`) of whatever `App` currently reports: title/breadcrumb bar, scrollable row list with inline `[current value]`, bottom description pane for the focused row, and a centered popup overlay when one is open. No state lives here — it's a function of `&App`. Row kinds get a deliberate minimal accent color in `row_item`: submenu rows are cyan, group headers are bold yellow, the current `[value]` on a Toggle/Choice row is green, placeholders stay dim/uncolored, and the selection highlight (shared by the main list and the popup via `highlight_style()`) is a bold white-on-blue bar that patches over any of the above when a row is focused — pick a color deliberately for any new `Row` variant rather than leaving it flat.

- **`src/tree.rs`** — the `--tree` path: a pure, stateless dump of a `&[Row]` tree to plain text (`tree`-style `├──`/`└──` connectors), colored via `crossterm::style::Stylize` to match `ui.rs`'s color language (cyan submenus, bold yellow group headers, green default choice). Distinct from `ui.rs` because it's a one-shot `println!` of the parsed tree rather than a `Frame`-driven render of live `App` state, and deliberately never touches terminal raw-mode/alt-screen so its output stays pipeable.

- **`src/main.rs`** — CLI arg parsing (`clap`; `yml_path` is optional, auto-detected via `find_single_yml_in` when omitted and exactly one `*.yml` exists in the cwd), file load/parse with contextual error messages (`anyhow`), a `--tree` branch that calls `tree::print_tree` and exits before any terminal setup, terminal raw-mode/alt-screen setup via a `TerminalGuard` (`Drop` impl restores the terminal on any exit path, including early returns), and the draw/input event loop.

Data flow is one-directional and non-cyclic: `main.rs` parses the yml once into an owned `Vec<Row>` tree (`model.rs`), then `App` (`app.rs`) borrows into that tree for the lifetime of the session — nothing mutates the parsed tree itself, only the selection map and navigation stack change during a run.
