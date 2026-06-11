# RustyClaw — データ永続化・RAG インデックス仕様

> [!NOTE]
> **ステータス**: `[実装済]`
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 11. Storage 設計 `[実装済]`

### 11.1 書き込み信頼性の分類

```
System automatic (guaranteed)          Agent-initiated (best-effort)
◄──────────────────────────────────────────────────────────────────►

sessions/*.jsonl  memory.db  MEMORY.md(flush)  summaries/  │  MEMORY.md(agent)  logs/
  fail-closed     fail-closed   fail-open       on-idle     │    voluntary       voluntary
```

- **fail-closed**: 書き込み失敗で pipeline を停止（`sessions/*.jsonl`、`memory.db`）
- **fail-open**: 失敗しても `warn` ログのみで続行（memory flush、summary 生成）
- **on-idle**: アイドル時に実行（daily summary、search index reindex）

### 11.2 SQLite 設定

```rust
conn.execute_batch("
    PRAGMA journal_mode=WAL;
    PRAGMA synchronous=NORMAL;
    PRAGMA cache_size=-32000;  -- 32MB（8GB あるので余裕）
    PRAGMA temp_store=MEMORY;
")?;
```

### 11.3 atomic write（電源断対策）

重要ファイルへの書き込みは必ず tempfile → rename パターン。

```rust
async fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
```

---

## RAG ファイル操作方針 `[実装済]`

各ファイル種別に対するプロンプト適用方式と RAG インジェスト方針の定義。

### ファイル種別対応マトリクス

| ファイルパス / パターン | プロンプト適用方式 | RAG インジェスト方針 | 親子関係 | 備考 |
|---|---|---|---|---|
| `SOUL.md` | **常時固定注入** | 対象外 | なし | 人格ブレ防止・LLM キャッシュ効率化のため先頭固定 |
| `USER.md` | 一部固定 ＋ 一部 RAG | 対象（増分） | あり | セキュリティルールは常時固定、詳細好みは RAG |
| `AGENTS.md` | 動的注入（RAG 経由） | 対象（静的読込） | あり | 必要な時のみロード |
| `MEMORY.md` | 動的注入（RAG 経由） | 対象（随時更新） | あり | インジェスト時に親子関係を構築して文脈保持 |
| `HEARTBEAT.md` | Heartbeat 実行時のみ固定 | 対象外 | なし | 通常チャット時は完全除外（トークン節約） |
| `memory/heartbeat-digest.md` | Heartbeat 実行時のみ固定 | 対象外 | なし | エフェメラル（上書き）データのため RAG 蓄積しない |
| `memory/logs/*.md` | 動的注入（RAG 経由） | 対象（差分・TTL 14日） | あり | 直近 14 日間のみ。古いものは自動プルーニング |
| `memory/summaries/*.md` | 動的注入（RAG 経由） | 対象（差分・永続） | あり | 生ログ消去後の長期文脈保持用 |
| `patrol/findings.md` | 動的注入（RAG 経由） | 対象（上書き） | なし | 最新 1 ファイルのみ維持（前回分は DELETE） |
| `skills/*.md` | 動的注入（RAG 経由） | 対象（静的読込） | あり | 引数マッチ時に仕様書全体を引き上げ |
| `docs/specs/*.md` | 対象外 | 対象外 | なし | 実運用環境では参照不要 |

### 親子チャンキング規則

RAG にインジェストするファイルのうち「親子関係あり」と定義されているものは以下の規則で `memory.db` に格納する。

1. **親チャンク**: Markdown 見出し（`##`, `###`）から次の見出しまで、または 1,000〜3,000 文字の論理的な段落ブロック。`vector`（Embedding）は生成しない（NULL）。
2. **子チャンク**: 親チャンク内の箇条書き（`- `）の各行、または 100〜300 文字の短い文。ローカル Embedding でベクトルを生成し、`parent_id` を設定して格納。
3. **引き当て**: 子チャンクがコサイン類似度 >= 0.60 でヒットした際、`parent_id` を介して親チャンクのテキスト全体をシステムプロンプトに動的注入。

### ライフサイクル & クリーンアップ規則

- **`memory/logs/*.md` のプルーニング**: ファイル作成日から 14 日以上経過したものを `ingest_session_summary` のクリーンアップバッチ時に RAG インデックス（SQLite の該当レコード）から削除する。
- **`patrol/findings.md` の上書き**: 新たな Patrol 監視結果書き込み時に前回の `source_id = 'doc:patrol/findings.md'` に紐づく全レコードを物理削除し、新規 findings のみをインジェストする。

---

## ローカル Embedding インデックス `[実装済]`

### `memory_embeddings` テーブルスキーマ

```sql
CREATE TABLE IF NOT EXISTS memory_embeddings (
    id           TEXT PRIMARY KEY,
    source       TEXT NOT NULL,
    session_id   TEXT,
    text_content TEXT NOT NULL,
    embedding    BLOB NOT NULL,    -- f32 配列（Little Endian バイト列）
    created_at   TEXT NOT NULL     -- RFC 3339 形式
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
| `channel_top_k` | `Option<usize>` | `None` | LINE / Discord / Dashboard チャット共通の RAG top-k 件数 |
| `time_decay_half_life_days` | `Option<f64>` | `None` | RAG 検索結果の時間減衰 half-life（日数）。設定時は `search_similar_with_decay` を使用。例: `30.0` → 30日で combined_score が半減 |

### インデックス更新フロー

MEMORY.md の変更は `flush_memory()` → `ingest_memory_md()` の連鎖により即時再インデックスされる（イベント駆動、バッチ不要）。セッション要約は `ingest_session_summary()` でセッション完了後に非同期インデックス登録される。
