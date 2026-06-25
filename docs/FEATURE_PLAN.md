# Aurelia-TUI feature plan

Goal: surface **virtually every `aurelia` CLI command (except luxtorpeda)** in the
TUI, one feature per branch.

## Workflow

- One `feat/<name>` branch per feature, implemented by a subagent in an isolated
  git worktree (`cargo check` clean, zero warnings — never `cargo build`, the
  running exe may be locked).
- Commits are **title-only** (`[Feat] …`), authored by the repo's git identity
  (Alex), no co-author, no push.
- Branches are merged into `main` with **union** conflict resolution (both sides
  kept); the recurring merge hazard is a dropped closing `}` where two features
  concatenate — fix with a brace-depth scan, then re-check.
- TLS: use **native-tls** (not rustls — `ring` breaks aarch64-windows) with
  `schannel >= 0.1.27` pinned on Windows. See `Cargo.toml`.

## Status

### Done — merged into `main`

Core (pre-existing): `login` (health/qr/classic) · `list` (browse) · `info`
(detail) · `install` · `play` · `image` (internal artwork).

Feature branches (all merged):

| Command | Key | Notes |
|---|---|---|
| `account` | `A` | overlay; `o` = logout |
| `achievements` | `a` | scrollable, unlock state + rarity |
| `dlc` (enable/disable) | `D` | `e`/`x` toggle |
| `uninstall` | `x` | `y`/`n` confirm |
| `verify` | `v` | streaming badge |
| `cloud` (list/sync) | `C` | `s` = sync |
| `update` | `U` | streaming badge |
| `proton` (list/default) | `P` | `d` = set default |
| `branches` (set-branch) | `b` | `Enter` = switch |
| `friends` (list) | `F` | scrollable |
| `wallet` | `w` | balance overlay |
| `market` (listings) | `m` | listings + buy orders |
| `depots` | `o` | depot list |
| `launch-options` | `L` | launch entries |
| `inventory` | `I` | per-game items |
| `running` (+stop) | `R` | `s` = stop |
| `config` (show/presence) | `p` | `o` = toggle presence |
| `workshop` (list) | `W` | subscribed items |
| `logout` | `A`→`o` | in account overlay |
| `move` | `M` | path-entry relocate |
| `relink` | `K` | path-entry re-point install at another library |
| `import` | `N` | path-entry register existing on-disk install |
| `chat` (history/send) | `c` | from friends overlay; history view + send input line |

### Remaining — to implement (branch per feature)

_None — every in-scope `aurelia` command is now surfaced in the TUI._

### Optional extensions — implemented (beyond original scope)

- `friends` search/add/remove (`F` → `a` add overlay with `Enter` search + `a` send
  request; `x` remove with `y`/`n` confirm) — all backend calls off the UI thread.
- `market` search/price (`S` opens a search overlay: `Enter` search, `Up`/`Down` move,
  `Tab` price the highlighted result, `Esc` close) — read-only, off the UI thread.
- `proton` install/uninstall (in the `P` overlay: `i` install the highlighted runtime
  with streamed progress, `u` uninstall a custom GE runtime with `y`/`n` confirm) —
  off the UI thread, list refreshes after.
- `cloud` directional sync (in the `C` overlay: `s` both ways, `d` download-only,
  `u` upload-only — `cloud sync --down`/`--up`), off the UI thread.
- `workshop` browse + subscribe/unsubscribe (in the `W` overlay: `b` opens a browse/search
  pane — `Enter` searches, `Up`/`Down` move, `Tab` subscribes/unsubscribes the highlighted
  item, `Esc` back to the subscribed list) — browse/search/subscribe and the post-action
  list refresh all run off the UI thread, gen-tagged to drop stale results.

### Out of scope / optional extensions (documented, not planned)

- Sub-command extensions of merged features:
  `workshop` rate/comments,
  `config` language / per-game proton, `dlc`/`install` `--restart-steam`.
- `available` — effectively covered by the install/status badges.
- `daemon` / `kill` — process/daemon infrastructure, not user-facing TUI features.
- `luxtorpeda` — explicitly excluded.
