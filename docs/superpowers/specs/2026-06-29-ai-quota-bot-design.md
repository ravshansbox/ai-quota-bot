# AI Quota Bot Design

Date: 2026-06-29
Project: `ai-quota-bot`
Status: Approved design

## Summary

Build a Rust daemon that reads Claude and Codex credentials from `~/.pi/agent/auth.json`, polls provider usage endpoints every 10 minutes, refreshes tokens when needed, and sends Telegram notifications only when a 5-hour or 7-day quota has actually reset.

The service should be stateless on disk. It may keep the previous poll snapshot in memory while running in order to detect reset transitions reliably during normal daemon operation.

## Goals

- Run as a long-lived local daemon
- Read provider auth data from `~/.pi/agent/auth.json`
- Call provider endpoints directly to fetch usage and quota-reset metadata
- Refresh expired access tokens automatically when possible
- Poll every 10 minutes
- Send Telegram alerts only when a quota actually resets
- Support Claude Pro/Max and Codex Plus/Pro first
- Keep runtime reset-detection state only in memory

## Non-Goals

- No disk-backed persistence for runtime state
- No web UI
- No periodic summary notifications
- No pre-reset reminders
- No support for unrelated providers in the first version

## Key Constraints and Tradeoffs

### Daemon model
The user prefers a long-running daemon instead of a scheduled CLI job. This makes in-memory state practical and avoids reliance on external schedulers.

### Stateless on disk
The service should not persist runtime state to SQLite or JSON. This keeps the daemon simple, but it means a reset event can be missed if the daemon is not running during the reset window.

### Direct provider integration
Usage data should come from provider endpoints using credentials found in `~/.pi/agent/auth.json`. This is preferred over scraping local CLI state.

### Token refresh
The daemon must be able to refresh expired tokens. Refresh should happen either proactively when expiry is known or reactively when a provider returns an authentication failure.

## High-Level Architecture

The system is split into focused modules:

- `config` loads environment variables and paths
- `auth` reads and parses `~/.pi/agent/auth.json`
- `auth_refresh` refreshes expired provider tokens and updates in-memory credentials
- `providers` contains provider-specific usage adapters
- `poller` runs the 10-minute loop and fetches quota state from each provider
- `detector` compares current quota windows with the prior in-memory snapshot to find reset events
- `telegram` formats and sends Telegram notifications
- `app` wires the modules together, handles retries, logging, and graceful shutdown

### Proposed file/module layout

- `src/main.rs`
- `src/config.rs`
- `src/auth.rs`
- `src/auth_refresh.rs`
- `src/detector.rs`
- `src/telegram.rs`
- `src/daemon.rs`
- `src/providers/mod.rs`
- `src/providers/claude.rs`
- `src/providers/codex.rs`

## Data Model

The internal provider-neutral quota representation should be compact and explicit.

### `QuotaSnapshot`
Represents one provider-plan quota state returned by a poll.

Suggested fields:

- `provider`: enum, such as `Claude` or `Codex`
- `plan`: string or enum, such as `pro`, `max`, `plus`
- `window_kind`: enum, such as `FiveHours` or `SevenDays`
- `window_id`: stable identifier for the current quota window if the provider exposes one
- `reset_at`: UTC timestamp for the next reset
- `usage`: optional numeric usage data if available
- `limit`: optional numeric quota limit if available
- `metadata`: provider-specific fields needed for debugging or comparison

### `ProviderCredentials`
Represents the current in-memory credentials for a provider.

Suggested fields:

- `access_token`
- `refresh_token`: optional
- `expires_at`: optional UTC timestamp
- `account_id`: optional
- `raw_source`: optional provider-specific metadata from `auth.json`

### `ResetEvent`
Represents a detected reset that should produce a Telegram notification.

Suggested fields:

- `provider`
- `plan`
- `window_kind`
- `reset_at`
- `previous_window_id`: optional
- `current_window_id`: optional

## Runtime Data Flow

1. The daemon starts.
2. `config` loads:
   - `TELEGRAM_BOT_TOKEN`
   - `TELEGRAM_CHAT_ID`
   - optional auth file path override, defaulting to `~/.pi/agent/auth.json`
   - optional poll interval override, defaulting to 10 minutes
3. `auth` parses `auth.json` into provider-specific credentials for Claude and Codex.
4. The daemon enters the polling loop.
5. On each cycle, the daemon reloads `auth.json` so externally updated credentials are picked up automatically.
6. Each provider adapter requests current usage and quota-reset information.
7. If a token is expired, near expiry, or rejected by the provider, `auth_refresh` refreshes credentials and retries the request once.
8. Each provider returns one or more normalized `QuotaSnapshot` values.
9. `detector` compares the current snapshots against the previous in-memory snapshots from the prior cycle.
10. If a reset is detected, `telegram` sends a notification.
11. The current snapshots replace the previous in-memory snapshots.
12. The daemon sleeps until the next poll interval.

## Reset Detection Rules

The daemon is disk-stateless but may keep the last successful poll result in memory.

A reset event is detected when one of the following happens for a given provider-plan-window pair:

- `reset_at` moves forward to a later timestamp in a way that indicates a new quota window
- `window_id` changes, when the provider exposes a stable current-window identifier
- provider metadata indicates that a new billing or quota window began

The first successful poll after startup initializes in-memory state and must not send notifications. This avoids false positives on boot.

### Consequence of no disk persistence
If the daemon is stopped during a reset and restarted later, it may not know that a reset event happened while offline. This is accepted behavior for the first version.

## Provider Adapter Design

Each provider adapter should implement a common trait or interface that returns normalized quota snapshots.

Responsibilities of each adapter:

- build authenticated HTTP requests
- parse provider-specific usage responses
- extract reset-related timestamps and identifiers
- surface authentication failures so refresh logic can run
- convert provider responses into common `QuotaSnapshot` values

### Claude adapter
The Claude adapter should support the user’s Claude Pro/Max usage data and relevant reset windows. It should extract 5-hour or 7-day reset information if available in the upstream response.

### Codex adapter
The Codex adapter should support Codex Plus/Pro usage data and relevant reset windows. It should normalize reset information into the same internal model.

## Auth and Refresh Design

### Auth file loading
`auth` should parse `~/.pi/agent/auth.json` and locate provider-specific credential material for Claude and Codex.

The parser should be tolerant of unrelated entries in the file and fail only when required provider fields are missing or malformed.

### Refresh flow
`auth_refresh` should support two refresh triggers:

- proactive refresh if `expires_at` is known and the token is near expiry
- reactive refresh when a provider request returns an authentication error such as 401

Refresh behavior:

1. Attempt the normal provider request
2. If credentials are expired or rejected, call the provider’s refresh endpoint or refresh workflow
3. Update the in-memory `ProviderCredentials`
4. Retry the usage request once
5. If refresh or retry fails, mark that provider as failed for the current poll cycle

### Disk writes
The default runtime design is disk-stateless. However, if the upstream auth mechanism requires replacing stored tokens in `~/.pi/agent/auth.json` for future compatibility, the implementation may optionally write back refreshed credentials in a controlled way. If supported, writes should be atomic and preserve unrelated file contents as much as practical.

## Telegram Notification Design

Telegram credentials are supplied via environment variables:

- `TELEGRAM_BOT_TOKEN`
- `TELEGRAM_CHAT_ID`

Notifications should be concise and reset-only.

Example messages:

- `Claude Max 5h quota reset at 12:00 UTC`
- `Codex Pro 7d quota reset at 00:00 UTC`

Notification rules:

- do not send pre-reset reminders
- do not send periodic summaries
- do not send a startup snapshot
- send at most one notification per detected reset event per daemon runtime

## Error Handling

### Polling failures
If a poll cycle fails for one provider, log the error and continue processing the other provider. The daemon should try again on the next scheduled cycle.

### Provider isolation
Claude and Codex failures should be isolated. One provider failing must not block the other.

### Refresh failures
If token refresh fails, log the failure with enough context to diagnose the problem, skip that provider for the current cycle, and retry on the next cycle.

### Telegram failures
Telegram sends should retry a small number of times with backoff during the same cycle. If delivery still fails, log the failure and continue.

### Auth file changes
The daemon should reload `~/.pi/agent/auth.json` every cycle to pick up external token changes.

### Shutdown behavior
On process signals, the daemon should stop gracefully without persisting runtime state.

## Observability

Use structured logs for:

- startup and shutdown
- poll cycle start and completion
- provider request success and failure
- token refresh attempts and outcomes
- detected reset events
- Telegram delivery attempts and outcomes

Logs should not print raw access tokens or refresh tokens.

## Testing Strategy

### Unit tests
- parse realistic `auth.json` fixtures
- detect no-op transitions correctly
- detect reset transitions when `reset_at` advances
- detect reset transitions when `window_id` changes
- validate message formatting for Telegram
- validate refresh decision logic for expired and non-expired tokens

### Mocked integration tests
- provider request success without refresh
- provider request 401 then refresh then success
- provider request 401 then refresh failure
- Telegram send failure with retries
- daemon cycle with fake providers and fake Telegram sender

### Boundary tests
- first successful poll sends no notifications
- provider A fails while provider B still succeeds
- malformed or incomplete `auth.json` fails clearly
- auth file reload picks up changed credentials on the next cycle

## Configuration

Required environment variables:

- `TELEGRAM_BOT_TOKEN`
- `TELEGRAM_CHAT_ID`

Optional environment variables:

- `AI_QUOTA_AUTH_PATH`, default `~/.pi/agent/auth.json`
- `AI_QUOTA_POLL_INTERVAL_SECS`, default `600`
- `RUST_LOG` for logging verbosity

## Recommended Initial Dependencies

The exact dependency list can be refined during implementation, but a likely starting set is:

- `tokio` for async runtime and timers
- `reqwest` for HTTP
- `serde` and `serde_json` for config and auth parsing
- `chrono` or `time` for timestamps
- `thiserror` or `anyhow` for error handling
- `tracing` and `tracing-subscriber` for structured logging
- a mocking strategy for provider and Telegram tests

## Implementation Plan Boundaries

The first implementation should focus on:

- a clean provider abstraction
- robust `auth.json` parsing
- token refresh support
- in-memory reset detection
- Telegram delivery
- test coverage for all critical transitions

The first version should not expand into unrelated features such as dashboards, persistence layers, or broader account analytics.

## Open Implementation Risks

- provider APIs may differ in how clearly they expose reset-window identity
- `auth.json` structure may contain provider-specific quirks that require tolerant parsing
- token refresh flows may differ substantially between providers
- some providers may not expose enough information for fully stateless reset inference without in-memory comparison

These risks are acceptable and should be addressed through adapter-specific tests and realistic fixtures during implementation.
