# Publish Container Form-Encoding Fix Design

## Problem

`threads-cli publish reply` fails with `HTTP 400` / Threads API error code `24` because `ThreadsClient::create_publish_container()` sends an `application/json` request body. The Threads Graph API expects form-encoded parameters for container creation. `publish post` currently succeeds through the same client path, but reply publishing consistently fails when `reply_to_id` is present.

## Goals

- Fix reply publishing by sending publish-container parameters as form data.
- Preserve the current publish flow, error handling, persistence, and output behavior.
- Add regression coverage that proves the request is form-encoded and includes reply-specific fields.
- Audit the existing POST/write endpoints and avoid unrelated protocol changes unless the audit shows another confirmed mismatch.

## Non-Goals

- Refactoring all client POST construction behind a new helper.
- Moving `access_token` from the query string into the form body.
- Changing publish-attempt semantics, retry behavior, or JSON output envelopes.

## Current State

`publish post`, `publish reply`, and the source-link reply path all call `ThreadsClient::create_publish_container()`. That method currently builds the request with `.json(payload)`, while the OAuth POST methods in the same file already use `.form(...)`. The broader audit found no other JSON-body POST mismatch in `src/client.rs`.

## Chosen Design

### API boundary change

Change `ThreadsClient::create_publish_container()` to serialize `CreateContainerRequest` with `.form(payload)` instead of `.json(payload)`.

This keeps the fix at the client boundary shared by all publish-container callers:

- direct posts
- direct replies
- source-link reply chaining for `publish post --link-mode reply`

`CreateContainerRequest` already derives `Serialize`, so the existing type remains usable for form submission. The existing `#[serde(skip_serializing_if = "Option::is_none")]` attributes continue to control omission of optional fields, which means:

- reply requests include `reply_to_id`
- plain posts omit `reply_to_id`
- optional topic tag and link fields remain absent when not set

### Scope control

Do not change `publish_container()` or the OAuth POST methods as part of this fix. The audit found:

- OAuth endpoints already use form encoding correctly.
- `publish_container()` is not part of the reported failure path and does not currently show an encoding mismatch.

This keeps the change tightly scoped to the confirmed defect.

## Test Design

Extend the existing mock-server-based publish tests in `src/cli/publish.rs` so they can inspect incoming request headers and bodies, not just return canned responses.

Regression coverage should assert:

1. `publish reply` sends `application/x-www-form-urlencoded`.
2. `publish reply` includes `text`, `media_type=TEXT`, and `reply_to_id`.
3. `publish post` uses the same form-encoded create-container request successfully.
4. Requests omit optional fields when their values are `None`.

The tests should continue to exercise the real CLI publish path through `run(...)`, so the coverage validates the behavior at the integration boundary rather than only unit-testing serialization in isolation.

## Verification

- Run targeted publish tests while iterating.
- Run `cargo test` before considering the fix complete.

## Expected Outcome

After this change, reply publishing should use the same endpoint with the request shape expected by the Threads API, eliminating the reply-specific `HTTP 400` / code `24` failure while preserving existing publish behavior elsewhere.
