use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml::Value;

#[derive(Deserialize, Default)]
pub struct CoreYml {
    #[serde(default)]
    pub shared_features: Vec<String>,
    #[serde(default)]
    pub custom_features: IndexMap<String, CustomFeature>,
}

#[derive(Deserialize)]
pub struct CustomFeature {
    #[serde(alias = "group")]
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
    },
    Choice {
        key: String,
        prompt: String,
        description: Option<String>,
        choices: Vec<(String, String)>,
    },
    Submenu {
        title: String,
        rows: Vec<Row>,
    },
    Placeholder {
        label: String,
        note: &'static str,
    },
}

impl Row {
    pub fn prompt(&self) -> &str {
        match self {
            Row::Toggle { prompt, .. } => prompt,
            Row::Choice { prompt, .. } => prompt,
            Row::Submenu { title, .. } => title,
            Row::Placeholder { label, .. } => label,
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            Row::Toggle { description, .. } => description.as_deref(),
            Row::Choice { description, .. } => description.as_deref(),
            Row::Submenu { .. } => None,
            Row::Placeholder { note, .. } => Some(note),
        }
    }
}

/// Build the top-level menu tree from a parsed core yml, per the ordering
/// rules in the design doc:
/// 1. Ungrouped custom_features (file order) become Toggle/Choice rows.
/// 2. Grouped custom_features are bucketed by first-seen submenu name
///    (file order within each bucket), appended after ungrouped rows in
///    first-seen submenu order.
/// 3. 0-choice custom_features become Placeholder rows in whichever spot
///    they'd otherwise occupy (top-level or within their submenu).
/// 4. shared_features become Placeholder rows appended at the very end.
pub fn build_menu(core: &CoreYml) -> Vec<Row> {
    let mut top_level: Vec<Row> = Vec::new();
    let mut submenu_order: Vec<String> = Vec::new();
    let mut submenu_rows: IndexMap<String, Vec<Row>> = IndexMap::new();

    for (key, feature) in &core.custom_features {
        let row = feature_to_row(key, feature);
        match &feature.submenu {
            Some(name) => {
                submenu_rows.entry(name.clone()).or_insert_with(|| {
                    submenu_order.push(name.clone());
                    Vec::new()
                });
                submenu_rows.get_mut(name).unwrap().push(row);
            }
            None => top_level.push(row),
        }
    }

    for name in submenu_order {
        if let Some(rows) = submenu_rows.shift_remove(&name) {
            top_level.push(Row::Submenu { title: name, rows });
        }
    }

    for shared in &core.shared_features {
        top_level.push(Row::Placeholder {
            label: shared.clone(),
            note: "(shared feature, defined elsewhere)",
        });
    }

    top_level
}

fn feature_to_row(key: &str, feature: &CustomFeature) -> Row {
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
        };
    }

    if choices.len() == 2 {
        let arr = [choices[0].clone(), choices[1].clone()];
        return Row::Toggle {
            key: key.to_string(),
            prompt: feature.prompt.clone(),
            description: feature.description.clone(),
            choices: arr,
        };
    }

    Row::Choice {
        key: key.to_string(),
        prompt: feature.prompt.clone(),
        description: feature.description.clone(),
        choices,
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
        let rows = build_menu(&core);
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
        let rows = build_menu(&core);
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
    fn group_and_submenu_are_aliases() {
        let core = parse(
            r#"
custom_features:
  a:
    submenu: ADVANCED
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
        let rows = build_menu(&core);
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            Row::Submenu { title, rows } => {
                assert_eq!(title, "ADVANCED");
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].prompt(), "A");
                assert_eq!(rows[1].prompt(), "B");
            }
            _ => panic!("expected Submenu row"),
        }
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
        let rows = build_menu(&core);
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
        let rows = build_menu(&core);
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
        let rows = build_menu(&core);
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
        let rows = build_menu(&core);
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
        let rows = build_menu(&core);
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
}
