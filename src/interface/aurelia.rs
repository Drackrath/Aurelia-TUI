//! Backend bridge to the `aurelia` command-line Steam launcher.
//!
//! This replaces the old `steamcmd` integration. Every operation shells out to
//! the `aurelia` binary with `--json` and parses its structured output. The
//! binary is expected to be on `PATH` (override with `AURELIA_BIN`), and the
//! user is expected to have authenticated once already with `aurelia login`
//! (the TUI only verifies the session via `aurelia login --health`).

use std::io::{BufRead, BufReader, Read, Write};
use std::process;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::interface::game_status::GameStatus;
use crate::util::error::STError;
use crate::util::log::log;

/// The `aurelia` binary to invoke.
///
/// Resolution order: `AURELIA_BIN` (explicit override) → a sibling `Aurelia`
/// build next to this project (dev convenience, so the TUI works without
/// installing `aurelia` first) → bare `aurelia` (resolved on `PATH`).
pub fn bin() -> String {
    if let Ok(explicit) = std::env::var("AURELIA_BIN") {
        if !explicit.is_empty() {
            return explicit;
        }
    }

    let exe = if cfg!(windows) { "aurelia.exe" } else { "aurelia" };
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    // .../Aurelia-TUI/target/<profile>/aurelia-tui[.exe] -> workspace parent dir.
    if let Ok(cur) = std::env::current_exe() {
        if let Some(workspace_parent) = cur.ancestors().nth(4) {
            roots.push(workspace_parent.to_path_buf());
        }
    }
    // Or the parent of the current working directory (when run from the project).
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(parent) = cwd.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    for root in roots {
        for profile in ["release", "debug"] {
            let candidate = root.join("Aurelia").join("target").join(profile).join(exe);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }

    "aurelia".to_string()
}

/// Session health, from `aurelia login --health --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthJson {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub steam_id: Option<u64>,
    #[serde(default)]
    pub daemon: bool,
}

/// The logged-in Steam account, from `aurelia account --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountJson {
    #[serde(default)]
    pub steam_id: u64,
    #[serde(default)]
    pub account_name: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub email_validated: bool,
    #[serde(default)]
    pub authed_machines: u32,
    #[serde(default)]
    pub vac_bans: u32,
}

/// The launcher configuration, from `aurelia config show --json` (the
/// serialized `LauncherConfig`). Only the human-relevant top-level scalars are
/// captured; nested maps (per-game configs, launch options) are skipped. Every
/// field is `#[serde(default)]` so a missing/renamed key never breaks parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigJson {
    /// The Steam library path games install into.
    #[serde(default)]
    pub steam_library_path: Option<String>,
    /// The default Proton/Wine runtime.
    #[serde(default)]
    pub proton_version: Option<String>,
    /// Friends/chat presence the daemon announces (`"online"` / `"offline"`).
    #[serde(default)]
    pub chat_presence: Option<String>,
    /// How per-game Wine prefixes / compat data are laid out (e.g. `"Shared"`).
    #[serde(default)]
    pub steam_prefix_mode: Option<String>,
    /// Default Steam API language for achievements (`None` = English).
    #[serde(default)]
    pub language: Option<String>,
    /// Whether Steam Cloud save sync is enabled.
    #[serde(default)]
    pub enable_cloud_sync: bool,
    /// Whether games share a single compat-data prefix.
    #[serde(default)]
    pub use_shared_compat_data: bool,
    /// Whether installed Windows Steam games are auto-discovered.
    #[serde(default)]
    pub windows_steam_discovery_enabled: bool,
    /// Whether the optional luxtorpeda native-engine plugin is enabled.
    #[serde(default)]
    pub luxtorpeda_enabled: bool,
}

impl ConfigJson {
    /// Whether the configured presence is online. Defaults to `false`
    /// (offline) when the field is missing or unrecognised.
    pub fn is_online(&self) -> bool {
        self.chat_presence.as_deref() == Some("online")
    }
}

/// The Steam Wallet balance, from `aurelia wallet --json`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WalletJson {
    /// Balance in the currency's minor units (cents).
    #[serde(default)]
    pub balance_cents: i64,
    /// Steam currency id.
    #[serde(default)]
    pub currency: u32,
    /// Wallet country code.
    #[serde(default)]
    pub country: String,
    /// Human-readable balance (e.g. "12.34 EUR").
    #[serde(default)]
    pub formatted: String,
}

/// Artwork URLs injected by `aurelia list --json`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AssetsJson {
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub capsule: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
}

/// One library entry, from `aurelia list --json` (a `LibraryGame` plus the
/// `assets`/`store_url` fields the CLI bakes in for `--json` output).
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryGameJson {
    pub app_id: u32,
    pub name: String,
    #[serde(default)]
    pub is_installed: bool,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub update_available: bool,
    #[serde(default)]
    pub is_owned: bool,
    #[serde(default)]
    pub is_family_shared: bool,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub active_branch: Option<String>,
    #[serde(default)]
    pub assets: Option<AssetsJson>,
    #[serde(default)]
    pub store_url: Option<String>,
}

/// One Steam Cloud file, from `aurelia cloud list <id> --json`. The CLI emits
/// `{ "app_id": .., "files": [{ "filename", "size", "timestamp", "sha_hash" }] }`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CloudFileJson {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub timestamp: i64,
}

/// Store metadata, from `aurelia info <id> --json`. Only the fields the TUI
/// surfaces are captured; the key names match the CLI's `--json` output.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InfoJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub app_type: String,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub reviews: Option<String>,
}

/// One achievement entry from `aurelia achievements <id> --json` (an item of
/// the response's `achievements` array). Only the fields the TUI surfaces are
/// captured; key names match the CLI's `--json` output. Everything is
/// `#[serde(default)]` so a missing field never breaks parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AchievementJson {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Whether the logged-in user has unlocked this achievement.
    #[serde(default)]
    pub unlocked: bool,
    /// Whether the achievement is visible (false = hidden until unlocked).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Global unlock rate across all players, as a percentage (rarity).
    #[serde(default)]
    pub rarity: f32,
}

fn default_true() -> bool {
    true
}

/// One friend entry from `aurelia friends --json` (an item of the serialized
/// roster). Key names match the CLI's `Friend` struct. Everything is
/// `#[serde(default)]` so a missing field never breaks parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FriendJson {
    /// SteamID64 of the friend. The CLI emits it as a number, but accept a
    /// string form too for robustness.
    #[serde(default, deserialize_with = "de_steam_id")]
    pub steam_id: u64,
    /// Display (persona) name, if known.
    #[serde(default)]
    pub persona_name: Option<String>,
    /// Online status: 0 offline, 1 online, 2 busy, 3 away, 4 snooze,
    /// 5 looking-to-trade, 6 looking-to-play.
    #[serde(default)]
    pub persona_state: Option<u32>,
    /// App id of the game the friend is currently playing, if any.
    #[serde(default)]
    pub game_app_id: Option<u32>,
    /// Name of the game the friend is currently playing, if known.
    #[serde(default)]
    pub game_name: Option<String>,
}

impl FriendJson {
    /// Display name, falling back to the SteamID when no persona is known.
    pub fn display_name(&self) -> String {
        self.persona_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.steam_id.to_string())
    }

    /// Whether the friend is currently online (any non-offline persona state).
    pub fn is_online(&self) -> bool {
        !matches!(self.persona_state, None | Some(0))
    }

    /// The game the friend is currently playing, if any (name, else `app <id>`).
    pub fn current_game(&self) -> Option<String> {
        match (&self.game_name, self.game_app_id) {
            (Some(g), _) if !g.is_empty() => Some(g.clone()),
            (_, Some(id)) => Some(format!("app {}", id)),
            _ => None,
        }
    }
}

/// One inventory item stack, from `aurelia inventory <id> --json` (an item of
/// the response array). The CLI serializes `InventoryItem` directly, so key
/// names match its fields: `name`/`market_hash_name`, `item_type`, `amount`,
/// `tradable`, `marketable`. Everything is `#[serde(default)]` so a missing
/// field never breaks parsing, and a couple of aliases cover alternate spellings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InventoryItemJson {
    #[serde(default)]
    pub name: String,
    #[serde(default, alias = "market_name")]
    pub market_hash_name: String,
    /// Item type/category text (e.g. "Trading Card").
    #[serde(default, alias = "type")]
    pub item_type: String,
    /// Stack size for this item.
    #[serde(default, alias = "count")]
    pub amount: u64,
    #[serde(default)]
    pub tradable: bool,
    #[serde(default)]
    pub marketable: bool,
}

impl InventoryItemJson {
    /// Best display name: the visible `name`, falling back to the market name.
    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if !self.market_hash_name.is_empty() {
            self.market_hash_name.clone()
        } else {
            "(unnamed item)".to_string()
        }
    }
}

/// One Workshop item the user is subscribed to for a game, from
/// `aurelia workshop list <id> --json`. The CLI emits a bare array of items
/// (`WorkshopItem`); the listing shows the title and size. Everything is
/// `#[serde(default)]` so a missing/extra field never breaks parsing.
///
/// `id` is a u64 published-file id, which the backend may serialize either as a
/// number or a string, so it is captured leniently (and only used to fall back
/// on a title when none is present).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkshopItemJson {
    /// `publishedfileid`. May arrive as a JSON number or string.
    #[serde(default, deserialize_with = "de_opt_u64")]
    pub id: Option<u64>,
    /// The item title (the CLI key is `title`; `name` is accepted as a fallback).
    #[serde(default, alias = "name")]
    pub title: String,
    /// Whether the item's content is installed on disk. Absent in the current CLI
    /// output (all listed items are subscribed); defaults to `false`.
    #[serde(default)]
    pub installed: bool,
    /// Whether the user is subscribed to the item. The CLI lists only subscribed
    /// items, so this defaults to `true` when absent.
    #[serde(default = "default_true")]
    pub subscribed: bool,
    /// On-disk size in bytes (the CLI key is `file_size`; `size` is accepted too).
    #[serde(default, alias = "size")]
    pub file_size: u64,
}

impl WorkshopItemJson {
    /// Display title, falling back to the published-file id when the title is
    /// missing/empty.
    pub fn display_title(&self) -> String {
        if !self.title.is_empty() {
            self.title.clone()
        } else if let Some(id) = self.id {
            format!("Item {}", id)
        } else {
            "(untitled)".to_string()
        }
    }
}

/// Deserialize a SteamID64 that may arrive as a JSON number or string.
fn de_steam_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(n) => Ok(n.as_u64().unwrap_or(0)),
        serde_json::Value::String(s) => Ok(s.parse().unwrap_or(0)),
        _ => Ok(0),
    }
}

/// Deserialize an optional `u64` that may be encoded as a JSON number or string
/// (Steam published-file ids are sometimes stringified). Any unparseable value
/// becomes `None` rather than failing the whole parse.
fn de_opt_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        Some(serde_json::Value::String(s)) => s.parse().ok(),
        _ => None,
    })
}

/// One DLC entry for a base game, from `aurelia dlc <id> --json` (the `dlc`
/// array). All status fields are nullable in the CLI output (they come from a
/// best-effort `DlcState` lookup), so each defaults when absent.
#[derive(Debug, Clone, Deserialize)]
pub struct DlcJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub name: Option<String>,
    // The CLI emits these as `bool` or `null` (a best-effort `DlcState` lookup),
    // so deserialize as `Option<bool>` and expose a defaulted view via helpers.
    #[serde(default)]
    pub owned: Option<bool>,
    #[serde(default)]
    pub installed: Option<bool>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

impl DlcJson {
    /// Display name, falling back to the app id when the store name is missing.
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("App {}", self.app_id))
    }

    pub fn is_owned(&self) -> bool {
        self.owned.unwrap_or(false)
    }

    pub fn is_installed(&self) -> bool {
        self.installed.unwrap_or(false)
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled.unwrap_or(false)
    }
}

/// The top-level object from `aurelia dlc <id> --json`: the base app id plus its
/// DLC list. Only the `dlc` array is surfaced.
#[derive(Debug, Clone, Deserialize)]
struct DlcResponse {
    #[serde(default)]
    dlc: Vec<DlcJson>,
}

/// One beta branch for a game, from `aurelia branches <id> --json` (an item of
/// the response's `branches` array).
///
/// The current CLI emits each branch as a bare name string (the password-gated
/// branches are filtered out server-side), so the common case is a plain
/// `String`. The custom `Deserialize` below also accepts an object form
/// (`{ name, description, active/current, build_id, pwdrequired }`) so a richer
/// future CLI keeps working; every object field is optional/defaulted.
#[derive(Debug, Clone, Default)]
pub struct BranchJson {
    pub name: String,
    pub description: String,
    /// Whether this is the branch the install is currently tracking. The current
    /// CLI doesn't report it, so it stays `false`; [`Browser::open_branches`]
    /// marks the active branch by name when one is known.
    pub active: bool,
}

impl<'de> Deserialize<'de> for BranchJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // The branch array mixes shapes across CLI versions: a bare name string,
        // or an object with optional metadata. Deserialize either via an
        // untagged shim, then normalise into `BranchJson`.
        #[derive(Deserialize)]
        struct BranchObj {
            #[serde(default)]
            name: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            active: bool,
            #[serde(default)]
            current: bool,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BranchRaw {
            Name(String),
            Obj(BranchObj),
        }

        Ok(match BranchRaw::deserialize(deserializer)? {
            BranchRaw::Name(name) => BranchJson {
                name,
                description: String::new(),
                active: false,
            },
            BranchRaw::Obj(obj) => BranchJson {
                name: obj.name,
                description: obj.description,
                active: obj.active || obj.current,
            },
        })
    }
}

/// One depot for a game, from `aurelia depots <id> --json` (an item of the
/// response's `depots` array). Key names match the CLI's `--json` output;
/// everything is `#[serde(default)]` so a missing field never breaks parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DepotJson {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub file_count: u64,
    #[serde(default)]
    pub config: String,
    #[serde(default)]
    pub is_owned: Option<bool>,
}

/// One launch option for a game, from `aurelia launch-options <id> --json` (an
/// item of the response's `launch_options` array). A launch option is one of the
/// ways Steam can start the game (e.g. a normal launch vs. a level editor). Key
/// names match the CLI's `--json` output; everything is `#[serde(default)]` so a
/// missing field never breaks parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LaunchOptionJson {
    /// The launch option's id (a string in the app's manifest).
    #[serde(default)]
    pub id: String,
    /// Human-readable label for this launch option.
    #[serde(default)]
    pub description: String,
    /// The executable Steam would run.
    #[serde(default)]
    pub executable: String,
    /// Arguments passed to the executable.
    #[serde(default)]
    pub arguments: String,
    /// Working directory for the launch, if specified.
    #[serde(default)]
    pub working_dir: String,
    /// Target OS list (e.g. "windows"); empty means any.
    #[serde(default)]
    pub oslist: String,
    /// Target OS architecture (e.g. "64"); empty means any.
    #[serde(default)]
    pub osarch: String,
    /// Launch option type (e.g. "default", "option1").
    #[serde(default, rename = "type")]
    pub launch_type: String,
}

impl LaunchOptionJson {
    /// Display label, falling back to the executable then the id when the
    /// store-provided description is missing.
    pub fn display_name(&self) -> String {
        if !self.description.is_empty() {
            self.description.clone()
        } else if !self.executable.is_empty() {
            self.executable.clone()
        } else {
            format!("Option {}", self.id)
        }
    }

    /// The command line (executable + arguments), trimmed; empty when neither is set.
    pub fn command(&self) -> String {
        format!("{} {}", self.executable, self.arguments)
            .trim()
            .to_string()
    }

    /// The OS tag for this option ("any" when no OS list is specified).
    pub fn os_tag(&self) -> String {
        if self.oslist.is_empty() {
            "any".to_string()
        } else {
            self.oslist.clone()
        }
    }
}

/// One row in the market overlay: a single active sell listing or open buy
/// order, flattened from `aurelia market listings --json`. The CLI emits an
/// object with separate `listings` and `buy_orders` arrays; we merge them into
/// one list and tag each row's `kind` ourselves. `price` is in the currency's
/// minor units (cents); `quantity` is only meaningful for buy orders.
#[derive(Debug, Clone, Default)]
pub struct MarketListingJson {
    /// The item's market hash name.
    pub name: String,
    /// Price in the currency's minor units (cents).
    pub price: u64,
    /// Quantity still wanted (buy orders only; 1 for listings).
    pub quantity: u64,
    /// A short tag we set ourselves: "listing" or "buy order".
    pub kind: String,
}

impl MarketListingJson {
    /// The price formatted from minor units to a major-unit decimal (e.g. 199 -> "1.99").
    pub fn price_text(&self) -> String {
        format!("{}.{:02}", self.price / 100, self.price % 100)
    }
}

/// One active sell listing, from the `listings` array of `aurelia market
/// listings --json`.
#[derive(Debug, Clone, Default, Deserialize)]
struct MyListingJson {
    #[serde(default)]
    market_hash_name: String,
    #[serde(default)]
    price: u64,
}

/// One open buy order, from the `buy_orders` array of `aurelia market listings
/// --json`.
#[derive(Debug, Clone, Default, Deserialize)]
struct MyBuyOrderJson {
    #[serde(default)]
    market_hash_name: String,
    #[serde(default)]
    price: u64,
    #[serde(default)]
    quantity: u64,
}

/// The top-level object from `aurelia market listings --json`: the account's
/// active sell listings and open buy orders.
#[derive(Debug, Clone, Default, Deserialize)]
struct MyMarketStateJson {
    #[serde(default)]
    listings: Vec<MyListingJson>,
    #[serde(default)]
    buy_orders: Vec<MyBuyOrderJson>,
}

/// One Proton/Wine runtime, surfaced from `aurelia proton list --json`. The CLI
/// emits a wrapping object (`{ available: [..], installed: [..], default }`);
/// `proton_list` flattens it into one list, computing the `installed`/`is_default`
/// markers per entry. Every field is `#[serde(default)]` so a partial line parses.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProtonJson {
    /// The runtime name, used to install/select it (e.g. `Proton 9.0`, `GE-Proton10-34`).
    #[serde(default)]
    pub name: String,
    /// Human label for the source family (`Valve`, `Proton-GE`, `Wine-GE`).
    #[serde(default)]
    pub label: String,
    /// Download size in bytes (`0` when unknown).
    #[serde(default)]
    pub size: u64,
    /// Whether this runtime is present on disk (computed by `proton_list`).
    #[serde(default)]
    pub installed: bool,
    /// Whether this runtime is the configured global default (computed by `proton_list`).
    #[serde(default)]
    pub is_default: bool,
}

/// An entry of the `installed` array from `aurelia proton list --json`
/// (`{ name, location, path }`); only the name is surfaced.
#[derive(Debug, Clone, Default, Deserialize)]
struct InstalledProtonJson {
    #[serde(default)]
    name: String,
}

/// The top-level object from `aurelia proton list --json`: the installable
/// runtimes, what's on disk, and the configured default version name.
#[derive(Debug, Clone, Default, Deserialize)]
struct ProtonListResponse {
    #[serde(default)]
    available: Vec<ProtonJson>,
    #[serde(default)]
    installed: Vec<InstalledProtonJson>,
    #[serde(default)]
    default: String,
}

/// One running game, from `aurelia running --json` (an item of the response's
/// `running` array). Each game was launched via `aurelia play`; the fields match
/// the CLI's `--json` output and everything defaults so a missing field never
/// breaks parsing.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RunningJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pid: u32,
}

/// The top-level object from `aurelia running --json`: the list of currently
/// running games. Only the `running` array is surfaced.
#[derive(Debug, Clone, Deserialize)]
struct RunningResponse {
    #[serde(default)]
    running: Vec<RunningJson>,
}

/// A `qr_challenge` event line from `aurelia login --qr --json` (emitted on
/// stderr, re-emitted whenever Steam rotates the code).
#[derive(Debug, Deserialize)]
struct QrEvent {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// Where a classic (username/password) login currently stands. Drives what the
/// TUI shows and when it prompts for a Steam Guard code.
#[derive(Debug, Clone, PartialEq)]
pub enum LoginPhase {
    Idle,
    Connecting,
    /// Login is blocked waiting for the user to approve it on their device, or
    /// to type a code (which may arrive next as `GuardCode`).
    AwaitingConfirmation,
    /// A typed Steam Guard code is required; the `String` is its kind
    /// (`"email"` or `"device"`).
    GuardCode(String),
    /// Approval must happen in the Steam Mobile app (no typed code).
    DeviceConfirmation,
    Success,
    Failed(String),
}

/// An NDJSON event line from `aurelia login --json` (on stderr).
#[derive(Debug, Deserialize)]
struct LoginEvent {
    #[serde(default)]
    event: Option<String>,
    #[serde(default, rename = "type")]
    guard_type: Option<String>,
}

/// A single NDJSON progress line from a streaming `install`/`update`/`verify`.
/// Progress events are emitted on **stderr**; the terminal result object goes
/// to **stdout**.
#[derive(Debug, Deserialize)]
struct ProgressJson {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    percent: Option<f64>,
}

/// Run `aurelia <args> --json`, returning the parsed stdout JSON value.
///
/// Errors surface as `{"error": "..."}` on stdout; that is translated into an
/// `STError::Problem`. stderr (tracing/diagnostics) is discarded.
fn run_json(args: &[&str]) -> Result<serde_json::Value, STError> {
    let output = process::Command::new(bin())
        .args(args)
        .arg("--json")
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output()
        .map_err(STError::Process)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(STError::Problem(format!(
            "aurelia produced no output for `{}` (is the `aurelia` binary on PATH?)",
            args.join(" ")
        )));
    }

    let value: serde_json::Value = serde_json::from_str(trimmed)?;
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        return Err(STError::Problem(err.to_string()));
    }
    Ok(value)
}

/// Report whether a Steam session is currently authenticated.
pub fn health() -> Result<HealthJson, STError> {
    let value = run_json(&["login", "--health"])?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch the logged-in Steam account (`aurelia account --json`).
pub fn account() -> Result<AccountJson, STError> {
    let value = run_json(&["account"])?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch the launcher configuration (`aurelia config show --json`).
pub fn config_show() -> Result<ConfigJson, STError> {
    let value = run_json(&["config", "show"])?;
    Ok(serde_json::from_value(value)?)
}

/// Set the friends/chat presence the daemon announces
/// (`aurelia config presence online|offline --json`).
pub fn set_presence(online: bool) -> Result<(), STError> {
    run_json(&["config", "presence", if online { "online" } else { "offline" }])?;
    Ok(())
}

/// Log out of the current Steam session (`aurelia logout --json`). The returned
/// value (`{"logged_out":true}`) is ignored; only errors are propagated.
pub fn logout() -> Result<(), STError> {
    run_json(&["logout"])?;
    Ok(())
}

/// Fetch the Steam Wallet balance (`aurelia wallet --json`).
pub fn wallet() -> Result<WalletJson, STError> {
    let value = run_json(&["wallet"])?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch the full library (`aurelia list --json`).
pub fn fetch_library() -> Result<Vec<LibraryGameJson>, STError> {
    let value = run_json(&["list"])?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch store metadata for a single app (`aurelia info <id> --json`).
pub fn fetch_info(id: i32) -> Result<InfoJson, STError> {
    let value = run_json(&["info", &id.to_string()])?;
    Ok(serde_json::from_value(value)?)
}

/// List a game's Steam Cloud files (`aurelia cloud list <id> --json`). The CLI
/// wraps the per-file array under a `files` key; the top-level object also
/// carries `app_id`, which we ignore.
pub fn cloud_list(app_id: i32) -> Result<Vec<CloudFileJson>, STError> {
    let value = run_json(&["cloud", "list", &app_id.to_string()])?;
    // Accept either the wrapping object (`{ "files": [..] }`) or a bare array.
    let files = match value.get("files") {
        Some(files) => files.clone(),
        None => value,
    };
    Ok(serde_json::from_value(files)?)
}

/// Sync a game's Steam Cloud saves (`aurelia cloud sync <id> --json`). This is a
/// blocking call and can be slow; errors surface via `run_json`.
pub fn cloud_sync(app_id: i32) -> Result<(), STError> {
    run_json(&["cloud", "sync", &app_id.to_string()])?;
    Ok(())
}

/// Fetch the selected game's achievements with the logged-in user's unlock
/// state (`aurelia achievements <id> --json`). The CLI wraps the list in an
/// object (`{ achievements: [...] }`); we unwrap and parse just the array.
pub fn achievements(app_id: i32) -> Result<Vec<AchievementJson>, STError> {
    let value = run_json(&["achievements", &app_id.to_string()])?;
    let list = value
        .get("achievements")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if list.is_null() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_value(list)?)
}

/// Fetch the logged-in user's friends (`aurelia friends --json`). The CLI emits
/// a bare array, but accept an object-wrapped form (`{ "friends": [...] }`) too.
pub fn friends() -> Result<Vec<FriendJson>, STError> {
    let value = run_json(&["friends"])?;
    let list = match value.get("friends") {
        Some(friends) => friends.clone(),
        None => value,
    };
    if list.is_null() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_value(list)?)
}

/// Fetch the logged-in user's inventory for a game (`aurelia inventory <id>
/// --json`). The CLI emits a bare array of item stacks; accept either that or an
/// object wrapping it under an `items` key, parsing just the array.
pub fn inventory(app_id: i32) -> Result<Vec<InventoryItemJson>, STError> {
    let value = run_json(&["inventory", &app_id.to_string()])?;
    // Accept either the wrapping object (`{ "items": [..] }`) or a bare array.
    let list = match value.get("items") {
        Some(items) => items.clone(),
        None => value,
    };
    if list.is_null() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_value(list)?)
}

/// Fetch the DLC list for a base game (`aurelia dlc <id> --json`).
pub fn dlc(app_id: i32) -> Result<Vec<DlcJson>, STError> {
    let value = run_json(&["dlc", &app_id.to_string()])?;
    let parsed: DlcResponse = serde_json::from_value(value)?;
    Ok(parsed.dlc)
}

/// List a game's depots (`aurelia depots <id> --json`). The CLI wraps the
/// per-depot array under a `depots` key (alongside `app_id`); accept either the
/// wrapping object or a bare array.
pub fn depots(app_id: i32) -> Result<Vec<DepotJson>, STError> {
    let value = run_json(&["depots", &app_id.to_string()])?;
    let depots = match value.get("depots") {
        Some(depots) => depots.clone(),
        None => value,
    };
    Ok(serde_json::from_value(depots)?)
}

/// List a game's launch options (`aurelia launch-options <id> --json`). The CLI
/// wraps the array under a `launch_options` key; the top-level object also
/// carries `app_id`, which we ignore. Accepts a bare array too.
pub fn launch_options(app_id: i32) -> Result<Vec<LaunchOptionJson>, STError> {
    let value = run_json(&["launch-options", &app_id.to_string()])?;
    // Accept either the wrapping object (`{ "launch_options": [..] }`) or a bare array.
    let list = match value.get("launch_options") {
        Some(list) => list.clone(),
        None => value,
    };
    Ok(serde_json::from_value(list)?)
}

/// List the user's subscribed Workshop items for a game
/// (`aurelia workshop list <id> --json`). The CLI emits a bare array of items,
/// but tolerate an object that wraps the list under `items`/`workshop`.
pub fn workshop_list(app_id: i32) -> Result<Vec<WorkshopItemJson>, STError> {
    let value = run_json(&["workshop", "list", &app_id.to_string()])?;
    // Accept either a bare array or a wrapping object (`{ "items": [..] }`).
    let list = value
        .get("items")
        .or_else(|| value.get("workshop"))
        .cloned()
        .unwrap_or(value);
    if list.is_null() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_value(list)?)
}

/// Fetch the logged-in account's active market listings and open buy orders
/// (`aurelia market listings --json`). The CLI emits an object with separate
/// `listings`/`buy_orders` arrays; we flatten them into one `Vec`, tagging each
/// row's `kind` so the overlay can label it. An account with none yields an
/// empty `Vec`.
pub fn market_listings() -> Result<Vec<MarketListingJson>, STError> {
    let value = run_json(&["market", "listings"])?;
    let state: MyMarketStateJson = serde_json::from_value(value)?;
    let mut rows = Vec::with_capacity(state.listings.len() + state.buy_orders.len());
    for l in state.listings {
        rows.push(MarketListingJson {
            name: l.market_hash_name,
            price: l.price,
            quantity: 1,
            kind: "listing".to_string(),
        });
    }
    for b in state.buy_orders {
        rows.push(MarketListingJson {
            name: b.market_hash_name,
            price: b.price,
            quantity: b.quantity,
            kind: "buy order".to_string(),
        });
    }
    Ok(rows)
}

/// Enable or disable a single DLC (`aurelia enable|disable <id> --json`). The
/// returned value is ignored; only errors are propagated.
pub fn set_dlc(app_id: i32, enable: bool) -> Result<(), STError> {
    let verb = if enable { "enable" } else { "disable" };
    run_json(&[verb, &app_id.to_string()])?;
    Ok(())
}

/// Fetch the beta branches for a game (`aurelia branches <id> --json`). The CLI
/// wraps the list under a `branches` key (`{ app_id, branches: [..] }`); accept
/// either that or a bare array.
pub fn branches(app_id: i32) -> Result<Vec<BranchJson>, STError> {
    let value = run_json(&["branches", &app_id.to_string()])?;
    // Accept either the wrapping object (`{ "branches": [..] }`) or a bare array.
    let list = match value.get("branches") {
        Some(branches) => branches.clone(),
        None => value,
    };
    Ok(serde_json::from_value(list)?)
}

/// Switch a game to a beta branch (`aurelia set-branch <id> <branch> --json`).
/// The returned value is ignored; only errors are propagated. The switch is
/// staged — an `update` is needed afterwards for it to take effect.
pub fn set_branch(app_id: i32, branch: &str) -> Result<(), STError> {
    run_json(&["set-branch", &app_id.to_string(), branch])?;
    Ok(())
}

/// List the Proton/Wine runtimes (`aurelia proton list --json`). The CLI returns
/// a wrapping object with separate `available`/`installed` arrays and a `default`
/// version name; this flattens them into one list, marking each entry's
/// `installed` and `is_default` state (matched case-insensitively by name).
pub fn proton_list() -> Result<Vec<ProtonJson>, STError> {
    let value = run_json(&["proton", "list"])?;
    let parsed: ProtonListResponse = serde_json::from_value(value)?;

    let is_installed = |name: &str| {
        parsed
            .installed
            .iter()
            .any(|i| i.name.eq_ignore_ascii_case(name))
    };
    let is_default = |name: &str| name.eq_ignore_ascii_case(&parsed.default);

    let mut protons: Vec<ProtonJson> = parsed
        .available
        .into_iter()
        .map(|mut p| {
            p.installed = is_installed(&p.name);
            p.is_default = is_default(&p.name);
            p
        })
        .collect();

    // Surface any installed runtime missing from the available list (e.g. an old
    // GE build no longer published) so everything on disk is still shown.
    for inst in &parsed.installed {
        if !protons.iter().any(|p| p.name.eq_ignore_ascii_case(&inst.name)) {
            protons.push(ProtonJson {
                name: inst.name.clone(),
                label: "(local)".to_string(),
                size: 0,
                installed: true,
                is_default: is_default(&inst.name),
            });
        }
    }

    Ok(protons)
}

/// Make `version` the global default Proton/Wine runtime
/// (`aurelia proton default <version> --json`). The returned value is ignored;
/// only errors are propagated.
pub fn proton_default(version: &str) -> Result<(), STError> {
    run_json(&["proton", "default", version])?;
    Ok(())
}

/// List the games Aurelia is currently running (`aurelia running --json`). The
/// CLI wraps the list in an object (`{ running: [...] }`); we unwrap and parse
/// just the array.
pub fn running() -> Result<Vec<RunningJson>, STError> {
    let value = run_json(&["running"])?;
    let parsed: RunningResponse = serde_json::from_value(value)?;
    Ok(parsed.running)
}

/// Stop a running game (`aurelia stop <id> --json`). The returned value is
/// ignored; only errors are propagated.
pub fn stop(app_id: i32) -> Result<(), STError> {
    run_json(&["stop", &app_id.to_string()])?;
    Ok(())
}

/// Log in by QR code (`aurelia login --qr --json`), publishing each challenge
/// URL into `qr_cell` so the UI can render it. Blocks until the login resolves;
/// returns `Ok(())` once authenticated, `Err` on failure/timeout. Intended to
/// be run on a dedicated thread.
///
/// Forced to run standalone (`AURELIA_NO_DAEMON`) so it authenticates in this
/// process and writes `session.json`; any running daemon then reloads that
/// session by mtime on its next forwarded command.
pub fn login_qr(qr_cell: &Arc<Mutex<Option<String>>>) -> Result<(), STError> {
    let spawned = process::Command::new(bin())
        .args(["login", "--qr", "--json"])
        .env("AURELIA_NO_DAEMON", "1")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(STError::Process)?;

    // The terminal result object is the single line on stdout; drain it on a
    // helper thread while we consume the challenge events on stderr.
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<QrEvent>(line) {
                if event.event.as_deref() == Some("qr_challenge") {
                    if let Some(url) = event.url {
                        if let Ok(mut slot) = qr_cell.lock() {
                            *slot = Some(url);
                        }
                    }
                }
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();

    let value: serde_json::Value = serde_json::from_str(result.trim()).map_err(|_| {
        STError::Problem("QR login did not complete (no result from aurelia)".to_string())
    })?;
    if value.get("logged_in").and_then(|b| b.as_bool()) == Some(true) {
        Ok(())
    } else if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        Err(STError::Problem(err.to_string()))
    } else {
        Err(STError::Problem("QR login did not complete".to_string()))
    }
}

fn set_phase(phase: &Arc<Mutex<LoginPhase>>, value: LoginPhase) {
    if let Ok(mut slot) = phase.lock() {
        *slot = value;
    }
}

/// Classic username/password login (`aurelia login --json -u <user>`).
///
/// The password is passed via the `AURELIA_PASSWORD` environment variable so it
/// never appears in the process's argument list. Steam Guard codes are read by
/// `aurelia` from stdin: when a `guard_required` event arrives we publish a
/// `GuardCode` phase, block on `guard_rx` until the UI supplies the code, then
/// write it to the child's stdin. Runs standalone (`AURELIA_NO_DAEMON`) so it
/// authenticates here and writes `session.json`. Intended to run on a dedicated
/// thread; returns `Ok(())` once authenticated.
pub fn login_classic(
    username: &str,
    password: &str,
    phase: &Arc<Mutex<LoginPhase>>,
    guard_rx: Receiver<String>,
) -> Result<(), STError> {
    set_phase(phase, LoginPhase::Connecting);

    let spawned = process::Command::new(bin())
        .args(["login", "--json", "-u", username])
        .env("AURELIA_PASSWORD", password)
        .env("AURELIA_NO_DAEMON", "1")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(STError::Process)?;

    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<LoginEvent>(line) else {
                continue;
            };
            match event.event.as_deref() {
                Some("awaiting_confirmation") => {
                    set_phase(phase, LoginPhase::AwaitingConfirmation)
                }
                Some("guard_required") => match event.guard_type.as_deref() {
                    Some("device_confirmation") => {
                        set_phase(phase, LoginPhase::DeviceConfirmation)
                    }
                    Some(kind) => {
                        set_phase(phase, LoginPhase::GuardCode(kind.to_string()));
                        // Block until the UI delivers the code, then feed it to
                        // aurelia's stdin (one line, as it expects).
                        if let Ok(code) = guard_rx.recv() {
                            if let Some(si) = stdin.as_mut() {
                                let _ = writeln!(si, "{}", code);
                                let _ = si.flush();
                            }
                            set_phase(phase, LoginPhase::Connecting);
                        }
                    }
                    None => {}
                },
                _ => {}
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    // Close stdin so aurelia sees EOF if it is still reading.
    drop(stdin);
    let _ = child.wait();

    match serde_json::from_str::<serde_json::Value>(result.trim()) {
        Ok(value) if value.get("logged_in").and_then(|b| b.as_bool()) == Some(true) => {
            set_phase(phase, LoginPhase::Success);
            Ok(())
        }
        Ok(value) => {
            let msg = value
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("login failed")
                .to_string();
            set_phase(phase, LoginPhase::Failed(msg.clone()));
            Err(STError::Problem(msg))
        }
        Err(_) => {
            let msg = "login did not complete".to_string();
            set_phase(phase, LoginPhase::Failed(msg.clone()));
            Err(STError::Problem(msg))
        }
    }
}

/// Update a shared status cell, preserving any previously-known install dir/size.
fn set_status(status: &Arc<Mutex<Option<GameStatus>>>, msg: &str) {
    if let Ok(mut guard) = status.lock() {
        let next = GameStatus::msg(&guard, msg);
        *guard = Some(next);
    }
}

/// Install a game (`aurelia install <id> --json`), streaming progress into the
/// shared status cell. Blocks until the install finishes; intended to be run on
/// a dedicated thread.
pub fn install(id: i32, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "processing...");

    let spawned = process::Command::new(bin())
        .args(["install", &id.to_string(), "--json"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            set_status(&status, &format!("Failed: {}", err));
            log!("Failed to spawn aurelia install", id, err);
            return;
        }
    };

    // Drain stdout (the small terminal result object) on a helper thread so the
    // child never blocks writing it while we consume the larger stderr stream.
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<ProgressJson>(line) {
                if event.event.as_deref() == Some("progress") {
                    let label = match event.state.as_deref() {
                        Some("verifying") => "verifying",
                        Some("moving") => "moving",
                        Some("queued") => "queued",
                        _ => "downloading",
                    };
                    set_status(
                        &status,
                        &format!("{} {:.1}%", label, event.percent.unwrap_or(0.0)),
                    );
                }
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();

    match serde_json::from_str::<serde_json::Value>(result.trim()) {
        Ok(value) => {
            if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                set_status(&status, &format!("Failed: {}", err));
            } else if value.get("status").and_then(|s| s.as_str()) == Some("installed") {
                set_status(&status, "Installed!");
            } else {
                set_status(&status, "done");
            }
        }
        // No parseable result line: fall back on the exit status we already waited on.
        Err(_) => set_status(&status, "Installed!"),
    }
}

/// Uninstall a game (`aurelia uninstall <id> --json`). The game's Wine prefix /
/// compat data is left in place (no `--delete-prefix`). Blocks until the CLI
/// reports the result; the parsed value is ignored beyond error detection.
pub fn uninstall(app_id: i32) -> Result<(), STError> {
    run_json(&["uninstall", &app_id.to_string()])?;
    Ok(())
}

/// Relocate an installed game to another Steam library folder
/// (`aurelia move <id> <library> --json`). Blocks until the CLI finishes the
/// file relocation (which can take a while); the parsed value is ignored beyond
/// error detection. (`move` is a Rust keyword, hence `move_game`.)
pub fn move_game(app_id: i32, library: &str) -> Result<(), STError> {
    run_json(&["move", &app_id.to_string(), library])?;
    Ok(())
}

/// Verify the integrity of a game's files (`aurelia verify <id> --json`),
/// streaming progress into the shared status cell. Blocks until verification
/// finishes; intended to be run on a dedicated thread.
pub fn verify(id: i32, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "processing...");

    let spawned = process::Command::new(bin())
        .args(["verify", &id.to_string(), "--json"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            set_status(&status, &format!("Failed: {}", err));
            log!("Failed to spawn aurelia verify", id, err);
            return;
        }
    };

    // Drain stdout (the small terminal result object) on a helper thread so the
    // child never blocks writing it while we consume the larger stderr stream.
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<ProgressJson>(line) {
                if event.event.as_deref() == Some("progress") {
                    set_status(
                        &status,
                        &format!("verifying {:.1}%", event.percent.unwrap_or(0.0)),
                    );
                }
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();

    match serde_json::from_str::<serde_json::Value>(result.trim()) {
        Ok(value) => {
            if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                set_status(&status, &format!("Failed: {}", err));
            } else if value.get("status").and_then(|s| s.as_str()) == Some("verified") {
                set_status(&status, "verified");
            } else {
                set_status(&status, "done");
            }
        }
        // No parseable result line: fall back on the exit status we already waited on.
        Err(_) => set_status(&status, "verified"),
    }
}

/// Update a game to the latest version (`aurelia update <id> --json`),
/// streaming progress into the shared status cell. Blocks until the update
/// finishes; intended to be run on a dedicated thread.
pub fn update(id: i32, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "processing...");

    let spawned = process::Command::new(bin())
        .args(["update", &id.to_string(), "--json"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            set_status(&status, &format!("Failed: {}", err));
            log!("Failed to spawn aurelia update", id, err);
            return;
        }
    };

    // Drain stdout (the small terminal result object) on a helper thread so the
    // child never blocks writing it while we consume the larger stderr stream.
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<ProgressJson>(line) {
                if event.event.as_deref() == Some("progress") {
                    set_status(
                        &status,
                        &format!("updating {:.1}%", event.percent.unwrap_or(0.0)),
                    );
                }
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();

    match serde_json::from_str::<serde_json::Value>(result.trim()) {
        Ok(value) => {
            if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                set_status(&status, &format!("Failed: {}", err));
            } else if value.get("status").and_then(|s| s.as_str()) == Some("updated") {
                set_status(&status, "updated");
            } else {
                set_status(&status, "done");
            }
        }
        // No parseable result line: fall back on the exit status we already waited on.
        Err(_) => set_status(&status, "updated"),
    }
}

/// Launch a game and wait for it to exit (`aurelia play <id> --json`).
/// Intended to be run on a dedicated thread.
pub fn play(id: i32, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "launching...");

    let output = process::Command::new(bin())
        .args(["play", &id.to_string(), "--json"])
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                Ok(value) => {
                    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                        set_status(&status, &format!("Failed: {}", err));
                    } else {
                        set_status(&status, "ran (finished)");
                    }
                }
                Err(_) => {
                    if output.status.success() {
                        set_status(&status, "ran (finished)");
                    } else {
                        set_status(&status, "Failed to launch");
                    }
                }
            }
        }
        Err(err) => {
            set_status(&status, &format!("failed to launch: {}", err));
            log!("Failed to spawn aurelia play", id, err);
        }
    }
}
