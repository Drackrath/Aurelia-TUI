//! Invite, profile and persona-wait state.

use std::sync::{Arc, Mutex};
use std::thread;

use crate::browse::Browser;
use crate::interface::social::{self, UserProfileJson};

/// Which invite field receives typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InviteField {
    #[default]
    App,
    Connect,
}

/// The open "Invite to game" overlay.
#[derive(Debug, Clone, Default)]
pub struct InviteDraft {
    pub steamid: u64,
    pub friend_name: String,
    /// App id, prefilled from library.
    pub app_input: String,
    /// Library name for the prefilled app.
    pub game_name: Option<String>,
    pub connect: String,
    pub field: InviteField,
    /// Progress, success or Aurelia's refusal.
    pub status: Arc<Mutex<String>>,
}

impl InviteDraft {
    /// The current status line.
    pub fn status_text(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// Result slot for a profile lookup.
pub type ProfileSlot = Arc<Mutex<Option<Result<UserProfileJson, String>>>>;

/// The open read-only profile overlay.
#[derive(Debug, Clone, Default)]
pub struct ProfileView {
    /// The `user` argument (`me` or SteamID).
    pub ident: String,
    /// Name shown while loading.
    pub label: String,
    pub result: ProfileSlot,
    pub scroll: u16,
}

impl ProfileView {
    /// A finished lookup, if any.
    pub fn finished(&self) -> Option<Result<UserProfileJson, String>> {
        self.result.lock().ok().and_then(|r| r.clone())
    }
}

/// All social state hung off `Browser`.
#[derive(Debug, Clone, Default)]
pub struct SocialState {
    pub invite: Option<InviteDraft>,
    pub profile: Option<ProfileView>,
    /// Settings persona-wait editor input.
    pub wait_input: Option<String>,
}

impl Browser {
    // --- Game invite ---

    /// Invite the selected friend.
    pub fn open_invite(&mut self) {
        let Some(friend) = self.selected_friend() else {
            return;
        };
        let (steamid, friend_name) = (friend.steam_id, friend.display_name());
        let game = self.selected();
        self.social.invite = Some(InviteDraft {
            steamid,
            friend_name,
            app_input: game
                .as_ref()
                .filter(|g| g.id > 0)
                .map(|g| g.id.to_string())
                .unwrap_or_default(),
            game_name: game.map(|g| g.name),
            ..Default::default()
        });
    }

    /// Close the invite overlay.
    pub fn close_invite(&mut self) {
        self.social.invite = None;
    }

    /// Toggle app / connect field.
    pub fn invite_toggle_field(&mut self) {
        if let Some(draft) = self.social.invite.as_mut() {
            draft.field = match draft.field {
                InviteField::App => InviteField::Connect,
                InviteField::Connect => InviteField::App,
            };
        }
    }

    /// Type into the focused invite field.
    pub fn invite_push(&mut self, c: char) {
        let Some(draft) = self.social.invite.as_mut() else {
            return;
        };
        match draft.field {
            InviteField::App if c.is_ascii_digit() => {
                draft.app_input.push(c);
                draft.game_name = None;
            }
            InviteField::App => {}
            InviteField::Connect => draft.connect.push(c),
        }
    }

    /// Backspace the focused invite field.
    pub fn invite_pop(&mut self) {
        let Some(draft) = self.social.invite.as_mut() else {
            return;
        };
        match draft.field {
            InviteField::App => {
                draft.app_input.pop();
                draft.game_name = None;
            }
            InviteField::Connect => {
                draft.connect.pop();
            }
        }
    }

    /// Send the invite off-thread.
    pub fn invite_send(&mut self) {
        let Some(draft) = self.social.invite.as_ref() else {
            return;
        };
        let set = |status: &Arc<Mutex<String>>, text: String| {
            if let Ok(mut s) = status.lock() {
                *s = text;
            }
        };
        let app_id = match draft.app_input.trim().parse::<u32>() {
            Ok(id) if id > 0 => id,
            _ => {
                set(
                    &draft.status,
                    "Enter the app id of your running game.".into(),
                );
                return;
            }
        };
        let status = draft.status.clone();
        let steamid = draft.steamid;
        let name = draft.friend_name.clone();
        let connect = draft.connect.clone();
        set(&status, format!("Inviting {name} to app {app_id}…"));
        thread::spawn(move || {
            let text = match social::chat_invite(steamid, app_id, Some(&connect)) {
                Ok(_) => format!("Game invite for app {app_id} sent to {name}."),
                Err(error) => format!("Invite refused: {error}"),
            };
            set(&status, text);
        });
    }

    // --- Profile ---

    /// Open selected friend's profile.
    pub fn open_friend_profile(&mut self) {
        if let Some(friend) = self.selected_friend() {
            let (ident, label) = (friend.steam_id.to_string(), friend.display_name());
            self.open_profile(ident, label);
        }
    }

    /// Open the logged-in user's own profile.
    pub fn open_own_profile(&mut self) {
        self.open_profile("me".into(), "me".into());
    }

    /// Fetch `aurelia user <ident>` off-thread.
    pub fn open_profile(&mut self, ident: String, label: String) {
        let result: ProfileSlot = Arc::new(Mutex::new(None));
        let slot = result.clone();
        let query = ident.clone();
        thread::spawn(move || {
            let fetched = social::user_profile(&query).map_err(|e| e.to_string());
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(fetched);
            }
        });
        self.social.profile = Some(ProfileView {
            ident,
            label,
            result,
            scroll: 0,
        });
    }

    /// Close the profile overlay.
    pub fn close_profile(&mut self) {
        self.social.profile = None;
    }

    /// Scroll the profile text down.
    pub fn profile_scroll_down(&mut self) {
        if let Some(view) = self.social.profile.as_mut() {
            view.scroll = view.scroll.saturating_add(1);
        }
    }

    /// Scroll the profile text up.
    pub fn profile_scroll_up(&mut self) {
        if let Some(view) = self.social.profile.as_mut() {
            view.scroll = view.scroll.saturating_sub(1);
        }
    }

    // --- Settings: persona wait ---

    /// Start editing persona wait, prefilled.
    pub fn begin_persona_wait_edit(&mut self) {
        let current = self
            .config_info
            .as_ref()
            .and_then(|c| c.persona_wait_secs)
            .unwrap_or(social::PERSONA_WAIT_DEFAULT);
        self.social.wait_input = Some(current.to_string());
    }

    /// Drop the persona-wait edit.
    pub fn cancel_persona_wait_edit(&mut self) {
        self.social.wait_input = None;
    }

    /// Append a digit to the edit.
    pub fn persona_wait_push(&mut self, c: char) {
        if let Some(input) = self.social.wait_input.as_mut()
            && c.is_ascii_digit()
            && input.len() < 3
        {
            input.push(c);
        }
    }

    /// Backspace the edit.
    pub fn persona_wait_pop(&mut self) {
        if let Some(input) = self.social.wait_input.as_mut() {
            input.pop();
        }
    }

    /// Validate and save via `config persona-wait`.
    pub fn commit_persona_wait_edit(&mut self) {
        let Some(input) = self.social.wait_input.clone() else {
            return;
        };
        let secs = match social::parse_persona_wait(&input) {
            Ok(secs) => secs,
            Err(error) => {
                self.notice = Some(format!("Persona wait: {error}"));
                return;
            }
        };
        self.social.wait_input = None;
        match social::set_persona_wait(secs) {
            Ok(saved) => {
                if let Some(info) = self.config_info.as_mut() {
                    info.persona_wait_secs = Some(saved);
                }
                self.notice = Some(format!("Persona wait set to {saved}s"));
            }
            Err(error) => self.notice = Some(format!("Persona wait: {error}")),
        }
    }
}
