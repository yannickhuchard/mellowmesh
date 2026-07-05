use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Topic(String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamedTopic {
    pub name: String,
    pub topic: String,
}

impl Topic {
    pub fn new(s: impl Into<String>) -> Result<Self, String> {
        let raw = s.into();
        if raw.is_empty() {
            return Err("Topic cannot be empty".into());
        }
        // Check characters
        for c in raw.chars() {
            if c.is_control() {
                return Err("Topic cannot contain control characters".into());
            }
            if c == '*' || c == '>' {
                return Err(format!(
                    "Invalid character '{c}' in topic. Topic names cannot contain wildcards"
                ));
            }
        }
        Ok(Topic(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Matches a topic pattern (e.g. `_agent.codex.*` or `_agent.**`) against a concrete topic.
/// Returns true if it matches, false otherwise.
pub fn match_topic(pattern: &str, topic: &str) -> bool {
    match_topic_with_options(pattern, topic, false)
}

pub fn match_topic_with_options(pattern: &str, topic: &str, case_insensitive: bool) -> bool {
    let p_segs: Vec<&str> = pattern.split('.').collect();
    let t_segs: Vec<&str> = topic.split('.').collect();
    match_segments(&p_segs, &t_segs, case_insensitive)
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(|c| c.to_lowercase())
        .eq(b.chars().flat_map(|c| c.to_lowercase()))
}

fn match_segments(p_segs: &[&str], t_segs: &[&str], case_insensitive: bool) -> bool {
    if p_segs.is_empty() {
        return t_segs.is_empty();
    }

    let p = p_segs[0];
    if p == "**" {
        if p_segs.len() == 1 {
            return true;
        }
        for i in 0..=t_segs.len() {
            if match_segments(&p_segs[1..], &t_segs[i..], case_insensitive) {
                return true;
            }
        }
        return false;
    }

    if p == ">" {
        if p_segs.len() == 1 {
            return !t_segs.is_empty();
        }
        return false;
    }

    if t_segs.is_empty() {
        return false;
    }

    let is_match = if case_insensitive {
        eq_ignore_case(p, t_segs[0])
    } else {
        p == t_segs[0]
    };

    if p == "*" || is_match {
        return match_segments(&p_segs[1..], &t_segs[1..], case_insensitive);
    }

    false
}

/// Matches a pre-split topic pattern against a pre-split concrete topic.
/// This avoids allocating vectors of segments on the hot path.
pub fn match_pre_split(pattern_segs: &[String], topic_segs: &[&str]) -> bool {
    match_pre_split_with_options(pattern_segs, topic_segs, false)
}

pub fn match_pre_split_with_options(
    pattern_segs: &[String],
    topic_segs: &[&str],
    case_insensitive: bool,
) -> bool {
    match_segments_pre_split(pattern_segs, topic_segs, case_insensitive)
}

fn match_segments_pre_split(p_segs: &[String], t_segs: &[&str], case_insensitive: bool) -> bool {
    if p_segs.is_empty() {
        return t_segs.is_empty();
    }

    let p = &p_segs[0];
    if p == "**" {
        if p_segs.len() == 1 {
            return true;
        }
        for i in 0..=t_segs.len() {
            if match_segments_pre_split(&p_segs[1..], &t_segs[i..], case_insensitive) {
                return true;
            }
        }
        return false;
    }

    if p == ">" {
        if p_segs.len() == 1 {
            return !t_segs.is_empty();
        }
        return false;
    }

    if t_segs.is_empty() {
        return false;
    }

    let is_match = if case_insensitive {
        eq_ignore_case(p, t_segs[0])
    } else {
        p == t_segs[0]
    };

    if p == "*" || is_match {
        return match_segments_pre_split(&p_segs[1..], &t_segs[1..], case_insensitive);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation() {
        assert!(Topic::new("valid.topic.name").is_ok());
        assert!(Topic::new("valid_topic-123.name").is_ok());
        assert!(Topic::new("Invalid.Topic").is_ok());
        assert!(Topic::new("invalid topic").is_ok());
        assert!(Topic::new("valid.topic.🙂").is_ok());
        assert!(Topic::new("中文.日本語.العربية").is_ok());
        assert!(Topic::new("").is_err());
        assert!(Topic::new("invalid*topic").is_err());
        assert!(Topic::new("invalid>topic").is_err());
    }

    #[test]
    fn test_exact_match() {
        assert!(match_topic("a.b.c", "a.b.c"));
        assert!(!match_topic("a.b.c", "a.b.d"));
        assert!(!match_topic("a.b.c", "a.b"));
        assert!(!match_topic("a.b", "a.b.c"));
    }

    #[test]
    fn test_single_wildcard() {
        assert!(match_topic("a.*.c", "a.b.c"));
        assert!(match_topic("a.b.*", "a.b.c"));
        assert!(match_topic("*.b.c", "a.b.c"));
        assert!(!match_topic("a.*.c", "a.b.d.c"));
        assert!(!match_topic("a.*.c", "a.c"));
    }

    #[test]
    fn test_multi_wildcard() {
        assert!(match_topic("a.**", "a.b.c"));
        assert!(match_topic("a.**", "a"));
        assert!(match_topic("**", "a.b.c"));
        assert!(match_topic("_agent.codex.**", "_agent.codex.status"));
        assert!(match_topic(
            "_agent.**.status",
            "_agent.codex.detailed.status"
        ));
        assert!(!match_topic(
            "_agent.**.status",
            "_agent.codex.detailed.info"
        ));
    }

    #[test]
    fn test_greater_than_wildcard() {
        assert!(match_topic("news.>", "news.french.technology"));
        assert!(match_topic("news.>", "news.english.technology"));
        assert!(match_topic("news.>", "news.french.art"));
        assert!(match_topic("news.>", "news.french"));
        assert!(!match_topic("news.>", "news"));
        assert!(!match_topic("news.>", "other.french"));
        assert!(match_topic(">", "news"));
        assert!(match_topic(">", "news.french"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(match_topic_with_options("news.french", "NEWS.FRENCH", true));
        assert!(match_topic_with_options("news.french", "news.French", true));
        assert!(!match_topic_with_options(
            "news.french",
            "news.French",
            false
        ));
        assert!(match_topic_with_options("NEWS.>", "news.french", true));
        assert!(!match_topic_with_options("NEWS.>", "news.french", false));

        // Unicode case folding
        assert!(match_topic_with_options("grüße.welt", "GRÜẞE.WELT", true));
        assert!(match_topic_with_options("ΕΛΛΆΔΑ", "Ελλάδα", true));
        assert!(match_topic_with_options("ΚΑΙ", "και", true));
    }

    #[test]
    fn test_pre_split_matching() {
        let p_segs: Vec<String> = "_agent.**.status".split('.').map(String::from).collect();
        let t_segs1: Vec<&str> = "_agent.codex.detailed.status".split('.').collect();
        let t_segs2: Vec<&str> = "_agent.codex.detailed.info".split('.').collect();
        assert!(match_pre_split(&p_segs, &t_segs1));
        assert!(!match_pre_split(&p_segs, &t_segs2));
    }
}
