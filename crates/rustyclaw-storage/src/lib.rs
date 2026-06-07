use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use rusqlite::Connection;
use rustyclaw_providers::Message;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

pub mod search;
pub use search::SearchIndexManager;

type EmbeddingRow = (String, String, String, Vec<f32>);

// ==============================================================================
// 0. パスロック管理 (Path Lock Manager)
// ==============================================================================

static PATH_LOCKS: Lazy<StdMutex<HashMap<PathBuf, Arc<RwLock<()>>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

fn canonicalize_path(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    }
}

/// 指定したファイルパスの非同期読み込みロック（共有ロック）を取得する
pub async fn acquire_read_lock(path: &Path) -> OwnedRwLockReadGuard<()> {
    let normalized = canonicalize_path(path);
    let lock = {
        let mut locks = PATH_LOCKS.lock().unwrap();
        locks
            .entry(normalized)
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    };
    lock.read_owned().await
}

/// 指定したファイルパスの非同期書き込みロック（排他ロック）を取得する
pub async fn acquire_write_lock(path: &Path) -> OwnedRwLockWriteGuard<()> {
    let normalized = canonicalize_path(path);
    let lock = {
        let mut locks = PATH_LOCKS.lock().unwrap();
        locks
            .entry(normalized)
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    };
    lock.write_owned().await
}

// ==============================================================================
// 1. SQLite データベース管理 (DbManager)
// ==============================================================================

pub struct DbManager {
    conn: Connection,
}

impl DbManager {
    /// データベースファイルを接続し、初期化する
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref()).context("Failed to open SQLite database")?;

        // データベースパフォーマンスと信頼性のための PRAGMA 設定 (WALモード等)
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-32000;
            PRAGMA temp_store=MEMORY;
        ",
        )
        .context("Failed to apply SQLite PRAGMA settings")?;

        let manager = Self { conn };
        manager.create_tables()?;
        Ok(manager)
    }

    /// 初期テーブル作成（マイグレーション）
    fn create_tables(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "
            CREATE TABLE IF NOT EXISTS usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                model TEXT NOT NULL DEFAULT '',
                trigger_type TEXT NOT NULL DEFAULT 'unknown',
                duration_ms INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_created_at ON usage (created_at);

            CREATE TABLE IF NOT EXISTS patrol_state (
                patrol_name TEXT PRIMARY KEY,
                last_run_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS seen_items (
                item_id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                seen_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS memory_embeddings (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                session_id TEXT,
                text_content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_embeddings_source
                ON memory_embeddings(source);
            CREATE TABLE IF NOT EXISTS document_states (
                file_path TEXT PRIMARY KEY,
                last_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        ",
            )
            .context("Failed to create SQLite tables")?;

        // Migration: add columns for DBs created before the schema extension.
        // Each ALTER is run independently; errors (column already exists) are ignored.
        for stmt in [
            "ALTER TABLE usage ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE usage ADD COLUMN model TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE usage ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'unknown'",
            "ALTER TABLE usage ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE usage ADD COLUMN provider_id TEXT",
        ] {
            let _ = self.conn.execute(stmt, []);
        }

        Ok(())
    }

    // --- Memory Embeddings (RAG) ---

    pub fn serialize_embedding(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    pub fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    pub fn upsert_embedding(
        &self,
        id: &str,
        source: &str,
        session_id: Option<&str>,
        text_content: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let blob = Self::serialize_embedding(embedding);
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO memory_embeddings
             (id, source, session_id, text_content, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, source, session_id, text_content, blob, now],
            )
            .context("Failed to upsert embedding")?;
        Ok(())
    }

    pub fn load_all_embeddings(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT text_content, embedding FROM memory_embeddings")
            .context("Failed to prepare load_all_embeddings")?;
        let rows = stmt
            .query_map([], |row| {
                let text: String = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((text, blob))
            })
            .context("Failed to query embeddings")?;
        let mut out = Vec::new();
        for row in rows {
            let (text, blob) = row.context("Failed to read embedding row")?;
            out.push((text, Self::deserialize_embedding(&blob)));
        }
        Ok(out)
    }

    /// (id, source, text_content, embedding) の全行を返す。UnifiedRagEngine の rebuild に使用。
    pub fn load_all_embeddings_with_ids(&self) -> Result<Vec<EmbeddingRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source, text_content, embedding FROM memory_embeddings")
            .context("Failed to prepare load_all_embeddings_with_ids")?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let source: String = row.get(1)?;
                let text: String = row.get(2)?;
                let blob: Vec<u8> = row.get(3)?;
                Ok((id, source, text, blob))
            })
            .context("Failed to query embeddings")?;
        let mut out = Vec::new();
        for row in rows {
            let (id, source, text, blob) = row.context("Failed to read row")?;
            out.push((id, source, text, Self::deserialize_embedding(&blob)));
        }
        Ok(out)
    }

    pub fn delete_embeddings_by_source(&self, source: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM memory_embeddings WHERE source = ?1",
                rusqlite::params![source],
            )
            .context("Failed to delete embeddings by source")?;
        Ok(())
    }

    /// source="session" の embedding のうち keep_days 日より古いものを削除する。
    pub fn delete_old_session_embeddings(&self, keep_days: u32) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM memory_embeddings
             WHERE source = 'session'
               AND created_at < datetime('now', ?1)",
                rusqlite::params![format!("-{} days", keep_days)],
            )
            .context("Failed to delete old session embeddings")?;
        Ok(())
    }

    /// ファイルのハッシュ値を検証し、前回から変更されているかを判定する。
    /// 変更されている（または未登録）の場合は true を返し、ハッシュ値を更新する。
    /// Fail-closed: DB エラー時は true を返して再インジェストを促す。
    pub fn check_and_update_doc_state(&self, file_path: &str, current_hash: &str) -> Result<bool> {
        // Use a closure to allow early-return with ? while catching all errors at the outer level.
        let inner = || -> Result<bool> {
            let mut stmt = self
                .conn
                .prepare("SELECT last_hash FROM document_states WHERE file_path = ?1")?;
            let mut rows = stmt.query([file_path])?;

            if let Some(row) = rows.next()? {
                let last_hash: String = row.get(0)?;
                if last_hash == current_hash {
                    return Ok(false);
                }
            }

            self.conn.execute(
                "INSERT OR REPLACE INTO document_states (file_path, last_hash, updated_at)
                 VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![file_path, current_hash],
            )?;
            Ok(true)
        };

        Ok(inner().unwrap_or_else(|e| {
            tracing::warn!(
                "check_and_update_doc_state: DB error for '{}', forcing re-ingest: {}",
                file_path,
                e
            );
            true
        }))
    }

    /// memory_embeddings テーブルに保存されているベクトルの次元数を検出する。
    /// テーブルが空の場合は None を返す（BLOB サイズ / 4 = f32 の個数）。
    pub fn detect_stored_embedding_dims(&self) -> Option<usize> {
        self.conn
            .query_row(
                "SELECT LENGTH(embedding) FROM memory_embeddings LIMIT 1",
                [],
                |row| row.get::<_, usize>(0),
            )
            .ok()
            .map(|bytes| bytes / 4)
    }

    /// 保存済みベクトルの次元数が expected_dims と異なる場合、
    /// memory_embeddings と document_states を全削除する。
    /// 削除が発生した場合 true、不要だった場合 false を返す。
    pub fn migrate_embedding_dims_if_needed(&self, expected_dims: usize) -> Result<bool> {
        match self.detect_stored_embedding_dims() {
            Some(stored_dims) if stored_dims != expected_dims => {
                self.conn
                    .execute("DELETE FROM memory_embeddings", [])
                    .context("migrate: failed to clear memory_embeddings")?;
                self.conn
                    .execute("DELETE FROM document_states", [])
                    .context("migrate: failed to clear document_states")?;
                tracing::info!(
                    "migrate_embedding_dims: cleared DB ({}→{} dims)",
                    stored_dims,
                    expected_dims
                );
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// クエリベクトルに近い記憶を (source, text_content, score) 形式で返す。
    /// 既存の search_similar_memories と異なり source 列も含む。
    pub fn search_similar_with_source(
        &self,
        query_vec: &[f32],
        top_k: usize,
        threshold: f32,
    ) -> Result<Vec<(String, String, f64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source, text_content, embedding FROM memory_embeddings")
            .context("search_similar_with_source: prepare failed")?;
        let rows = stmt
            .query_map([], |row| {
                let source: String = row.get(0)?;
                let text: String = row.get(1)?;
                let blob: Vec<u8> = row.get(2)?;
                Ok((source, text, blob))
            })
            .context("search_similar_with_source: query failed")?;

        let mut scored: Vec<(String, String, f64)> = rows
            .filter_map(|r| r.ok())
            .map(|(source, text, blob)| {
                let emb = Self::deserialize_embedding(&blob);
                let score = Self::cosine_similarity(query_vec, &emb) as f64;
                (source, text, score)
            })
            .filter(|(_, _, score)| *score >= threshold as f64)
            .collect();

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    /// コサイン類似度に時間減衰係数を乗じた combined_score でリランキングする。
    /// threshold は cosine_sim に適用（relevance ゲート）。
    /// combined_score = cosine_sim * 0.5^(age_days / half_life_days)
    /// created_at の parse 失敗は fail-open（age_days = 0.0 → decay なし）。
    pub fn search_similar_with_decay(
        &self,
        query_vec: &[f32],
        top_k: usize,
        threshold: f32,
        half_life_days: f64,
    ) -> Result<Vec<(String, String, f64)>> {
        if half_life_days <= 0.0 {
            return Ok(vec![]);
        }
        let threshold = threshold as f64;
        let now_utc = chrono::Utc::now();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT source, text_content, embedding, created_at FROM memory_embeddings",
            )
            .context("search_similar_with_decay: prepare failed")?;
        let rows = stmt
            .query_map([], |row| {
                let source: String = row.get(0)?;
                let text: String = row.get(1)?;
                let blob: Vec<u8> = row.get(2)?;
                let created_at: String = row.get(3)?;
                Ok((source, text, blob, created_at))
            })
            .context("search_similar_with_decay: query failed")?;

        let mut scored: Vec<(String, String, f64)> = rows
            .filter_map(|r| r.ok())
            .filter_map(|(source, text, blob, created_at_str)| {
                let emb = Self::deserialize_embedding(&blob);
                let sim = Self::cosine_similarity(query_vec, &emb) as f64;
                if sim < threshold {
                    return None;
                }
                let age_days = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| {
                        let secs = (now_utc - dt.to_utc()).num_seconds().max(0) as f64;
                        secs / 86400.0
                    })
                    .unwrap_or(0.0);
                let factor = 0.5_f64.powf(age_days / half_life_days);
                let combined = sim * factor;
                Some((source, text, combined))
            })
            .collect();

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    /// コサイン類似度を計算する
    /// 同じ次元数・ノンゼロのベクトルが必須。不正な場合は 0.0 を返す。
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        dot / (norm_a * norm_b)
    }

    /// クエリベクトルに近い記憶を上位 top_k 件返す → Vec<(text_content, similarity)>
    /// threshold 未満のエントリは除外。外部依存なし。
    pub fn search_similar_memories(
        &self,
        query_vec: &[f32],
        top_k: usize,
        threshold: f32,
    ) -> Result<Vec<(String, f32)>> {
        let all = self.load_all_embeddings()?;
        let mut scored: Vec<(String, f32)> = all
            .into_iter()
            .map(|(text, emb)| {
                let sim = Self::cosine_similarity(query_vec, &emb);
                (text, sim)
            })
            .filter(|(_, sim)| *sim >= threshold)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    // --- Usage (トークン使用量) 操作 ---
    #[allow(clippy::too_many_arguments)]
    pub fn record_usage(
        &self,
        session_id: &str,
        prompt: u32,
        completion: u32,
        total: u32,
        model: &str,
        trigger_type: &str,
        provider_id: Option<&str>,
        duration_ms: u64,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, provider_id, duration_ms, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![session_id, prompt, completion, total, model, trigger_type, provider_id, duration_ms as i64, now],
        )
        .context("Failed to record usage in SQLite")?;
        Ok(())
    }

    pub fn get_usage_summary(&self, since: Option<&str>) -> serde_json::Value {
        let where_clause = if since.is_some() {
            "WHERE created_at >= ?1"
        } else {
            ""
        };
        let since_owned = since.map(|s| s.to_string());
        let params: Vec<&dyn rusqlite::ToSql> = match since_owned.as_ref() {
            Some(s) => vec![s],
            None => vec![],
        };

        let total: (i64, i64, i64, i64) = self.conn.query_row(
            &format!("SELECT COALESCE(COUNT(*),0), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0) FROM usage {}", where_clause),
            params.as_slice(),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap_or((0, 0, 0, 0));

        let mut by_model = serde_json::Map::new();
        if let Ok(mut stmt) = self.conn.prepare(
            &format!("SELECT model, COUNT(*), COALESCE(SUM(total_tokens),0) FROM usage {} GROUP BY model ORDER BY SUM(total_tokens) DESC LIMIT 10", where_clause)
        ) && let Ok(rows) = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        }) {
            for row in rows.flatten() {
                by_model.insert(row.0, serde_json::json!({ "runs": row.1, "tokens": row.2 }));
            }
        }

        let mut by_provider = serde_json::Map::new();
        {
            let prov_sql = format!(
                "SELECT provider_id, COUNT(*), COALESCE(SUM(total_tokens),0) FROM usage WHERE provider_id IS NOT NULL {} GROUP BY provider_id ORDER BY SUM(total_tokens) DESC",
                if since.is_some() {
                    "AND created_at >= ?1"
                } else {
                    ""
                }
            );
            if let Ok(mut stmt) = self.conn.prepare(&prov_sql)
                && let Ok(rows) = stmt.query_map(params.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
            {
                for row in rows.flatten() {
                    by_provider
                        .insert(row.0, serde_json::json!({ "runs": row.1, "tokens": row.2 }));
                }
            }
        }

        serde_json::json!({
            "total_runs": total.0,
            "total_input_tokens": total.1,
            "total_completion_tokens": total.2,
            "total_tokens": total.3,
            "by_model": by_model,
            "by_provider": by_provider,
        })
    }

    /// 使用量をトークン数で時刻バケット集計する。
    pub fn get_usage_timeline(
        &self,
        since_epoch: Option<i64>,
        until_epoch: i64,
        granularity_secs: u64,
    ) -> Vec<serde_json::Value> {
        use chrono::TimeZone;
        let g = (granularity_secs.max(1)) as i64;
        let since_rfc = since_epoch.map(|s| {
            chrono::Utc
                .timestamp_opt(s, 0)
                .earliest()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        });
        let where_clause = if since_rfc.is_some() {
            "WHERE created_at >= ?1"
        } else {
            ""
        };
        let params: Vec<&dyn rusqlite::ToSql> = match since_rfc.as_ref() {
            Some(s) => vec![s],
            None => vec![],
        };
        let sql = format!(
            "SELECT (CAST(strftime('%s', created_at) AS INTEGER) / {g}) * {g} AS bucket, \
             COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0) \
             FROM usage {where_clause} GROUP BY bucket ORDER BY bucket ASC",
            g = g,
            where_clause = where_clause
        );
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let sparse: std::collections::BTreeMap<i64, (i64, i64, i64)> = stmt
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    (
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ),
                ))
            })
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();

        if sparse.is_empty() {
            return vec![];
        }

        // 窓開始: since 指定があればそのフロア、無ければ最初のデータバケット
        let first_entry = *sparse.keys().next().unwrap();
        let mut start = match since_epoch {
            Some(s) => (s / g) * g,
            None => first_entry,
        };
        let end = (until_epoch / g) * g;
        // Cap to ensure at most 1000 data points to avoid massive loops and preserve recent data
        if start < end - 1000 * g {
            start = end - 1000 * g;
        }
        let mut out = Vec::new();
        let mut b = start;
        let mut count = 0;
        while b <= end {
            if count > 10_000 {
                break; // Safety limit
            }
            let (i, c, t) = sparse.get(&b).copied().unwrap_or((0, 0, 0));
            out.push(serde_json::json!({
                "bucket_epoch": b,
                "input_tokens": i,
                "completion_tokens": c,
                "tokens": t,
            }));
            b += g;
            count += 1;
        }
        out
    }

    pub fn get_usage_by_trigger(&self, since: Option<&str>) -> Vec<serde_json::Value> {
        let where_clause = if since.is_some() {
            "WHERE created_at >= ?1"
        } else {
            ""
        };
        let since_owned = since.map(|s| s.to_string());
        let params: Vec<&dyn rusqlite::ToSql> = match since_owned.as_ref() {
            Some(s) => vec![s],
            None => vec![],
        };
        let mut stmt = match self.conn.prepare(&format!(
            "SELECT trigger_type, COUNT(*), COALESCE(SUM(total_tokens),0) FROM usage {} GROUP BY trigger_type ORDER BY SUM(total_tokens) DESC",
            where_clause
        )) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params.as_slice(), |row| {
            Ok(serde_json::json!({
                "trigger": row.get::<_, String>(0)?,
                "runs": row.get::<_, i64>(1)?,
                "tokens": row.get::<_, i64>(2)?,
            }))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    // --- Patrol State (Heartbeatパトロール実行時刻) 操作 ---
    pub fn update_patrol_state(&self, patrol_name: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.set_state_value(patrol_name, &now)
    }

    pub fn set_state_value(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO patrol_state (patrol_name, last_run_at) VALUES (?1, ?2)",
                (key, value),
            )
            .context("Failed to update patrol state value in SQLite")?;
        Ok(())
    }

    pub fn get_last_patrol_run(&self, patrol_name: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT last_run_at FROM patrol_state WHERE patrol_name = ?1")?;
        let mut rows = stmt.query((patrol_name,))?;
        if let Some(row) = rows.next()? {
            let last_run: String = row.get(0)?;
            Ok(Some(last_run))
        } else {
            Ok(None)
        }
    }

    // --- Seen Items (既読アイテム管理) 操作 ---
    pub fn mark_item_seen(&self, item_id: &str, category: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO seen_items (item_id, category, seen_at) VALUES (?1, ?2, ?3)",
            (item_id, category, now),
        )
        .context("Failed to mark item as seen in SQLite")?;
        Ok(())
    }

    pub fn is_item_seen(&self, item_id: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM seen_items WHERE item_id = ?1")?;
        let exists = stmt
            .exists((item_id,))
            .context("Failed to query seen items in SQLite")?;
        Ok(exists)
    }
}

// ==============================================================================
// 2. 原子性書き込み (Atomic Write)
// ==============================================================================

/// 電源断やクラッシュ時にもファイル破損を防ぐ原子性書き込み
pub async fn atomic_write<P: AsRef<Path>>(path: P, data: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let _guard = acquire_write_lock(path).await;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .context("Failed to create temporary file for atomic write")?;

    tmp.write_all(data)
        .context("Failed to write data to temporary file")?;
    tmp.as_file()
        .sync_all()
        .context("Failed to sync temporary file content to disk")?;
    tmp.persist(path)
        .context("Failed to persist temporary file to target path")?;
    Ok(())
}

// ==============================================================================
// 3. セッションJSONLロガー (SessionLogger)
// ==============================================================================

pub struct SessionLogger {
    sessions_dir: PathBuf,
}

impl SessionLogger {
    pub fn new<P: AsRef<Path>>(workspace_dir: P) -> Self {
        let sessions_dir = workspace_dir.as_ref().join("sessions");
        Self { sessions_dir }
    }

    /// 会話メッセージを session_id に対応する jsonl に追記する (fail-closed)
    pub fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        std::fs::create_dir_all(&self.sessions_dir)
            .context("Failed to create sessions directory")?;

        let safe_session_id = session_id.replace(':', "-");
        let file_path = self.sessions_dir.join(format!("{}.jsonl", safe_session_id));

        let mut logged_msg = message.clone();
        if logged_msg.timestamp.is_none() {
            logged_msg.timestamp = Some(chrono::Local::now().to_rfc3339());
        }

        let json_line =
            serde_json::to_string(&logged_msg).context("Failed to serialize message to JSON")?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .with_context(|| format!("Failed to open session log file at {:?}", file_path))?;

        writeln!(file, "{}", json_line)
            .with_context(|| format!("Failed to write message to session log {:?}", file_path))?;

        file.sync_all()
            .with_context(|| format!("Failed to sync session log file to disk {:?}", file_path))?;

        Ok(())
    }

    /// 指定された session_id の全会話履歴をロードする
    pub fn load_history(&self, session_id: &str) -> Result<Vec<Message>> {
        let safe_session_id = session_id.replace(':', "-");
        let file_path = self.sessions_dir.join(format!("{}.jsonl", safe_session_id));
        if !file_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&file_path).with_context(|| {
            format!(
                "Failed to open session log file for reading {:?}",
                file_path
            )
        })?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for line_res in reader.lines() {
            let line = line_res.context("Failed to read line from session log")?;
            if line.trim().is_empty() {
                continue;
            }
            let msg: Message = serde_json::from_str(&line)
                .with_context(|| format!("Failed to parse session log line: {}", line))?;
            messages.push(msg);
        }

        Ok(messages)
    }
}

// ==============================================================================
// 4. 会話履歴の管理と 70/20/10 圧縮 (ConversationHistory)
// ==============================================================================

#[derive(Debug, Clone)]
pub struct ConversationHistory {
    pub messages: Vec<Message>,
}

impl ConversationHistory {
    pub fn new(messages: Vec<Message>) -> Self {
        Self { messages }
    }

    /// トークン数推定（LLaMA 系トークナイザー補正済み）
    /// 日本語は1文字あたり約1.5 BPEトークンになるため、chars数に×1.5の補正係数を適用する
    pub fn estimate_tokens(&self) -> usize {
        let mut total = 0;
        for msg in &self.messages {
            total += msg.content.chars().count();
        }
        (total * 3) / 2
    }

    /// 会話履歴の圧縮 (70/20/10 戦略)
    /// - `limit` は総トークン上限
    /// - 推定トークン数が `limit` の 80% を超えたらトリガー
    /// - 先頭 40% (背景) と末尾 40% (直近の対話) を保持し、中間 20% を省略メッセージで置換
    pub fn compact_if_needed(&mut self, limit: usize) -> bool {
        let current_tokens = self.estimate_tokens();
        let trigger_threshold = (limit * 80) / 100;

        if current_tokens <= trigger_threshold || self.messages.len() < 5 {
            return false;
        }

        tracing::info!(
            "Triggering context compression. Current estimated tokens: {}, limit: {}",
            current_tokens,
            limit
        );

        let total_count = self.messages.len();

        // 先頭の 70% (四捨五入) — 会話の背景・前提情報を保持
        let head_count = ((total_count as f64) * 0.7).round() as usize;
        // 末尾の 20% (四捨五入) — 直近の会話を保持
        let tail_count = ((total_count as f64) * 0.2).round() as usize;
        // 中間の数
        let middle_count = total_count - head_count - tail_count;

        if middle_count == 0 {
            return false;
        }

        let mut new_messages = Vec::with_capacity(head_count + 1 + tail_count);

        // 先頭 40% をコピー
        new_messages.extend_from_slice(&self.messages[0..head_count]);

        // 中間の省略を表現するシステムメッセージを挿入
        let omitted_msg = Message {
            role: "system".to_string(),
            content: format!(
                "[{} messages omitted for context compression to save token quota]",
                middle_count
            ),
            name: None,
            ..Default::default()
        };
        new_messages.push(omitted_msg);

        // 末尾 40% をコピー
        new_messages.extend_from_slice(&self.messages[total_count - tail_count..]);

        let before_tokens = self.estimate_tokens();
        self.messages = new_messages;
        let after_tokens = self.estimate_tokens();

        tracing::info!(
            "Context compression complete. Reduced messages from {} to {}. Estimated tokens: {} -> {}",
            total_count,
            self.messages.len(),
            before_tokens,
            after_tokens
        );

        true
    }

    /// system プロンプト＋ツール定義などの固定オーバーヘッド（推定トークン）を考慮して圧縮する。
    /// 実効上限を `limit - overhead_tokens` に下げてから既存の圧縮判定を行うことで、
    /// 履歴＋システム＋ツールの合計がモデル上限を超えて 413 になるのを防ぐ。
    pub fn compact_if_needed_with_overhead(
        &mut self,
        limit: usize,
        overhead_tokens: usize,
    ) -> bool {
        let effective = limit.saturating_sub(overhead_tokens);
        self.compact_if_needed(effective)
    }

    /// TPM 超過を防ぐためのハードキャップ: 末尾 max_messages 件のみ残す。
    /// compact_if_needed_with_overhead の後段に適用することで、トークン推定の
    /// アンダーフロー問題を補完する。
    pub fn trim_to_last(&mut self, max_messages: usize) {
        if self.messages.len() <= max_messages {
            return;
        }
        let trimmed = self.messages.len() - max_messages;
        tracing::info!(
            "Hard-trimming history: {} → {} messages ({} removed for TPM compliance)",
            self.messages.len(),
            max_messages,
            trimmed
        );
        self.messages = self.messages[trimmed..].to_vec();
    }
}

// ==============================================================================
// Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overhead_lowers_effective_limit_and_triggers_compaction() {
        // 10 件 × 100 文字 ≒ 1,500 推定トークン
        let msgs: Vec<Message> = (0..10)
            .map(|_| Message {
                role: "user".into(),
                content: "a".repeat(100),
                name: None,
                ..Default::default()
            })
            .collect();
        // overhead=0・大きな上限 → 圧縮されない
        let mut h0 = ConversationHistory::new(msgs.clone());
        assert!(!h0.compact_if_needed_with_overhead(10_000, 0));
        // overhead 大で実効上限が下がり圧縮が発火する
        let mut h1 = ConversationHistory::new(msgs.clone());
        assert!(h1.compact_if_needed_with_overhead(2_000, 1_800));
    }

    #[test]
    fn test_db_manager_creation_and_basic_ops() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let db_path = tmp_dir.path().join("test_memory.db");

        let db = DbManager::new(&db_path)?;

        // Usage テスト
        db.record_usage("session-1", 100, 50, 150, "test-model", "cli", None, 0)?;

        // Patrol State テスト
        assert!(db.get_last_patrol_run("patrol-1")?.is_none());
        db.update_patrol_state("patrol-1")?;
        assert!(db.get_last_patrol_run("patrol-1")?.is_some());

        // Seen Items テスト
        assert!(!db.is_item_seen("item-1")?);
        db.mark_item_seen("item-1", "news")?;
        assert!(db.is_item_seen("item-1")?);

        Ok(())
    }

    #[tokio::test]
    async fn test_atomic_write() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let file_path = tmp_dir.path().join("test_atomic.txt");

        atomic_write(&file_path, b"Hello Atomic").await?;
        let content = std::fs::read_to_string(&file_path)?;
        assert_eq!(content, "Hello Atomic");

        Ok(())
    }

    #[test]
    fn test_session_logger() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let logger = SessionLogger::new(tmp_dir.path());

        let msg = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            name: None,
            ..Default::default()
        };

        logger.append_message("session-abc", &msg)?;
        let history = logger.load_history("session-abc")?;

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "Hello");

        Ok(())
    }

    #[test]
    fn test_usage_aggregation() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let db = DbManager::new(tmp_dir.path().join("agg.db"))?;
        db.record_usage(
            "cron:heartbeat",
            100,
            50,
            150,
            "model-a",
            "heartbeat",
            Some("groq"),
            0,
        )?;
        db.record_usage(
            "discord-1",
            200,
            80,
            280,
            "model-a",
            "discord",
            Some("groq"),
            0,
        )?;

        let summary = db.get_usage_summary(None);
        assert_eq!(summary["total_runs"], 2);
        assert_eq!(summary["total_tokens"], 430);
        assert_eq!(summary["by_model"]["model-a"]["tokens"], 430);

        let now = chrono::Utc::now().timestamp();
        let timeline = db.get_usage_timeline(None, now, 86400);
        assert!(!timeline.is_empty());
        let day_total: i64 = timeline
            .iter()
            .map(|r| r["tokens"].as_i64().unwrap_or(0))
            .sum();
        assert_eq!(day_total, 430);

        let triggers = db.get_usage_by_trigger(None);
        assert_eq!(triggers.len(), 2);

        // since-filter excluding everything (far future) yields zero runs
        let empty = db.get_usage_summary(Some("2999-01-01"));
        assert_eq!(empty["total_runs"], 0);
        Ok(())
    }

    #[test]
    fn test_usage_timeline_hourly_buckets_and_zero_fill() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let db = DbManager::new(tmp_dir.path().join("tl.db"))?;
        // 既知の created_at を 2 件（同一日・2時間離れ）直接挿入する
        db.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, duration_ms, created_at) \
             VALUES ('s1', 100, 50, 150, 'm', 'discord', 0, '2026-05-31T01:00:00+00:00')",
            [],
        )?;
        db.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, duration_ms, created_at) \
             VALUES ('s2', 10, 5, 15, 'm', 'discord', 0, '2026-05-31T03:00:00+00:00')",
            [],
        )?;
        // 窓: 01:00〜03:00 UTC、粒度 1 時間 → 3 バケット（01,02,03時）、02時は 0 埋め
        let since = 1780189200; // 2026-05-31T01:00:00Z
        let until = 1780196400; // 2026-05-31T03:00:00Z
        let rows = db.get_usage_timeline(Some(since), until, 3600);
        assert_eq!(rows.len(), 3, "01/02/03 時の3バケット（0埋め含む）");
        assert_eq!(rows[0]["tokens"], 150);
        assert_eq!(rows[1]["tokens"], 0); // 02時は0埋め
        assert_eq!(rows[2]["tokens"], 15);
        assert_eq!(rows[0]["bucket_epoch"], since);
        Ok(())
    }

    #[test]
    fn test_conversation_history_compression() {
        let mut messages = Vec::new();
        // 10メッセージ、各100文字 (合計1000文字相当)
        for i in 0..10 {
            messages.push(Message {
                role: if i % 2 == 0 {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                content: "A".repeat(100),
                name: None,
                ..Default::default()
            });
        }

        let mut history = ConversationHistory::new(messages);

        // 限界値 1000 とすると、推定 1000 文字は 800 (80%) を超えるので圧縮がトリガーされるはず
        let triggered = history.compact_if_needed(1000);

        assert!(triggered);
        // 70/20/10 戦略: head=7, tail=2, middle=1 → 7 + 1(省略) + 2 = 10
        assert_eq!(history.messages.len(), 10);
        assert_eq!(history.messages[7].role, "system");
        assert!(history.messages[7].content.contains("omitted"));
    }

    #[test]
    fn test_by_provider_aggregation() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let db = DbManager::new(tmp_dir.path().join("prov.db"))?;
        db.record_usage(
            "s1",
            100,
            50,
            150,
            "model-cf",
            "heartbeat",
            Some("cloudflare"),
            0,
        )?;
        db.record_usage("s2", 200, 80, 280, "model-gr", "discord", Some("groq"), 0)?;
        db.record_usage("s3", 50, 20, 70, "model-gr", "cli", Some("groq"), 0)?;
        db.record_usage("s4", 30, 10, 40, "model-old", "unknown", None, 0)?;

        let summary = db.get_usage_summary(None);
        let by_provider = &summary["by_provider"];
        assert_eq!(by_provider["cloudflare"]["runs"], 1);
        assert_eq!(by_provider["cloudflare"]["tokens"], 150);
        assert_eq!(by_provider["groq"]["runs"], 2);
        assert_eq!(by_provider["groq"]["tokens"], 350);
        // None entries should not appear
        assert!(by_provider.get("").is_none());
        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_embedding() {
        let v: Vec<f32> = vec![1.0, -0.5, 0.0, 2.5];
        let bytes = DbManager::serialize_embedding(&v);
        assert_eq!(bytes.len(), 16); // 4 × 4 bytes
        let back = DbManager::deserialize_embedding(&bytes);
        for (a, b) in v.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-7);
        }
    }

    #[test]
    fn test_embedding_crud() {
        let tmp = tempfile::tempdir().unwrap();
        let db = DbManager::new(tmp.path().join("test.db")).unwrap();
        let emb: Vec<f32> = vec![0.1, 0.2, 0.3];
        db.upsert_embedding("id1", "memory", None, "hello world", &emb)
            .unwrap();
        let rows = db.load_all_embeddings().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "hello world");
        assert_eq!(rows[0].1.len(), 3);
        db.delete_embeddings_by_source("memory").unwrap();
        assert!(db.load_all_embeddings().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_path_locking_concurrency() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let file_path = tmp_dir.path().join("lock_test.txt");

        // 1. 最初に共有読み込みロックを2つ同時に取得できるか（並行読み込み可能か）検証
        let r_guard1 = acquire_read_lock(&file_path).await;
        let r_guard2 = acquire_read_lock(&file_path).await;
        drop(r_guard1);
        drop(r_guard2);

        // 2. 書き込みロックを取得し、その間は読み込みロックがブロックされるか検証
        let w_guard = acquire_write_lock(&file_path).await;

        let file_path_clone = file_path.clone();
        let handle = tokio::spawn(async move {
            let _r_guard = acquire_read_lock(&file_path_clone).await;
            42
        });

        // 少し待機しても handle は完了しないはず（書き込みロックが排他されているため）
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished());

        // 書き込みロックを解放すると、待機していた読み込みロックが完了する
        drop(w_guard);

        let val = handle.await?;
        assert_eq!(val, 42);

        Ok(())
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0_f32, 0.0, 0.0];
        assert!((DbManager::cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        assert!(DbManager::cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_search_similar_memories() {
        let tmp = tempfile::tempdir().unwrap();
        let db = DbManager::new(tmp.path().join("test.db")).unwrap();
        // (1, 0, 0) = "hello" に近いベクトル
        db.upsert_embedding("a", "memory", None, "hello", &[1.0, 0.0, 0.0])
            .unwrap();
        // (0, 1, 0) = "world" は直交
        db.upsert_embedding("b", "memory", None, "world", &[0.0, 1.0, 0.0])
            .unwrap();

        // クエリ (1, 0, 0) → threshold 0.9 → "hello" のみヒット
        let results = db
            .search_similar_memories(&[1.0, 0.0, 0.0], 5, 0.9)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "hello");
        assert!((results[0].1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_load_all_embeddings_with_ids() {
        let dir = tempfile::tempdir().unwrap();
        let db = DbManager::new(dir.path().join("t.db").to_str().unwrap()).unwrap();
        db.upsert_embedding("m0", "memory", None, "text A", &[1.0f32, 0.0])
            .unwrap();
        db.upsert_embedding("s0", "session", Some("ses-1"), "text B", &[0.0f32, 1.0])
            .unwrap();

        let rows = db.load_all_embeddings_with_ids().unwrap();
        assert_eq!(rows.len(), 2);
        let ids: Vec<&str> = rows.iter().map(|(id, _, _, _)| id.as_str()).collect();
        assert!(ids.contains(&"m0"));
        assert!(ids.contains(&"s0"));
        let (_, src, _, _) = rows.iter().find(|(id, _, _, _)| id == "m0").unwrap();
        assert_eq!(src, "memory");
    }

    #[test]
    fn test_delete_old_session_embeddings() {
        let dir = tempfile::tempdir().unwrap();
        let db = DbManager::new(dir.path().join("t.db").to_str().unwrap()).unwrap();
        db.conn.execute(
            "INSERT INTO memory_embeddings(id,source,session_id,text_content,embedding,created_at)
             VALUES('old','session','s-old','old',X'00000000','2020-01-01T00:00:00Z')",
            [],
        ).unwrap();
        db.upsert_embedding("new", "session", Some("s-new"), "new", &[0.0f32])
            .unwrap();
        db.upsert_embedding("mem", "memory", None, "keep", &[0.0f32])
            .unwrap();

        db.delete_old_session_embeddings(30).unwrap();

        let n_session: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM memory_embeddings WHERE source='session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let n_memory: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM memory_embeddings WHERE source='memory'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_session, 1);
        assert_eq!(n_memory, 1);
    }

    #[test]
    fn test_check_and_update_doc_state() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = DbManager::new(&db_path).unwrap();

        // 新規ファイル → 変更あり (true)
        assert!(
            db.check_and_update_doc_state("AGENTS.md", "hash_v1")
                .unwrap()
        );
        // 同じハッシュ → 変更なし (false)
        assert!(
            !db.check_and_update_doc_state("AGENTS.md", "hash_v1")
                .unwrap()
        );
        // ハッシュ変更 → 変更あり (true)
        assert!(
            db.check_and_update_doc_state("AGENTS.md", "hash_v2")
                .unwrap()
        );
        // 別ファイル → 変更あり (true)
        assert!(
            db.check_and_update_doc_state("skills/test.md", "hash_v1")
                .unwrap()
        );
    }

    #[test]
    fn test_detect_stored_embedding_dims_empty_returns_none() {
        let db = DbManager::new(":memory:").unwrap();
        assert_eq!(db.detect_stored_embedding_dims(), None);
    }

    #[test]
    fn test_detect_stored_embedding_dims_1024() {
        let db = DbManager::new(":memory:").unwrap();
        let fake = vec![0.0f32; 1024];
        db.upsert_embedding("id1", "test", None, "hello", &fake)
            .unwrap();
        assert_eq!(db.detect_stored_embedding_dims(), Some(1024));
    }

    #[test]
    fn test_detect_stored_embedding_dims_384() {
        let db = DbManager::new(":memory:").unwrap();
        let fake = vec![0.0f32; 384];
        db.upsert_embedding("id1", "test", None, "hello", &fake)
            .unwrap();
        assert_eq!(db.detect_stored_embedding_dims(), Some(384));
    }

    #[test]
    fn test_migrate_embedding_dims_not_needed() {
        let db = DbManager::new(":memory:").unwrap();
        let fake = vec![0.0f32; 384];
        db.upsert_embedding("id1", "test", None, "text", &fake)
            .unwrap();
        // 既に 384 次元 → マイグレーション不要
        let migrated = db.migrate_embedding_dims_if_needed(384).unwrap();
        assert!(!migrated);
        assert_eq!(db.detect_stored_embedding_dims(), Some(384));
    }

    #[test]
    fn test_migrate_embedding_dims_clears_on_mismatch() {
        let db = DbManager::new(":memory:").unwrap();
        // 1024 次元のダミーデータを投入
        let fake_1024 = vec![0.0f32; 1024];
        db.upsert_embedding("id1", "test", None, "text", &fake_1024)
            .unwrap();
        db.check_and_update_doc_state("AGENTS.md", "abc123")
            .unwrap();

        // 384 次元へのマイグレーション実行
        let migrated = db.migrate_embedding_dims_if_needed(384).unwrap();
        assert!(migrated, "should return true when dims changed");
        assert_eq!(
            db.detect_stored_embedding_dims(),
            None,
            "embeddings should be cleared"
        );

        // document_states もクリアされているか確認
        let is_changed = db
            .check_and_update_doc_state("AGENTS.md", "abc123")
            .unwrap();
        assert!(
            is_changed,
            "doc state should be cleared (forcing re-ingest)"
        );
    }

    #[test]
    fn test_migrate_embedding_dims_empty_db_no_op() {
        let db = DbManager::new(":memory:").unwrap();
        // 空 DB では次元数が None → マイグレーション不要
        let migrated = db.migrate_embedding_dims_if_needed(384).unwrap();
        assert!(!migrated);
    }

    #[test]
    fn test_search_similar_with_source_returns_source() {
        let db = DbManager::new(":memory:").unwrap();
        // 単位ベクトルを 3 件投入
        let v1 = vec![1.0f32, 0.0, 0.0]; // source="doc:AGENTS.md"
        let v2 = vec![0.0f32, 1.0, 0.0]; // source="memory"
        let v3 = vec![0.9f32, 0.1, 0.0]; // source="doc:SKILL.md" (v1 に近い)
        db.upsert_embedding("id1", "doc:AGENTS.md", None, "agents text", &v1)
            .unwrap();
        db.upsert_embedding("id2", "memory", None, "memory text", &v2)
            .unwrap();
        db.upsert_embedding("id3", "doc:SKILL.md", None, "skill text", &v3)
            .unwrap();

        // クエリ = v1 方向 → v1, v3 が近い
        let query = vec![1.0f32, 0.0, 0.0];
        let results = db.search_similar_with_source(&query, 2, 0.5).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results[0].2 >= results[1].2, "sorted by score desc");
        // 上位 2 件の source に "doc:AGENTS.md" が含まれる
        let sources: Vec<&str> = results.iter().map(|(s, _, _)| s.as_str()).collect();
        assert!(sources.contains(&"doc:AGENTS.md"));
    }

    #[test]
    fn test_search_similar_with_source_threshold_filter() {
        let db = DbManager::new(":memory:").unwrap();
        let v1 = vec![1.0f32, 0.0];
        let v2 = vec![0.0f32, 1.0]; // 直交 → score = 0.0
        db.upsert_embedding("id1", "doc:A", None, "text A", &v1)
            .unwrap();
        db.upsert_embedding("id2", "doc:B", None, "text B", &v2)
            .unwrap();

        let query = vec![1.0f32, 0.0];
        let results = db.search_similar_with_source(&query, 5, 0.5).unwrap();
        // v2 は直交 (score=0) なので threshold 0.5 未満 → 除外
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "doc:A");
    }

    #[test]
    fn test_search_similar_with_decay_newer_ranks_higher() {
        let db = DbManager::new(":memory:").unwrap();
        let v = vec![1.0f32, 0.0, 0.0];
        let blob = DbManager::serialize_embedding(&v);

        // 新しいチャンク (recent)
        db.conn
            .execute(
                "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
                rusqlite::params!["new", "memory", "new text", blob.clone(), "2026-06-06T00:00:00+00:00"],
            )
            .unwrap();

        // 古いチャンク (old)
        db.conn
            .execute(
                "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
                rusqlite::params!["old", "memory", "old text", blob, "2020-01-01T00:00:00+00:00"],
            )
            .unwrap();

        // 同じクエリベクトルで検索。half_life = 30日 → 古い方は大幅ペナルティ
        let results = db
            .search_similar_with_decay(&v, 2, 0.5, 30.0)
            .unwrap();

        assert_eq!(results.len(), 2);
        // new text が先頭
        assert_eq!(results[0].1, "new text", "newer item should rank first");
        // combined_score は降順
        assert!(results[0].2 >= results[1].2, "scores should be descending");
    }

    #[test]
    fn test_search_similar_with_decay_threshold_filters() {
        let db = DbManager::new(":memory:").unwrap();
        let v1 = vec![1.0f32, 0.0];
        let v2 = vec![0.0f32, 1.0]; // 直交 → cosine = 0.0 → threshold 未満
        let blob1 = DbManager::serialize_embedding(&v1);
        let blob2 = DbManager::serialize_embedding(&v2);

        db.conn
            .execute(
                "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
                rusqlite::params!["r", "memory", "relevant", blob1, "2026-06-06T00:00:00+00:00"],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
                rusqlite::params!["u", "memory", "unrelated", blob2, "2026-06-06T00:00:00+00:00"],
            )
            .unwrap();

        let results = db
            .search_similar_with_decay(&v1, 5, 0.5, 30.0)
            .unwrap();

        // 直交ベクトルは cosine=0.0 < threshold 0.5 → 除外
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "relevant");
    }

    #[test]
    fn test_search_similar_with_decay_invalid_created_at_does_not_panic() {
        let db = DbManager::new(":memory:").unwrap();
        let v = vec![1.0f32, 0.0];
        let blob = DbManager::serialize_embedding(&v);

        db.conn
            .execute(
                "INSERT INTO memory_embeddings (id, source, session_id, text_content, embedding, created_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5)",
                rusqlite::params!["x", "memory", "text", blob, "not-a-valid-date"],
            )
            .unwrap();

        // parse 失敗 → fail-open（age_days = 0.0、decay なし）
        let results = db
            .search_similar_with_decay(&v, 5, 0.5, 30.0)
            .unwrap();

        assert_eq!(results.len(), 1, "item with invalid created_at still returned");
    }
}
