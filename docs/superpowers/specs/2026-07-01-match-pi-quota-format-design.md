# Match pi-quota notification format

## Goal

Change the Telegram summary format to match `ravshansbox/pi-quota`'s widget
text exactly (source: https://raw.githubusercontent.com/ravshansbox/pi-quota/main/index.ts).

Target line shape (per provider):

```
claude: 7d: 73% left (4d 19h), 5h: 100% left (unknown)
codex: 7d: 85% left (5d 18h), 5h: 96% left (4h 51m), 3 resets
```

## Changes

1. **Remaining, not used** — display `100 - utilization`% instead of used%.
2. **Order** — 7d before 5h for both providers.
3. **Time formatting** — replace the window-kind-specific logic in
   `format_remaining` with pi-quota's exact algorithm, independent of window
   kind:
   - `diff <= 0` → `"now"`
   - `days > 0` → `"{d}d {h}h"` (always both components)
   - `hours > 0` → `"{h}h {m}m"` (always both components)
   - `minutes > 0` → `"{m}m"`
   - else → `"now"`
4. **Unknown reset time** — `QuotaSnapshot.reset_at` and `ResetEvent.reset_at`
   become `Option<OffsetDateTime>`. A window with no parseable reset time is
   still reported (usage still known) with `(unknown)` in place of the
   duration. `daemon.rs` scheduling (`schedule_from_snapshots`) skips
   scheduling a window when `reset_at` is `None`; the in-memory detector
   still catches those resets via the usage-drop signal.
5. **Codex reset credits** — parse `rate_limit_reset_credits.available_count`
   from the Codex usage response and surface it as a provider-level count.
   Append `"{n} reset{s}"` to the Codex line only when `n > 0`. Claude never
   shows this segment.
6. **Labels** — lowercase `claude:` / `codex:`.
7. **No header** — drop the `📊 Quota summary` line; message body is just the
   provider lines (bare `label: parts` join), matching pi-quota's widget.

## Out of scope

- Auto-redeem of Codex reset credits (pi-quota's `codexResets.autoRedeem`
  behavior).
- pi-quota's `~/.pi/agent/pi-quota.log` file logging.

## Affected files

- `src/model.rs` — `reset_at: Option<OffsetDateTime>` on `QuotaSnapshot` and
  `ResetEvent`; rewrite `format_remaining`.
- `src/providers/claude.rs` — always emit a snapshot for a present window
  even when `resets_at` is missing/unparseable (`reset_at: None`).
- `src/providers/codex.rs` — same for Codex windows; parse
  `rate_limit_reset_credits.available_count` and thread it onto Codex
  snapshots.
- `src/telegram.rs` — remaining%, 7d-then-5h order, lowercase labels, reset
  count suffix, drop summary header.
- `src/daemon.rs` — `schedule_from_snapshots` skips snapshots with
  `reset_at: None`.
- `src/detector.rs` — key/logic unaffected by `Option<OffsetDateTime>` change
  (reset detection already keys off `usage`, not `reset_at`); update type
  only.
