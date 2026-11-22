//! `logout --json` result (v0.2.1).

use serde::Deserialize;

/// What `aurelia logout --json` reports.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogoutJson {
    #[serde(default)]
    pub logged_out: bool,
    /// `revoked`, `none` or `not_revoked`.
    #[serde(default)]
    pub token: Option<String>,
    /// Why revocation failed.
    #[serde(default)]
    pub token_error: Option<String>,
}

impl LogoutJson {
    /// One status line for the user.
    pub fn summary(&self) -> String {
        match self.token.as_deref() {
            Some("revoked") => "Logged out; refresh token revoked at Steam.".to_string(),
            Some("none") => "Logged out; no refresh token was stored.".to_string(),
            Some("not_revoked") => match self.token_error.as_deref() {
                Some(e) if !e.is_empty() => {
                    format!("Logged out, but Steam did not revoke the token: {e}")
                }
                _ => "Logged out, but Steam did not revoke the token.".to_string(),
            },
            _ => "Logged out.".to_string(),
        }
    }
}
