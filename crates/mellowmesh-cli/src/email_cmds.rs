//! `mellowmesh email`: give the hub its own mailbox, choose who may write
//! to it, check it, and send an email to a person on the list.

use crate::commands::as_local_human;
use mellowmesh_client::sanitize::sanitize_line;
use mellowmesh_client::MellowMeshClient;

fn on_off(v: &serde_json::Value) -> &'static str {
    if v.as_bool().unwrap_or(false) {
        "on"
    } else {
        "off"
    }
}

fn text(v: &serde_json::Value) -> String {
    sanitize_line(v.as_str().unwrap_or(""))
}

fn server_line(v: &serde_json::Value) -> String {
    format!(
        "{}:{} ({})",
        text(&v["host"]),
        v["port"].as_u64().unwrap_or(0),
        text(&v["security"])
    )
}

fn print_view(v: &serde_json::Value) {
    let s = &v["settings"];
    let address = text(&s["address"]);
    if address.is_empty() {
        println!("The hub has no mailbox yet.");
        println!("  mellowmesh email set --address hub@example.com --me you@example.com --password-stdin");
        return;
    }
    println!("Email:        {}", on_off(&s["enabled"]));
    println!("Hub address:  {address}");
    let login = text(&s["username"]);
    if !login.is_empty() {
        println!("Login:        {login}");
    }
    println!(
        "Reads:        {} folder {}",
        server_line(&s["imap"]),
        text(&s["folder"])
    );
    println!("Sends:        {}", server_line(&s["smtp"]));
    let password = if v["password_from_env"].as_bool().unwrap_or(false) {
        "from the hub's environment"
    } else if v["password_set"].as_bool().unwrap_or(false) {
        "saved"
    } else {
        "missing"
    };
    println!("App password: {password}");
    println!(
        "Every:        {} s · decisions {} · mentions {} · provider proof {}",
        s["poll_seconds"].as_u64().unwrap_or(30),
        on_off(&s["notify_decisions"]),
        on_off(&s["notify_mentions"]),
        on_off(&s["trust_provider"])
    );
    let provider = text(&s["provider_id"]);
    println!(
        "Provider:     {}",
        if provider.is_empty() {
            "none named: commands not sent as replies are confirmed by reply".to_string()
        } else {
            format!("{provider} (its Authentication-Results are believed)")
        }
    );
    let st = &v["status"];
    if let Some(at) = st["last_check"].as_str() {
        println!("Last read:    {}", sanitize_line(at));
    }
    if let Some(e) = st["last_error"].as_str() {
        println!("Last error:   {}", sanitize_line(e));
    }
    println!(
        "Received {} · ignored {} · sent {}",
        st["received"].as_u64().unwrap_or(0),
        st["ignored"].as_u64().unwrap_or(0),
        st["sent"].as_u64().unwrap_or(0)
    );
    println!();
    println!("Who can give commands by email:");
    match v["people"].as_array() {
        Some(list) if !list.is_empty() => {
            for p in list {
                println!("  {:<32} {}", text(&p["address"]), text(&p["person"]));
            }
        }
        _ => println!("  nobody yet: mellowmesh email allow you@example.com"),
    }
}

pub async fn run_show(client: &MellowMeshClient) -> anyhow::Result<()> {
    let v = as_local_human(client).get_email().await?;
    print_view(&v);
    Ok(())
}

/// `host:port` or `host:port:security` (tls, starttls, none).
fn parse_server(spec: &str) -> anyhow::Result<serde_json::Value> {
    let mut parts = spec.split(':');
    let host = parts.next().unwrap_or("").trim();
    let port: u16 = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Write the server as host:port[:tls|starttls|none]"))?
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("'{spec}': the port is not a number"))?;
    let security = parts.next().unwrap_or(match port {
        143 | 587 | 25 => "starttls",
        _ => "tls",
    });
    anyhow::ensure!(
        matches!(security, "tls" | "starttls" | "none"),
        "'{security}': use tls, starttls or none"
    );
    Ok(serde_json::json!({ "host": host, "port": port, "security": security }))
}

fn switch(v: &str) -> anyhow::Result<bool> {
    match v.to_lowercase().as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        _ => anyhow::bail!("'{v}': use on or off"),
    }
}

#[derive(Debug, Default, Clone)]
pub struct SetArgs {
    pub address: Option<String>,
    pub me: Option<String>,
    pub login: Option<String>,
    pub imap: Option<String>,
    pub smtp: Option<String>,
    pub folder: Option<String>,
    pub every: Option<u64>,
    pub decisions: Option<String>,
    pub mentions: Option<String>,
    pub trust_provider: Option<String>,
    pub provider_id: Option<String>,
    pub password_stdin: bool,
    pub forget_password: bool,
}

pub async fn run_set(client: &MellowMeshClient, a: SetArgs) -> anyhow::Result<()> {
    let mut patch = serde_json::Map::new();
    let mut put = |k: &str, v: serde_json::Value| {
        patch.insert(k.to_string(), v);
    };
    if let Some(v) = a.address {
        put("address", v.into());
    }
    if let Some(v) = a.me {
        put("owner_address", v.into());
    }
    if let Some(v) = a.login {
        put("username", v.into());
    }
    if let Some(v) = a.imap {
        put("imap", parse_server(&v)?);
    }
    if let Some(v) = a.smtp {
        put("smtp", parse_server(&v)?);
    }
    if let Some(v) = a.folder {
        put("folder", v.into());
    }
    if let Some(v) = a.every {
        put("poll_seconds", v.into());
    }
    if let Some(v) = a.decisions {
        put("notify_decisions", switch(&v)?.into());
    }
    if let Some(v) = a.mentions {
        put("notify_mentions", switch(&v)?.into());
    }
    if let Some(v) = a.trust_provider {
        put("trust_provider", switch(&v)?.into());
    }
    if let Some(v) = a.provider_id {
        put("provider_id", v.into());
    }
    if a.forget_password {
        put("forget_password", true.into());
    }
    if a.password_stdin {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            eprint!("App password for the hub's mailbox (shown as you type): ");
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let password = line.trim_end_matches(['\r', '\n']).to_string();
        anyhow::ensure!(!password.is_empty(), "No password was given");
        put("password", password.into());
    }
    anyhow::ensure!(
        !patch.is_empty(),
        "Nothing to change (see mellowmesh email set --help)"
    );
    let v = as_local_human(client)
        .set_email(serde_json::Value::Object(patch))
        .await?;
    print_view(&v);
    if !v["settings"]["enabled"].as_bool().unwrap_or(false)
        && v["password_set"].as_bool().unwrap_or(false)
    {
        println!();
        println!("Check it, then turn it on:  mellowmesh email check  ·  mellowmesh email on");
    }
    Ok(())
}

pub async fn run_switch(client: &MellowMeshClient, on: bool) -> anyhow::Result<()> {
    let v = as_local_human(client)
        .set_email(serde_json::json!({ "enabled": on }))
        .await?;
    if on {
        println!(
            "Email is on: the hub reads {} every {} s.",
            text(&v["settings"]["address"]),
            v["settings"]["poll_seconds"].as_u64().unwrap_or(30)
        );
    } else {
        println!("Email is off: the hub neither reads nor sends email.");
    }
    Ok(())
}

pub async fn run_check(client: &MellowMeshClient) -> anyhow::Result<()> {
    let v = as_local_human(client).check_email().await?;
    for (key, label) in [("inbox", "Reads its inbox"), ("send", "Sends email")] {
        let ok = v[key]["ok"].as_bool().unwrap_or(false);
        println!(
            "{} {label}: {}",
            if ok { "✓" } else { "✗" },
            text(&v[key]["detail"])
        );
    }
    Ok(())
}

pub async fn run_allow(
    client: &MellowMeshClient,
    address: &str,
    person: Option<String>,
) -> anyhow::Result<()> {
    let v = as_local_human(client)
        .set_email_person(address, person.as_deref(), false)
        .await?;
    println!("{} may now give commands by email.", sanitize_line(address));
    let _ = v;
    Ok(())
}

pub async fn run_forget(client: &MellowMeshClient, address: &str) -> anyhow::Result<()> {
    as_local_human(client)
        .set_email_person(address, None, true)
        .await?;
    println!(
        "{} can no longer give commands by email.",
        sanitize_line(address)
    );
    Ok(())
}

pub async fn run_send(
    client: &MellowMeshClient,
    to: &str,
    subject: &str,
    body: &[String],
) -> anyhow::Result<()> {
    let body = body.join(" ");
    let v = client.send_email(to, subject, &body).await?;
    println!("Sent to {}.", text(&v["sent_to"]));
    Ok(())
}

/// The catalog id for the short names people type.
pub fn invite_platform(ai: &str) -> String {
    match ai.trim().to_lowercase().as_str() {
        "claude" | "claude.ai" => "claude-web".into(),
        "gemini" => "gemini-web".into(),
        "le-chat" | "lechat" | "le chat" => "mistral".into(),
        "grokbot" => "grok".into(),
        other => other.to_string(),
    }
}

/// `mellowmesh invite`: a one-click link for an AI.
pub async fn run_invite(
    client: &MellowMeshClient,
    ai: Option<&str>,
    name: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        ai.is_some() || name.is_some(),
        "Say which AI: mellowmesh invite grok (chatgpt, claude, gemini, mistral), or --name Buzz"
    );
    let platform = ai.map(invite_platform);
    let v = as_local_human(client)
        .invite_ai(platform.as_deref(), name)
        .await?;
    println!("{}'s link (paste it in its chat):", text(&v["name"]));
    println!();
    println!("  {}", text(&v["join_url"]));
    println!();
    println!("Or this message: {}", text(&v["message"]));
    println!(
        "It opens {} times, until {}. It gives {} its own limited key, never yours.",
        v["opens"].as_u64().unwrap_or(0),
        text(&v["expires_at"]),
        text(&v["principal"])
    );
    if !v["reachable_from_web"].as_bool().unwrap_or(true) {
        println!();
        println!("Your hub has no public address: web assistants cannot open this link (a bot on");
        println!(
            "the same network can). Start the hub with --public-host <tunnel name> or the relay."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ai_names_map_to_the_catalog() {
        assert_eq!(invite_platform("Claude"), "claude-web");
        assert_eq!(invite_platform("gemini"), "gemini-web");
        assert_eq!(invite_platform("GrokBot"), "grok");
        assert_eq!(invite_platform("chatgpt"), "chatgpt");
    }

    #[test]
    fn servers_are_host_port_and_security() {
        let v = parse_server("imap.example.com:993").unwrap();
        assert_eq!(v["security"], "tls");
        let v = parse_server("smtp.example.com:587").unwrap();
        assert_eq!(v["security"], "starttls");
        let v = parse_server("127.0.0.1:1143:none").unwrap();
        assert_eq!(v["port"], 1143);
        assert_eq!(v["security"], "none");
        assert!(parse_server("imap.example.com").is_err());
        assert!(parse_server("imap.example.com:x").is_err());
        assert!(parse_server("imap.example.com:993:ssl").is_err());
    }
}
