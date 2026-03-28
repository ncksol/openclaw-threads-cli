# Discovery and Engagement Features — Design Spec

## Problem

The CLI covers the "do" phase of engagement (publish post, publish reply, check own post insights/replies) but everything before and after posting requires the browser:

- **No way to find posts to reply to.** Can't browse feeds or search tags.
- **Can't read other people's posts.** `post get` fails for non-owned posts.
- **No visibility into own replies.** After posting a reply, there's no way to get its URL or list recent replies.
- **No way to check who replied to us.** Activity/notifications require the browser.

## Approach

Direct implementation — each feature gets its own command module, client methods, and DB tables following the exact patterns already in the codebase. No new abstractions.

## New API Client Methods (`src/client.rs`)

### `keyword_search`

- **Endpoint:** `GET /keyword_search`
- **Params:** `q` (query string), `search_type` (`TOP` or `RECENT`), `fields` (id, username, text, timestamp, permalink, like_count, reply_count)
- **Returns:** `KeywordSearchResponse` — paginated list of matching public posts
- **Scope required:** `threads_keyword_search`
- **Rate limit:** 500 queries per 7-day window

### `fetch_own_threads`

- **Endpoint:** `GET /me/threads`
- **Params:** `fields`, `limit`, `after` (cursor pagination)
- **Returns:** `UserThreadsResponse` — paginated list of authenticated user's posts

### `fetch_own_replies`

- **Endpoint:** `GET /me/replies`
- **Params:** `fields`, `limit`, `after` (cursor pagination)
- **Returns:** `UserRepliesResponse` — paginated list of authenticated user's replies
- **Scope required:** `threads_read_replies`
- **Must include:** `reply_to_id` field so replies can be traced back to parent posts

### `fetch_post_details` (existing — adjustment)

- For non-owned posts, the API may return an error or a subset of fields.
- Handle gracefully: if the API returns a 403 or error for a non-owned post, return a structured response with `accessible: false` and the post ID echoed back. Do not crash.
- Try a reduced field set as a fallback if the full field set fails.

### Post-publish permalink fetch (enhancement)

- After `publish reply` succeeds, call `fetch_post_details` on the new reply's ID to get `permalink`.
- Best-effort: if the fetch fails, the publish still reports success with `permalink: null`.
- No retry on the permalink fetch — it's a convenience, not a critical path.

All new response types: `derive(Debug, Deserialize, Serialize)`, use `#[serde(default)]` for optional fields.

## New CLI Commands

### `search posts` — keyword search for public posts

```
threads-cli search posts --query "AI" [--type top|recent] [--limit 10] [--refresh]
```

- Default `--type top`, default `--limit 10`
- Without `--refresh`: returns cached results for the same query+type if they exist
- With `--refresh` or no cache: hits the API, caches results
- Output: list of posts with `id`, `username`, `text` (truncated in human mode), `timestamp`, `permalink`, `like_count`, `reply_count`
- JSON output follows the stable envelope: `{ "ok": true, "command": "search posts", "data": { ... } }`

### `me threads` — list own recent posts

```
threads-cli me threads [--limit 10] [--refresh]
```

- Uses `GET /me/threads` with pagination
- Same cache/refresh pattern as `post insights`
- Output: list of posts with `id`, `text`, `permalink`, `timestamp`

### `me replies` — list own recent replies

```
threads-cli me replies [--limit 10] [--refresh]
```

- Uses `GET /me/replies`
- Output: reply ID, `reply_to_id` (parent post ID), text, permalink, timestamp
- `reply_to_id` is the key field for tracing replies back to parent posts
- Same cache/refresh pattern

### `activity replies` — aggregated replies from others on your posts

```
threads-cli activity replies [--recent 10] [--refresh]
```

- Fetches your N most recent posts via `me/threads` (default N=10)
- For each post, fetches replies via `/{post_id}/replies`
- Filters out your own replies (by matching against authenticated user ID)
- Groups results by parent post for readability
- Caches results; `--refresh` re-fetches
- `--recent N` controls how many of your posts to check (default 10)

### Enhanced `publish reply` output

- After successful publish, fetches post details for the new reply ID
- Adds `permalink` to both human and JSON output
- Human output: `publish reply: Reply published. https://threads.net/...`
- JSON output: `permalink` field added to the `data` object
- If permalink fetch fails, publish still succeeds with `permalink: null`

## Persistence (SQLite)

Three new tables added to `src/store/migrations.rs`:

### `search_results`

```sql
CREATE TABLE IF NOT EXISTS search_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    query TEXT NOT NULL,
    search_type TEXT NOT NULL,
    threads_post_id TEXT NOT NULL,
    username TEXT,
    text TEXT,
    permalink TEXT,
    timestamp TEXT,
    like_count INTEGER,
    reply_count INTEGER,
    raw_json TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

- Cache lookups keyed by `(query, search_type)`
- Without `--refresh`, return cached rows if they exist for the query

### `own_threads`

```sql
CREATE TABLE IF NOT EXISTS own_threads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    threads_post_id TEXT NOT NULL UNIQUE,
    text TEXT,
    permalink TEXT,
    timestamp TEXT,
    username TEXT,
    raw_json TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### `own_replies`

```sql
CREATE TABLE IF NOT EXISTS own_replies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    threads_post_id TEXT NOT NULL UNIQUE,
    reply_to_id TEXT,
    text TEXT,
    permalink TEXT,
    timestamp TEXT,
    username TEXT,
    raw_json TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

- `reply_to_id` populated from the API response to trace replies back to parent posts

`activity replies` does not need its own table — it composes `own_threads` + the existing `replies` table.

Store methods follow existing patterns: `insert_*`, `latest_*`, `list_*` with `map_db_error`.

## Error Handling

- **Rate limiting on keyword search:** Surface as `RATE_LIMIT_ERROR` with a clear message about the 500/7-day window. Cached results help avoid burning queries on re-reads.
- **Non-owned post `post get`:** If the API returns 403 or an error for a non-owned post, return a structured response with `accessible: false` and the post ID echoed back. Not a crash, not a new error category — a data-level field in the response.
- **Activity replies — partial failures:** If fetching replies for one post fails mid-loop, collect what succeeded and report partial results with a warning. Do not abort the entire activity check because one post's replies failed.
- **Permalink fetch after publish reply:** Best-effort. Failure returns `permalink: null` in the output. No retry.
- **Empty results:** Search with no matches, `me replies` with no replies — return empty lists, not errors.
- **Retry policy:** All new read endpoints use `SafeRead` with the same `RetryPolicy { max_retries: 3, base_delay_ms: 100 }` as existing read commands.

## Testing

Following existing patterns (integration tests in `tests/`, unit tests inline with `#[cfg(test)]`):

- **`tests/search_tests.rs`:** Cache hit/miss for same query, different queries don't collide, empty results, rate limit error surfacing
- **`tests/me_tests.rs`:** Own threads listing, own replies listing with `reply_to_id` populated, cache/refresh behavior
- **Inline tests in `src/cli/activity.rs`:** Aggregation logic — filters out own replies, groups by parent post, handles partial failures gracefully
- **Inline tests in `src/cli/publish.rs`:** Enhanced reply output includes permalink field (mocked fetch)
- **`src/client.rs` inline tests:** New response type deserialization for keyword search, own threads, own replies
- **`tests/store_tests.rs`:** Insert/query tests for the 3 new tables (search_results, own_threads, own_replies)

All DB test fixtures use the existing `tempfile` + `Store::open` + `run_migrations` pattern.

## Files Changed

New files:
- `src/cli/search.rs` — search command handler
- `src/cli/me.rs` — me threads/replies command handler
- `src/cli/activity.rs` — activity replies command handler

Modified files:
- `src/cli/mod.rs` — add `Search`, `Me`, `Activity` command variants
- `src/client.rs` — add 3 new API methods + response types, adjust `fetch_post_details` for non-owned posts
- `src/store/mod.rs` — add row types and CRUD methods for 3 new tables
- `src/store/migrations.rs` — add 3 new `CREATE TABLE` statements
- `src/cli/publish.rs` — add permalink fetch after successful reply publish
- `src/main.rs` — add dispatch for new commands
- `.github/copilot-instructions.md` — update with new command surface

New test files:
- `tests/search_tests.rs`
- `tests/me_tests.rs`

Extended test files:
- `tests/store_tests.rs`
