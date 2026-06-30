use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OKFDocument {
    pub wiki: String,
    pub path: String,
    pub doc_type: String, // from YAML `type`
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub timestamp: DateTime<Utc>,
    pub resource: Option<String>,
    pub body: String,
    pub links: Vec<String>,
}

pub fn parse_okf_string(wiki: &str, path: &str, content: &str) -> anyhow::Result<OKFDocument> {
    let mut lines = content.lines();
    let first = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty content"))?
        .trim();
    if first != "---" {
        return Err(anyhow::anyhow!("Missing YAML frontmatter start (---)"));
    }

    let mut yaml_lines = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_frontmatter = true;

    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
            } else {
                yaml_lines.push(line);
            }
        } else {
            body_lines.push(line);
        }
    }

    if in_frontmatter {
        return Err(anyhow::anyhow!("Missing YAML frontmatter end (---)"));
    }

    let body = body_lines.join("\n");

    // Parse YAML lines
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_tags: Vec<String> = Vec::new();

    for line in yaml_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('-') && current_key.as_deref() == Some("tags") {
            // Indented array list item e.g. "- tag1"
            let tag_val = trimmed[1..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !tag_val.is_empty() {
                current_tags.push(tag_val);
            }
            continue;
        }

        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_lowercase();
            let value = line[colon_pos + 1..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            current_key = Some(key.clone());

            if key == "tags" {
                if value.starts_with('[') && value.ends_with(']') {
                    // Inline array e.g. "[tag1, tag2]"
                    let inner = &value[1..value.len() - 1];
                    for part in inner.split(',') {
                        let t = part.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !t.is_empty() {
                            current_tags.push(t);
                        }
                    }
                } else if !value.is_empty() {
                    // Comma-separated tags e.g. "tag1, tag2"
                    for part in value.split(',') {
                        let t = part.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !t.is_empty() {
                            current_tags.push(t);
                        }
                    }
                }
            } else {
                fields.insert(key, value);
            }
        }
    }

    let doc_type = fields
        .remove("type")
        .ok_or_else(|| anyhow::anyhow!("Missing required field 'type' in YAML frontmatter"))?;

    // Title defaults to first H1 in body if not specified
    let mut title = fields.remove("title").unwrap_or_default();
    if title.is_empty() {
        for line in body.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                title = t.trim_start_matches('#').trim().to_string();
                break;
            }
        }
    }
    if title.is_empty() {
        title = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .replace('_', " ");
    }

    let description = fields.remove("description");
    let resource = fields.remove("resource");

    let timestamp = if let Some(ts_str) = fields.remove("timestamp") {
        chrono::DateTime::parse_from_rfc3339(&ts_str)
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                // Try naive date parse or fall back to now
                chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y-%m-%d %H:%M:%S")
                    .map(|ndt| Utc.from_utc_datetime(&ndt))
                    .or_else(|_| {
                        #[allow(deprecated)]
                        chrono::NaiveDate::parse_from_str(&ts_str, "%Y-%m-%d")
                            .map(|nd| Utc.from_utc_date(&nd).and_hms_opt(0, 0, 0).unwrap())
                    })
            })
            .unwrap_or_else(|_| Utc::now())
    } else {
        Utc::now()
    };

    // Extract links manually
    let links = extract_markdown_links(&body);

    Ok(OKFDocument {
        wiki: wiki.to_string(),
        path: path.to_string(),
        doc_type,
        title,
        description,
        tags: current_tags,
        timestamp,
        resource,
        body,
        links,
    })
}

pub fn serialize_okf(doc: &OKFDocument) -> String {
    let mut yaml = String::new();
    yaml.push_str("---\n");
    yaml.push_str(&format!("type: {}\n", doc.doc_type));
    yaml.push_str(&format!("title: \"{}\"\n", doc.title.replace('"', "\\\"")));
    if let Some(desc) = &doc.description {
        yaml.push_str(&format!("description: \"{}\"\n", desc.replace('"', "\\\"")));
    }
    if !doc.tags.is_empty() {
        yaml.push_str("tags:\n");
        for tag in &doc.tags {
            yaml.push_str(&format!("  - \"{}\"\n", tag.replace('"', "\\\"")));
        }
    }
    yaml.push_str(&format!("timestamp: {}\n", doc.timestamp.to_rfc3339()));
    if let Some(res) = &doc.resource {
        yaml.push_str(&format!("resource: \"{}\"\n", res.replace('"', "\\\"")));
    }
    yaml.push_str("---\n\n");
    yaml.push_str(&doc.body);
    yaml
}

fn extract_markdown_links(body: &str) -> Vec<String> {
    let mut links = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            // Find closing ']'
            let mut j = i + 1;
            let mut bracket_depth = 1;
            while j < chars.len() {
                if chars[j] == '[' {
                    bracket_depth += 1;
                } else if chars[j] == ']' {
                    bracket_depth -= 1;
                    if bracket_depth == 0 {
                        break;
                    }
                }
                j += 1;
            }
            if j < chars.len() && j + 1 < chars.len() && chars[j + 1] == '(' {
                // Find closing ')'
                let mut k = j + 2;
                let mut paren_depth = 1;
                while k < chars.len() {
                    if chars[k] == '(' {
                        paren_depth += 1;
                    } else if chars[k] == ')' {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            break;
                        }
                    }
                    k += 1;
                }
                if k < chars.len() {
                    let link_content: String = chars[j + 2..k].iter().collect();
                    let trimmed = link_content.trim();
                    // Extract the path (strip query parameters/hashes if any)
                    let path_part = trimmed
                        .split('#')
                        .next()
                        .unwrap_or(trimmed)
                        .split('?')
                        .next()
                        .unwrap_or(trimmed);
                    if path_part.ends_with(".md") {
                        links.push(path_part.to_string());
                    }
                    i = k; // advance to closing parenthesis
                    continue;
                }
            }
        }
        i += 1;
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_serialize() {
        let raw = "---\ntype: procedure\ntitle: \"Rotates credentials\"\ndescription: \"Steps for rotation\"\ntags:\n  - \"security\"\n  - \"iam\"\ntimestamp: 2026-06-12T10:00:00+00:00\nresource: \"users-api\"\n---\n\n# Rotate credentials\n\nGo to [IAM Settings](iam.md) or follow [runbook](docs/runbook.md#rotate).";

        let doc = parse_okf_string("default", "procedures/rotate.md", raw).unwrap();
        assert_eq!(doc.wiki, "default");
        assert_eq!(doc.path, "procedures/rotate.md");
        assert_eq!(doc.doc_type, "procedure");
        assert_eq!(doc.title, "Rotates credentials");
        assert_eq!(doc.description.as_deref(), Some("Steps for rotation"));
        assert_eq!(doc.tags, vec!["security", "iam"]);
        assert_eq!(doc.resource.as_deref(), Some("users-api"));
        assert_eq!(doc.links, vec!["iam.md", "docs/runbook.md"]);

        let serialized = serialize_okf(&doc);
        assert!(serialized.contains("type: procedure"));
        assert!(serialized.contains("title: \"Rotates credentials\""));
        assert!(serialized.contains("resource: \"users-api\""));
        assert!(serialized.contains("Go to [IAM Settings](iam.md)"));
    }
}
