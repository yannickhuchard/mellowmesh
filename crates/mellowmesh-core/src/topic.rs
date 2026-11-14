use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Topic(String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamedTopic {
    pub name: String,
    pub topic: String,
}

/// Maximum length of a topic or topic pattern, in bytes.
pub const MAX_TOPIC_LEN: usize = 1024;
/// Maximum number of `.`-separated segments in a topic or topic pattern.
pub const MAX_TOPIC_SEGMENTS: usize = 64;

fn check_size(raw: &str, what: &str) -> Result<(), String> {
    if raw.len() > MAX_TOPIC_LEN {
        return Err(format!(
            "{what} is too long ({} bytes, max {MAX_TOPIC_LEN})",
            raw.len()
        ));
    }
    let segments = raw.split('.').count();
    if segments > MAX_TOPIC_SEGMENTS {
        return Err(format!(
            "{what} has too many segments ({segments}, max {MAX_TOPIC_SEGMENTS})"
        ));
    }
    Ok(())
}

impl Topic {
    pub fn new(s: impl Into<String>) -> Result<Self, String> {
        let raw = s.into();
        if raw.is_empty() {
            return Err("Topic cannot be empty".into());
        }
        check_size(&raw, "Topic")?;
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

/// Validates a subscription / filter pattern: it must be non-empty, at most
/// [`MAX_TOPIC_LEN`] bytes and at most [`MAX_TOPIC_SEGMENTS`] segments, and
/// must not contain control characters.
pub fn validate_pattern(pattern: &str) -> Result<(), String> {
    if pattern.is_empty() {
        return Err("Pattern cannot be empty".into());
    }
    check_size(pattern, "Pattern")?;
    if pattern.chars().any(|c| c.is_control()) {
        return Err("Pattern cannot contain control characters".into());
    }
    Ok(())
}

/// Returns the literal (wildcard-free) leading part of a pattern: the
/// segments before the first `*`, `**` or `>` segment, joined with `.`.
/// Every topic matched (case-sensitively) by `pattern` starts with this
/// string, so it can be used as a cheap prefix prefilter, e.g. in SQL.
/// Returns an empty string when the pattern starts with a wildcard.
pub fn pattern_literal_prefix(pattern: &str) -> String {
    let segs: Vec<&str> = pattern.split('.').collect();
    let first_wild = segs
        .iter()
        .position(|s| *s == "*" || *s == "**" || *s == ">");
    match first_wild {
        None => pattern.to_string(),
        Some(0) => String::new(),
        Some(i) => {
            let mut prefix = segs[..i].join(".");
            // `*` and `>` consume at least one more segment, so the separator
            // is part of every match; `**` may match zero segments.
            if segs[i] != "**" {
                prefix.push('.');
            }
            prefix
        }
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

#[inline]
fn seg_eq(p: &str, t: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        eq_ignore_case(p, t)
    } else {
        p == t
    }
}

/// Core matcher.
///
/// Semantics:
/// * `*` matches exactly one segment;
/// * `**` matches zero or more segments (a run of consecutive `**` is
///   equivalent to a single one);
/// * `>` as the last segment matches one or more trailing segments; a `>`
///   anywhere else never matches;
/// * any other segment matches literally (optionally case-insensitively,
///   with Unicode lowercase folding).
///
/// Patterns without `**` are matched in a single linear pass. Patterns with
/// `**` use dynamic programming over (pattern segment, topic segment) pairs,
/// so the cost is O(pattern · topic) rather than exponential backtracking.
fn match_segments<P: AsRef<str>, T: AsRef<str>>(
    p_segs: &[P],
    t_segs: &[T],
    case_insensitive: bool,
) -> bool {
    if !p_segs.iter().any(|p| p.as_ref() == "**") {
        return match_linear(p_segs, t_segs, case_insensitive);
    }

    // Collapse runs of consecutive `**`.
    let mut pats: Vec<&str> = Vec::with_capacity(p_segs.len());
    for p in p_segs {
        let p = p.as_ref();
        if p == "**" && pats.last() == Some(&"**") {
            continue;
        }
        pats.push(p);
    }

    let n = t_segs.len();
    // `next[ti]`: does the pattern suffix after the current segment match
    // `t_segs[ti..]`? Initialised for the empty pattern, which matches only
    // the empty remainder.
    let mut next: Vec<bool> = vec![false; n + 1];
    next[n] = true;
    let mut cur: Vec<bool> = vec![false; n + 1];

    for (pi, &p) in pats.iter().enumerate().rev() {
        let is_last = pi + 1 == pats.len();
        if p == "**" {
            // `**` matches t[ti..i] for any i >= ti: a suffix-OR of `next`.
            let mut acc = false;
            for ti in (0..=n).rev() {
                acc = acc || next[ti];
                cur[ti] = acc;
            }
        } else if p == ">" {
            for (ti, slot) in cur.iter_mut().enumerate() {
                *slot = is_last && ti < n;
            }
        } else {
            cur[n] = false;
            for ti in 0..n {
                cur[ti] =
                    next[ti + 1] && (p == "*" || seg_eq(p, t_segs[ti].as_ref(), case_insensitive));
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    next[0]
}

fn match_linear<P: AsRef<str>, T: AsRef<str>>(
    p_segs: &[P],
    t_segs: &[T],
    case_insensitive: bool,
) -> bool {
    for (pi, p) in p_segs.iter().enumerate() {
        let p = p.as_ref();
        if p == ">" {
            return pi + 1 == p_segs.len() && pi < t_segs.len();
        }
        let Some(t) = t_segs.get(pi) else {
            return false;
        };
        if p != "*" && !seg_eq(p, t.as_ref(), case_insensitive) {
            return false;
        }
    }
    p_segs.len() == t_segs.len()
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
    match_segments(pattern_segs, topic_segs, case_insensitive)
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
    fn test_topic_size_limits() {
        assert!(Topic::new("a".repeat(MAX_TOPIC_LEN)).is_ok());
        assert!(Topic::new("a".repeat(MAX_TOPIC_LEN + 1)).is_err());
        let segs = vec!["a"; MAX_TOPIC_SEGMENTS].join(".");
        assert!(Topic::new(segs).is_ok());
        let segs = vec!["a"; MAX_TOPIC_SEGMENTS + 1].join(".");
        assert!(Topic::new(segs).is_err());
    }

    #[test]
    fn test_validate_pattern() {
        assert!(validate_pattern("_agent.**").is_ok());
        assert!(validate_pattern("news.>").is_ok());
        assert!(validate_pattern("").is_err());
        assert!(validate_pattern("a\nb").is_err());
        assert!(validate_pattern(&"a".repeat(MAX_TOPIC_LEN + 1)).is_err());
        assert!(validate_pattern(&vec!["**"; MAX_TOPIC_SEGMENTS].join(".")).is_ok());
        assert!(validate_pattern(&vec!["**"; MAX_TOPIC_SEGMENTS + 1].join(".")).is_err());
    }

    #[test]
    fn test_pattern_literal_prefix() {
        assert_eq!(pattern_literal_prefix("a.b.c"), "a.b.c");
        assert_eq!(pattern_literal_prefix("a.b.**"), "a.b");
        assert_eq!(pattern_literal_prefix("a.b.*.c"), "a.b.");
        assert_eq!(pattern_literal_prefix("a.>"), "a.");
        assert_eq!(pattern_literal_prefix("**"), "");
        assert_eq!(pattern_literal_prefix("*.b"), "");
        for (pat, topic) in [
            ("a.b.**", "a.b"),
            ("a.b.**", "a.b.c.d"),
            ("a.*.c", "a.x.c"),
            ("a.>", "a.x"),
        ] {
            assert!(match_topic(pat, topic));
            assert!(topic.starts_with(&pattern_literal_prefix(pat)));
        }
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
    fn test_multi_wildcard_edge_cases() {
        assert!(match_topic("**.**", "a"));
        assert!(match_topic("a.**.**.b", "a.b"));
        assert!(match_topic("a.**.**.b", "a.x.y.b"));
        assert!(!match_topic("a.**.**.b", "a.x.y.c"));
        assert!(match_topic("**.b.**", "a.b.c"));
        assert!(match_topic("**.b.**", "b"));
        assert!(!match_topic("**.b.**", "a.c"));
        assert!(match_topic("a.**.*", "a.b"));
        assert!(!match_topic("a.**.*", "a"));
        assert!(match_topic("**.>", "a"));
        assert!(match_topic("a.**.>", "a.b.c"));
        assert!(!match_topic("a.**.>", "a"));
        // `>` that is not the last segment never matches.
        assert!(!match_topic("a.>.b", "a.x.b"));
        assert!(!match_topic("a.**.>.b", "a.x.b"));
        assert!(match_topic_with_options("A.**.C", "a.b.c", true));
        assert!(!match_topic_with_options("A.**.C", "a.b.c", false));
    }

    /// The original backtracking matcher, kept as a reference oracle.
    fn reference_match(p: &[&str], t: &[&str], ci: bool) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        if p[0] == "**" {
            if p.len() == 1 {
                return true;
            }
            return (0..=t.len()).any(|i| reference_match(&p[1..], &t[i..], ci));
        }
        if p[0] == ">" {
            return p.len() == 1 && !t.is_empty();
        }
        if t.is_empty() {
            return false;
        }
        if p[0] == "*" || seg_eq(p[0], t[0], ci) {
            return reference_match(&p[1..], &t[1..], ci);
        }
        false
    }

    #[test]
    fn test_matches_reference_exhaustively() {
        fn all_seqs<'a>(alphabet: &[&'a str], max_len: usize) -> Vec<Vec<&'a str>> {
            let mut out = vec![vec![]];
            let mut frontier = vec![vec![]];
            for _ in 0..max_len {
                let mut next = Vec::new();
                for seq in &frontier {
                    for a in alphabet {
                        let mut s: Vec<&str> = seq.clone();
                        s.push(a);
                        next.push(s);
                    }
                }
                out.extend(next.iter().cloned());
                frontier = next;
            }
            out
        }
        let patterns = all_seqs(&["a", "B", "*", "**", ">"], 4);
        let topics = all_seqs(&["a", "b", "c"], 4);
        for p in &patterns {
            let p_owned: Vec<String> = p.iter().map(|s| s.to_string()).collect();
            for t in &topics {
                for ci in [false, true] {
                    assert_eq!(
                        match_segments(p, t, ci),
                        reference_match(p, t, ci),
                        "pattern {p:?} topic {t:?} ci={ci}"
                    );
                    assert_eq!(
                        match_pre_split_with_options(&p_owned, t, ci),
                        reference_match(p, t, ci)
                    );
                }
            }
        }
    }

    #[test]
    fn test_pathological_pattern_is_fast() {
        let topic: String = (0..40)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join(".");
        let consecutive = format!("{}.zz", ["**"; 12].join("."));
        let interleaved = format!("{}.zz", ["**.s1"; 12].join("."));
        let start = std::time::Instant::now();
        assert!(!match_topic(&consecutive, &topic));
        assert!(!match_topic(&interleaved, &topic));
        assert!(!match_topic_with_options(&interleaved, &topic, true));
        let p_segs: Vec<String> = interleaved.split('.').map(String::from).collect();
        let t_segs: Vec<&str> = topic.split('.').collect();
        assert!(!match_pre_split(&p_segs, &t_segs));
        assert!(
            start.elapsed() < std::time::Duration::from_millis(100),
            "matching took {:?}",
            start.elapsed()
        );
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
