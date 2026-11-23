//! Optional HowLongToBeat metadata. The UI schedules this after selection settles,
//! just like ProtonDB; only matches with a verified Steam app ID are displayed.
//!
//! There are three boundaries here: `lookup_cached` applies persistent-cache
//! policy, `Provider` handles the website protocol, and `verified_game` validates
//! identity before a result reaches either the cache or UI. Website failures
//! remain optional metadata failures; they must not interrupt library browsing.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use regex::Regex;
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::util::{error::STError, log::log, paths::cache_directory};

const BASE: &str = "https://howlongtobeat.com";
const WIKI_BASE: &str = "https://www.pcgamingwiki.com";
const WIKI_USER_AGENT: &str = concat!(
    "Aurelia-TUI/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/Drackrath/Aurelia-TUI)"
);
const DAY: u64 = 86_400;
// Bump when stored data or the matching guarantees change. Old cache entries
// then become misses instead of bypassing newer validation.
const CACHE_VERSION: u8 = 1;
const WIKI_CACHE_VERSION: u8 = 1;
// Bound title-search traffic; a missing result is not an exhaustive HLTB search.
const MAX_CANDIDATES: usize = 3;
// Discovery downloads third-party scripts as well as JSON; bound both bodies.
const MAX_BODY: u64 = 8 * 1024 * 1024;
// The public website rejects requests with reqwest's default user agent.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
// These guards serve different purposes: BUSY keeps the UI from spawning a
// queue of workers, while PROVIDER owns the reusable client, discovered route,
// and rate-limit cooldown shared across games. Neither is held by the UI while
// it renders or handles input.
static BUSY: AtomicBool = AtomicBool::new(false);
static PROVIDER: Mutex<Option<Provider>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static WORKER_TEST_LOCK: Mutex<()> = Mutex::new(());
// Extract only the embedded data/route references needed by this integration;
// scripts are never executed. LazyLock avoids recompiling these expressions on
// every selection. Relative chunk paths stay on the provider's own host.
static SCRIPTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<script[^>]+src=["']([^"']+)["']"#).unwrap());
static ENDPOINT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"fetch\s*\(\s*["'](/api/[a-zA-Z0-9_/]+)[^"']*["']\s*,\s*\{[^}]*method:\s*["']POST["']"#,
    )
    .unwrap()
});
static PAGE_DATA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<script\b[^>]*\bid=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#).unwrap()
});
static WIKI_COMMENTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
static WIKI_INFOBOX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\{\{\s*Infobox[ _]game\b").unwrap());
static WIKI_FIELDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^\|[ \t]*(steam[ _]appid(?:[ _]side)?|hltb)[ \t]*=[ \t]*([^\r\n]*)").unwrap()
});

/// A worker owns this until completion. Drop also releases the reservation if
/// unwinding occurs, so one failed lookup cannot permanently block later games.
pub(crate) struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// A single worker bounds request volume without queuing games scrolled past.
pub(crate) fn reserve() -> Option<Permit> {
    BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .ok()
        .map(|_| Permit)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HltbData {
    /// HLTB's game ID, distinct from the Steam app ID used for verification.
    pub game_id: u64,
    pub game_name: String,
    /// Main Story seconds; None means no submitted time for this style.
    pub main: Option<u64>,
    /// Main + Extras total seconds, not time to add to `main`.
    pub extra: Option<u64>,
    /// Completionist seconds, independently optional like the other styles.
    pub completionist: Option<u64>,
}

impl HltbData {
    pub fn url(&self) -> String {
        format!("{BASE}/game/{}", self.game_id)
    }
}

/// Per-game lookup lifecycle, shared by cloned `Game`s. Terminal results remain
/// stable for this library load; `,ct` retries failures and `r` rebuilds the games.
#[derive(Debug, Clone, Default)]
pub enum HltbState {
    /// No worker has been reserved, including while another game owns it.
    #[default]
    Pending,
    Loading,
    Ready {
        data: HltbData,
        /// Expired data is refreshing or its refresh failed; do not extend its TTL.
        stale: bool,
    },
    /// Search/verification completed without finding a matching Steam app ID.
    NotFound,
    /// Transport or schema failure; keep the diagnostic for the browser action.
    Unavailable(String),
}

/// `data: None` is a successful negative lookup, never a request failure. Both
/// forms need timestamps so missing associations can be retried as HLTB changes.
#[derive(Serialize, Deserialize)]
struct Cached {
    version: u8,
    steam_id: i32,
    fetched_at: u64,
    data: Option<HltbData>,
}

impl Cached {
    fn fresh(&self, now: u64) -> bool {
        // Misses expire sooner: an incomplete association should not hide a game
        // for as long as a positively verified match can be reused.
        let ttl = if self.data.is_some() { 7 * DAY } else { DAY };
        // A future timestamp (for example after a clock adjustment) is a miss,
        // rather than an underflow or an entry that never expires.
        now.checked_sub(self.fetched_at)
            .is_some_and(|age| age < ttl)
    }
}

// Wall-clock seconds survive process restarts; request budgets below use Instant.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// An unreadable, old-schema, or mismatched entry is only a cache miss. It must
/// not prevent a fresh lookup or turn optional metadata into a startup failure.
fn read_cache(path: &Path, steam_id: i32) -> Option<Cached> {
    let cached: Cached = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (cached.version == CACHE_VERSION
        && cached.steam_id == steam_id
        && cached
            .data
            .as_ref()
            .is_none_or(|d| d.game_id > 0 && !d.game_name.trim().is_empty()))
    .then_some(cached)
}

fn write_cache(path: &Path, cached: &impl Serialize) -> Result<(), STError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // The worker permit serializes writes within this process; the PID keeps
    // concurrently running TUI instances from sharing their temporary file.
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    // Same-directory rename avoids partially written entries after interruption.
    let result = (|| {
        fs::write(&temp, serde_json::to_vec(cached)?)?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}

/// Run on the metadata worker, not the draw path: even a fresh cache read does
/// filesystem I/O. Construct the provider lazily only if the cache needs a fetch.
pub(crate) fn lookup(steam_id: i32, name: &str, publish: &mut dyn FnMut(HltbData)) -> HltbState {
    let path = cache_directory()
        .ok()
        .map(|p| p.join("hltb").join(format!("{steam_id}.json")));
    lookup_cached(path.as_deref(), steam_id, now(), publish, |game_id| {
        let mut provider = PROVIDER.lock().map_err(|e| e.to_string())?;
        if provider.is_none() {
            let wiki = PcGamingWiki::new(
                WIKI_BASE,
                path.as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.join("pcgamingwiki")),
            )
            .map_err(|error| log!("PCGamingWiki client unavailable", error))
            .ok();
            *provider = Some(Provider::new(BASE, wiki)?);
        }
        provider
            .as_mut()
            .unwrap()
            .fetch(steam_id, name, game_id)
            .map_err(|e| e.to_string())
    })
}

/// Cache policy is independent of transport so successful misses, expired
/// positives, and provider failures have distinct, testable outcomes.
fn lookup_cached(
    path: Option<&Path>,
    steam_id: i32,
    timestamp: u64,
    mut publish: impl FnMut(HltbData),
    fetch: impl FnOnce(Option<u64>) -> Result<Option<HltbData>, String>,
) -> HltbState {
    let cached = path.and_then(|p| read_cache(p, steam_id));
    if let Some(c) = &cached
        && c.fresh(timestamp)
    {
        return match &c.data {
            Some(data) => HltbState::Ready {
                data: data.clone(),
                stale: false,
            },
            None => HltbState::NotFound,
        };
    }
    // An expired positive still identifies a page worth checking first. It is
    // only a hint: the provider must verify the Steam association again.
    let game_id = cached.as_ref().and_then(|c| c.data.as_ref()).map(|data| {
        // Publish on the worker before network I/O. Rendering can retain
        // verified times while the same lookup refreshes them in place.
        publish(data.clone());
        data.game_id
    });
    match fetch(game_id) {
        Ok(data) => {
            if let Some(path) = path {
                let cached = Cached {
                    version: CACHE_VERSION,
                    steam_id,
                    fetched_at: now(),
                    data: data.clone(),
                };
                if let Err(error) = write_cache(path, &cached) {
                    log!("HLTB cache write failed", steam_id, error);
                }
            }
            match data {
                Some(data) => HltbState::Ready { data, stale: false },
                None => HltbState::NotFound,
            }
        }
        Err(error) => {
            // Do not rewrite a failure as a negative cache entry or renew stale
            // data. The UI may still show an earlier verified result.
            log!("HLTB lookup failed", steam_id, error);
            match cached.and_then(|c| c.data) {
                Some(data) => HltbState::Ready { data, stale: true },
                None => HltbState::Unavailable(error),
            }
        }
    }
}

/// Keep HTTP status separate from the human diagnostic: only specific status
/// classes justify rediscovering a route or applying a service-wide cooldown.
#[derive(Debug)]
struct FetchError {
    context: String,
    status: Option<StatusCode>,
}
impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.context)
    }
}
impl std::error::Error for FetchError {}

impl From<String> for FetchError {
    fn from(context: String) -> Self {
        Self {
            context,
            status: None,
        }
    }
}
impl From<&str> for FetchError {
    fn from(context: &str) -> Self {
        context.to_string().into()
    }
}

type FetchResult<T> = Result<T, FetchError>;

/// Page IDs are hints, never completion data. Keep this cache separate so a
/// transient HLTB failure does not require repeating both wiki API requests.
#[derive(Serialize, Deserialize)]
struct WikiCached {
    version: u8,
    steam_id: i32,
    fetched_at: u64,
    game_id: Option<u64>,
}

impl WikiCached {
    fn fresh(&self, steam_id: i32, timestamp: u64) -> bool {
        self.version == WIKI_CACHE_VERSION
            && self.steam_id == steam_id
            && self.game_id.is_none_or(|id| id > 0)
            && timestamp
                .checked_sub(self.fetched_at)
                .is_some_and(|age| age < if self.game_id.is_some() { 7 * DAY } else { DAY })
    }
}

/// PCGamingWiki needs an identifying user agent and no HLTB Origin/Referer.
/// A separate client also keeps its pacing/cooldown independent of HLTB.
struct PcGamingWiki {
    client: Client,
    base: String,
    cache: Option<PathBuf>,
    next_request: Option<Instant>,
    retry_after: Option<Instant>,
}

impl PcGamingWiki {
    fn new(base: &str, cache: Option<PathBuf>) -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(WIKI_USER_AGENT)
            .connect_timeout(Duration::from_secs(5))
            // The two documented API calls need no redirects. Do not let an
            // unexpected redirect spend unpaced requests on another host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            base: base.into(),
            cache,
            next_request: None,
            retry_after: None,
        })
    }

    fn json(&mut self, query: &[(&str, &str)], deadline: Instant) -> FetchResult<Value> {
        if self.retry_after.is_some_and(|t| t > Instant::now()) {
            return Err("PCGamingWiki rate limited; wait a minute before retrying".into());
        }
        // PCGamingWiki documents 60 requests/minute. This worker is serialized;
        // spacing request starts also prevents bursts across different games.
        if let Some(next) = self.next_request {
            let wait = next.saturating_duration_since(Instant::now());
            if wait >= deadline.saturating_duration_since(Instant::now()) {
                return Err("PCGamingWiki lookup timed out while waiting to request".into());
            }
            std::thread::sleep(wait);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("PCGamingWiki lookup timed out")?;
        self.next_request = Some(Instant::now() + Duration::from_secs(1));
        let response = self
            .client
            .get(format!("{}/w/api.php", self.base))
            .query(query)
            .timeout(remaining.min(Duration::from_secs(8)))
            .send();
        let body = match Provider::body(response, "PCGamingWiki API request") {
            Err(e) if e.status == Some(StatusCode::TOO_MANY_REQUESTS) => {
                self.retry_after = Some(Instant::now() + Duration::from_secs(60));
                return Err(e);
            }
            result => result?,
        };
        let value: Value =
            serde_json::from_str(&body).map_err(|e| format!("PCGamingWiki API response: {e}"))?;
        // MediaWiki may report failures (including ratelimited) with HTTP 200.
        if let Some(error) = value.get("error") {
            if error["code"].as_str() == Some("ratelimited") {
                self.retry_after = Some(Instant::now() + Duration::from_secs(60));
            }
            return Err(format!("PCGamingWiki API error: {error}").into());
        }
        Ok(value)
    }

    fn lookup(&mut self, steam_id: i32, deadline: Instant) -> FetchResult<Option<u64>> {
        let path = self
            .cache
            .as_ref()
            .map(|p| p.join(format!("{steam_id}.json")));
        if let Some(cached) = path
            .as_ref()
            .and_then(|p| fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<WikiCached>(&bytes).ok())
            && cached.fresh(steam_id, now())
        {
            return Ok(cached.game_id);
        }
        let result = self.lookup_inner(steam_id, deadline)?;
        // Cache only successful mappings/misses; HTTP/schema errors stay retryable.
        if let Some(path) = path {
            let cached = WikiCached {
                version: WIKI_CACHE_VERSION,
                steam_id,
                fetched_at: now(),
                game_id: result,
            };
            if let Err(error) = write_cache(&path, &cached) {
                log!("PCGamingWiki cache write failed", steam_id, error);
            }
        }
        Ok(result)
    }

    fn lookup_inner(&mut self, steam_id: i32, deadline: Instant) -> FetchResult<Option<u64>> {
        let id = steam_id.to_string();
        let lookup = self.json(
            &[
                ("action", "idlookup"),
                ("field", "steamappid"),
                ("value", &id),
                ("format", "json"),
            ],
            deadline,
        )?;
        let pages = lookup["idlookup"]
            .as_array()
            .ok_or("PCGamingWiki lookup pages missing")?;
        if pages.is_empty() {
            return Ok(None);
        }
        // Do not arbitrarily select a page when an app ID has multiple matches.
        if pages.len() != 1 {
            return Err("PCGamingWiki Steam app ID maps to multiple pages".into());
        }
        let title = pages[0]
            .pointer("/title/Page")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("PCGamingWiki page title missing")?;
        let page = self.json(
            &[
                ("action", "parse"),
                ("page", title),
                ("prop", "wikitext"),
                ("redirects", "1"),
                ("format", "json"),
            ],
            deadline,
        )?;
        let text = page
            .pointer("/parse/wikitext/*")
            .and_then(Value::as_str)
            .ok_or("PCGamingWiki page data missing")?;
        wiki_game_id(text, steam_id).map_err(FetchError::from)
    }
}

/// Read only the game infobox, ignoring commented-out IDs and unrelated text.
/// The mapping is checked against the page's Steam IDs and then against HLTB.
fn wiki_game_id(text: &str, steam_id: i32) -> Result<Option<u64>, String> {
    let text = WIKI_COMMENTS.replace_all(text, "");
    let start = WIKI_INFOBOX
        .find(&text)
        .ok_or("PCGamingWiki game infobox missing")?;
    let bytes = text.as_bytes();
    let (mut pos, mut depth) = (start.start(), 0usize);
    let end = loop {
        match bytes.get(pos..pos + 2) {
            Some(b"{{") => {
                depth += 1;
                pos += 2;
            }
            Some(b"}}") => {
                depth -= 1;
                if depth == 0 {
                    break pos;
                }
                pos += 2;
            }
            Some(_) => pos += 1,
            None => return Err("PCGamingWiki game infobox incomplete".into()),
        }
    };
    let mut fields = std::collections::HashMap::new();
    for field in WIKI_FIELDS.captures_iter(&text[start.end()..end]) {
        let key = field[1].to_ascii_lowercase().replace('_', " ");
        if fields.insert(key, field[2].trim().to_owned()).is_some() {
            return Err("PCGamingWiki duplicate game identity field".into());
        }
    }
    let expected = u64::try_from(steam_id).map_err(|_| "Invalid Steam app ID")?;
    if !["steam appid", "steam appid side"].iter().any(|key| {
        fields.get(*key).is_some_and(|ids| {
            ids.split(',')
                .any(|id| id.trim().parse::<u64>().ok() == Some(expected))
        })
    }) {
        return Err("PCGamingWiki page did not confirm the Steam app ID".into());
    }
    match fields
        .get("hltb")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        None => Ok(None),
        Some(id) => id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(Some)
            .ok_or_else(|| "PCGamingWiki HLTB ID invalid".into()),
    }
}

/// Website-session state shared across selections. The base URL is injectable
/// for local HTTP tests; production always uses the fixed HLTB origin above.
struct Provider {
    client: Client,
    base: String,
    endpoint: Option<String>,
    // Reuse only in this process; rejected tokens are renewed before discovery.
    token: Option<String>,
    // Learned from an HTML page already needed by a lookup; never persisted.
    build_id: Option<String>,
    retry_after: Option<Instant>,
    wiki: Option<PcGamingWiki>,
}

impl Provider {
    fn new(base: &str, wiki: Option<PcGamingWiki>) -> Result<Self, String> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::REFERER,
            format!("{base}/")
                .parse()
                .map_err(|e| format!("HLTB referer: {e}"))?,
        );
        headers.insert(
            reqwest::header::ORIGIN,
            base.parse().map_err(|e| format!("HLTB origin: {e}"))?,
        );
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            base: base.into(),
            endpoint: None,
            token: None,
            build_id: None,
            retry_after: None,
            wiki,
        })
    }

    /// Apply the remaining lookup budget to each GET. Discovery and candidate
    /// checks share one deadline rather than each receiving a new 30 seconds.
    fn request(
        &self,
        path: &str,
        deadline: Instant,
    ) -> FetchResult<reqwest::blocking::RequestBuilder> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("HLTB lookup timed out")?;
        Ok(self
            .client
            .get(format!("{}{path}", self.base))
            .timeout(remaining.min(Duration::from_secs(8))))
    }

    /// Check status before parsing, preserving 429 even when its body is HTML.
    /// Read at most MAX_BODY + 1 bytes so oversized site responses fail explicitly.
    fn body(response: reqwest::Result<Response>, context: &str) -> FetchResult<String> {
        let response = response.map_err(|e| FetchError {
            context: format!("{context}: {e}"),
            status: e.status(),
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError {
                context: format!("{context}: HTTP {status}"),
                status: Some(status),
            });
        }
        let mut text = String::new();
        response
            .take(MAX_BODY + 1)
            .read_to_string(&mut text)
            .map_err(|e| format!("{context}: {e}"))?;
        if text.len() as u64 > MAX_BODY {
            return Err(format!("{context}: response too large").into());
        }
        Ok(text)
    }

    fn text(&self, path: &str, deadline: Instant) -> FetchResult<String> {
        Self::body(self.request(path, deadline)?.send(), "HLTB page request")
    }

    /// Prefer the smaller Next.js response once a page has taught us its build
    /// ID. Only route/format failures warrant HTML fallback; an outage or rate
    /// limit must not double traffic. Both paths share the original deadline.
    fn game(&mut self, steam_id: i32, id: u64, deadline: Instant) -> FetchResult<Option<HltbData>> {
        if let Some(build) = self.build_id.clone() {
            match self.text(&format!("/_next/data/{build}/game/{id}.json"), deadline) {
                Ok(body) => {
                    let result = serde_json::from_str::<Value>(&body)
                        .map_err(|e| format!("HLTB game JSON: {e}"))
                        .and_then(|data| verified_game(&data["pageProps"], steam_id, id));
                    match result {
                        Ok(data) => return Ok(data),
                        Err(error) => {
                            log!("HLTB game JSON invalid; falling back to HTML", id, error)
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.status,
                        Some(
                            StatusCode::UNAUTHORIZED
                                | StatusCode::FORBIDDEN
                                | StatusCode::NOT_FOUND
                                | StatusCode::GONE
                        )
                    ) =>
                {
                    log!(
                        "HLTB JSON route unavailable; falling back to HTML",
                        id,
                        error
                    )
                }
                Err(error) => return Err(error),
            }
            self.build_id = None;
        }
        let html = self.text(&format!("/game/{id}"), deadline)?;
        let data = html_data(&html)?;
        let result = verified_game(&data["props"]["pageProps"], steam_id, id)?;
        self.build_id = build_id(&data);
        Ok(result)
    }

    /// HLTB changes route names and chunk hashes. Discover the POST route from
    /// the currently served homepage instead of pinning either in the binary.
    fn discover(&mut self, deadline: Instant) -> FetchResult<String> {
        let html = self.text("/", deadline)?;
        // Reuse the homepage already needed for search so its first match can
        // use JSON too. Missing build metadata must not prevent title search.
        self.build_id = html_data(&html).ok().and_then(|data| build_id(&data));
        let mut paths: Vec<_> = SCRIPTS
            .captures_iter(&html)
            .map(|c| c[1].to_owned())
            .filter(|p| p.starts_with("/_next/static/chunks/"))
            .collect();
        // The index chunk normally owns search; try it before unrelated page
        // bundles to reduce requests, while retaining the bounded fallback.
        paths.sort_by_key(|p| !p.contains("pages/index-"));
        for path in paths.into_iter().take(12) {
            // A missing chunk can follow a site deployment; other HTTP failures
            // should surface immediately, especially rate limiting.
            let script = match self.text(&path, deadline) {
                Ok(script) => script,
                Err(e) if e.status == Some(StatusCode::NOT_FOUND) => continue,
                Err(e) => return Err(e),
            };
            if let Some(c) = ENDPOINT.captures(&script) {
                return Ok(c[1].trim_end_matches('/').into());
            }
        }
        Err("HLTB search endpoint not found in site scripts".into())
    }

    /// Mirror the public search form. Its short-lived website token is unrelated
    /// to Steam authentication and must never be stored in the game cache.
    fn search(&mut self, endpoint: &str, name: &str, deadline: Instant) -> FetchResult<Value> {
        if self.token.is_none() {
            // A fresh cache-busting timestamp prevents reusing an expired init
            // response when the search route is retried after a deployment.
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let auth = self.text(&format!("{endpoint}/init?t={nonce}"), deadline)?;
            let auth: Value = serde_json::from_str(&auth)
                .map_err(|e| format!("HLTB search token response: {e}"))?;
            let token = auth
                .get("token")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .ok_or("HLTB search token missing")?;
            self.token = Some(token.into());
        }
        let payload = json!({
            "searchType": "games", "searchTerms": name.split_whitespace().collect::<Vec<_>>(),
            "searchPage": 1, "size": 20, "useCache": true,
            "searchOptions": {
                "games": {"userId": 0, "platform": "", "sortCategory": "popular",
                    "rangeCategory": "main", "rangeTime": {"min": 0, "max": 0},
                    "gameplay": {"perspective": "", "flow": "", "genre": "", "difficulty": ""},
                    "rangeYear": {"min": "", "max": ""}, "modifier": ""},
                "users": {"sortCategory": "postcount"}, "lists": {"sortCategory": "follows"},
                "filter": "", "sort": 0, "randomizer": 0
            }
        });
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("HLTB lookup timed out")?;
        let response = self
            .client
            .post(format!("{}{endpoint}", self.base))
            .header("x-auth-token", self.token.as_deref().unwrap())
            .timeout(remaining.min(Duration::from_secs(8)))
            .json(&payload)
            .send();
        let body = Self::body(response, "HLTB search request")?;
        serde_json::from_str(&body).map_err(|e| format!("HLTB search response: {e}").into())
    }

    /// One selected-game lookup. Rate limiting belongs to the shared provider
    /// so moving to a different game cannot immediately bypass the cooldown.
    fn fetch(
        &mut self,
        steam_id: i32,
        name: &str,
        game_id: Option<u64>,
    ) -> FetchResult<Option<HltbData>> {
        if steam_id <= 0 || name.trim().is_empty() {
            return Err("Invalid Steam game identity".into());
        }
        if self.retry_after.is_some_and(|t| t > Instant::now()) {
            return Err("HLTB rate limited; wait a minute before retrying".into());
        }
        let result = self.fetch_inner(
            steam_id,
            name,
            game_id,
            Instant::now() + Duration::from_secs(30),
        );
        if result
            .as_ref()
            .err()
            .is_some_and(|e| e.status == Some(StatusCode::TOO_MANY_REQUESTS))
        {
            self.retry_after = Some(Instant::now() + Duration::from_secs(60));
        }
        result
    }

    fn fetch_inner(
        &mut self,
        steam_id: i32,
        name: &str,
        game_id: Option<u64>,
        deadline: Instant,
    ) -> FetchResult<Option<HltbData>> {
        if let Some(id) = game_id {
            // Refresh a known page without search discovery or authentication.
            // A removed page/association can still be recovered by title search;
            // transport/schema failures retain stale data without extra traffic.
            match self.game(steam_id, id, deadline) {
                Ok(Some(data)) => return Ok(Some(data)),
                Ok(None) => {}
                Err(e) if e.status == Some(StatusCode::NOT_FOUND) => {}
                Err(e) => return Err(e),
            }
        }
        let mut wiki_error = None;
        if let Some(wiki) = self.wiki.as_mut() {
            match wiki.lookup(steam_id, deadline) {
                Ok(Some(id)) if Some(id) != game_id => match self.game(steam_id, id, deadline) {
                    Ok(Some(data)) => return Ok(Some(data)),
                    Ok(None) => {}
                    Err(e) if e.status == Some(StatusCode::NOT_FOUND) => {}
                    Err(e) => return Err(e),
                },
                Ok(_) => {}
                Err(error) => {
                    log!(
                        "PCGamingWiki lookup failed; falling back to HLTB search",
                        steam_id,
                        error
                    );
                    wiki_error = Some(error);
                }
            }
        }
        let result = self.search_game(steam_id, name, deadline);
        // A source outage is not a cacheable missing game. A verified fallback
        // is still usable, but a search miss must retain the original failure.
        match (result, wiki_error) {
            (Ok(None), Some(error)) => Err(error.to_string().into()),
            (result, _) => result,
        }
    }

    fn search_game(
        &mut self,
        steam_id: i32,
        name: &str,
        deadline: Instant,
    ) -> FetchResult<Option<HltbData>> {
        if self.endpoint.is_none() {
            self.endpoint = Some(self.discover(deadline)?);
        }
        let endpoint = self.endpoint.clone().unwrap();
        let reused_token = self.token.is_some();
        let mut search = self.search(&endpoint, name, deadline);
        if reused_token
            && search.as_ref().err().is_some_and(|e| {
                matches!(
                    e.status,
                    Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                )
            })
        {
            // An expired token does not imply a changed route. Renew it once
            // before downloading the homepage and scripts again.
            self.token = None;
            search = self.search(&endpoint, name, deadline);
        }
        let response = match search {
            Ok(response) => response,
            // These statuses may indicate an expired token or changed route.
            // Rediscover once within the original deadline; 429/5xx must not
            // trigger another homepage/chunk/search sequence.
            Err(e)
                if matches!(
                    e.status,
                    Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND)
                ) =>
            {
                self.endpoint = None;
                self.token = None;
                self.endpoint = Some(self.discover(deadline)?);
                let endpoint = self.endpoint.clone().unwrap();
                self.search(&endpoint, name, deadline)?
            }
            Err(e) => return Err(e),
        };
        let games = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or("HLTB search results missing")?;
        let mut candidates: Vec<_> = games.iter().collect();
        // Title similarity chooses which pages to check first; it never decides
        // whether a game matches. The Steam association below is authoritative.
        let title = normalized(name);
        candidates.sort_by_key(|g| {
            !(normalized(g["game_name"].as_str().unwrap_or("")) == title
                || g["game_alias"]
                    .as_str()
                    .unwrap_or("")
                    .split(',')
                    .any(|a| normalized(a) == title))
        });
        let mut last_error = None;
        for game in candidates.into_iter().take(MAX_CANDIDATES) {
            let candidate = (|| {
                let id = number(&game["game_id"])
                    .filter(|id| *id > 0)
                    .ok_or("HLTB result ID missing")?;
                self.game(steam_id, id, deadline)
            })();
            match candidate {
                Ok(Some(data)) => return Ok(Some(data)),
                Ok(None) => {}
                Err(e) if e.status == Some(StatusCode::TOO_MANY_REQUESTS) => return Err(e),
                // A bad candidate page need not hide a later valid match, but
                // remember failures so they cannot become a cached "not found".
                Err(e) => last_error = Some(e),
            }
        }
        // Failed verification requests are errors, not cacheable negative matches.
        match last_error {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }
}

// Ranking only: punctuation/case differences must not relax identity checks.
fn normalized(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

// Public responses may encode numeric IDs/times as JSON numbers or strings.
fn number(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Missing/null/zero times mean no submission. Malformed values instead signal
/// a response-format problem; displaying them as a normal miss would conceal it.
fn seconds(game: &Value, key: &str) -> Result<Option<u64>, String> {
    match game.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => number(value)
            .map(|n| (n > 0).then_some(n))
            .ok_or_else(|| format!("HLTB {key}: invalid completion time")),
    }
}

fn build_id(data: &Value) -> Option<String> {
    // Discovery is an untrusted URL path segment, not an arbitrary URL.
    data["buildId"]
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .map(str::to_owned)
}

fn html_data(html: &str) -> Result<Value, String> {
    let data = PAGE_DATA.captures(html).ok_or("HLTB page data missing")?;
    serde_json::from_str(&data[1]).map_err(|e| format!("HLTB page data: {e}"))
}

/// Search results omit Steam IDs, so verify the HTML or JSON game-page data before
/// publishing a result. Both the selected Steam ID and requested HLTB page ID
/// must agree; a similarly named game or unexpected page is insufficient.
fn verified_game(
    props: &Value,
    steam_id: i32,
    expected_id: u64,
) -> Result<Option<HltbData>, String> {
    let steam_id = u64::try_from(steam_id)
        .ok()
        .filter(|id| *id > 0)
        .ok_or("Invalid Steam app ID")?;
    let games = props
        .pointer("/game/data/game")
        .and_then(Value::as_array)
        .ok_or("HLTB game data missing")?;
    for game in games {
        // HLTB sometimes associates two Steam releases (such as an alternate
        // edition) with one entry. Accept either explicitly recorded association.
        if !["profile_steam", "profile_steam_alt"]
            .iter()
            .any(|key| number(&game[key]) == Some(steam_id))
        {
            continue;
        }
        let game_id = number(&game["game_id"])
            .filter(|id| *id > 0)
            .ok_or("HLTB game ID missing")?;
        if game_id != expected_id {
            return Err("HLTB returned a different game page".into());
        }
        let name = game["game_name"]
            .as_str()
            .filter(|n| !n.trim().is_empty())
            .ok_or("HLTB title missing")?;
        return Ok(Some(HltbData {
            game_id,
            game_name: name.into(),
            main: seconds(game, "comp_main")?,
            extra: seconds(game, "comp_plus")?,
            completionist: seconds(game, "comp_100")?,
        }));
    }
    Ok(None)
}

#[cfg(test)]
#[path = "hltb_tests.rs"]
mod tests;
