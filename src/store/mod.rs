use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{CliError, ErrorCategory};

pub mod migrations;

pub struct Store {
    db_path: PathBuf,
}

pub struct AttemptRow {
    pub id: i64,
    pub attempt_uuid: String,
    pub kind: String,
    pub status: String,
    pub threads_post_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[allow(dead_code)]
pub struct PublishAttemptInput {
    pub kind: String,
    pub text: String,
    pub reply_to_id: Option<String>,
    pub topic_tag: Option<String>,
    pub source_url: Option<String>,
    pub source_link_mode: Option<String>,
    pub request_json: String,
}

#[allow(dead_code)]
pub struct TokenRow {
    pub id: i64,
    pub account_id: i64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub raw_json: String,
    pub created_at: String,
}

#[allow(dead_code)]
pub struct AccountRow {
    pub id: i64,
    pub threads_user_id: String,
    pub username: Option<String>,
    pub name: Option<String>,
    pub created_at: String,
}

#[allow(dead_code)]
pub struct PostRow {
    pub id: i64,
    pub threads_post_id: String,
    pub parent_threads_post_id: Option<String>,
    pub post_url: Option<String>,
    pub text: String,
    pub topic_tag: Option<String>,
    pub source_url: Option<String>,
    pub source_link_mode: Option<String>,
    pub kind: String,
    pub published_at: Option<String>,
    pub created_at: String,
}

pub struct PersistPostInput {
    pub threads_post_id: String,
    pub parent_threads_post_id: Option<String>,
    pub post_url: Option<String>,
    pub text: String,
    pub topic_tag: Option<String>,
    pub source_url: Option<String>,
    pub source_link_mode: Option<String>,
    pub kind: String,
    pub published_at: Option<String>,
    pub raw_json: String,
}

#[allow(dead_code)]
pub struct InsightRow {
    pub fetched_at: String,
    pub views: Option<i64>,
    pub likes: Option<i64>,
    pub replies: Option<i64>,
    pub reposts: Option<i64>,
    pub quotes: Option<i64>,
    pub shares: Option<i64>,
}

#[allow(dead_code)]
pub struct ReplyRow {
    pub threads_reply_id: String,
    pub parent_threads_post_id: String,
    pub author_username: Option<String>,
    pub text: Option<String>,
    pub posted_at: Option<String>,
    pub created_at: String,
}

#[allow(dead_code)]
pub struct ReplyFetchStateRow {
    pub threads_post_id: String,
    pub next_cursor: Option<String>,
    pub last_fetched_at: String,
}

#[allow(dead_code)]
pub struct SearchResultRow {
    pub threads_post_id: String,
    pub username: Option<String>,
    pub text: Option<String>,
    pub permalink: Option<String>,
    pub timestamp: Option<String>,
    pub like_count: Option<i64>,
    pub reply_count: Option<i64>,
    pub fetched_at: String,
}

#[allow(dead_code)]
pub struct OwnThreadRow {
    pub threads_post_id: String,
    pub text: Option<String>,
    pub permalink: Option<String>,
    pub timestamp: Option<String>,
    pub username: Option<String>,
    pub fetched_at: String,
}

#[allow(dead_code)]
pub struct OwnReplyRow {
    pub threads_post_id: String,
    pub reply_to_id: Option<String>,
    pub text: Option<String>,
    pub permalink: Option<String>,
    pub timestamp: Option<String>,
    pub username: Option<String>,
    pub fetched_at: String,
}

#[allow(dead_code)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum RetryOperation {
    SafeRead,
    TokenRefresh,
    UnsafePublish,
}

impl Store {
    pub fn open(path: &str) -> Result<Self, CliError> {
        let db_path = expand_path(path)?;
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::new(
                    ErrorCategory::Database,
                    format!("failed creating db directory {}: {}", parent.display(), e),
                )
            })?;
        }
        let _ = Connection::open(&db_path).map_err(|e| {
            CliError::new(
                ErrorCategory::Database,
                format!("failed opening sqlite db {}: {}", db_path.display(), e),
            )
        })?;
        Ok(Self { db_path })
    }

    pub fn run_migrations(&self) -> Result<(), CliError> {
        let mut conn = self.connection()?;
        migrations::run(&mut conn)
    }

    pub fn connection(&self) -> Result<Connection, CliError> {
        Connection::open(&self.db_path).map_err(|e| {
            CliError::new(
                ErrorCategory::Database,
                format!("failed opening sqlite db {}: {}", self.db_path.display(), e),
            )
        })
    }

    #[allow(dead_code)]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    #[allow(dead_code)]
    pub fn list_attempts(&self, limit: usize) -> Result<Vec<AttemptRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, attempt_uuid, kind, status, threads_post_id, error_code, error_message, created_at
                 FROM publish_attempts
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(AttemptRow {
                    id: row.get(0)?,
                    attempt_uuid: row.get(1)?,
                    kind: row.get(2)?,
                    status: row.get(3)?,
                    threads_post_id: row.get(4)?,
                    error_code: row.get(5)?,
                    error_message: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(map_db_error)?;

        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn list_posts(&self, limit: usize) -> Result<Vec<PostRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, threads_post_id, parent_threads_post_id, post_url, text, topic_tag,
                        source_url, source_link_mode, kind, published_at, created_at
                 FROM posts
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(PostRow {
                    id: row.get(0)?,
                    threads_post_id: row.get(1)?,
                    parent_threads_post_id: row.get(2)?,
                    post_url: row.get(3)?,
                    text: row.get(4)?,
                    topic_tag: row.get(5)?,
                    source_url: row.get(6)?,
                    source_link_mode: row.get(7)?,
                    kind: row.get(8)?,
                    published_at: row.get(9)?,
                    created_at: row.get(10)?,
                })
            })
            .map_err(map_db_error)?;

        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn get_post_by_local_id(&self, local_id: i64) -> Result<Option<PostRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, threads_post_id, parent_threads_post_id, post_url, text, topic_tag,
                        source_url, source_link_mode, kind, published_at, created_at
                 FROM posts
                 WHERE id = ?1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([local_id], |row| {
            Ok(PostRow {
                id: row.get(0)?,
                threads_post_id: row.get(1)?,
                parent_threads_post_id: row.get(2)?,
                post_url: row.get(3)?,
                text: row.get(4)?,
                topic_tag: row.get(5)?,
                source_url: row.get(6)?,
                source_link_mode: row.get(7)?,
                kind: row.get(8)?,
                published_at: row.get(9)?,
                created_at: row.get(10)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    #[allow(dead_code)]
    pub fn get_post_by_threads_post_id(&self, post_id: &str) -> Result<Option<PostRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, threads_post_id, parent_threads_post_id, post_url, text, topic_tag,
                        source_url, source_link_mode, kind, published_at, created_at
                 FROM posts
                 WHERE threads_post_id = ?1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([post_id], |row| {
            Ok(PostRow {
                id: row.get(0)?,
                threads_post_id: row.get(1)?,
                parent_threads_post_id: row.get(2)?,
                post_url: row.get(3)?,
                text: row.get(4)?,
                topic_tag: row.get(5)?,
                source_url: row.get(6)?,
                source_link_mode: row.get(7)?,
                kind: row.get(8)?,
                published_at: row.get(9)?,
                created_at: row.get(10)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    #[allow(dead_code)]
    pub fn list_posts_by_parent(
        &self,
        parent_post_id: &str,
        limit: usize,
    ) -> Result<Vec<PostRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, threads_post_id, parent_threads_post_id, post_url, text, topic_tag,
                        source_url, source_link_mode, kind, published_at, created_at
                 FROM posts
                 WHERE parent_threads_post_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map((parent_post_id, limit as i64), |row| {
                Ok(PostRow {
                    id: row.get(0)?,
                    threads_post_id: row.get(1)?,
                    parent_threads_post_id: row.get(2)?,
                    post_url: row.get(3)?,
                    text: row.get(4)?,
                    topic_tag: row.get(5)?,
                    source_url: row.get(6)?,
                    source_link_mode: row.get(7)?,
                    kind: row.get(8)?,
                    published_at: row.get(9)?,
                    created_at: row.get(10)?,
                })
            })
            .map_err(map_db_error)?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn latest_token(&self) -> Result<Option<TokenRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, access_token, refresh_token, expires_at, raw_json, created_at
                 FROM tokens
                 ORDER BY id DESC
                 LIMIT 1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([], |row| {
            Ok(TokenRow {
                id: row.get(0)?,
                account_id: row.get(1)?,
                access_token: row.get(2)?,
                refresh_token: row.get(3)?,
                expires_at: row.get(4)?,
                raw_json: row.get(5)?,
                created_at: row.get(6)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    pub fn get_account_by_id(&self, account_id: i64) -> Result<Option<AccountRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, threads_user_id, username, name, created_at
                 FROM accounts
                 WHERE id = ?1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([account_id], |row| {
            Ok(AccountRow {
                id: row.get(0)?,
                threads_user_id: row.get(1)?,
                username: row.get(2)?,
                name: row.get(3)?,
                created_at: row.get(4)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    pub fn upsert_account(
        &self,
        threads_user_id: &str,
        username: Option<&str>,
        name: Option<&str>,
    ) -> Result<i64, CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO accounts (threads_user_id, username, name)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(threads_user_id) DO UPDATE SET
               username=excluded.username,
               name=excluded.name,
               updated_at=CURRENT_TIMESTAMP",
            (threads_user_id, username, name),
        )
        .map_err(map_db_error)?;
        let account_id = conn
            .query_row(
                "SELECT id FROM accounts WHERE threads_user_id = ?1",
                [threads_user_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(map_db_error)?;
        Ok(account_id)
    }

    pub fn insert_token(
        &self,
        account_id: i64,
        access_token: &str,
        refresh_token: Option<&str>,
        issued_at: Option<&str>,
        expires_at: Option<&str>,
        raw_json: &str,
    ) -> Result<i64, CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO tokens (account_id, access_token, refresh_token, issued_at, expires_at, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                account_id,
                access_token,
                refresh_token,
                issued_at,
                expires_at,
                raw_json,
            ),
        )
        .map_err(map_db_error)?;
        Ok(conn.last_insert_rowid())
    }

    pub fn latest_access_token(&self) -> Result<Option<String>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT access_token
                 FROM tokens
                 ORDER BY id DESC
                 LIMIT 1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([], |row| row.get(0));
        match result {
            Ok(token) => Ok(Some(token)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    #[allow(dead_code)]
    pub fn delete_all_tokens(&self) -> Result<usize, CliError> {
        let conn = self.connection()?;
        let deleted = conn
            .execute("DELETE FROM tokens", [])
            .map_err(map_db_error)?;
        Ok(deleted)
    }

    #[allow(dead_code)]
    pub fn latest_insight(&self, post_id: &str) -> Result<Option<InsightRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT threads_post_id, fetched_at, views, likes, replies, reposts, quotes, shares
                 FROM insight_snapshots
                 WHERE threads_post_id = ?1
                 ORDER BY id DESC
                 LIMIT 1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([post_id], |row| {
            Ok(InsightRow {
                fetched_at: row.get(1)?,
                views: row.get(2)?,
                likes: row.get(3)?,
                replies: row.get(4)?,
                reposts: row.get(5)?,
                quotes: row.get(6)?,
                shares: row.get(7)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    #[allow(dead_code)]
    pub fn latest_replies(&self, post_id: &str, limit: usize) -> Result<Vec<ReplyRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT threads_reply_id, parent_threads_post_id, author_username, text, posted_at, created_at
                 FROM replies
                 WHERE parent_threads_post_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map((post_id, limit as i64), |row| {
                Ok(ReplyRow {
                    threads_reply_id: row.get(0)?,
                    parent_threads_post_id: row.get(1)?,
                    author_username: row.get(2)?,
                    text: row.get(3)?,
                    posted_at: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(map_db_error)?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }

    pub fn insert_insight_snapshot(
        &self,
        post_id: &str,
        views: Option<i64>,
        likes: Option<i64>,
        replies: Option<i64>,
        reposts: Option<i64>,
        quotes: Option<i64>,
        shares: Option<i64>,
        raw_json: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO insight_snapshots
             (threads_post_id, views, likes, replies, reposts, quotes, shares, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (post_id, views, likes, replies, reposts, quotes, shares, raw_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    pub fn upsert_reply(
        &self,
        threads_reply_id: &str,
        parent_threads_post_id: &str,
        author_username: Option<&str>,
        text: Option<&str>,
        posted_at: Option<&str>,
        raw_json: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO replies
             (threads_reply_id, parent_threads_post_id, author_username, text, posted_at, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(threads_reply_id) DO UPDATE SET
               parent_threads_post_id=excluded.parent_threads_post_id,
               author_username=excluded.author_username,
               text=excluded.text,
               posted_at=excluded.posted_at,
               raw_json=excluded.raw_json",
            (
                threads_reply_id,
                parent_threads_post_id,
                author_username,
                text,
                posted_at,
                raw_json,
            ),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    pub fn reply_fetch_state(&self, post_id: &str) -> Result<Option<ReplyFetchStateRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT threads_post_id, next_cursor, last_fetched_at
                 FROM reply_fetch_state
                 WHERE threads_post_id = ?1",
            )
            .map_err(map_db_error)?;
        let result = stmt.query_row([post_id], |row| {
            Ok(ReplyFetchStateRow {
                threads_post_id: row.get(0)?,
                next_cursor: row.get(1)?,
                last_fetched_at: row.get(2)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(map_db_error(e)),
        }
    }

    pub fn upsert_reply_fetch_state(
        &self,
        post_id: &str,
        next_cursor: Option<&str>,
        raw_json: Option<&str>,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO reply_fetch_state (threads_post_id, next_cursor, raw_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(threads_post_id) DO UPDATE SET
               next_cursor=excluded.next_cursor,
               last_fetched_at=CURRENT_TIMESTAMP,
               raw_json=excluded.raw_json",
            (post_id, next_cursor, raw_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    pub fn create_publish_attempt(
        &self,
        input: PublishAttemptInput,
    ) -> Result<(i64, String), CliError> {
        let conn = self.connection()?;
        let attempt_uuid = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO publish_attempts
             (attempt_uuid, kind, text, reply_to_id, topic_tag, source_url, source_link_mode, status, request_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'started', ?8)",
            (
                &attempt_uuid,
                &input.kind,
                &input.text,
                &input.reply_to_id,
                &input.topic_tag,
                &input.source_url,
                &input.source_link_mode,
                &input.request_json,
            ),
        )
        .map_err(map_db_error)?;
        Ok((conn.last_insert_rowid(), attempt_uuid))
    }

    #[allow(dead_code)]
    pub fn mark_publish_attempt_ambiguous(
        &self,
        id: i64,
        error_code: &str,
        error_message: &str,
        response_json: Option<&str>,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE publish_attempts
             SET status='ambiguous', error_code=?2, error_message=?3, response_json=?4, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1",
            (id, error_code, error_message, response_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn mark_publish_attempt_failed(
        &self,
        id: i64,
        error_code: &str,
        error_message: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE publish_attempts
             SET status='failed', error_code=?2, error_message=?3, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1",
            (id, error_code, error_message),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    pub fn mark_publish_attempt_failed_with_response(
        &self,
        id: i64,
        error_code: &str,
        error_message: &str,
        response_json: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE publish_attempts
             SET status='failed', error_code=?2, error_message=?3, response_json=?4, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1",
            (id, error_code, error_message, response_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    pub fn mark_publish_attempt_published(
        &self,
        id: i64,
        threads_post_id: &str,
        response_json: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "UPDATE publish_attempts
             SET status='published', threads_post_id=?2, response_json=?3, updated_at=CURRENT_TIMESTAMP
             WHERE id=?1",
            (id, threads_post_id, response_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    pub fn persist_post(&self, input: PersistPostInput) -> Result<i64, CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO posts
             (threads_post_id, parent_threads_post_id, post_url, text, topic_tag, source_url, source_link_mode, kind, published_at, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                &input.threads_post_id,
                &input.parent_threads_post_id,
                &input.post_url,
                &input.text,
                &input.topic_tag,
                &input.source_url,
                &input.source_link_mode,
                &input.kind,
                &input.published_at,
                &input.raw_json,
            ),
        )
        .map_err(map_db_error)?;
        Ok(conn.last_insert_rowid())
    }

    #[allow(dead_code)]
    pub fn insert_search_results(
        &self,
        query: &str,
        search_type: &str,
        results: &[(String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>, Option<i64>, String)],
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "DELETE FROM search_results WHERE query = ?1 AND search_type = ?2",
            (query, search_type),
        )
        .map_err(map_db_error)?;
        for (post_id, username, text, permalink, timestamp, like_count, reply_count, raw_json) in results {
            conn.execute(
                "INSERT INTO search_results
                 (query, search_type, threads_post_id, username, text, permalink, timestamp, like_count, reply_count, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (query, search_type, post_id, username, text, permalink, timestamp, like_count, reply_count, raw_json),
            )
            .map_err(map_db_error)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn cached_search_results(
        &self,
        query: &str,
        search_type: &str,
        limit: usize,
    ) -> Result<Vec<SearchResultRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT threads_post_id, username, text, permalink, timestamp, like_count, reply_count, fetched_at
                 FROM search_results
                 WHERE query = ?1 AND search_type = ?2
                 ORDER BY id ASC
                 LIMIT ?3",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map((query, search_type, limit as i64), |row| {
                Ok(SearchResultRow {
                    threads_post_id: row.get(0)?,
                    username: row.get(1)?,
                    text: row.get(2)?,
                    permalink: row.get(3)?,
                    timestamp: row.get(4)?,
                    like_count: row.get(5)?,
                    reply_count: row.get(6)?,
                    fetched_at: row.get(7)?,
                })
            })
            .map_err(map_db_error)?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn upsert_own_thread(
        &self,
        threads_post_id: &str,
        text: Option<&str>,
        permalink: Option<&str>,
        timestamp: Option<&str>,
        username: Option<&str>,
        raw_json: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO own_threads (threads_post_id, text, permalink, timestamp, username, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(threads_post_id) DO UPDATE SET
               text=excluded.text,
               permalink=excluded.permalink,
               timestamp=excluded.timestamp,
               username=excluded.username,
               raw_json=excluded.raw_json,
               fetched_at=CURRENT_TIMESTAMP",
            (threads_post_id, text, permalink, timestamp, username, raw_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_own_threads(&self, limit: usize) -> Result<Vec<OwnThreadRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT threads_post_id, text, permalink, timestamp, username, fetched_at
                 FROM own_threads
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(OwnThreadRow {
                    threads_post_id: row.get(0)?,
                    text: row.get(1)?,
                    permalink: row.get(2)?,
                    timestamp: row.get(3)?,
                    username: row.get(4)?,
                    fetched_at: row.get(5)?,
                })
            })
            .map_err(map_db_error)?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn upsert_own_reply(
        &self,
        threads_post_id: &str,
        reply_to_id: Option<&str>,
        text: Option<&str>,
        permalink: Option<&str>,
        timestamp: Option<&str>,
        username: Option<&str>,
        raw_json: &str,
    ) -> Result<(), CliError> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO own_replies (threads_post_id, reply_to_id, text, permalink, timestamp, username, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(threads_post_id) DO UPDATE SET
               reply_to_id=excluded.reply_to_id,
               text=excluded.text,
               permalink=excluded.permalink,
               timestamp=excluded.timestamp,
               username=excluded.username,
               raw_json=excluded.raw_json,
               fetched_at=CURRENT_TIMESTAMP",
            (threads_post_id, reply_to_id, text, permalink, timestamp, username, raw_json),
        )
        .map_err(map_db_error)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_own_replies(&self, limit: usize) -> Result<Vec<OwnReplyRow>, CliError> {
        let conn = self.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT threads_post_id, reply_to_id, text, permalink, timestamp, username, fetched_at
                 FROM own_replies
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(map_db_error)?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(OwnReplyRow {
                    threads_post_id: row.get(0)?,
                    reply_to_id: row.get(1)?,
                    text: row.get(2)?,
                    permalink: row.get(3)?,
                    timestamp: row.get(4)?,
                    username: row.get(5)?,
                    fetched_at: row.get(6)?,
                })
            })
            .map_err(map_db_error)?;
        let mut out = Vec::new();
        for item in rows {
            out.push(item.map_err(map_db_error)?);
        }
        Ok(out)
    }
}

#[allow(dead_code)]
pub async fn retry_with_backoff<F, Fut, T>(
    policy: RetryPolicy,
    operation_kind: RetryOperation,
    mut operation: F,
) -> Result<T, CliError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, CliError>>,
{
    let mut attempt = 0_u32;
    loop {
        let result = operation().await;
        match result {
            Ok(value) => return Ok(value),
            Err(err) => {
                let operation_retryable =
                    matches!(operation_kind, RetryOperation::SafeRead | RetryOperation::TokenRefresh);
                let retryable = matches!(
                    err.category,
                    ErrorCategory::Network | ErrorCategory::RateLimit
                ) || (matches!(err.category, ErrorCategory::Api)
                    && err.message.contains("HTTP 5"));
                if !operation_retryable || !retryable || attempt >= policy.max_retries {
                    return Err(err);
                }
                let delay = policy.base_delay_ms.saturating_mul(2_u64.saturating_pow(attempt));
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                attempt += 1;
            }
        }
    }
}

fn map_db_error(err: rusqlite::Error) -> CliError {
    CliError::new(ErrorCategory::Database, format!("database error: {}", err))
}

fn expand_path(path: &str) -> Result<PathBuf, CliError> {
    let expanded = shellexpand::full(path).map_err(|e| {
        CliError::new(
            ErrorCategory::Config,
            format!("failed expanding db path '{}': {}", path, e),
        )
    })?;
    Ok(PathBuf::from(expanded.as_ref()))
}
