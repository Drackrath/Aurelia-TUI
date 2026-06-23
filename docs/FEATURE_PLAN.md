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

### Out of scope / optional extensions (documented, not planned)

- Sub-command extensions of merged features: `friends` add/remove/search,
  `market` price/search, `workshop` browse/install/subscribe/rate/comments,
  `proton` install/uninstall, `config` language / per-game proton,
  `cloud` direction flags, `dlc`/`install` `--restart-steam`.
- `available` — effectively covered by the install/status badges.
- `daemon` / `kill` — process/daemon infrastructure, not user-facing TUI features.
- `luxtorpeda` — explicitly excluded.
