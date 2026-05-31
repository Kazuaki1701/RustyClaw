use anyhow::{Context, Result};
use rusqlite::Connection;
use rustyclaw_providers::Message;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub mod search;
pub use search::SearchIndexManager;


// ==============================================================================
// 1. SQLite データベース管理 (DbManager)
// ==============================================================================

pub struct DbManager {
    conn: Connection,
}

impl DbManager {
    /// データベースファイルを接続し、初期化する
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref())
            .context("Failed to open SQLite database")?;

        // データベースパフォーマンスと信頼性のための PRAGMA 設定 (WALモード等)
        conn.execute_batch("
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-32000;
            PRAGMA temp_store=MEMORY;
        ")
        .context("Failed to apply SQLite PRAGMA settings")?;

        let manager = Self { conn };
        manager.create_tables()?;
        Ok(manager)
    }

    /// 初期テーブル作成（マイグレーション）
    fn create_tables(&self) -> Result<()> {
        self.conn.execute_batch("
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
        ")
        .context("Failed to create SQLite tables")?;

        // Migration: add columns for DBs created before the schema extension.
        // Each ALTER is run independently; errors (column already exists) are ignored.
        for stmt in [
            "ALTER TABLE usage ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE usage ADD COLUMN model TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE usage ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'unknown'",
            "ALTER TABLE usage ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0",
        ] {
            let _ = self.conn.execute(stmt, []);
        }

        Ok(())
    }

    // --- Usage (トークン使用量) 操作 ---
    pub fn record_usage(
        &self,
        session_id: &str,
        prompt: u32,
        completion: u32,
        total: u32,
        model: &str,
        trigger_type: &str,
        duration_ms: u64,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, total_tokens, model, trigger_type, duration_ms, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![session_id, prompt, completion, total, model, trigger_type, duration_ms as i64, now],
        )
        .context("Failed to record usage in SQLite")?;
        Ok(())
    }

    pub fn get_usage_summary(&self, since: Option<&str>) -> serde_json::Value {
        let where_clause = if since.is_some() { "WHERE created_at >= ?1" } else { "" };
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
        ) {
            if let Ok(rows) = stmt.query_map(params.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
            }) {
                for row in rows.flatten() {
                    by_model.insert(row.0, serde_json::json!({ "runs": row.1, "tokens": row.2 }));
                }
            }
        }

        serde_json::json!({
            "total_runs": total.0,
            "total_input_tokens": total.1,
            "total_completion_tokens": total.2,
            "total_tokens": total.3,
            "by_model": by_model,
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
            chrono::Utc.timestamp_opt(s, 0)
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
                    (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?),
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
        let where_clause = if since.is_some() { "WHERE created_at >= ?1" } else { "" };
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
        }).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    // --- Patrol State (Heartbeatパトロール実行時刻) 操作 ---
    pub fn update_patrol_state(&self, patrol_name: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.set_state_value(patrol_name, &now)
    }

    pub fn set_state_value(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO patrol_state (patrol_name, last_run_at) VALUES (?1, ?2)",
            (key, value),
        )
        .context("Failed to update patrol state value in SQLite")?;
        Ok(())
    }

    pub fn get_last_patrol_run(&self, patrol_name: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT last_run_at FROM patrol_state WHERE patrol_name = ?1")?;
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
        let mut stmt = self.conn.prepare("SELECT 1 FROM seen_items WHERE item_id = ?1")?;
        let exists = stmt.exists((item_id,))
            .context("Failed to query seen items in SQLite")?;
        Ok(exists)
    }
}

// ==============================================================================
// 2. 原子性書き込み (Atomic Write)
// ==============================================================================

/// 電源断やクラッシュ時にもファイル破損を防ぐ原子性書き込み
pub fn atomic_write<P: AsRef<Path>>(path: P, data: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .context("Failed to create temporary file for atomic write")?;
    
    tmp.write_all(data)
        .context("Failed to write data to temporary file")?;
    tmp.as_file().sync_all()
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
        
        let json_line = serde_json::to_string(message)
            .context("Failed to serialize message to JSON")?;

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

        let file = File::open(&file_path)
            .with_context(|| format!("Failed to open session log file for reading {:?}", file_path))?;
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
}

// ==============================================================================
// Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_manager_creation_and_basic_ops() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let db_path = tmp_dir.path().join("test_memory.db");
        
        let db = DbManager::new(&db_path)?;
        
        // Usage テスト
        db.record_usage("session-1", 100, 50, 150, "test-model", "cli", 0)?;
        
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

    #[test]
    fn test_atomic_write() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let file_path = tmp_dir.path().join("test_atomic.txt");

        atomic_write(&file_path, b"Hello Atomic")?;
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
        let db = DbManager::new(&tmp_dir.path().join("agg.db"))?;
        db.record_usage("cron:heartbeat", 100, 50, 150, "model-a", "heartbeat", 0)?;
        db.record_usage("discord-1", 200, 80, 280, "model-a", "discord", 0)?;

        let summary = db.get_usage_summary(None);
        assert_eq!(summary["total_runs"], 2);
        assert_eq!(summary["total_tokens"], 430);
        assert_eq!(summary["by_model"]["model-a"]["tokens"], 430);

        let now = chrono::Utc::now().timestamp();
        let timeline = db.get_usage_timeline(None, now, 86400);
        assert!(!timeline.is_empty());
        let day_total: i64 = timeline.iter().map(|r| r["tokens"].as_i64().unwrap_or(0)).sum();
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
        let db = DbManager::new(&tmp_dir.path().join("tl.db"))?;
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
        assert_eq!(rows[1]["tokens"], 0);   // 02時は0埋め
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
                role: if i % 2 == 0 { "user".to_string() } else { "assistant".to_string() },
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
}
