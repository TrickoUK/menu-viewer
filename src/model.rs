use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml::Value;
use std::collections::HashMap;

#[derive(Deserialize, Default)]
pub struct CoreYml {
    #[serde(default)]
    pub shared_features: Vec<String>,
    #[serde(default)]
    pub custom_features: IndexMap<String, CustomFeature>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct CustomFeature {
    pub group: Option<String>,
    pub submenu: Option<String>,
    pub prompt: String,
    pub description: Option<String>,
    #[serde(default)]
    pub choices: IndexMap<Value, Value>,
}

/// Stringify a YAML scalar the way it actually resolved (e.g. a bare
/// unquoted `100` or `true` resolves as a number/bool, not the string the
/// author probably meant). Reproducing that rather than silently
/// stringifying every scalar is deliberate: it's exactly the class of
/// authoring bug (forgetting to quote a choice label/value) this tool
/// exists to surface.
pub fn yaml_scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "~".to_string(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

pub enum Row {
    Toggle {
        key: String,
        prompt: String,
        description: Option<String>,
        choices: [(String, String); 2],
        enabled: bool,
    },
    Choice {
        key: String,
        prompt: String,
        description: Option<String>,
        choices: Vec<(String, String)>,
        enabled: bool,
    },
    Submenu {
        title: String,
        rows: Vec<Row>,
    },
    /// An inline, non-interactive section header (from `group:`) that
    /// clusters the rows following it within the *same* screen — distinct
    /// from `Submenu`, which drills into a separate screen. See
    /// `build_menu` for why these never nest inside a `Submenu`.
    GroupHeader {
        title: String,
    },
    Placeholder {
        label: String,
        note: &'static str,
        enabled: bool,
    },
}

impl Row {
    pub fn prompt(&self) -> &str {
        match self {
            Row::Toggle { prompt, .. } => prompt,
            Row::Choice { prompt, .. } => prompt,
            Row::Submenu { title, .. } => title,
            Row::GroupHeader { title } => title,
            Row::Placeholder { label, .. } => label,
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            Row::Toggle { description, .. } => description.as_deref(),
            Row::Choice { description, .. } => description.as_deref(),
            Row::Submenu { .. } => None,
            Row::GroupHeader { .. } => None,
            Row::Placeholder { note, .. } => Some(note),
        }
    }

    /// Whether this row's underlying `custom_features` block is currently
    /// active in the source file (vs. commented out). Always `true` for
    /// row kinds that aren't backed by a single feature block
    /// (`Submenu`/`GroupHeader`).
    pub fn enabled(&self) -> bool {
        match self {
            Row::Toggle { enabled, .. } => *enabled,
            Row::Choice { enabled, .. } => *enabled,
            Row::Placeholder { enabled, .. } => *enabled,
            Row::Submenu { .. } | Row::GroupHeader { .. } => true,
        }
    }

    /// The `custom_features` key backing this row, if any. Only
    /// `Toggle`/`Choice` rows are toggleable via Space, so this is the
    /// gate `App` uses to make Space a no-op on every other row kind.
    pub fn key(&self) -> Option<&str> {
        match self {
            Row::Toggle { key, .. } => Some(key),
            Row::Choice { key, .. } => Some(key),
            Row::Submenu { .. } | Row::GroupHeader { .. } | Row::Placeholder { .. } => None,
        }
    }
}

/// Build the top-level menu tree from a parsed core yml.
///
/// `group` and `submenu` are independent EmulationStation concepts (see
/// `GuiMenu::addFeatures` in es-app), not aliases of each other:
/// - `group` renders as an inline, non-interactive section header
///   clustering the rows that follow it within the *same* screen.
/// - `submenu` renders as a real drill-down into a separate screen.
/// - A feature may set both: it's clustered under its `group` header, and
///   that clustered slot is itself a submenu-drilldown row.
/// - Group headers only ever appear at the top level — EmulationStation's
///   pushed submenu screens add their features flatly and never nest
///   another group header inside, so `submenu` bucketing here is always
///   scoped *within* a single group's slice (or the ungrouped slice), never
///   the reverse.
///
/// Ordering:
/// 1. Ungrouped custom_features are bucketed by first-seen submenu name
///    (file order within each bucket); ungrouped-and-unsubmenu'd features
///    stay as top-level Toggle/Choice/Placeholder rows in file order.
/// 2. Grouped custom_features are bucketed by first-seen group name; each
///    group's cluster is emitted as a GroupHeader followed by that group's
///    own rows, submenu-bucketed exactly as in step 1 but scoped to that
///    group's features. Groups are emitted in first-seen order, after all
///    ungrouped rows.
/// 3. 0-choice custom_features become Placeholder rows in whichever spot
///    they'd otherwise occupy.
/// 4. shared_features become Placeholder rows appended at the very end,
///    unaffected by any group/submenu on custom_features.
pub fn build_menu(core: &CoreYml, enabled: &HashMap<String, bool>) -> Vec<Row> {
    let mut group_order: Vec<String> = Vec::new();
    let mut grouped: IndexMap<String, Vec<(&String, &CustomFeature)>> = IndexMap::new();
    let mut ungrouped: Vec<(&String, &CustomFeature)> = Vec::new();

    for (key, feature) in &core.custom_features {
        match &feature.group {
            Some(name) => {
                grouped.entry(name.clone()).or_insert_with(|| {
                    group_order.push(name.clone());
                    Vec::new()
                });
                grouped.get_mut(name).unwrap().push((key, feature));
            }
            None => ungrouped.push((key, feature)),
        }
    }

    let mut top_level: Vec<Row> = bucket_by_submenu(ungrouped, enabled);

    for name in group_order {
        if let Some(features) = grouped.shift_remove(&name) {
            top_level.push(Row::GroupHeader { title: name });
            top_level.extend(bucket_by_submenu(features, enabled));
        }
    }

    for shared in &core.shared_features {
        top_level.push(Row::Placeholder {
            label: shared.clone(),
            note: "(shared feature, defined elsewhere)",
            enabled: true,
        });
    }

    top_level
}

/// Bucket a flat slice of (key, feature) pairs by first-seen `submenu`
/// name (file order preserved within each bucket and among ungrouped
/// rows). Scoped per-group by `build_menu` (and applied once to the
/// ungrouped slice), since submenus never nest inside a group differently
/// than described above.
fn bucket_by_submenu<'a>(
    features: Vec<(&'a String, &'a CustomFeature)>,
    enabled: &HashMap<String, bool>,
) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut submenu_order: Vec<String> = Vec::new();
    let mut submenu_rows: IndexMap<String, Vec<Row>> = IndexMap::new();

    for (key, feature) in features {
        let row = feature_to_row(key, feature, enabled);
        match &feature.submenu {
            Some(name) => {
                submenu_rows.entry(name.clone()).or_insert_with(|| {
                    submenu_order.push(name.clone());
                    Vec::new()
                });
                submenu_rows.get_mut(name).unwrap().push(row);
            }
            None => rows.push(row),
        }
    }

    for name in submenu_order {
        if let Some(sub_rows) = submenu_rows.shift_remove(&name) {
            rows.push(Row::Submenu {
                title: name,
                rows: sub_rows,
            });
        }
    }

    rows
}

fn feature_to_row(key: &str, feature: &CustomFeature, enabled: &HashMap<String, bool>) -> Row {
    let is_enabled = enabled.get(key).copied().unwrap_or(true);

    let choices: Vec<(String, String)> = feature
        .choices
        .iter()
        .map(|(label, value)| {
            (
                yaml_scalar_to_string(label),
                yaml_scalar_to_string(value),
            )
        })
        .collect();

    if choices.is_empty() {
        return Row::Placeholder {
            label: feature.prompt.clone(),
            note: "(preset-based option, not shown)",
            enabled: is_enabled,
        };
    }

    if choices.len() == 2 {
        let arr = [choices[0].clone(), choices[1].clone()];
        return Row::Toggle {
            key: key.to_string(),
            prompt: feature.prompt.clone(),
            description: feature.description.clone(),
            choices: arr,
            enabled: is_enabled,
        };
    }

    Row::Choice {
        key: key.to_string(),
        prompt: feature.prompt.clone(),
        description: feature.description.clone(),
        choices,
        enabled: is_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> CoreYml {
        serde_yaml::from_str(yaml).expect("valid yaml")
    }

    #[test]
    fn preserves_custom_feature_order() {
        let core = parse(
            r#"
custom_features:
  zeta:
    prompt: ZETA
    choices:
      'Off': disabled
      'On': enabled
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows[0].prompt(), "ZETA");
        assert_eq!(rows[1].prompt(), "ALPHA");
    }

    #[test]
    fn preserves_choice_order() {
        let core = parse(
            r#"
custom_features:
  res:
    prompt: RES
    choices:
      1x: one
      2x: two
      4x: four
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        match &rows[0] {
            Row::Choice { choices, .. } => {
                assert_eq!(choices[0].0, "1x");
                assert_eq!(choices[1].0, "2x");
                assert_eq!(choices[2].0, "4x");
            }
            _ => panic!("expected Choice row"),
        }
    }

    #[test]
    fn group_creates_header_and_clusters_rows() {
        let core = parse(
            r#"
custom_features:
  a:
    group: ADVANCED
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
  b:
    group: ADVANCED
    prompt: B
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows.len(), 3);
        match &rows[0] {
            Row::GroupHeader { title } => assert_eq!(title, "ADVANCED"),
            _ => panic!("expected GroupHeader row"),
        }
        assert_eq!(rows[1].prompt(), "A");
        assert_eq!(rows[2].prompt(), "B");
    }

    #[test]
    fn group_order_is_first_seen_and_independent_of_submenu_order() {
        let core = parse(
            r#"
custom_features:
  a:
    group: BETA
    submenu: SUBX
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
  b:
    group: ALPHA
    submenu: SUBY
    prompt: B
    choices:
      'Off': disabled
      'On': enabled
  c:
    group: BETA
    submenu: SUBY
    prompt: C
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows.len(), 5);

        match &rows[0] {
            Row::GroupHeader { title } => assert_eq!(title, "BETA"),
            _ => panic!("expected GroupHeader row"),
        }
        match &rows[1] {
            Row::Submenu { title, rows } => {
                assert_eq!(title, "SUBX");
                assert_eq!(rows[0].prompt(), "A");
            }
            _ => panic!("expected Submenu row"),
        }
        match &rows[2] {
            Row::Submenu { title, rows } => {
                assert_eq!(title, "SUBY");
                assert_eq!(rows[0].prompt(), "C");
            }
            _ => panic!("expected Submenu row"),
        }
        match &rows[3] {
            Row::GroupHeader { title } => assert_eq!(title, "ALPHA"),
            _ => panic!("expected GroupHeader row"),
        }
        match &rows[4] {
            Row::Submenu { title, rows } => {
                assert_eq!(title, "SUBY");
                assert_eq!(rows[0].prompt(), "B");
            }
            _ => panic!("expected Submenu row"),
        }
    }

    #[test]
    fn group_and_submenu_combine_into_nested_submenu_under_header() {
        let core = parse(
            r#"
custom_features:
  a:
    group: ADVANCED
    submenu: TIMING
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
  b:
    group: ADVANCED
    submenu: TIMING
    prompt: B
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows.len(), 2);
        match &rows[0] {
            Row::GroupHeader { title } => assert_eq!(title, "ADVANCED"),
            _ => panic!("expected GroupHeader row"),
        }
        match &rows[1] {
            Row::Submenu { title, rows } => {
                assert_eq!(title, "TIMING");
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].prompt(), "A");
                assert_eq!(rows[1].prompt(), "B");
            }
            _ => panic!("expected Submenu row"),
        }
    }

    #[test]
    fn ungrouped_feature_unaffected_by_sibling_group() {
        let core = parse(
            r#"
custom_features:
  plain:
    prompt: PLAIN
    choices:
      'Off': disabled
      'On': enabled
  grouped:
    group: ADVANCED
    prompt: GROUPED
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].prompt(), "PLAIN");
        match &rows[1] {
            Row::GroupHeader { title } => assert_eq!(title, "ADVANCED"),
            _ => panic!("expected GroupHeader row"),
        }
        assert_eq!(rows[2].prompt(), "GROUPED");
    }

    #[test]
    fn shared_features_remain_trailing_placeholders_with_groups_and_submenus() {
        let core = parse(
            r#"
shared_features:
  - autosave
  - use_guns
custom_features:
  a:
    group: ADVANCED
    submenu: TIMING
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0], Row::GroupHeader { .. }));
        assert!(matches!(rows[1], Row::Submenu { .. }));
        assert_eq!(rows[2].prompt(), "autosave");
        assert_eq!(rows[3].prompt(), "use_guns");
        assert!(matches!(rows[2], Row::Placeholder { .. }));
        assert!(matches!(rows[3], Row::Placeholder { .. }));
    }

    #[test]
    fn two_choices_is_toggle_three_plus_is_choice() {
        let core = parse(
            r#"
custom_features:
  t:
    prompt: T
    choices:
      'Off': disabled
      'On': enabled
  c:
    prompt: C
    choices:
      A: a
      B: b
      C: c
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert!(matches!(rows[0], Row::Toggle { .. }));
        assert!(matches!(rows[1], Row::Choice { .. }));
    }

    #[test]
    fn zero_choices_becomes_placeholder() {
        let core = parse(
            r#"
custom_features:
  slider:
    prompt: SLIDER
    preset: something
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert!(matches!(rows[0], Row::Placeholder { .. }));
    }

    #[test]
    fn missing_description_is_none() {
        let core = parse(
            r#"
custom_features:
  a:
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows[0].description(), None);
    }

    #[test]
    fn shared_features_become_trailing_placeholders() {
        let core = parse(
            r#"
shared_features:
  - autosave
  - use_guns
custom_features:
  a:
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].prompt(), "autosave");
        assert_eq!(rows[2].prompt(), "use_guns");
        assert!(matches!(rows[1], Row::Placeholder { .. }));
    }

    #[test]
    fn unquoted_scalars_are_coerced_and_reproduced_not_hidden() {
        // A choice author forgetting to quote a label/value that looks
        // like a number or bool is a real authoring bug in these yml
        // files. serde_yaml resolves such bare scalars to Number/Bool
        // (not YAML-1.1 extras like sexagesimal `16:9` or `on`/`off`,
        // which this parser does not implement) — the tool should show
        // the resolved value, not paper over it.
        let core = parse(
            r#"
custom_features:
  numeric:
    prompt: NUMERIC
    choices:
      Auto: auto
      100: 100
      200: 200
  boolflag:
    prompt: BOOLFLAG
    choices:
      'Off': false
      'On': true
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        match &rows[0] {
            Row::Choice { choices, .. } => {
                assert_eq!(choices[0], ("Auto".to_string(), "auto".to_string()));
                assert_eq!(choices[1], ("100".to_string(), "100".to_string()));
                assert_eq!(choices[2], ("200".to_string(), "200".to_string()));
            }
            _ => panic!("expected Choice row"),
        }
        match &rows[1] {
            Row::Toggle { choices, .. } => {
                assert_eq!(choices[0], ("Off".to_string(), "false".to_string()));
                assert_eq!(choices[1], ("On".to_string(), "true".to_string()));
            }
            _ => panic!("expected Toggle row"),
        }
    }

    #[test]
    fn enabled_override_marks_toggle_row_disabled() {
        let core = parse(
            r#"
custom_features:
  a:
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let mut overrides = HashMap::new();
        overrides.insert("a".to_string(), false);
        let rows = build_menu(&core, &overrides);
        assert!(!rows[0].enabled());
    }

    #[test]
    fn missing_key_in_overrides_defaults_to_enabled() {
        let core = parse(
            r#"
custom_features:
  a:
    prompt: A
    choices:
      'Off': disabled
      'On': enabled
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert!(rows[0].enabled());
        assert_eq!(rows[0].key(), Some("a"));
    }

    #[test]
    fn enabled_override_applies_to_choice_and_placeholder_rows() {
        let core = parse(
            r#"
custom_features:
  c:
    prompt: C
    choices:
      A: a
      B: b
      C: c
  p:
    prompt: P
    preset: something
"#,
        );
        let mut overrides = HashMap::new();
        overrides.insert("c".to_string(), false);
        overrides.insert("p".to_string(), false);
        let rows = build_menu(&core, &overrides);

        assert!(matches!(rows[0], Row::Choice { .. }));
        assert!(!rows[0].enabled());
        assert!(matches!(rows[1], Row::Placeholder { .. }));
        assert!(!rows[1].enabled());
        // Placeholder rows aren't backed by a toggleable key.
        assert_eq!(rows[1].key(), None);
    }

    #[test]
    fn shared_feature_placeholders_are_always_enabled() {
        let core = parse(
            r#"
shared_features:
  - autosave
"#,
        );
        let rows = build_menu(&core, &HashMap::new());
        assert!(rows[0].enabled());
    }
}
