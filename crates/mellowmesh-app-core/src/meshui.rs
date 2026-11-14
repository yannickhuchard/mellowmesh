//! MeshUI: declarative cards agents attach to messages, rendered by each OS
//! with its native components (SwiftUI, Compose, WinUI).
//!
//! A card travels in a message's `payload.mesh_ui`. It is **data, never
//! code**: there is no script, no HTML, no remote layout. Parsing is strict —
//! unknown fields, unknown block types, oversized strings, non-finite
//! numbers, non-https images and script-like content are all rejected, and a
//! message whose card fails validation is shown without the card.
//!
//! [`UiAction`]s never answer decisions. Tapping one publishes a plain reply
//! message (`payload.ui_action`) into the same conversation; the agent that
//! sent the card decides what it means. Decisions are answered only through
//! `MeshSession::respond_decision`, behind OS biometrics.

use crate::error::MeshError;
use mellowmesh_client::sanitize::sanitize_text;
use serde::Deserialize;

/// Current (and only) MeshUI schema version.
pub const MESH_UI_VERSION: u32 = 1;
pub const MAX_CARD_JSON_BYTES: usize = 64 * 1024;
pub const MAX_BLOCKS: usize = 32;
const MAX_TITLE: usize = 120;
const MAX_MARKDOWN: usize = 4000;
const MAX_LABEL: usize = 120;
const MAX_KV_PAIRS: usize = 32;
const MAX_KEY: usize = 64;
const MAX_VALUE: usize = 256;
const MAX_METRIC_LABEL: usize = 64;
const MAX_METRIC_VALUE: usize = 32;
const MAX_LIST_ITEMS: usize = 50;
const MAX_LIST_ITEM: usize = 256;
const MAX_SERIES: usize = 500;
const MAX_CHART_LABEL: usize = 32;
const MAX_URL: usize = 2048;
const MAX_ALT: usize = 256;
const MAX_ACTIONS: usize = 6;
const MAX_ACTION_ID: usize = 64;
const MAX_ACTION_LABEL: usize = 40;

/// A validated card.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MeshUiCard {
    pub version: u32,
    pub title: String,
    pub blocks: Vec<UiBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KeyValuePair {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ChartKind {
    Line,
    Bar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UiActionStyle {
    Primary,
    Secondary,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UiAction {
    pub id: String,
    pub label: String,
    pub style: UiActionStyle,
}

/// One block of a card. Shells render each variant with a native component.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum UiBlock {
    /// CommonMark subset: emphasis, code, lists, links. Shells MUST NOT
    /// render raw HTML or auto-load images referenced from markdown.
    Text {
        markdown: String,
    },
    KeyValue {
        pairs: Vec<KeyValuePair>,
    },
    /// `value` in `0.0..=1.0`.
    Progress {
        label: String,
        value: f64,
    },
    /// A KPI tile: preformatted value (e.g. "98.2%") and optional delta.
    Metric {
        label: String,
        value: String,
        delta: Option<String>,
    },
    List {
        items: Vec<String>,
    },
    /// `labels` is empty or has exactly one entry per point of `series`.
    Chart {
        kind: ChartKind,
        series: Vec<f64>,
        labels: Vec<String>,
    },
    /// https only.
    Image {
        url: String,
        alt: String,
    },
    Actions {
        actions: Vec<UiAction>,
    },
}

// ---- wire format -------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCard {
    version: u32,
    #[serde(default)]
    title: String,
    blocks: Vec<WireBlock>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePair {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAction {
    id: String,
    label: String,
    #[serde(default)]
    style: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireBlock {
    Text {
        markdown: String,
    },
    KeyValue {
        pairs: Vec<WirePair>,
    },
    Progress {
        label: String,
        value: f64,
    },
    Metric {
        label: String,
        value: String,
        #[serde(default)]
        delta: Option<String>,
    },
    List {
        items: Vec<String>,
    },
    Chart {
        kind: String,
        series: Vec<f64>,
        #[serde(default)]
        labels: Vec<String>,
    },
    Image {
        url: String,
        alt: String,
    },
    Actions {
        actions: Vec<WireAction>,
    },
}

/// Parse and strictly validate a MeshUI card from JSON.
#[uniffi::export]
pub fn parse_ui_card(json: String) -> Result<MeshUiCard, MeshError> {
    if json.len() > MAX_CARD_JSON_BYTES {
        return Err(MeshError::invalid(format!(
            "MeshUI card exceeds {MAX_CARD_JSON_BYTES} bytes"
        )));
    }
    let wire: WireCard =
        serde_json::from_str(&json).map_err(|e| MeshError::invalid(format!("MeshUI card: {e}")))?;
    validate(wire)
}

/// Parse a card from an already-decoded `payload.mesh_ui` value.
pub(crate) fn card_from_value(value: &serde_json::Value) -> Result<MeshUiCard, MeshError> {
    let json = serde_json::to_string(value).map_err(|e| MeshError::invalid(e.to_string()))?;
    parse_ui_card(json)
}

fn check_text(field: &str, s: &str, max: usize, allow_empty: bool) -> Result<String, MeshError> {
    if !allow_empty && s.trim().is_empty() {
        return Err(MeshError::invalid(format!(
            "MeshUI: {field} must not be empty"
        )));
    }
    if s.chars().count() > max {
        return Err(MeshError::invalid(format!(
            "MeshUI: {field} longer than {max} characters"
        )));
    }
    let lower = s.to_lowercase();
    const FORBIDDEN: [&str; 9] = [
        "<script",
        "</script",
        "<iframe",
        "<object",
        "<embed",
        "javascript:",
        "vbscript:",
        "data:text/html",
        "<svg",
    ];
    if let Some(bad) = FORBIDDEN.iter().find(|p| lower.contains(*p)) {
        return Err(MeshError::invalid(format!(
            "MeshUI: {field} contains forbidden content '{bad}'"
        )));
    }
    if contains_html_tag(&lower) {
        return Err(MeshError::invalid(format!(
            "MeshUI: {field} contains raw HTML (not allowed)"
        )));
    }
    Ok(sanitize_text(s))
}

/// True when `s` contains something that looks like an HTML tag, comment or
/// processing instruction: `<` directly followed by a letter, `/`, `!` or
/// `?`. Markdown autolinks (`<https://…>`) are allowed; `a < b` is prose.
fn contains_html_tag(lower: &str) -> bool {
    lower.match_indices('<').any(|(i, _)| {
        let rest = &lower[i + 1..];
        let starts_tag = rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'));
        starts_tag && !rest.starts_with("https://")
    })
}

fn check_https_url(url_str: &str) -> Result<String, MeshError> {
    if url_str.len() > MAX_URL {
        return Err(MeshError::invalid("MeshUI: image url too long"));
    }
    let url = url::Url::parse(url_str)
        .map_err(|e| MeshError::invalid(format!("MeshUI: invalid image url: {e}")))?;
    if url.scheme() != "https" {
        return Err(MeshError::invalid("MeshUI: image url must be https"));
    }
    if url.host_str().is_none_or(|h| h.is_empty()) {
        return Err(MeshError::invalid("MeshUI: image url has no host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(MeshError::invalid(
            "MeshUI: image url must not carry credentials",
        ));
    }
    Ok(url.to_string())
}

fn validate(wire: WireCard) -> Result<MeshUiCard, MeshError> {
    if wire.version != MESH_UI_VERSION {
        return Err(MeshError::invalid(format!(
            "MeshUI: unsupported version {} (expected {MESH_UI_VERSION})",
            wire.version
        )));
    }
    if wire.blocks.is_empty() {
        return Err(MeshError::invalid(
            "MeshUI: a card needs at least one block",
        ));
    }
    if wire.blocks.len() > MAX_BLOCKS {
        return Err(MeshError::invalid(format!(
            "MeshUI: more than {MAX_BLOCKS} blocks"
        )));
    }
    let title = check_text("title", &wire.title, MAX_TITLE, true)?;
    let mut action_blocks = 0;
    let mut blocks = Vec::with_capacity(wire.blocks.len());
    for block in wire.blocks {
        blocks.push(match block {
            WireBlock::Text { markdown } => UiBlock::Text {
                markdown: check_text("text.markdown", &markdown, MAX_MARKDOWN, false)?,
            },
            WireBlock::KeyValue { pairs } => {
                if pairs.is_empty() || pairs.len() > MAX_KV_PAIRS {
                    return Err(MeshError::invalid(format!(
                        "MeshUI: key_value needs 1..={MAX_KV_PAIRS} pairs"
                    )));
                }
                let pairs = pairs
                    .into_iter()
                    .map(|p| {
                        Ok(KeyValuePair {
                            key: check_text("key_value.key", &p.key, MAX_KEY, false)?,
                            value: check_text("key_value.value", &p.value, MAX_VALUE, true)?,
                        })
                    })
                    .collect::<Result<Vec<_>, MeshError>>()?;
                UiBlock::KeyValue { pairs }
            }
            WireBlock::Progress { label, value } => {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err(MeshError::invalid(
                        "MeshUI: progress.value must be within 0..=1",
                    ));
                }
                UiBlock::Progress {
                    label: check_text("progress.label", &label, MAX_LABEL, true)?,
                    value,
                }
            }
            WireBlock::Metric {
                label,
                value,
                delta,
            } => UiBlock::Metric {
                label: check_text("metric.label", &label, MAX_METRIC_LABEL, false)?,
                value: check_text("metric.value", &value, MAX_METRIC_VALUE, false)?,
                delta: delta
                    .map(|d| check_text("metric.delta", &d, MAX_METRIC_VALUE, true))
                    .transpose()?,
            },
            WireBlock::List { items } => {
                if items.is_empty() || items.len() > MAX_LIST_ITEMS {
                    return Err(MeshError::invalid(format!(
                        "MeshUI: list needs 1..={MAX_LIST_ITEMS} items"
                    )));
                }
                let items = items
                    .iter()
                    .map(|i| check_text("list.item", i, MAX_LIST_ITEM, false))
                    .collect::<Result<Vec<_>, _>>()?;
                UiBlock::List { items }
            }
            WireBlock::Chart {
                kind,
                series,
                labels,
            } => {
                let kind = match kind.as_str() {
                    "line" => ChartKind::Line,
                    "bar" => ChartKind::Bar,
                    other => {
                        return Err(MeshError::invalid(format!(
                            "MeshUI: unknown chart kind '{other}'"
                        )))
                    }
                };
                if series.is_empty() || series.len() > MAX_SERIES {
                    return Err(MeshError::invalid(format!(
                        "MeshUI: chart needs 1..={MAX_SERIES} points"
                    )));
                }
                if series.iter().any(|v| !v.is_finite()) {
                    return Err(MeshError::invalid("MeshUI: chart values must be finite"));
                }
                if !labels.is_empty() && labels.len() != series.len() {
                    return Err(MeshError::invalid(
                        "MeshUI: chart labels must match the series length",
                    ));
                }
                let labels = labels
                    .iter()
                    .map(|l| check_text("chart.label", l, MAX_CHART_LABEL, true))
                    .collect::<Result<Vec<_>, _>>()?;
                UiBlock::Chart {
                    kind,
                    series,
                    labels,
                }
            }
            WireBlock::Image { url, alt } => UiBlock::Image {
                url: check_https_url(&url)?,
                alt: check_text("image.alt", &alt, MAX_ALT, false)?,
            },
            WireBlock::Actions { actions } => {
                action_blocks += 1;
                if action_blocks > 1 {
                    return Err(MeshError::invalid("MeshUI: at most one actions block"));
                }
                if actions.is_empty() || actions.len() > MAX_ACTIONS {
                    return Err(MeshError::invalid(format!(
                        "MeshUI: actions needs 1..={MAX_ACTIONS} actions"
                    )));
                }
                let mut ids = std::collections::HashSet::new();
                let mut out = Vec::with_capacity(actions.len());
                for a in actions {
                    if !is_valid_action_id(&a.id) {
                        return Err(MeshError::invalid(format!(
                            "MeshUI: invalid action id '{}'",
                            a.id
                        )));
                    }
                    if !ids.insert(a.id.clone()) {
                        return Err(MeshError::invalid(format!(
                            "MeshUI: duplicate action id '{}'",
                            a.id
                        )));
                    }
                    let style = match a.style.as_deref() {
                        None | Some("secondary") => UiActionStyle::Secondary,
                        Some("primary") => UiActionStyle::Primary,
                        Some("destructive") => UiActionStyle::Destructive,
                        Some(other) => {
                            return Err(MeshError::invalid(format!(
                                "MeshUI: unknown action style '{other}'"
                            )))
                        }
                    };
                    out.push(UiAction {
                        label: check_text("action.label", &a.label, MAX_ACTION_LABEL, false)?,
                        id: a.id,
                        style,
                    });
                }
                UiBlock::Actions { actions: out }
            }
        });
    }
    Ok(MeshUiCard {
        version: wire.version,
        title,
        blocks,
    })
}

pub(crate) fn is_valid_action_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ACTION_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{
      "version": 1,
      "title": "Nightly build",
      "blocks": [
        {"type": "text", "markdown": "**Green** on `main`"},
        {"type": "key_value", "pairs": [{"key": "commit", "value": "7330242"}]},
        {"type": "progress", "label": "Tests", "value": 0.75},
        {"type": "metric", "label": "Coverage", "value": "82.4%", "delta": "+1.2%"},
        {"type": "list", "items": ["lint ok", "unit ok"]},
        {"type": "chart", "kind": "line", "series": [1, 2.5, 3], "labels": ["a", "b", "c"]},
        {"type": "image", "url": "https://example.com/graph.png", "alt": "graph"},
        {"type": "actions", "actions": [
          {"id": "retry", "label": "Retry", "style": "primary"},
          {"id": "drop.cache", "label": "Drop cache", "style": "destructive"},
          {"id": "details", "label": "Details"}
        ]}
      ]
    }"#;

    fn card_with(block: &str) -> String {
        format!(r#"{{"version":1,"title":"t","blocks":[{block}]}}"#)
    }

    fn rejects(json: &str) {
        assert!(
            matches!(
                parse_ui_card(json.to_string()),
                Err(MeshError::InvalidInput { .. })
            ),
            "should reject: {json}"
        );
    }

    #[test]
    fn parses_every_block_type() {
        let card = parse_ui_card(FULL.to_string()).unwrap();
        assert_eq!(card.version, 1);
        assert_eq!(card.title, "Nightly build");
        assert_eq!(card.blocks.len(), 8);
        assert!(matches!(
            &card.blocks[5],
            UiBlock::Chart { kind: ChartKind::Line, series, .. } if series.len() == 3
        ));
        let UiBlock::Actions { actions } = &card.blocks[7] else {
            panic!()
        };
        assert_eq!(actions[0].style, UiActionStyle::Primary);
        assert_eq!(actions[1].style, UiActionStyle::Destructive);
        assert_eq!(actions[2].style, UiActionStyle::Secondary);
    }

    #[test]
    fn rejects_structural_problems() {
        rejects("not json");
        rejects(r#"{"version":2,"title":"t","blocks":[{"type":"text","markdown":"x"}]}"#);
        rejects(r#"{"version":1,"title":"t","blocks":[]}"#);
        rejects(r#"{"version":1,"title":"t","blocks":[{"type":"text","markdown":"x"}],"extra":1}"#);
        rejects(&card_with(r#"{"type":"html","html":"<b>x</b>"}"#));
        rejects(&card_with(
            r#"{"type":"text","markdown":"x","onclick":"y"}"#,
        ));
        let many = vec![r#"{"type":"text","markdown":"x"}"#; MAX_BLOCKS + 1].join(",");
        rejects(&format!(r#"{{"version":1,"title":"t","blocks":[{many}]}}"#));
        let ok = vec![r#"{"type":"text","markdown":"x"}"#; MAX_BLOCKS].join(",");
        assert!(parse_ui_card(format!(r#"{{"version":1,"title":"t","blocks":[{ok}]}}"#)).is_ok());
        rejects(&"x".repeat(MAX_CARD_JSON_BYTES + 1));
    }

    #[test]
    fn rejects_script_and_unsafe_urls() {
        rejects(&card_with(
            r#"{"type":"text","markdown":"<script>alert(1)</script>"}"#,
        ));
        rejects(&card_with(
            r#"{"type":"text","markdown":"[x](javascript:alert(1))"}"#,
        ));
        rejects(&card_with(
            r#"{"type":"text","markdown":"<img src=x onerror=alert(1)>"}"#,
        ));
        rejects(&card_with(r#"{"type":"list","items":["<iframe src=x>"]}"#));
        rejects(&card_with(
            r#"{"type":"image","url":"http://example.com/a.png","alt":"a"}"#,
        ));
        rejects(&card_with(
            r#"{"type":"image","url":"data:image/png;base64,AAAA","alt":"a"}"#,
        ));
        rejects(&card_with(
            r#"{"type":"image","url":"https://u:p@example.com/a.png","alt":"a"}"#,
        ));
        rejects(&card_with(
            r#"{"type":"image","url":"https://example.com/a.png","alt":""}"#,
        ));
        rejects(&card_with(r#"{"type":"text","markdown":"<b>bold</b>"}"#));
        rejects(&card_with(
            r#"{"type":"text","markdown":"<!-- hidden -->"}"#,
        ));
        // Plain prose with '<' and markdown autolinks are fine.
        assert!(parse_ui_card(card_with(
            r#"{"type":"text","markdown":"Turn on = enable, see a < b and <https://example.com>"}"#
        ))
        .is_ok());
    }

    #[test]
    fn rejects_out_of_range_values() {
        rejects(&card_with(r#"{"type":"progress","label":"p","value":1.5}"#));
        rejects(&card_with(
            r#"{"type":"progress","label":"p","value":-0.1}"#,
        ));
        rejects(&card_with(r#"{"type":"chart","kind":"pie","series":[1]}"#));
        rejects(&card_with(r#"{"type":"chart","kind":"bar","series":[]}"#));
        rejects(&card_with(
            r#"{"type":"chart","kind":"bar","series":[1,2],"labels":["a"]}"#,
        ));
        rejects(&card_with(r#"{"type":"metric","label":"m","value":""}"#));
        rejects(&card_with(&format!(
            r#"{{"type":"text","markdown":"{}"}}"#,
            "x".repeat(MAX_MARKDOWN + 1)
        )));
        rejects(&card_with(r#"{"type":"key_value","pairs":[]}"#));
        rejects(&card_with(r#"{"type":"list","items":[]}"#));
    }

    #[test]
    fn rejects_bad_actions() {
        rejects(&card_with(r#"{"type":"actions","actions":[]}"#));
        rejects(&card_with(
            r#"{"type":"actions","actions":[{"id":"a","label":"A"},{"id":"a","label":"B"}]}"#,
        ));
        rejects(&card_with(
            r#"{"type":"actions","actions":[{"id":"a b","label":"A"}]}"#,
        ));
        rejects(&card_with(
            r#"{"type":"actions","actions":[{"id":"a","label":"A","style":"blink"}]}"#,
        ));
        rejects(
            r#"{"version":1,"title":"t","blocks":[
              {"type":"actions","actions":[{"id":"a","label":"A"}]},
              {"type":"actions","actions":[{"id":"b","label":"B"}]}]}"#,
        );
    }

    #[test]
    fn strips_control_characters() {
        let card = parse_ui_card(card_with(
            r#"{"type":"text","markdown":"ok\u001b[31m red"}"#,
        ))
        .unwrap();
        assert_eq!(
            card.blocks[0],
            UiBlock::Text {
                markdown: "ok red".to_string()
            }
        );
    }
}
