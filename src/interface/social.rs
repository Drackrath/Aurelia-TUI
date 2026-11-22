//! Social CLI calls and JSON types.

use std::borrow::Cow;

use serde::Deserialize;

use super::aurelia::run_json;
use crate::util::error::STError;

/// Lowest `config persona-wait` value Aurelia accepts.
pub const PERSONA_WAIT_MIN: u64 = 1;
/// Highest `config persona-wait` value Aurelia accepts.
pub const PERSONA_WAIT_MAX: u64 = 300;
/// Aurelia's default `persona_wait_secs`.
pub const PERSONA_WAIT_DEFAULT: u64 = 30;

/// Aurelia's label for EPersonaState.
pub fn persona_state_label(state: Option<u32>) -> &'static str {
    match state {
        Some(0) | None => "offline",
        Some(1) => "online",
        Some(2) => "busy",
        Some(3) => "away",
        Some(4) => "snooze",
        Some(5) => "looking to trade",
        Some(6) => "looking to play",
        Some(7) => "invisible",
        Some(_) => "online",
    }
}

/// Profile from `aurelia user --json`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserProfileJson {
    #[serde(default)]
    pub steam_id: u64,
    #[serde(default)]
    pub persona_name: Option<String>,
    #[serde(default)]
    pub profile_url: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// 1 private, 2 friends-only, 3 public.
    #[serde(default)]
    pub visibility: i32,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub is_limited: bool,
    #[serde(default)]
    pub ban_expires: Option<u64>,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub real_name: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub headline: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub persona_state: Option<u32>,
    #[serde(default)]
    pub game_app_id: Option<u32>,
    #[serde(default)]
    pub game_name: Option<String>,
    #[serde(default)]
    pub last_logoff: Option<u64>,
    /// Which Steam sources answered the lookup.
    #[serde(default)]
    pub sources: Vec<String>,
}

impl UserProfileJson {
    /// Persona name, else SteamID.
    pub fn display_name(&self) -> String {
        self.persona_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.steam_id.to_string())
    }

    /// Privacy label like `aurelia user`.
    pub fn visibility_label(&self) -> &'static str {
        match self.visibility {
            3 => "public",
            2 => "friends-only",
            1 => "private",
            _ => "unknown",
        }
    }

    /// Status label for `persona_state`.
    pub fn status_label(&self) -> &'static str {
        persona_state_label(self.persona_state)
    }

    /// Current game name or app.
    pub fn playing(&self) -> Option<String> {
        match (&self.game_name, self.game_app_id) {
            (Some(g), _) if !g.is_empty() => Some(g.clone()),
            (_, Some(id)) => Some(format!("app {id}")),
            _ => None,
        }
    }
}

/// Reply from `chat invite --json`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InviteSentJson {
    #[serde(default)]
    pub steamid: u64,
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub sent: bool,
}

/// Reply from `aurelia config persona-wait --json`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PersonaWaitJson {
    #[serde(default)]
    pub persona_wait_secs: Option<u64>,
}

/// Arguments for `chat invite` (without `--json`).
pub fn invite_args(steamid: u64, app_id: u32, connect: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "chat".to_string(),
        "invite".to_string(),
        steamid.to_string(),
        "--app".to_string(),
        app_id.to_string(),
    ];
    if let Some(connect) = connect.map(str::trim).filter(|c| !c.is_empty()) {
        args.push("--connect".to_string());
        args.push(connect.to_string());
    }
    args
}

fn run_owned(args: &[String]) -> Result<serde_json::Value, STError> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_json(&refs)
}

/// Invite; errors carry Aurelia's refusal.
pub fn chat_invite(
    steamid: u64,
    app_id: u32,
    connect: Option<&str>,
) -> Result<InviteSentJson, STError> {
    let value = run_owned(&invite_args(steamid, app_id, connect))?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch `aurelia user <ident|me>`.
pub fn user_profile(ident: &str) -> Result<UserProfileJson, STError> {
    let value = run_json(&["user", ident])?;
    Ok(serde_json::from_value(value)?)
}

/// Set `config persona-wait`; returns stored.
pub fn set_persona_wait(seconds: u64) -> Result<u64, STError> {
    let value = run_owned(&[
        "config".to_string(),
        "persona-wait".to_string(),
        seconds.to_string(),
    ])?;
    let parsed: PersonaWaitJson = serde_json::from_value(value)?;
    Ok(parsed.persona_wait_secs.unwrap_or(seconds))
}

/// Validate typed persona-wait seconds (1-300).
pub fn parse_persona_wait(input: &str) -> Result<u64, String> {
    let secs: u64 = input
        .trim()
        .parse()
        .map_err(|_| format!("enter whole seconds ({PERSONA_WAIT_MIN}-{PERSONA_WAIT_MAX})"))?;
    if (PERSONA_WAIT_MIN..=PERSONA_WAIT_MAX).contains(&secs) {
        Ok(secs)
    } else {
        Err(format!(
            "must be {PERSONA_WAIT_MIN}-{PERSONA_WAIT_MAX} seconds"
        ))
    }
}

/// Read a BBCode attribute value.
fn bbcode_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Show BBCode invites in history readably.
pub fn chat_display_text(message: &str) -> Cow<'_, str> {
    let trimmed = message.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("[gameinvite") || lower.starts_with("[lobbyinvite")) {
        return Cow::Borrowed(message);
    }
    let tag = trimmed.split(']').next().unwrap_or(trimmed);
    let app = bbcode_attr(tag, "appid").filter(|a| !a.is_empty() && *a != "0");
    Cow::Owned(match app {
        Some(app) => format!("* sent a game invite (app {app})"),
        None => "* sent a game invite".to_string(),
    })
}
