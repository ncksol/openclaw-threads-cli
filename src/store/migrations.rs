use rusqlite::Connection;

use crate::error::{CliError, ErrorCategory};

pub fn run(conn: &mut Connection) -> Result<(), CliError> {
    let tx = conn.transaction().map_err(map_db_error)?;
    tx.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS accounts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            threads_user_id TEXT NOT NULL UNIQUE,
            username TEXT,
            name TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS tokens (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            access_token TEXT NOT NULL,
            refresh_token TEXT,
            issued_at TEXT,
            expires_at TEXT,
            raw_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(account_id) REFERENCES accounts(id)
        );

        CREATE TABLE IF NOT EXISTS posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            threads_post_id TEXT NOT NULL UNIQUE,
            parent_threads_post_id TEXT,
            post_url TEXT,
            text TEXT NOT NULL,
            topic_tag TEXT,
            source_url TEXT,
            source_link_mode TEXT,
            kind TEXT NOT NULL,
            published_at TEXT,
            raw_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS publish_attempts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            attempt_uuid TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL,
            text TEXT NOT NULL,
            reply_to_id TEXT,
            topic_tag TEXT,
            source_url TEXT,
            source_link_mode TEXT,
            status TEXT NOT NULL,
            threads_post_id TEXT,
            error_code TEXT,
            error_message TEXT,
            request_json TEXT NOT NULL,
            response_json TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS insight_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            threads_post_id TEXT NOT NULL,
            fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            views INTEGER,
            likes INTEGER,
            replies INTEGER,
            reposts INTEGER,
            quotes INTEGER,
            shares INTEGER,
            raw_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS replies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            threads_reply_id TEXT NOT NULL UNIQUE,
            parent_threads_post_id TEXT NOT NULL,
            author_id TEXT,
            author_username TEXT,
            text TEXT,
            posted_at TEXT,
            raw_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS reply_fetch_state (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            threads_post_id TEXT NOT NULL UNIQUE,
            next_cursor TEXT,
            last_fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            raw_json TEXT
        );

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
        "#,
    )
    .map_err(map_db_error)?;
    tx.commit().map_err(map_db_error)?;
    Ok(())
}

fn map_db_error(err: rusqlite::Error) -> CliError {
    CliError::new(ErrorCategory::Database, format!("database migration error: {}", err))
}
