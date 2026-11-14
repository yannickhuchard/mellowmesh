use crate::agent::AgentRegistration;
use crate::topic::NamedTopic;

/// Parses message body for mentions of agents (like `@Claude Cowork` or `@[Claude Cowork]`)
/// or human owners (like `@yannick`), as well as named topics (like `#Mario Galaxy` or `#[Mario Galaxy]`).
///
/// Returns a tuple of:
/// 1. The rewritten body with mentions formatted as markdown links.
/// 2. A list of unique URIs (agent://..., human://..., or topic://...) that were matched.
pub fn parse_mentions(
    body: &str,
    agents: &[AgentRegistration],
    named_topics: &[NamedTopic],
) -> (String, Vec<String>) {
    let mut result = String::new();
    let mut mentions = Vec::new();

    // 1. Gather all candidate agents and sort by name length descending
    let mut sorted_agents = agents.to_vec();
    sorted_agents.sort_by_key(|a| std::cmp::Reverse(a.name.len()));

    // 2. Gather unique human owners from registered agents (e.g. "human://yannick")
    let mut owners = Vec::new();
    for agent in agents {
        if !owners.contains(&agent.owner) {
            owners.push(agent.owner.clone());
        }
    }
    // Sort owners by username length descending
    owners.sort_by(|a, b| {
        let name_a = a.strip_prefix("human://").unwrap_or(a.as_str());
        let name_b = b.strip_prefix("human://").unwrap_or(b.as_str());
        name_b.len().cmp(&name_a.len())
    });

    // 3. Sort named topics by name length descending
    let mut sorted_topics = named_topics.to_vec();
    sorted_topics.sort_by_key(|t| std::cmp::Reverse(t.name.len()));

    // Case-fold every name once per call instead of once per body position.
    let agent_names: Vec<Vec<Folded>> = sorted_agents.iter().map(|a| fold_str(&a.name)).collect();
    let owner_names: Vec<(&str, Vec<Folded>)> = owners
        .iter()
        .map(|o| {
            let username = o.strip_prefix("human://").unwrap_or(o.as_str());
            (username, fold_str(username))
        })
        .collect();
    let topic_names: Vec<Vec<Folded>> = sorted_topics.iter().map(|t| fold_str(&t.name)).collect();

    let chars: Vec<char> = body.chars().collect();
    let folded: Vec<Folded> = chars.iter().map(|&c| fold_char(c)).collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' && i + 1 < chars.len() {
            // A. Check for bracketed syntax first: @[Name]
            if chars[i + 1] == '[' {
                if let Some(end_pos) = find_closing_bracket(&chars, i + 2) {
                    let name: String = chars[i + 2..end_pos].iter().collect();
                    let trimmed_name = name.trim();

                    // Check agent match
                    if let Some(agent) = sorted_agents
                        .iter()
                        .find(|a| a.name.eq_ignore_ascii_case(trimmed_name))
                    {
                        result.push_str(&format!("[@{}]({})", agent.name, agent.id));
                        if !mentions.contains(&agent.id) {
                            mentions.push(agent.id.clone());
                        }
                        i = end_pos + 1;
                        continue;
                    }

                    // Check human owner match
                    if let Some(owner_uri) = owners.iter().find(|o| {
                        let username = o.strip_prefix("human://").unwrap_or(o.as_str());
                        username.eq_ignore_ascii_case(trimmed_name)
                    }) {
                        let display_name = owner_uri
                            .strip_prefix("human://")
                            .unwrap_or(owner_uri.as_str());
                        result.push_str(&format!("[@{display_name}]({owner_uri})"));
                        if !mentions.contains(owner_uri) {
                            mentions.push(owner_uri.clone());
                        }
                        i = end_pos + 1;
                        continue;
                    }
                }
            }

            // B. Try dynamic agent registry match (greedy matching)
            let agent_hit = sorted_agents
                .iter()
                .zip(&agent_names)
                .find_map(|(agent, name)| {
                    greedy_match(&chars, &folded, i + 1, name).map(|n| (agent, n))
                });
            if let Some((agent, next_idx)) = agent_hit {
                result.push_str(&format!("[@{}]({})", agent.name, agent.id));
                if !mentions.contains(&agent.id) {
                    mentions.push(agent.id.clone());
                }
                i = next_idx;
                continue;
            }

            // C. Try dynamic human owner match (greedy matching)
            let owner_hit =
                owners
                    .iter()
                    .zip(&owner_names)
                    .find_map(|(owner_uri, (username, name))| {
                        greedy_match(&chars, &folded, i + 1, name)
                            .map(|n| (owner_uri, *username, n))
                    });
            if let Some((owner_uri, username, next_idx)) = owner_hit {
                result.push_str(&format!("[@{username}]({owner_uri})"));
                if !mentions.contains(owner_uri) {
                    mentions.push(owner_uri.clone());
                }
                i = next_idx;
                continue;
            }
        }

        if chars[i] == '#' && i + 1 < chars.len() {
            // A. Check for bracketed syntax first: #[Name]
            if chars[i + 1] == '[' {
                if let Some(end_pos) = find_closing_bracket(&chars, i + 2) {
                    let name: String = chars[i + 2..end_pos].iter().collect();
                    let trimmed_name = name.trim();

                    // Check named topic match
                    if let Some(nt) = sorted_topics
                        .iter()
                        .find(|t| t.name.eq_ignore_ascii_case(trimmed_name))
                    {
                        let topic_uri = format!("topic://{}", nt.topic);
                        result.push_str(&format!("[#{}]({})", nt.name, topic_uri));
                        if !mentions.contains(&topic_uri) {
                            mentions.push(topic_uri);
                        }
                        i = end_pos + 1;
                        continue;
                    }
                }
            }

            // B. Try dynamic topic name match (greedy matching)
            let topic_hit = sorted_topics
                .iter()
                .zip(&topic_names)
                .find_map(|(nt, name)| greedy_match(&chars, &folded, i + 1, name).map(|n| (nt, n)));
            if let Some((nt, next_idx)) = topic_hit {
                let topic_uri = format!("topic://{}", nt.topic);
                result.push_str(&format!("[#{}]({})", nt.name, topic_uri));
                if !mentions.contains(&topic_uri) {
                    mentions.push(topic_uri);
                }
                i = next_idx;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    (result, mentions)
}

/// Maximum number of characters scanned after `@[` / `#[` for the closing
/// `]`. Names are short; bounding the lookahead keeps parsing linear in the
/// body length even for adversarial input full of unclosed brackets.
pub const MAX_BRACKET_NAME_CHARS: usize = 64;

/// Finds the index of the `]` closing a bracketed name that starts at
/// `start`, looking at most [`MAX_BRACKET_NAME_CHARS`] characters ahead.
fn find_closing_bracket(chars: &[char], start: usize) -> Option<usize> {
    let end = chars
        .len()
        .min(start.saturating_add(MAX_BRACKET_NAME_CHARS + 1));
    chars
        .get(start..end)?
        .iter()
        .position(|&c| c == ']')
        .map(|idx| start + idx)
}

/// The lowercase form of a single character (at most three chars; unused
/// slots are `'\0'`). Comparing these is equivalent to comparing
/// `c.to_lowercase().to_string()`.
type Folded = [char; 3];

fn fold_char(c: char) -> Folded {
    let mut out = ['\0'; 3];
    for (slot, lc) in out.iter_mut().zip(c.to_lowercase()) {
        *slot = lc;
    }
    out
}

fn fold_str(s: &str) -> Vec<Folded> {
    s.chars().map(fold_char).collect()
}

/// Case-insensitively matches the pre-folded `name` at `start` in the body,
/// requiring a word boundary after it. Returns the index just past the name.
fn greedy_match(chars: &[char], folded: &[Folded], start: usize, name: &[Folded]) -> Option<usize> {
    let next_idx = start.checked_add(name.len())?;
    if next_idx > chars.len() || folded[start..next_idx] != *name {
        return None;
    }
    let is_boundary = match chars.get(next_idx) {
        Some(&next_c) => !next_c.is_alphanumeric() && next_c != '_',
        None => true,
    };
    is_boundary.then_some(next_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_agents() -> Vec<AgentRegistration> {
        vec![
            AgentRegistration {
                id: "agent://yannick/hermes".to_string(),
                name: "Hermes".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://yannick/claude-cowork".to_string(),
                name: "Claude Cowork".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://yannick/openclaw".to_string(),
                name: "OpenClaw AI".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
        ]
    }

    fn mock_topics() -> Vec<NamedTopic> {
        vec![
            NamedTopic {
                name: "Mario Galaxy".to_string(),
                topic: "_forum.games.mario galaxy".to_string(),
            },
            NamedTopic {
                name: "General".to_string(),
                topic: "_forum.general".to_string(),
            },
        ]
    }

    #[test]
    fn test_simple_mention() {
        let agents = mock_agents();
        let (body, uris) = parse_mentions("Please check with @Hermes and report.", &agents, &[]);
        assert_eq!(
            body,
            "Please check with [@Hermes](agent://yannick/hermes) and report."
        );
        assert_eq!(uris, vec!["agent://yannick/hermes"]);
    }

    #[test]
    fn test_spaces_mention() {
        let agents = mock_agents();
        let (body, uris) = parse_mentions("Ask @Claude Cowork for help.", &agents, &[]);
        assert_eq!(
            body,
            "Ask [@Claude Cowork](agent://yannick/claude-cowork) for help."
        );
        assert_eq!(uris, vec!["agent://yannick/claude-cowork"]);
    }

    #[test]
    fn test_greedy_matching() {
        let agents = vec![
            AgentRegistration {
                id: "agent://yannick/claude".to_string(),
                name: "Claude".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://yannick/claude-cowork".to_string(),
                name: "Claude Cowork".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
        ];
        let (body, uris) = parse_mentions("Ping @Claude Cowork today.", &agents, &[]);
        assert_eq!(
            body,
            "Ping [@Claude Cowork](agent://yannick/claude-cowork) today."
        );
        assert_eq!(uris, vec!["agent://yannick/claude-cowork"]);
    }

    #[test]
    fn test_bracketed_mention() {
        let agents = mock_agents();
        let (body, uris) = parse_mentions("Tag @[OpenClaw AI] for the next run.", &agents, &[]);
        assert_eq!(
            body,
            "Tag [@OpenClaw AI](agent://yannick/openclaw) for the next run."
        );
        assert_eq!(uris, vec!["agent://yannick/openclaw"]);
    }

    #[test]
    fn test_human_owner_mention() {
        let agents = mock_agents();
        let (body, uris) = parse_mentions("Assigned to @yannick for approval.", &agents, &[]);
        assert_eq!(
            body,
            "Assigned to [@yannick](human://yannick) for approval."
        );
        assert_eq!(uris, vec!["human://yannick"]);
    }

    #[test]
    fn test_case_insensitive() {
        let agents = mock_agents();
        let (body, uris) = parse_mentions("Ask @hermes about this.", &agents, &[]);
        assert_eq!(body, "Ask [@Hermes](agent://yannick/hermes) about this.");
        assert_eq!(uris, vec!["agent://yannick/hermes"]);
    }

    #[test]
    fn test_bracketed_special_chars() {
        let agents = vec![
            AgentRegistration {
                id: "agent://company/rd-agent".to_string(),
                name: "R&D Agent".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://company/ceo-bot".to_string(),
                name: "CEO & Co-founder".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
        ];
        let (body, uris) = parse_mentions(
            "Check with @[R&D Agent] and @[CEO & Co-founder] first.",
            &agents,
            &[],
        );
        assert_eq!(body, "Check with [@R&D Agent](agent://company/rd-agent) and [@CEO & Co-founder](agent://company/ceo-bot) first.");
        assert_eq!(
            uris,
            vec!["agent://company/rd-agent", "agent://company/ceo-bot"]
        );
    }

    #[test]
    fn test_multiple_mentions() {
        let agents = mock_agents();
        let (body, uris) = parse_mentions("Ping @Hermes and then @Claude Cowork.", &agents, &[]);
        assert_eq!(body, "Ping [@Hermes](agent://yannick/hermes) and then [@Claude Cowork](agent://yannick/claude-cowork).");
        assert_eq!(
            uris,
            vec!["agent://yannick/hermes", "agent://yannick/claude-cowork"]
        );
    }

    #[test]
    fn test_longest_prefix_matching() {
        let agents = vec![
            AgentRegistration {
                id: "agent://yannick/claude".to_string(),
                name: "Claude".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://yannick/claude-cowork".to_string(),
                name: "Claude Cowork".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://yannick/claude-coworker".to_string(),
                name: "Claude Coworker".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
        ];
        let (body, uris) = parse_mentions(
            "Notify @Claude Coworker, @Claude Cowork, and @Claude.",
            &agents,
            &[],
        );
        assert_eq!(body, "Notify [@Claude Coworker](agent://yannick/claude-coworker), [@Claude Cowork](agent://yannick/claude-cowork), and [@Claude](agent://yannick/claude).");
        assert_eq!(
            uris,
            vec![
                "agent://yannick/claude-coworker",
                "agent://yannick/claude-cowork",
                "agent://yannick/claude"
            ]
        );
    }

    #[test]
    fn test_unicode_mentions() {
        let agents = vec![
            AgentRegistration {
                id: "agent://yannick/chinese-agent".to_string(),
                name: "张伟".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
            AgentRegistration {
                id: "agent://yannick/arabic-agent".to_string(),
                name: "أحمد".to_string(),
                owner: "human://yannick".to_string(),
                mode: "autonomous".to_string(),
                capabilities: vec![],
            },
        ];
        let (body, uris) = parse_mentions("Tag @张伟 and @[أحمد] for validation.", &agents, &[]);
        assert_eq!(body, "Tag [@张伟](agent://yannick/chinese-agent) and [@أحمد](agent://yannick/arabic-agent) for validation.");
        assert_eq!(
            uris,
            vec![
                "agent://yannick/chinese-agent",
                "agent://yannick/arabic-agent"
            ]
        );
    }

    #[test]
    fn test_adversarial_brackets_are_linear() {
        let agents = mock_agents();
        let topics = mock_topics();
        let body = "@[".repeat(50_000) + &"#[".repeat(50_000); // 200 KB
        let start = std::time::Instant::now();
        let (out, uris) = parse_mentions(&body, &agents, &topics);
        let elapsed = start.elapsed();
        assert_eq!(out, body);
        assert!(uris.is_empty());
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "took {elapsed:?}"
        );
    }

    #[test]
    fn test_bracket_lookahead_is_bounded() {
        let agents = mock_agents();
        // A closing bracket beyond the lookahead window is not a bracketed name.
        let body = format!("@[{}Hermes]", " ".repeat(MAX_BRACKET_NAME_CHARS));
        let (out, uris) = parse_mentions(&body, &agents, &[]);
        assert_eq!(out, body);
        assert!(uris.is_empty());
        // Within the window, surrounding whitespace is still trimmed.
        let (out, uris) = parse_mentions("@[  Hermes  ] hi", &agents, &[]);
        assert_eq!(out, "[@Hermes](agent://yannick/hermes) hi");
        assert_eq!(uris, vec!["agent://yannick/hermes"]);
    }

    #[test]
    fn test_topic_mentions() {
        let agents = mock_agents();
        let topics = mock_topics();

        // 1. Simple, greedy matched topic
        let (body1, uris1) = parse_mentions("Let's talk in #General", &agents, &topics);
        assert_eq!(body1, "Let's talk in [#General](topic://_forum.general)");
        assert_eq!(uris1, vec!["topic://_forum.general"]);

        // 2. Bracketed topic with spaces
        let (body2, uris2) = parse_mentions("Did you see #[Mario Galaxy]?", &agents, &topics);
        assert_eq!(
            body2,
            "Did you see [#Mario Galaxy](topic://_forum.games.mario galaxy)?"
        );
        assert_eq!(uris2, vec!["topic://_forum.games.mario galaxy"]);

        // 3. Mixed agents, humans, and topics
        let (body3, uris3) =
            parse_mentions("Tell @Hermes that #General is active.", &agents, &topics);
        assert_eq!(body3, "Tell [@Hermes](agent://yannick/hermes) that [#General](topic://_forum.general) is active.");
        assert_eq!(
            uris3,
            vec!["agent://yannick/hermes", "topic://_forum.general"]
        );

        // 4. Greedy matched topic with spaces (no brackets)
        let (body4, uris4) = parse_mentions("Did you see #Mario Galaxy?", &agents, &topics);
        assert_eq!(
            body4,
            "Did you see [#Mario Galaxy](topic://_forum.games.mario galaxy)?"
        );
        assert_eq!(uris4, vec!["topic://_forum.games.mario galaxy"]);
    }
}
