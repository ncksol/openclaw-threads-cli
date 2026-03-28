# Copilot instructions for `threads-cli`

## Build and test commands

- Build: `cargo build`
- Run the CLI manually: `cargo run -- --json config show`
- Full test suite: `cargo test`
- Run a single test by name: `cargo test retries_transient_errors_then_succeeds`
- Run a single integration test in one file: `cargo test --test store_tests creates_and_updates_publish_attempt`

## High-level architecture

- `src/main.rs` is the entrypoint: initialize tracing, parse CLI flags/commands, load validated config, open SQLite store, run migrations, then dispatch to command handlers.
- `src/cli/mod.rs` defines the command tree and global flags (`--config`, `--json`). Submodules in `src/cli/` implement command behavior (`auth`, `publish`, `post`, `doctor`, etc.).
- `src/client.rs` is the Threads API boundary. It builds HTTP requests, maps HTTP/network failures to `CliError`, and returns typed API payloads.
- `src/store/mod.rs` is the persistence boundary over SQLite; `src/store/migrations.rs` owns schema creation. Command handlers persist/read through `Store` methods rather than embedding SQL in CLI modules.
- `src/output.rs` + `src/error.rs` define external contracts: structured output shape, redaction rules, error category codes, and category-based exit codes.

## Key codebase conventions

- JSON output must keep the stable envelope:
  - success: `{ "ok": true, "command": "...", "data": ... }`
  - failure: `{ "ok": false, "command": "...", "data": null, "error": { "code": "...", "message": "..." } }`
- Always emit through `print_success` / `print_error_and_exit` so redaction is applied consistently for text and nested JSON.
- Use `CliError` + `ErrorCategory` (not ad-hoc error strings) so output code + process exit code remain consistent.
- Publish flow is attempt-first:
  - create `publish_attempts` row with `started`
  - transition to `published`, `failed`, or `ambiguous`
  - ambiguous outcomes must include recovery guidance (`attempts list`) instead of encouraging blind retry.
- Retry policy is intentionally scoped:
  - retry only `SafeRead` and `TokenRefresh`
  - do **not** retry `UnsafePublish`
  - retryable failures are `NETWORK_ERROR`, `RATE_LIMIT_ERROR`, or API errors containing `HTTP 5`.
- Read commands (`post insights`, `post replies`, `search posts`, `me threads`, `me replies`, `activity replies`) are cache-aware:
  - default path reads local snapshots/cached results when available
  - `--refresh` fetches live data and updates snapshots/cursor state.
- Discovery commands:
  - `search posts` uses `GET /keyword_search` (rate-limited: 500 queries per 7-day window). Results are cached by `(query, search_type)`.
  - `me threads` / `me replies` use `GET /me/threads` and `GET /me/replies`. Own replies include `reply_to_id` for tracing back to parent posts.
  - `activity replies` composes own threads + per-post replies, filtering out the user's own replies. Partial failures (one post's replies failing) do not abort the whole check.
  - `post get` handles non-owned posts gracefully — returns `accessible: false` instead of crashing on 403/API errors.
  - `publish reply` fetches the permalink after publishing (best-effort; failure still reports success with `permalink: null`).
- Config and input validation is strict and should be preserved:
  - OAuth listen host must stay localhost-only
  - `defaults.link_mode` must stay `reply` or `attachment`
  - source URLs must be absolute `http/https`
  - topic tags are length/charset constrained.
- `log` data and `attempts` data are intentionally different surfaces (`posts` vs `publish_attempts`); keep both semantics intact when modifying publish/read logic.
