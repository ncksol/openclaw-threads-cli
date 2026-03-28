# threads-cli

Rust CLI for Threads API publishing, read-side monitoring, and local operational state.

## Status

The app is fully implemented for the PM in-scope command surface:

- OAuth auth lifecycle (`login`, `refresh`, `status`, `logout`)
- Account identity lookup (`account whoami`)
- Publish post/reply with attempt lifecycle tracking
- Source-link modes:
  - `reply` (publishes a chained source-link reply)
  - `attachment` (explicitly fails if unsupported)
- Read commands (`post get`, `post insights`, `post replies`) with cache/refresh and cursor state
- Local record and attempt inspection (`log list/get`, `attempts list`)
- Diagnostics (`doctor check`) and config visibility (`config show`)
- Stable JSON output contract and redaction of sensitive values

## Prerequisites

- Rust toolchain (`cargo`, `rustc`)

## Quick start

1. Copy config example:

```bash
mkdir -p ~/.config/threads-cli
cp config/config.toml.example ~/.config/threads-cli/config.toml
```

2. Create the app secret file referenced by `threads.app_secret_file`:

```bash
mkdir -p ~/.config/threads-cli
printf 'YOUR_APP_SECRET' > ~/.config/threads-cli/app_secret
```

`YOUR_APP_SECRET` is your real Threads app secret value from your app settings (replace the placeholder text).

3. Build and check config:

```bash
cargo build
cargo run -- --json config show
```

## Config

Default config path:

`~/.config/threads-cli/config.toml`

See `config/config.toml.example` for full structure and defaults.

Important guardrails:

- OAuth callback host must be localhost-only (`127.0.0.1` or `localhost`)
- Default callback is `http://127.0.0.1:8788/callback`
- `defaults.link_mode` must be `reply` or `attachment`

## Commands

```text
threads-cli auth login [--no-browser]
threads-cli auth refresh
threads-cli auth status
threads-cli auth logout --yes

threads-cli account whoami

threads-cli publish post --text "..." [--tag "..."] [--link "..."] [--link-mode reply|attachment]
threads-cli publish reply --reply-to <post-id> --text "..."

threads-cli post get --id <post-id>
threads-cli post insights --id <post-id> [--refresh]
threads-cli post replies --id <post-id> [--refresh] [--limit <n>]

threads-cli log list [--limit <n>]
threads-cli log get --id <local-record-id>
threads-cli attempts list [--limit <n>]

threads-cli config show
threads-cli doctor check
```

## Output and errors

- Human output is concise and command-scoped.
- `--json` returns a stable envelope:
  - success: `{ "ok": true, "command": "...", "data": ... }`
  - error: `{ "ok": false, "command": "...", "data": null, "error": { "code": "...", "message": "..." } }`
- Sensitive values are redacted in output (tokens, auth headers, secrets).

Error categories:

- `CONFIG_ERROR`
- `VALIDATION_ERROR`
- `AUTH_ERROR`
- `NETWORK_ERROR`
- `API_ERROR`
- `RATE_LIMIT_ERROR`
- `DATABASE_ERROR`
- `AMBIGUOUS_PUBLISH_ERROR`
- `INTERNAL_ERROR`

## Reliability notes

- Retries are bounded and only used for safe read paths and token refresh.
- Publish operations are not blindly retried on uncertain outcomes.
- Ambiguous publish outcomes are persisted and surfaced with recovery guidance via `attempts list`.

## Tests

```bash
cargo test
```
