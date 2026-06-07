# 03. ワークスペースファイル体系・ストレージ仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-06-07  
> **対象コード**: `crates/rustyclaw-storage/` の最新実装

## 1. ワークスペース構造

エージェントのデータ、記憶、および設定は以下のツリー構成に基づいて管理されます。

```
~/.rustyclaw/
├── config/                  # 設定・認証情報ディレクトリ
│   ├── config.json          # 設定ファイル（モデル・プロバイダ設定）
│   ├── vault.enc            # AES-256-GCM暗号化シークレット (優先)
│   └── vault.json           # 平文シークレットファイル (互換フォールバック)
└── workspace/               # ワークスペースルート（本番環境/開発機 production と同一）
    ├── memory.db            # SQLite WALモードデータベース
    │                        #   - usage テーブル（トークン使用量）
    │                        #   - patrol_state テーブル（状態値・パトロール実行時刻）
    │                        #   - seen_items テーブル（既読アイテム管理）
    │                        #   - memory_embeddings テーブル（長期記憶のRAG埋め込みベクトル）
    │
    │  [ 人格定義 4 ファイル (毎ターン system prompt に常時注入) ]
    ├── SOUL.md
    │   # 人格の核心、アイデンティティ、価値観、ルール
    │
    ├── AGENTS.md
    │   # 行動ルール、メモリ書き出し方針、ツール使用指針
    │
    ├── MEMORY.md
    │   # 長期記憶の核（5KB制限）。学習事項、ユーザー嗜好など
    │
    ├── USER.md
    │   # ユーザープロファイル（興味関心 Interests 含む）
    │
    │  [ Heartbeat 制御ファイル ]
    ├── HEARTBEAT.md
    │   # Heartbeat指示書（静的、自己改変不可、ユーザーのみ編集）
    │
    │  [ 自動生成される記憶・ログディレクトリ ]
    ├── memory/
        ├── heartbeat-state.json  # 各種パトロールの最終実行時刻（SQLiteと同期）
        ├── heartbeat-digest.md   # 前回Heartbeat以降の増分セッション差分ダイジェスト
        ├── logs/
        │   └── YYYY-MM-DD.md     # 日次活動ログ（Obsidian 互換 YAML frontmatter）
        ├── summaries/
        │   └── YYYY-MM-DD-{slug}.md  # セッションごとのサマリー（Continuationで使用）
        └── debug/                # デバッグ用通信ダンプディレクトリ
            ├── last_request.json  # 最新のAPI送信メッセージ生ダンプ (debug_dump時)
            └── last_response.json # 最新のAPI応答/ツールコール生ダンプ (debug_dump時)

    │
    │  [ セッション永続ログディレクトリ ]
    └── sessions/
        ├── cli-session.jsonl                  # CLI 実行時のセッション履歴
        ├── http-dashboard.jsonl               # Web Dashboard 汎用チャットセッション
        ├── http-{sessionId}.jsonl             # Web API 経由のカスタムチャットセッション
        ├── discord-C98765432-20260525.jsonl   # Discordチャンネル会話（日付ローテ）
        ├── cron-heartbeat.jsonl               # Heartbeat実行履歴ログ
        ├── cron-daily-summary.jsonl           # 日次サマリーログ
        ├── cron-session-summary-{id}.jsonl    # セッションごとの要約実行履歴ログ
        └── session-titles.json                # 各セッションのタイトル管理
```

---

## 2. セッションIDの命名規則

蓄積されるセッションログ（`sessions/*.jsonl`）のファイル名は以下の規則に従います。

```
cli-session.jsonl                          # CLI実行セッション
http-dashboard.jsonl                       # Dashboard 汎用チャットセッション
http-{UUID}.jsonl                          # API経由の個別チャットセッション
discord-C{チャンネルID}-{YYYYMMDD}.jsonl    # Discordのチャンネルチャット
cron-heartbeat.jsonl                       # Heartbeat実行セッション
cron-daily-summary.jsonl                   # 日次サマリーセッション
cron-session-summary-{session_id}.jsonl    # セッション要約実行履歴
```

---

## 3. 書き込み責任マトリクス

どのコンポーネントがどのファイルを書き換える権限・責任を持つかを厳密に定義し、不要なファイル競合や自己破壊を防ぎます。

| ファイル | ユーザー編集 | エージェント自発 | システム自動 | 自己改変禁止 |
|---|:---:|:---:|:---:|---|
| `SOUL.md` | ✓ | ✓ (変更時はユーザーに報告) | — | — |
| `AGENTS.md` | ✓ | — | — | 実質禁止 |
| `MEMORY.md` | ✓ | ✓ (重要発見時に随時) | ✓ (セッション後フラッシュ) | — |
| `USER.md` | ✓ | ✓ (プロフィール追記) | — | — |
| `HEARTBEAT.md` | ✓ | — | — | **厳禁** |
| `heartbeat-state.json` | — | ✓ (パトロール完了後) | — | — |
| `heartbeat-digest.md` | — | — | ✓ (Heartbeat pre-run時) | — |
| `logs/YYYY-MM-DD.md` | — | ✓ (任意のタイミング) | ✓ (セッション後フラッシュ) | — |
| `summaries/*.md` | — | — | ✓ (アイドル時) | — |
| `sessions/*.jsonl` | — | — | ✓ (対話毎追記) | — |

---

## 4. ストレージ書き込み信頼性スペクトラム

データの性質によって書き込みエラー時の挙動を分類し、システムの堅牢性を維持します。

```
System automatic (guaranteed)          Agent-initiated (best-effort)
◄──────────────────────────────────────────────────────────────────►
sessions/*.jsonl  memory.db  MEMORY.md(flush)  summaries/  │  MEMORY.md(agent)  logs/
  fail-closed     fail-closed   fail-open       on-idle     │    voluntary       voluntary
```

### ① fail-closed (即時停止・保護)
- **対象**: `sessions/*.jsonl` (対話ログ), `memory.db` (状態管理SQLite)
- **挙動**: 書き込みエラー発生時、パイプラインの実行を中断してエラーを上位に伝播。データが確実に保存されない限り、次の処理（配信など）に進みません。

### ② fail-open (継続優先・通知のみ)
- **対象**: `MEMORY.md` へのセッション後フラッシュ, `heartbeat-digest.md` 生成
- **挙動**: 書き込みに失敗しても、エラーログを出力した上で対話システムは正常に処理を継続します。一時的なファイルロックなどでメインの会話を妨げないための設計です。

### ③ on-idle (待機時実行・非同期)
- **対象**: `summaries/*.md` (日次サマリー生成), `SearchIndex` (全文検索インデックス更新)
- **挙動**: システムが待機状態（Idle）の時に、バックグラウンドの CronService が非同期で実行します。

---

## 5. ストレージ実装詳細

### SQLite設定
データベース接続のパフォーマンスと耐障害性を両立させるため、以下の接続設定（PRAGMA）を必須とします。

```rust
conn.execute_batch("
    PRAGMA journal_mode=WAL;         -- ライト時の読み取りブロッキング防止
    PRAGMA synchronous=NORMAL;       -- 速度と安全性のバランス
    PRAGMA cache_size=-32000;        -- 約32MBのメモリキャッシュを確保
    PRAGMA temp_store=MEMORY;        -- 一時テーブルをメモリ上に配置
")?;
```

### 原子性書き込み (Atomic Write) の実装と排他制御
電源断時やSSDのクラッシュ時に重要設定ファイル（`config.json` 等）の破損を防ぐため、常に一時ファイルを作成してからリネームする「原子性書き込み」パターンを採用します。また、`PATH_LOCKS` (in-process file RwLock map) を使って、同一ファイルの同時書き込みや衝突を防止するスレッドセーフな排他ロック制御を行います。

```rust
// crates/rustyclaw-storage/src/lib.rs
pub async fn atomic_write<P: AsRef<Path>>(path: P, data: &[u8]) -> Result<()> {
    let path = path.as_ref();
    let _guard = acquire_write_lock(path).await; // PATH_LOCKS による排他制御
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
```

---

## 6. ローカル Embedding インデックス

### `memory_embeddings` テーブルスキーマ

```sql
CREATE TABLE IF NOT EXISTS memory_embeddings (
    id         TEXT PRIMARY KEY,
    source     TEXT NOT NULL,
    session_id TEXT,
    text_content TEXT NOT NULL,
    embedding  BLOB NOT NULL,       -- f32 配列（Little Endian バイト列）
    created_at TEXT NOT NULL        -- RFC 3339 形式
);
CREATE INDEX IF NOT EXISTS idx_memory_embeddings_source
    ON memory_embeddings(source);
```

| `source` 値 | 内容 |
|---|---|
| `memory` | MEMORY.md のチャンク（`ingest_memory_md` により更新） |
| `session` | セッション要約（`ingest_session_summary` により追加、TTL 管理あり） |
| `doc:{filename}` | 任意ドキュメント（将来拡張用） |

### RAG 検索関数

| 関数 | 用途 |
|---|---|
| `search_similar_with_source(query, top_k, threshold)` | コサイン類似度のみでランキング（デフォルト） |
| `search_similar_with_decay(query, top_k, threshold, half_life_days)` | `combined_score = cosine_sim × 0.5^(age_days / half_life_days)` で時間減衰リランキング |

### `EmbeddingConfig` 設定パラメータ

| パラメータ | 型 | デフォルト | 説明 |
|---|---|---|---|
| `use_local_embedding` | `bool` | `false` | ローカル ONNX モデルで embedding を生成する |
| `local_model_path` | `Option<String>` | `None` | ONNX モデルファイルのパス |
| `discord_top_k` | `Option<usize>` | `None` | Discord RAG の top-k 件数 |
| `time_decay_half_life_days` | `Option<f64>` | `None` | RAG 検索結果の時間減衰 half-life（日数）。設定時は `search_similar_with_decay` を使用。未設定は従来挙動（後方互換）。例: `30.0` → 30日で combined_score が半減 |

### インデックス更新フロー

MEMORY.md の変更は `flush_memory()` → `ingest_memory_md()` の連鎖により即時再インデックスされます（イベント駆動、バッチ不要）。セッション要約は `ingest_session_summary()` でセッション完了後に非同期インデックス登録されます。
