#[path = "../src/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../src/store/mod.rs"]
mod store;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use error::{CliError, ErrorCategory};
#[allow(unused_imports)]
use store::{
    retry_with_backoff, PersistPostInput, PublishAttemptInput, RetryOperation, RetryPolicy, Store,
    SearchResultRow, OwnThreadRow, OwnReplyRow,
};
use rusqlite::OptionalExtension;

#[test]
fn creates_and_updates_publish_attempt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");

    let (id, attempt_uuid) = store
        .create_publish_attempt(PublishAttemptInput {
            kind: "post".to_string(),
            text: "hello world".to_string(),
            reply_to_id: None,
            topic_tag: Some("demo".to_string()),
            source_url: Some("https://example.com".to_string()),
            source_link_mode: Some("reply".to_string()),
            request_json: r#"{"text":"hello world"}"#.to_string(),
        })
        .expect("create attempt");

    assert!(id > 0);
    assert!(!attempt_uuid.is_empty());

    store
        .mark_publish_attempt_ambiguous(
            id,
            "NETWORK_ERROR",
            "request timed out after publish",
            Some(r#"{"timeout":true}"#),
        )
        .expect("mark ambiguous");

    let attempts = store.list_attempts(10).expect("list attempts");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].id, id);
    assert_eq!(attempts[0].status, "ambiguous");
    assert_eq!(attempts[0].error_code.as_deref(), Some("NETWORK_ERROR"));
}

#[test]
fn marks_publish_attempt_published_and_persists_response() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");
    let (id, _) = store
        .create_publish_attempt(PublishAttemptInput {
            kind: "post".to_string(),
            text: "hello world".to_string(),
            reply_to_id: None,
            topic_tag: None,
            source_url: None,
            source_link_mode: None,
            request_json: r#"{"text":"hello world"}"#.to_string(),
        })
        .expect("create attempt");

    store
        .mark_publish_attempt_published(id, "thr_123", r#"{"publish_container":{"id":"thr_123"}}"#)
        .expect("mark published");

    let conn = store.connection().expect("connection");
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT status, threads_post_id, response_json FROM publish_attempts WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .expect("query row");
    let (status, threads_post_id, response_json) = row.expect("attempt row");
    assert_eq!(status, "published");
    assert_eq!(threads_post_id.as_deref(), Some("thr_123"));
    assert_eq!(
        response_json.as_deref(),
        Some(r#"{"publish_container":{"id":"thr_123"}}"#)
    );
}

#[test]
fn persists_and_queries_posts_including_source_link_reply_relationship() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");

    store
        .persist_post(PersistPostInput {
            threads_post_id: "post_main".to_string(),
            parent_threads_post_id: None,
            post_url: None,
            text: "main text".to_string(),
            topic_tag: Some("demo".to_string()),
            source_url: Some("https://example.com/source".to_string()),
            source_link_mode: Some("reply".to_string()),
            kind: "post".to_string(),
            published_at: None,
            raw_json: r#"{"publish_container":{"id":"post_main"}}"#.to_string(),
        })
        .expect("persist main post");

    store
        .persist_post(PersistPostInput {
            threads_post_id: "post_source".to_string(),
            parent_threads_post_id: Some("post_main".to_string()),
            post_url: None,
            text: "https://example.com/source".to_string(),
            topic_tag: None,
            source_url: Some("https://example.com/source".to_string()),
            source_link_mode: Some("reply".to_string()),
            kind: "reply".to_string(),
            published_at: None,
            raw_json: r#"{"is_source_link_reply":true}"#.to_string(),
        })
        .expect("persist source-link reply post");

    let main = store
        .get_post_by_threads_post_id("post_main")
        .expect("query main")
        .expect("main row");
    assert_eq!(main.kind, "post");
    assert_eq!(main.source_link_mode.as_deref(), Some("reply"));
    assert_eq!(main.parent_threads_post_id, None);

    let replies = store
        .list_posts_by_parent("post_main", 20)
        .expect("list parent replies");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].threads_post_id, "post_source");
    assert_eq!(replies[0].kind, "reply");
    assert_eq!(replies[0].parent_threads_post_id.as_deref(), Some("post_main"));
    assert_eq!(replies[0].source_url.as_deref(), Some("https://example.com/source"));
}

#[test]
fn persists_and_reads_insights_replies_and_reply_fetch_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");

    store
        .insert_insight_snapshot(
            "post_1",
            Some(100),
            Some(12),
            Some(3),
            Some(1),
            Some(0),
            Some(0),
            r#"{"data":[]}"#,
        )
        .expect("insert insight");

    let insight = store
        .latest_insight("post_1")
        .expect("latest insight query")
        .expect("insight row");
    assert_eq!(insight.views, Some(100));
    assert_eq!(insight.likes, Some(12));
    assert_eq!(insight.replies, Some(3));

    store
        .upsert_reply(
            "reply_1",
            "post_1",
            Some("alice"),
            Some("hello"),
            Some("2024-01-01T00:00:00Z"),
            r#"{"id":"reply_1"}"#,
        )
        .expect("upsert reply");
    store
        .upsert_reply_fetch_state("post_1", Some("cursor_2"), Some(r#"{"paging":{}}"#))
        .expect("upsert reply state");

    let replies = store.latest_replies("post_1", 10).expect("latest replies");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].threads_reply_id, "reply_1");
    assert_eq!(replies[0].parent_threads_post_id, "post_1");
    assert_eq!(replies[0].author_username.as_deref(), Some("alice"));

    let fetch_state = store
        .reply_fetch_state("post_1")
        .expect("reply fetch state")
        .expect("state row");
    assert_eq!(fetch_state.threads_post_id, "post_1");
    assert_eq!(fetch_state.next_cursor.as_deref(), Some("cursor_2"));
}

#[tokio::test]
async fn retries_transient_errors_then_succeeds() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let policy = RetryPolicy {
        max_retries: 3,
        base_delay_ms: 1,
    };

    let result = retry_with_backoff(policy, RetryOperation::SafeRead, move || {
        let counter = counter.clone();
        async move {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(CliError::new(ErrorCategory::Network, "transient"))
            } else {
                Ok("ok")
            }
        }
    })
    .await
    .expect("retry should succeed");

    assert_eq!(result, "ok");
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn does_not_retry_validation_errors() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let policy = RetryPolicy {
        max_retries: 3,
        base_delay_ms: 1,
    };

    let err = retry_with_backoff(policy, RetryOperation::SafeRead, move || {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(CliError::new(ErrorCategory::Validation, "invalid input"))
        }
    })
    .await
    .expect_err("validation should fail immediately");

    assert_eq!(err.category.as_code(), "VALIDATION_ERROR");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retries_token_refresh_transient_errors_then_succeeds() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let policy = RetryPolicy {
        max_retries: 3,
        base_delay_ms: 1,
    };

    let result = retry_with_backoff(policy, RetryOperation::TokenRefresh, move || {
        let counter = counter.clone();
        async move {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(CliError::new(ErrorCategory::RateLimit, "retryable"))
            } else {
                Ok("ok")
            }
        }
    })
    .await
    .expect("retry should succeed");

    assert_eq!(result, "ok");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn does_not_retry_unsafe_publish_operations() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let policy = RetryPolicy {
        max_retries: 3,
        base_delay_ms: 1,
    };

    let err = retry_with_backoff(policy, RetryOperation::UnsafePublish, move || {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Err::<(), _>(CliError::new(ErrorCategory::Network, "publish timed out"))
        }
    })
    .await
    .expect_err("unsafe publish must fail without retry");

    assert_eq!(err.category.as_code(), "NETWORK_ERROR");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn inserts_and_queries_search_results_by_query() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");

    let results = vec![
        (
            "post_1".to_string(),
            Some("alice".to_string()),
            Some("hello AI".to_string()),
            Some("https://threads.net/@alice/post/1".to_string()),
            Some("2025-01-01T00:00:00Z".to_string()),
            Some(5_i64),
            Some(2_i64),
            r#"{"id":"post_1"}"#.to_string(),
        ),
        (
            "post_2".to_string(),
            Some("bob".to_string()),
            Some("AI is great".to_string()),
            None,
            None,
            None,
            None,
            r#"{"id":"post_2"}"#.to_string(),
        ),
    ];
    store
        .insert_search_results("AI", "TOP", &results)
        .expect("insert search results");

    let cached = store.cached_search_results("AI", "TOP", 10).expect("query cached");
    assert_eq!(cached.len(), 2);
    assert_eq!(cached[0].threads_post_id, "post_1");
    assert_eq!(cached[0].username.as_deref(), Some("alice"));
    assert_eq!(cached[0].like_count, Some(5));
    assert_eq!(cached[1].threads_post_id, "post_2");
    assert_eq!(cached[1].like_count, None);

    // Different query returns empty
    let other = store.cached_search_results("rust", "TOP", 10).expect("query other");
    assert!(other.is_empty());

    // Re-inserting replaces old results
    let new_results = vec![(
        "post_3".to_string(),
        Some("charlie".to_string()),
        Some("new AI post".to_string()),
        None,
        None,
        Some(10_i64),
        Some(0_i64),
        r#"{"id":"post_3"}"#.to_string(),
    )];
    store
        .insert_search_results("AI", "TOP", &new_results)
        .expect("re-insert search results");
    let refreshed = store.cached_search_results("AI", "TOP", 10).expect("query refreshed");
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].threads_post_id, "post_3");
}

#[test]
fn upserts_and_lists_own_threads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");

    store
        .upsert_own_thread("t1", Some("my post"), Some("https://link/t1"), Some("2025-01-01"), Some("me"), "{}")
        .expect("upsert thread 1");
    store
        .upsert_own_thread("t2", Some("second post"), None, None, Some("me"), "{}")
        .expect("upsert thread 2");

    let threads = store.list_own_threads(10).expect("list own threads");
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].threads_post_id, "t2"); // most recent first
    assert_eq!(threads[1].threads_post_id, "t1");
    assert_eq!(threads[1].text.as_deref(), Some("my post"));
    assert_eq!(threads[1].permalink.as_deref(), Some("https://link/t1"));

    // Upsert updates existing
    store
        .upsert_own_thread("t1", Some("updated post"), Some("https://link/t1"), Some("2025-01-02"), Some("me"), "{}")
        .expect("upsert thread 1 again");
    let threads = store.list_own_threads(10).expect("list after upsert");
    assert_eq!(threads.len(), 2);
    let t1 = threads.iter().find(|t| t.threads_post_id == "t1").expect("find t1");
    assert_eq!(t1.text.as_deref(), Some("updated post"));
}

#[test]
fn upserts_and_lists_own_replies_with_reply_to_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("threads.db");
    let store = Store::open(db.to_str().expect("db path")).expect("open store");
    store.run_migrations().expect("migrate");

    store
        .upsert_own_reply(
            "r1",
            Some("parent_1"),
            Some("my reply"),
            Some("https://link/r1"),
            Some("2025-01-01"),
            Some("me"),
            "{}",
        )
        .expect("upsert reply 1");
    store
        .upsert_own_reply("r2", Some("parent_2"), Some("another reply"), None, None, Some("me"), "{}")
        .expect("upsert reply 2");

    let replies = store.list_own_replies(10).expect("list own replies");
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0].threads_post_id, "r2"); // most recent first
    assert_eq!(replies[0].reply_to_id.as_deref(), Some("parent_2"));
    assert_eq!(replies[1].threads_post_id, "r1");
    assert_eq!(replies[1].reply_to_id.as_deref(), Some("parent_1"));
    assert_eq!(replies[1].permalink.as_deref(), Some("https://link/r1"));

    // Limit works
    let limited = store.list_own_replies(1).expect("list limited");
    assert_eq!(limited.len(), 1);
}
