# HowLongToBeat

Selecting a game loads completion times alongside the existing ProtonDB and
store metadata. Main + Extras is the total for that play style, not extra time
added to Main Story. Times are rounded to the nearest minute; a positive time
shorter than a minute displays as `1m`. A dash means no submitted time.

`,ct` (comma, then `c`, then `t`) and **Actions → HowLongToBeat page** open the
same verified game page using your platform's default browser. The full URL is
available through this action rather than taking another row in the detail
pane. On Linux this requires
`xdg-open`; macOS uses `open`, and Windows uses `start`.

The chord is active only during library navigation. `Esc` cancels an unfinished
sequence; an unrelated key cancels the sequence and keeps its normal action.
Text fields still accept these characters normally. Plain `t` no longer invokes
HLTB; `c` still cancels an install and `C` still opens Cloud Saves.

## Enabling and disabling

Press `p` to open Settings, then `h` to toggle **HowLongToBeat**. The switch is
on by default, including for older config files that do not yet have this
field. Its value is saved as `hltb_enabled` in
`~/.config/aurelia-tui/config.json` (or under `AURELIA_TUI_DIR`). You can also
edit that field while the TUI is closed.

The switch uses the existing settings overlay, like other third-party
integration settings such as Luxtorpeda. HLTB's preference is owned by the TUI,
while runtime-plugin settings are owned by the separate Aurelia launcher.
The local switch remains accessible if reading launcher settings fails.

When off, no new lookup through PCGamingWiki or HLTB is scheduled, its detail content and Actions menu
entry are hidden, and `,ct` displays a disabled notice rather than opening a
browser. A lookup or browser launch already running can finish; disabling
does not cancel an HTTP request or close an existing browser. Completed lookup
data may remain cached but stays hidden until re-enabled. Existing cache files
are retained, so turning it back on can reuse verified times.

`h` only toggles in Settings when the proxy edit field is not active; typing
`h` into a proxy URL does not change the preference.

## Matching and failures

The primary lookup uses PCGamingWiki's anonymous MediaWiki API: `idlookup`
resolves the Steam app ID to a wiki page, then `parse` reads its game infobox.
The page must confirm the Steam app ID and provide a numeric `hltb` ID. Multiple
matching wiki pages, malformed fields, and inconsistent identities are rejected.
PCGamingWiki supplies the page mapping; completion estimates still come from
HLTB, so this is not an independent source of times during an HLTB outage.

If the mapping is missing or PCGamingWiki fails, the lookup falls back to HLTB
title search, prioritizes exact normalized title/alias matches, and checks up
to three candidates. A result from either route is shown only if its HLTB game
page contains the selected Steam app ID in
`profile_steam` or `profile_steam_alt`. This is deliberately conservative:
**No verified match** can mean HLTB's Steam association is missing or the match
was outside the checked candidates. There is no title-only fallback.

**Fetch Failed** means the lookup failed, not that the game has no entry.
Press `,ct` (or choose the HLTB action) to retry without reloading the Steam
library or restarting the app. Failure details go to the diagnostic logger.
The row stays hidden during a first lookup. **Cached** means previously verified
estimates are displayed while refreshing or after a failed refresh; it does
not necessarily mean your network is offline. HTTP,
response-format, and cache write errors also go through the project's
diagnostic logger (stderr when redirected).

Press `r` to refresh the library and retry failed lookups. Lookups run once per
game per library load unless a failure is explicitly retried, after input has
settled, with at most one worker active. Browsing and game actions remain
available while the worker runs; other HLTB lookups wait for that worker.
Each network lookup has a 30-second budget and eight-second request timeouts.
HLTB HTTP 429 stops the lookup and defers further HLTB requests for one minute;
retry after that interval. PCGamingWiki has its own one-minute cooldown after
HTTP 429 or a MediaWiki `ratelimited` error; HLTB search remains available.
Wiki API request starts are spaced at least one second apart within this app
instance to respect its documented 60 requests/minute limit. Waiting happens
on the worker and counts toward the same lookup deadline.

There is no fixed retry count per game. The primary route normally makes three
requests: two to PCGamingWiki and one to the mapped HLTB game page. A cached
mapping needs only the HLTB page request. An expired positive estimate also
normally needs just one request to its already verified game page, before
consulting PCGamingWiki. Every fetched HLTB page is verified again. Removed
pages or changed associations fall through to mapping/search; transport/schema
failures preserve stale data without starting another search. Fresh positive
and negative estimate cache hits contact neither service.

The first HLTB game-page response supplies a Next.js build ID. Fallback search
also learns it from the homepage already fetched for endpoint discovery, so its
first match can use JSON immediately. Once known, game-page requests use the
smaller JSON route, with the same Steam ID and HLTB ID checks.
The build ID stays in memory; there is no extra homepage request to discover it.
A missing/rejected JSON route or unexpected response format falls back to HTML
within the original deadline and relearns the build ID. This can add one request
after a site deployment. Network errors, rate limits, and server errors do not
trigger an additional HTML request.

Fallback title search normally needs five HLTB requests with a newly discovered
endpoint: homepage, index script, token initialization, search, and game page.
Later title searches normally need two because the endpoint and token are
reused in memory. Wiki requests before a fallback add to that total.

Discovery may inspect up to twelve scripts, and verification checks up to three
candidate pages. A reused token rejected with 401/403 is renewed once before
rediscovery is considered. A 401/403/404 from the search/token route can trigger
one rediscovery attempt, within the original lookup budget. Rate limits and
server errors do not trigger token renewal or rediscovery.

## Cache

Entries live at `~/.cache/aurelia-tui/hltb/<steam-app-id>.json`, beneath
`AURELIA_TUI_CACHE_DIR` when overridden. Positive matches are reused for seven
days and negative matches for one day, checked when the next lookup starts.
Successful in-memory results remain unchanged until the library is reloaded.
Failed requests are never stored as negative matches. Expired positive results
are shown before their background refresh starts and remain available if it
fails; their timestamps are not extended.

PCGamingWiki mappings live in `hltb/pcgamingwiki/<steam-app-id>.json`, under
the same cache directory. Mappings last seven days and successful missing
mappings one day. They are hints, not verified completion data, and are stored
separately so a failed HLTB request can be retried without repeating the wiki
requests. HTTP/schema errors are not cached as missing mappings. A wiki outage
followed by an empty HLTB search is a failure rather than a cached missing game.

To force a lookup, remove just that game's cache entry and press `r`. For Portal:

```bash
rm ~/.cache/aurelia-tui/hltb/400.json
```

Cache files are versioned, and malformed or incompatible entries are ignored.
Writes use a temporary file in the same directory followed by a rename.

## Maintaining and testing

The provider in `src/interface/hltb.rs` uses PCGamingWiki's
[documented API](https://www.pcgamingwiki.com/wiki/API) and
[infobox identifiers](https://www.pcgamingwiki.com/wiki/Template:Infobox_game/Documentation).
The wiki client sends an identifying user agent and does not share HLTB's
Origin/Referer headers. It ignores commented-out infobox fields and never
executes wiki templates or follows arbitrary links from page content.

The HLTB fallback uses public website search and game pages, whose structure
can change. It discovers the search endpoint from the
homepage's Next.js scripts and obtains a short-lived token through that
endpoint's `/init` route. It reuses that token in memory and renews it when
rejected. It rediscovers the endpoint at most once after HTTP 401/403/404;
it does not retry rate limits or server errors this way. It checks
at most twelve script references and reuses the HTTP client and endpoint.
The fixtures contain only the public fields needed for matching and times;
no authentication tokens are recorded. The token in HTTP tests is synthetic.

Provider and cache tests live in `src/interface/hltb_tests.rs`, model/render
regressions in `src/interface/game.rs`, and menu tests in `tests/hltb_view.rs`.
The render regression includes installation data, long descriptions, cached
results, and small panes. Switch tests cover legacy config defaults, persisted
off/on values, suppressed cached results and actions, failed saves, and access
without a working launcher. Run the tests without external services:

```bash
cargo test --locked
```

Opt-in live checks cover Portal, Lost Ruins, and Graveyard Keeper. The wiki
check exercises mappings and direct JSON refreshes without discovering an HLTB
search endpoint; the other check exercises fallback search, token reuse, and
direct refresh.
They need internet access but no Steam login:

```bash
cargo test --locked live_pcgamingwiki_matches -- --ignored --nocapture
cargo test --locked live_steam_matches -- --ignored --nocapture
```

Browser launching belongs to `util::browser`, with off-thread results handled
by the browse model; completion-time formatting belongs to `ui::hltb`. The
provider does not render UI or launch processes. Native TLS remains unchanged.

On Linux, a terminal smoke test covers `,ct` and the Actions menu with both
successful and failing browser launchers, plus the Settings toggle and a
startup with HLTB disabled. An offline scenario checks failure rendering,
retry, and library navigation through a refused local proxy connection.
Chord checks cover Escape cancellation, modal transitions, literal filter text,
and the removal of plain `t`. A cached-miss scenario verifies that **No verified
match** remains distinct from **Fetch Failed**.
Two further scenarios hold a first lookup open to check the hidden loading row
and navigation, and verify that stale estimates survive an offline refresh.
The toggle scenario also verifies proxy typing and
that a later favourite save retains the preference. It uses a fake Steam backend, a
seeded HLTB cache, and an `xdg-open` stub; it does not open your real browser.
The existing ProtonDB lookup can still run in the background.

```bash
cargo build --release --locked
python3 scripts/test-hltb-terminal.py target/release/aurelia-tui
```
