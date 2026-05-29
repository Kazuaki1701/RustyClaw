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
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );

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
        Ok(())
    }

    // --- Usage (トークン使用量) 操作 ---
    pub fn record_usage(&self, session_id: &str, prompt: u32, completion: u32) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO usage (session_id, prompt_tokens, completion_tokens, created_at) VALUES (?1, ?2, ?3, ?4)",
            (session_id, prompt, completion, now),
        )
        .context("Failed to record usage in SQLite")?;
        Ok(())
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
        db.record_usage("session-1", 100, 50)?;
        
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
