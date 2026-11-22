//! Reading an email as a command: what the person wrote (without the quoted
//! history or the signature), what it asks for, and whether the sender is
//! vouched for by their mail provider. Pure functions, no I/O.

use mellowmesh_core::decision::{DecisionOption, DecisionOutcome};
use serde::{Deserialize, Serialize};

/// What an email asks the hub to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    /// `@name text`: a note to an agent.
    Note { agent: String, text: String },
    /// `task: title`, with the rest of the email as details.
    Task { title: String, details: String },
    /// `status`: what waits for the person.
    Status,
    /// `help`: the list of commands.
    Help,
    /// Text written in a thread that already knows where it goes (an
    /// answer to a decision, a reply to an agent or a conversation).
    Reply { text: String },
    /// Nothing the hub understands.
    Unknown,
}

/// The subject without `Re:`, `Fwd:` and their translations.
pub fn clean_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        let cut = ["re:", "fwd:", "fw:", "tr:", "aw:", "wg:", "réf:", "ref:"]
            .iter()
            .find(|p| lower.starts_with(*p))
            .map(|p| p.len());
        match cut {
            // `len()` of the lowercase prefix equals the original's: every
            // prefix is ASCII except `é`, which lowercases to itself.
            Some(n) => s = s[n..].trim_start(),
            None => return s.to_string(),
        }
    }
}

/// True for the line that introduces a quoted message or a signature.
fn starts_history(line: &str, next: Option<&str>) -> bool {
    let t = line.trim();
    let lower = t.to_lowercase();
    if t.starts_with('>') || t == "--" || t == "-- " || line == "-- " {
        return true;
    }
    if lower.contains("-----original message-----")
        || lower.contains("-----message d'origine-----")
        || lower.contains("-----message d’origine-----")
        || lower.starts_with("sent from my")
        || lower.starts_with("envoyé de mon")
        || lower.starts_with("envoyé depuis")
        || lower.starts_with("get outlook for")
    {
        return true;
    }
    // "On Mon, 22 Nov 2026, Léa <lea@…> wrote:" — sometimes cut in two.
    if lower.starts_with("on ")
        && (lower.ends_with("wrote:")
            || next.is_some_and(|n| n.trim().eq_ignore_ascii_case("wrote:")))
    {
        return true;
    }
    if lower.starts_with("le ")
        && (lower.ends_with("a écrit :")
            || lower.ends_with("a écrit:")
            || next.is_some_and(|n| n.trim().starts_with("a écrit")))
    {
        return true;
    }
    // Outlook's header block of the quoted email.
    (lower.starts_with("from:") || lower.starts_with("de :") || lower.starts_with("de:"))
        && next.is_some_and(|n| {
            let n = n.trim().to_lowercase();
            n.starts_with("sent:")
                || n.starts_with("envoyé :")
                || n.starts_with("envoyé:")
                || n.starts_with("date:")
                || n.starts_with("to:")
        })
}

/// What the person wrote, without the quoted history and the signature.
pub fn strip_quoted(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut kept: Vec<&str> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if starts_history(line, lines.get(i + 1).copied()) {
            break;
        }
        kept.push(line.trim_end());
    }
    kept.join("\n").trim().to_string()
}

/// Split `head` (one line) and `rest` into a command, if `head` is one.
fn command_in(head: &str, rest: &str) -> Option<Command> {
    let head = head.trim();
    let lower = head.to_lowercase();
    let join = |a: &str, b: &str| {
        let (a, b) = (a.trim(), b.trim());
        match (a.is_empty(), b.is_empty()) {
            (true, _) => b.to_string(),
            (false, true) => a.to_string(),
            (false, false) => format!("{a}\n{b}"),
        }
    };
    if let Some(after) = head.strip_prefix('@') {
        // `@[Name with spaces] text` or `@name text`.
        let (agent, text) = if let Some(inner) = after.strip_prefix('[') {
            let end = inner.find(']')?;
            (&inner[..end], &inner[end + 1..])
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        let agent = agent
            .trim()
            .trim_end_matches([',', ':', ';', '.'])
            .to_string();
        let text = join(text.trim_start_matches([',', ':', ';']), rest);
        if agent.is_empty() || text.is_empty() {
            return Some(Command::Unknown);
        }
        return Some(Command::Note { agent, text });
    }
    for prefix in ["task:", "tâche:", "tache:", "task :", "tâche :", "tache :"] {
        if lower.starts_with(prefix) {
            let title = head[prefix.len()..].trim().to_string();
            if title.is_empty() {
                return Some(Command::Unknown);
            }
            return Some(Command::Task {
                title,
                details: rest.trim().to_string(),
            });
        }
    }
    let word = lower.trim_end_matches(['.', '!', '?']).trim();
    if matches!(word, "status" | "statut" | "état" | "etat") {
        return Some(Command::Status);
    }
    if matches!(word, "help" | "aide") || lower == "?" {
        return Some(Command::Help);
    }
    None
}

/// Read an email as a command. `in_thread` is true when it replies to an
/// email of the hub that knows where the answer goes.
pub fn parse(subject: &str, text: &str, in_thread: bool) -> Command {
    let body = strip_quoted(text);
    if in_thread {
        return if body.is_empty() {
            Command::Unknown
        } else {
            Command::Reply { text: body }
        };
    }
    let subject = clean_subject(subject);
    // A command in the subject, with the body as its text.
    if let Some(cmd) = command_in(&subject, &body) {
        return cmd;
    }
    // Otherwise the first line of the body.
    let mut lines = body.lines().skip_while(|l| l.trim().is_empty());
    let head = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    command_in(head, &rest.join("\n")).unwrap_or(Command::Unknown)
}

/// The option an answer picks: its number, id or name (or the start of
/// it, when only one fits), or yes / no for the approve / reject option.
pub fn pick_option(options: &[DecisionOption], answer: &str) -> Option<String> {
    let first = answer.lines().map(str::trim).find(|l| !l.is_empty())?;
    let a = first
        .trim_end_matches(['.', '!', '?', ','])
        .trim()
        .to_lowercase();
    if a.is_empty() {
        return None;
    }
    if let Ok(n) = a.parse::<usize>() {
        return options.get(n.checked_sub(1)?).map(|o| o.id.clone());
    }
    if let Some(o) = options
        .iter()
        .find(|o| o.id.to_lowercase() == a || o.label.trim().to_lowercase() == a)
    {
        return Some(o.id.clone());
    }
    let by_outcome = |want: DecisionOutcome| {
        let found: Vec<&DecisionOption> =
            options.iter().filter(|o| o.outcome == Some(want)).collect();
        (found.len() == 1).then(|| found[0].id.clone())
    };
    let word = a
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches([',', '.', '!', ';', ':']);
    if matches!(
        word,
        "yes"
            | "y"
            | "oui"
            | "ok"
            | "okay"
            | "approve"
            | "approved"
            | "go"
            | "approuver"
            | "d'accord"
    ) {
        if let Some(id) = by_outcome(DecisionOutcome::Approve) {
            return Some(id);
        }
    }
    if matches!(
        word,
        "no" | "n" | "non" | "reject" | "rejected" | "refuser" | "refuse" | "stop"
    ) {
        if let Some(id) = by_outcome(DecisionOutcome::Reject) {
            return Some(id);
        }
    }
    let starts: Vec<&DecisionOption> = options
        .iter()
        .filter(|o| o.label.trim().to_lowercase().starts_with(&a))
        .collect();
    (starts.len() == 1).then(|| starts[0].id.clone())
}

/// The domain of an address, lowercase.
pub fn domain_of(address: &str) -> String {
    address
        .rsplit_once('@')
        .map(|(_, d)| d.trim().to_lowercase())
        .unwrap_or_default()
}

/// Whether `d` is `domain` or a parent of it (DMARC relaxed alignment).
fn aligned(d: &str, domain: &str) -> bool {
    !d.is_empty() && (domain == d || domain.ends_with(&format!(".{d}")))
}

/// The header without its `( … )` comments (nested ones too) and quotes:
/// a comment must never carry a result.
fn strip_comments(header: &str) -> String {
    let mut out = String::with_capacity(header.len());
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for c in header.chars() {
        if escaped {
            escaped = false;
            if depth == 0 {
                out.push(c);
            }
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' if depth == 0 => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Whether the hub's own mail provider vouched for the sender: the topmost
/// `Authentication-Results` header comes from `provider_id` (the provider's
/// authserv-id, e.g. `mx.google.com`, which adds that header on delivery
/// and drops forged copies of it) and reports DMARC, or an aligned DKIM
/// signature, as passing for the sender's domain. Without a `provider_id`
/// nothing is vouched for: a provider that adds no such header would let a
/// sender's own forged header through.
pub fn provider_vouches(auth_results: Option<&str>, sender: &str, provider_id: &str) -> bool {
    let (Some(ar), provider_id) = (auth_results, provider_id.trim().to_lowercase()) else {
        return false;
    };
    let domain = domain_of(sender);
    if domain.is_empty() || provider_id.is_empty() {
        return false;
    }
    let ar = strip_comments(ar)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut parts = ar.split(';');
    // The first part is the authserv-id (and an optional version).
    let authserv = parts
        .next()
        .and_then(|p| p.split_whitespace().next())
        .unwrap_or("")
        .to_lowercase();
    if authserv != provider_id {
        return false;
    }
    for part in parts {
        let p = part.trim().to_lowercase();
        let value = |key: &str| {
            p.split_whitespace()
                .find_map(|w| w.strip_prefix(key))
                .map(|v| v.trim_matches(['"', '(', ')', ';']).to_string())
        };
        if p.starts_with("dmarc=pass") {
            if let Some(from) = value("header.from=") {
                if aligned(&from, &domain) {
                    return true;
                }
            }
        }
        if p.starts_with("dkim=pass") {
            let d = value("header.d=")
                .or_else(|| value("header.i=").map(|i| i.trim_start_matches('@').to_string()));
            if let Some(d) = d {
                if aligned(&domain_of(&format!("x@{d}")), &domain) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &str, label: &str, outcome: Option<DecisionOutcome>) -> DecisionOption {
        DecisionOption {
            id: id.into(),
            label: label.into(),
            pros: vec![],
            cons: vec![],
            outcome,
        }
    }

    fn options() -> Vec<DecisionOption> {
        vec![
            opt("option_1", "Ship it", Some(DecisionOutcome::Approve)),
            opt("option_2", "Hold", Some(DecisionOutcome::Reject)),
        ]
    }

    #[test]
    fn subjects_lose_their_reply_prefixes() {
        assert_eq!(clean_subject("Re: RE: Fwd: status"), "status");
        assert_eq!(clean_subject("TR : hello"), "TR : hello");
        assert_eq!(clean_subject("tr: @coder go"), "@coder go");
    }

    #[test]
    fn quoted_history_and_signatures_are_dropped() {
        let gmail = "1\n\nOn Mon, 22 Nov 2026 at 08:31, MellowMesh <hub@x.com> wrote:\n> Deploy?\n";
        assert_eq!(strip_quoted(gmail), "1");
        let split =
            "Yes please\nOn Mon, 22 Nov 2026 at 08:31, MellowMesh <hub@x.com>\nwrote:\n> hi";
        assert_eq!(strip_quoted(split), "Yes please");
        let fr = "Oui\r\n\r\nLe lun. 22 nov. 2026 à 08:31, MellowMesh <hub@x.com> a écrit :\r\n> Déployer ?";
        assert_eq!(strip_quoted(fr), "Oui");
        let outlook = "2\n\nFrom: MellowMesh <hub@x.com>\nSent: Monday\nTo: me\n";
        assert_eq!(strip_quoted(outlook), "2");
        let sig = "@coder run the tests\n-- \nLéa\nSent from my phone";
        assert_eq!(strip_quoted(sig), "@coder run the tests");
        assert_eq!(strip_quoted("hello\nSent from my iPhone"), "hello");
    }

    #[test]
    fn commands_come_from_the_subject_or_the_first_line() {
        assert_eq!(
            parse("@coder run the tests", "", false),
            Command::Note {
                agent: "coder".into(),
                text: "run the tests".into()
            }
        );
        assert_eq!(
            parse("Tests", "@coder: run them\nand tell me", false),
            Command::Note {
                agent: "coder".into(),
                text: "run them\nand tell me".into()
            }
        );
        assert_eq!(
            parse("", "@[Night Owl] watch the logs", false),
            Command::Note {
                agent: "Night Owl".into(),
                text: "watch the logs".into()
            }
        );
        assert_eq!(
            parse("@coder", "please look at the build", false),
            Command::Note {
                agent: "coder".into(),
                text: "please look at the build".into()
            }
        );
        assert_eq!(
            parse("task: renew the domain", "before Friday", false),
            Command::Task {
                title: "renew the domain".into(),
                details: "before Friday".into()
            }
        );
        assert_eq!(
            parse("", "Tâche : appeler la banque", false),
            Command::Task {
                title: "appeler la banque".into(),
                details: String::new()
            }
        );
        assert_eq!(parse("Status", "", false), Command::Status);
        assert_eq!(parse("", "état", false), Command::Status);
        assert_eq!(parse("Re: help", "", false), Command::Help);
        assert_eq!(parse("", "aide", false), Command::Help);
        assert_eq!(parse("Hello", "How are you?", false), Command::Unknown);
        assert_eq!(parse("@coder", "", false), Command::Unknown);
        assert_eq!(parse("task:", "", false), Command::Unknown);
    }

    #[test]
    fn a_reply_in_a_thread_is_its_text() {
        assert_eq!(
            parse("Re: Decision", "1\n\nOn x wrote:\n> q", true),
            Command::Reply { text: "1".into() }
        );
        assert_eq!(parse("Re: x", "> only quotes", true), Command::Unknown);
    }

    #[test]
    fn answers_pick_an_option() {
        let o = options();
        assert_eq!(pick_option(&o, "1").as_deref(), Some("option_1"));
        assert_eq!(pick_option(&o, "2.").as_deref(), Some("option_2"));
        assert_eq!(pick_option(&o, "3"), None);
        assert_eq!(pick_option(&o, "0"), None);
        assert_eq!(pick_option(&o, "ship it").as_deref(), Some("option_1"));
        assert_eq!(pick_option(&o, "Hold!").as_deref(), Some("option_2"));
        assert_eq!(pick_option(&o, "option_2").as_deref(), Some("option_2"));
        assert_eq!(
            pick_option(&o, "Yes, go ahead").as_deref(),
            Some("option_1")
        );
        assert_eq!(pick_option(&o, "oui").as_deref(), Some("option_1"));
        assert_eq!(pick_option(&o, "non merci").as_deref(), Some("option_2"));
        assert_eq!(pick_option(&o, "sh").as_deref(), Some("option_1"));
        assert_eq!(pick_option(&o, "maybe later"), None);
        assert_eq!(
            pick_option(&o, "\n\n2\nthanks").as_deref(),
            Some("option_2")
        );
        // yes / no need exactly one approve / reject option.
        let two_yes = vec![
            opt("a", "Now", Some(DecisionOutcome::Approve)),
            opt("b", "Tonight", Some(DecisionOutcome::Approve)),
        ];
        assert_eq!(pick_option(&two_yes, "yes"), None);
    }

    #[test]
    fn the_provider_vouches_only_for_an_aligned_pass() {
        let gmail = "mx.google.com;\r\n       dkim=pass header.i=@studio.fr header.s=s1 header.b=abc;\r\n       spf=pass smtp.mailfrom=lea@studio.fr;\r\n       dmarc=pass (p=NONE sp=NONE dis=NONE) header.from=studio.fr";
        let g = "mx.google.com";
        assert!(provider_vouches(Some(gmail), "lea@studio.fr", g));
        assert!(provider_vouches(Some(gmail), "lea@mail.studio.fr", g));
        assert!(!provider_vouches(Some(gmail), "lea@evil.fr", g));
        let fail = "mx.google.com; dkim=fail header.d=studio.fr; dmarc=fail header.from=studio.fr";
        assert!(!provider_vouches(Some(fail), "lea@studio.fr", g));
        let dkim_only = "mx.example; dkim=pass header.d=studio.fr";
        assert!(provider_vouches(
            Some(dkim_only),
            "lea@studio.fr",
            "mx.example"
        ));
        let other_dkim = "mx.example; dkim=pass header.d=mailer.com";
        assert!(!provider_vouches(
            Some(other_dkim),
            "lea@studio.fr",
            "mx.example"
        ));
        // A result in the provider-id position is not a result.
        assert!(!provider_vouches(
            Some("dmarc=pass header.from=studio.fr"),
            "lea@studio.fr",
            "dmarc=pass"
        ));
        assert!(!provider_vouches(None, "lea@studio.fr", g));
        assert!(!provider_vouches(Some(gmail), "no-domain", g));
    }

    #[test]
    fn only_the_named_provider_is_believed() {
        // No provider named: nothing is vouched for.
        let real = "mx.google.com; dmarc=pass header.from=studio.fr";
        assert!(!provider_vouches(Some(real), "lea@studio.fr", ""));
        // A header written by someone else (a sender's forged copy reaching a
        // provider that adds none) names another server: refused.
        let forged = "mx.attacker.example; dmarc=pass header.from=studio.fr";
        assert!(!provider_vouches(
            Some(forged),
            "lea@studio.fr",
            "mx.google.com"
        ));
        assert!(!provider_vouches(
            Some(forged),
            "lea@studio.fr",
            "mx.google"
        ));
        // A pass hidden in a comment is not a result.
        let hidden =
            "mx.google.com; dkim=fail (x; dmarc=pass header.from=studio.fr) header.d=studio.fr";
        assert!(!provider_vouches(
            Some(hidden),
            "lea@studio.fr",
            "mx.google.com"
        ));
        let nested = "mx.google.com; spf=none ((a) ; dkim=pass header.d=studio.fr)";
        assert!(!provider_vouches(
            Some(nested),
            "lea@studio.fr",
            "mx.google.com"
        ));
        // The authserv-id may carry a version.
        let versioned = "mx.google.com 1; dmarc=pass header.from=studio.fr";
        assert!(provider_vouches(
            Some(versioned),
            "lea@studio.fr",
            "MX.Google.com"
        ));
        assert_eq!(strip_comments("a (b (c) d) e \"(f)\""), "a  e (f)");
    }
}
