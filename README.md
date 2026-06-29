# ai-quota-bot

A local Rust daemon that reads Claude and Codex credentials from `~/.pi/agent/auth.json`, polls quota usage windows, refreshes tokens when needed, detects reset transitions in memory, and sends Telegram notifications only when a quota actually resets.

## Configuration

Required environment variables:

- `TELEGRAM_BOT_TOKEN`
- `TELEGRAM_CHAT_ID`

Optional environment variables:

- `AI_QUOTA_AUTH_PATH` override for the auth file path. Default: `$HOME/.pi/agent/auth.json`
- `AI_QUOTA_POLL_INTERVAL_SECS` poll interval in seconds. Default: `600`
- `AI_QUOTA_CLAUDE_BASE_URL` Claude API base URL override. Default: `https://api.anthropic.com`
- `AI_QUOTA_CODEX_BASE_URL` Codex API base URL override. Default: `https://chatgpt.com`

## Auth file

By default the daemon reads provider credentials from:

- `~/.pi/agent/auth.json`

Expected top-level providers today:

- `claude`
- `codex`

## Running

```bash
cargo run
```

You can also build a release binary:

```bash
cargo build --release
./target/release/ai-quota-bot
```

## Behavior notes

- polls every 10 minutes by default
- refreshes provider credentials when expiry is within 5 minutes, or after an authentication failure
- sends Telegram messages only when a reset boundary is detected
- keeps reset detection state only in memory
- may miss reset notifications while the daemon is offline
- logs startup, poll failures, and shutdown without printing secrets

## Current implementation notes

- provider adapters query the real Pi provider API endpoints:
  - Claude: `https://api.anthropic.com/api/oauth/usage`
  - Codex: `https://chatgpt.com/backend-api/wham/usage`
- if the real provider payloads differ, update the adapter parsing while keeping the common quota model stable
