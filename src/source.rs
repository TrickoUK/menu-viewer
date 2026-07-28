use crate::model::{CoreYml, CustomFeature};
use anyhow::{Context, Result};
use indexmap::IndexMap;
use std::collections::HashMap;

/// A single `custom_features:` entry's exact location in the raw source
/// text, tracked independent of whether it's currently commented out. This
/// is how a feature disabled in a previous run (or hand-edited following
/// our own comment convention) is detected on load, and how a live toggle
/// later finds the same lines again to flip them back.
pub struct FeatureBlock {
    pub key: String,
    pub start_line: usize,
    pub end_line: usize,
    pub enabled: bool,
}

/// Raw-text view of a `*.core.yml` file, scanned independently of the
/// serde parse (which can never see YAML comments at all). This is the one
/// place in the codebase allowed to reason about line numbers and byte
/// columns instead of parsed structure — everything else stays tree-shaped
/// and never touches source text.
pub struct SourceFile {
    lines: Vec<String>,
    newline: &'static str,
    trailing_newline: bool,
    key_indent: usize,
    blocks: Vec<FeatureBlock>,
}

/// Strip our own `"# "` (or bare `"#"`) disable marker from the start of
/// `s`, if present. This is *our* comment convention, not a general YAML
/// comment parser: disabling a block always inserts the marker at exactly
/// column `key_indent`, so callers only ever apply this after already
/// slicing off the first `key_indent` columns of a line.
fn strip_marker(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix("# ") {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('#') {
        (true, rest)
    } else {
        (false, s)
    }
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// A line is a `custom_features` block start iff, once its indentation and
/// any disable marker are stripped, it's a bare `identifier:` with nothing
/// else on the line. This rejects `prompt: X` (has a value after the
/// colon) outright; deeper fields like `choices:` never reach this check
/// at all because the caller only calls it for lines at exactly
/// `key_indent` — one level shallower than a feature's own fields.
fn bare_key(content: &str) -> Option<&str> {
    let trimmed = content.trim_end();
    let key = trimmed.strip_suffix(':')?;
    if key.is_empty() || key != key.trim() || key.contains(':') {
        return None;
    }
    Some(key)
}

impl SourceFile {
    pub fn scan(text: &str) -> Self {
        let newline: &'static str = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_newline = text.ends_with('\n');
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();

        let mut blocks = Vec::new();
        let mut key_indent = 0usize;

        if let Some(header_idx) = lines.iter().position(|l| l == "custom_features:") {
            if let Some(first) = lines[header_idx + 1..]
                .iter()
                .find(|l| !l.trim().is_empty())
            {
                key_indent = leading_spaces(first);
            }

            let mut current: Option<FeatureBlock> = None;
            let mut i = header_idx + 1;
            while i < lines.len() {
                let line = &lines[i];
                if line.trim().is_empty() {
                    i += 1;
                    continue;
                }
                let indent = leading_spaces(line);
                if indent < key_indent {
                    break;
                }
                if indent == key_indent {
                    let (is_commented, content) = strip_marker(&line[key_indent..]);
                    if let Some(key) = bare_key(content) {
                        if let Some(prev) = current.take() {
                            blocks.push(FeatureBlock {
                                end_line: i,
                                ..prev
                            });
                        }
                        current = Some(FeatureBlock {
                            key: key.to_string(),
                            start_line: i,
                            end_line: i, // fixed up when this block is finalized
                            enabled: !is_commented,
                        });
                    }
                }
                i += 1;
            }
            if let Some(prev) = current.take() {
                blocks.push(FeatureBlock {
                    end_line: i,
                    ..prev
                });
            }
        }

        SourceFile {
            lines,
            newline,
            trailing_newline,
            key_indent,
            blocks,
        }
    }

    pub fn enabled_overrides(&self) -> HashMap<String, bool> {
        self.blocks
            .iter()
            .map(|b| (b.key.clone(), b.enabled))
            .collect()
    }

    /// Reconstruct a `CustomFeature` from a (possibly commented-out)
    /// block by stripping disable markers and dedenting every line by
    /// `key_indent` columns, producing a column-0-anchored YAML fragment
    /// identical in shape to an active block — then delegating entirely
    /// to serde_yaml/`CustomFeature`'s existing `Deserialize` rather than
    /// hand-rolling field extraction.
    pub fn reconstruct_feature(&self, block: &FeatureBlock) -> Result<CustomFeature> {
        let mut fragment = String::new();
        for line in &self.lines[block.start_line..block.end_line] {
            if line.trim().is_empty() {
                fragment.push('\n');
                continue;
            }
            let dedented = if line.len() >= self.key_indent {
                &line[self.key_indent..]
            } else {
                line.as_str()
            };
            let (_, content) = strip_marker(dedented);
            fragment.push_str(content);
            fragment.push('\n');
        }

        let map: IndexMap<String, CustomFeature> = serde_yaml::from_str(&fragment)
            .with_context(|| format!("failed to parse reconstructed feature {:?}", block.key))?;
        map.into_iter()
            .find(|(k, _)| k == &block.key)
            .map(|(_, v)| v)
            .ok_or_else(|| anyhow::anyhow!("key {:?} not found after reconstruction", block.key))
    }

    /// Comment or uncomment a feature's block in place. A no-op if the key
    /// is unknown or already in the requested state. Never inserts or
    /// removes a line, so no other block's line range is ever affected.
    pub fn set_enabled(&mut self, key: &str, enabled: bool) {
        let (start, end, already) = match self.blocks.iter().find(|b| b.key == key) {
            Some(b) => (b.start_line, b.end_line, b.enabled),
            None => return,
        };
        if already == enabled {
            return;
        }

        let key_indent = self.key_indent;
        for line in &mut self.lines[start..end] {
            if line.trim().is_empty() || line.len() < key_indent {
                continue;
            }
            let (before, after) = line.split_at(key_indent);
            let before = before.to_string();
            if enabled {
                if let Some(stripped) = after.strip_prefix("# ") {
                    *line = format!("{before}{stripped}");
                } else if let Some(stripped) = after.strip_prefix('#') {
                    *line = format!("{before}{stripped}");
                }
            } else {
                *line = format!("{before}# {after}");
            }
        }

        if let Some(b) = self.blocks.iter_mut().find(|b| b.key == key) {
            b.enabled = enabled;
        }
    }

    pub fn render(&self) -> String {
        let mut s = self.lines.join(self.newline);
        if self.trailing_newline {
            s.push_str(self.newline);
        }
        s
    }

    /// Rebuild `core.custom_features` in true file order: active blocks
    /// consume serde's already-typed entries, disabled blocks are
    /// reconstructed from raw text. Reconstruction failures are logged to
    /// stderr and skipped rather than a hard crash — an out-of-scope
    /// hand-edited comment shouldn't take the tool down. Anything the
    /// scanner didn't find a block for is appended, never silently
    /// dropped.
    pub fn merge_into(&self, core: &mut CoreYml) {
        let mut merged: IndexMap<String, CustomFeature> = IndexMap::new();
        for block in &self.blocks {
            if block.enabled {
                if let Some(cf) = core.custom_features.shift_remove(&block.key) {
                    merged.insert(block.key.clone(), cf);
                }
            } else {
                match self.reconstruct_feature(block) {
                    Ok(cf) => {
                        merged.insert(block.key.clone(), cf);
                    }
                    Err(e) => eprintln!(
                        "warning: could not parse disabled feature {:?}: {e} (skipping)",
                        block.key
                    ),
                }
            }
        }
        for (k, v) in std::mem::take(&mut core.custom_features) {
            merged.insert(k, v);
        }
        core.custom_features = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature_keys(source: &SourceFile) -> Vec<&str> {
        source.blocks.iter().map(|b| b.key.as_str()).collect()
    }

    #[test]
    fn finds_simple_active_block_range() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
  beta:
    prompt: BETA
    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        assert_eq!(source.key_indent, 2);
        assert_eq!(feature_keys(&source), vec!["alpha", "beta"]);
        assert!(source.blocks.iter().all(|b| b.enabled));

        let alpha = &source.blocks[0];
        assert_eq!((alpha.start_line, alpha.end_line), (1, 6));
        let beta = &source.blocks[1];
        assert_eq!((beta.start_line, beta.end_line), (6, 11));
    }

    #[test]
    fn blank_line_inside_block_is_tolerated() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA

    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        assert_eq!(feature_keys(&source), vec!["alpha"]);
        let alpha = &source.blocks[0];
        assert_eq!((alpha.start_line, alpha.end_line), (1, 7));
    }

    #[test]
    fn blank_lines_between_blocks_do_not_create_phantom_blocks() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled

  beta:
    prompt: BETA
    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        assert_eq!(feature_keys(&source), vec!["alpha", "beta"]);
        // The blank line is absorbed into the preceding block's range.
        assert_eq!(source.blocks[0].end_line, source.blocks[1].start_line);
    }

    #[test]
    fn last_key_in_section_ends_at_next_top_level_key() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        let alpha = &source.blocks[0];
        assert_eq!(alpha.end_line, 6); // the "systems:" line's index
    }

    #[test]
    fn last_key_in_section_ends_at_eof() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
";
        let source = SourceFile::scan(text);
        let alpha = &source.blocks[0];
        assert_eq!(alpha.end_line, source.lines.len());
    }

    #[test]
    fn group_and_submenu_decorated_feature_block_detected_like_any_other() {
        let text = "\
custom_features:
  alpha:
    group: ADVANCED
    submenu: TIMING
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        assert_eq!(feature_keys(&source), vec!["alpha"]);
        assert!(source.blocks[0].enabled);
    }

    #[test]
    fn detects_preexisting_commented_block_and_marks_disabled() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
  # beta:
  #   prompt: BETA
  #   choices:
  #     'Off': disabled
  #     'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        assert_eq!(feature_keys(&source), vec!["alpha", "beta"]);
        assert!(source.blocks[0].enabled);
        assert!(!source.blocks[1].enabled);
    }

    #[test]
    fn reconstruct_feature_from_commented_block_matches_active_equivalent() {
        let commented = "\
custom_features:
  # beta:
  #   group: ADVANCED
  #   prompt: BETA
  #   choices:
  #     'Off': disabled
  #     'On': enabled
";
        let active = "\
custom_features:
  beta:
    group: ADVANCED
    prompt: BETA
    choices:
      'Off': disabled
      'On': enabled
";
        let source = SourceFile::scan(commented);
        let reconstructed = source
            .reconstruct_feature(&source.blocks[0])
            .expect("reconstructs");

        let expected: CoreYml = serde_yaml::from_str(active).unwrap();
        let expected = expected.custom_features.get("beta").unwrap();

        assert_eq!(&reconstructed, expected);
    }

    #[test]
    fn toggle_disable_produces_expected_comment_markers() {
        let text = "\
custom_features:
  alpha:
    group: Beetle Specific
    prompt: OVERCLOCK
    choices:
      50%: 50%
";
        let mut source = SourceFile::scan(text);
        source.set_enabled("alpha", false);
        let rendered = source.render();
        assert_eq!(
            rendered,
            "\
custom_features:
  # alpha:
  #   group: Beetle Specific
  #   prompt: OVERCLOCK
  #   choices:
  #     50%: 50%
"
        );
    }

    #[test]
    fn toggle_uncomment_then_recomment_is_byte_identical_to_original() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
  beta:
    prompt: BETA
    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let mut source = SourceFile::scan(text);
        source.set_enabled("alpha", false);
        assert_ne!(source.render(), text);
        source.set_enabled("alpha", true);
        assert_eq!(source.render(), text);
    }

    #[test]
    fn scan_then_render_with_no_changes_is_identical_to_original() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
systems:
  - foo
";
        let source = SourceFile::scan(text);
        assert_eq!(source.render(), text);
    }

    #[test]
    fn render_preserves_trailing_newline_presence() {
        let with_newline = "custom_features:\n  alpha:\n    prompt: A\n";
        let source = SourceFile::scan(with_newline);
        assert_eq!(source.render(), with_newline);

        let without_newline = "custom_features:\n  alpha:\n    prompt: A";
        let source = SourceFile::scan(without_newline);
        assert_eq!(source.render(), without_newline);
    }

    #[test]
    fn scan_handles_missing_custom_features_section_without_crashing() {
        let text = "systems:\n  - foo\n";
        let source = SourceFile::scan(text);
        assert!(source.blocks.is_empty());
        assert_eq!(source.render(), text);
    }

    #[test]
    fn merge_into_preserves_file_order_including_reactivated_disabled_feature() {
        let text = "\
custom_features:
  alpha:
    prompt: ALPHA
    choices:
      'Off': disabled
      'On': enabled
  # beta:
  #   prompt: BETA
  #   choices:
  #     'Off': disabled
  #     'On': enabled
  gamma:
    prompt: GAMMA
    choices:
      'Off': disabled
      'On': enabled
";
        let mut core: CoreYml = serde_yaml::from_str(text).unwrap();
        assert_eq!(
            core.custom_features.keys().collect::<Vec<_>>(),
            vec!["alpha", "gamma"]
        );

        let source = SourceFile::scan(text);
        source.merge_into(&mut core);

        assert_eq!(
            core.custom_features.keys().collect::<Vec<_>>(),
            vec!["alpha", "beta", "gamma"]
        );
        assert_eq!(core.custom_features.get("beta").unwrap().prompt, "BETA");
    }
}
