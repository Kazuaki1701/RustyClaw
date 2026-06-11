# RustyClaw — ワークスペース体系 & Storage 仕様

> [!NOTE]
> **ステータス**: `[実装済]`（`skills/self_improved/` のみ `[将来拡張]`）
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 6. ワークスペースファイル体系 `[実装済]`

### 6.1 ディレクトリ構造

```
~/.rustyclaw/workspace/
├── SOUL.md           # アイデンティティ・価値観・人格の核
├── AGENTS.md         # 行動ルール・Heartbeat 応答規約・Tool 使用指針
├── MEMORY.md         # 長期記憶の核（5KB 以内厳守）
├── USER.md           # ユーザープロファイル（Interests セクション含む）
├── HEARTBEAT.md      # Heartbeat 振る舞い指示書（自己改変禁止）
├── memory/
│   ├── heartbeat-state.json   # 各チェックの最終実行時刻
│   ├── heartbeat-digest.md    # 前回 Heartbeat 以降のセッション差分ダイジェスト
│   ├── logs/
│   │   └── YYYY-MM-DD.md     # 日次活動ログ（Obsidian 互換 YAML frontmatter）
│   └── summaries/
│       └── YYYY-MM-DD-{slug}.md  # セッションサマリー（Session Continuation に使用）
├── sessions/
│   ├── discord-C{id}-YYYYMMDD.jsonl
│   ├── cron:heartbeat.jsonl
│   ├── cron:flush.jsonl
│   ├── cron:daily-summary.jsonl
│   └── session-titles.json
└── skills/
    ├── standard/              # 人間が記述する静的 Skill  [実装済]
    │   ├── home_assistant.md  # HA デバイス操作プロンプト  [将来拡張]
    │   └── secure_bash.md     # bwrap 実行基本プロンプト   [将来拡張]
    └── self_improved/         # エージェントが自律生成する動的 Skill  [将来拡張]
        └── *.md
```

```
~/.rustyclaw/
├── config/
│   ├── config.json              # 設定ファイル（追跡対象外の symlink）
│   ├── config.local-llm.json    # ローカル LLM 主力構成
│   └── config.cloud-llm.json   # クラウド LLM 主力構成
├── workspace/                   # 上記のワークスペース
└── memory.db                    # SQLite WAL モード
    # usage テーブル・patrol_state テーブル・seen_items テーブル
```

### 6.2 ファイル書き込み責任マトリクス

| ファイル | ユーザー編集 | エージェント自発 | system 自動 | 自己改変禁止 |
|---|---|---|---|---|
| `SOUL.md` | ✓ | ✓（変更時はユーザーに報告） | — | — |
| `AGENTS.md` | ✓ | — | — | 実質禁止 |
| `MEMORY.md` | ✓ | ✓（重要発見時に即時） | ✓（session 後 flush） | — |
| `USER.md` | ✓ | ✓（新情報を学んだとき） | — | — |
| `HEARTBEAT.md` | ✓ | — | — | **禁止** |
| `heartbeat-state.json` | — | ✓（Heartbeat 後に更新） | — | — |
| `heartbeat-digest.md` | — | — | ✓（pre-run 自動生成） | — |
| `logs/YYYY-MM-DD.md` | — | ✓（任意） | ✓（flush） | — |
| `summaries/*.md` | — | — | ✓（on-idle） | — |
| `sessions/*.jsonl` | — | — | ✓（fail-closed） | — |
| `skills/self_improved/*.md` `[将来拡張]` | — | ✓（AuditorWorker 経由のみ） | — | — |

### 6.3 セッション ID 命名規則

```
discord-C98765432-20260525     # チャンネル会話（日付でローテーション）
cron:heartbeat                 # Heartbeat 実行（毎回新規セッション）
cron:flush                     # Memory flush
cron:daily-summary             # 日次サマリー
```

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
