# 03. ワークスペースファイル体系・ストレージ仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-05-27  
> **対象コード**: `rustyclaw-storage` の最新実装

## 1. ワークスペース構造

エージェントのデータ、記憶、および設定は以下のツリー構成に基づいて管理されます。

```
~/.rustyclaw/
├── config.json           # 設定ファイル（atomic write）
├── .security.yml         # 暗号化シークレット（age）
├── memory.db             # SQLite WALモードデータベース
│                         #   - usage テーブル（トークン使用量）
│                         #   - patrol_state テーブル（パトロール実行時刻）
│                         #   - seen_items テーブル（既読アイテム管理）
└── workspace/            # ワークスペースルート
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
        ├── discord-C98765432-20260525.jsonl   # Discordチャンネル会話（初期優先・日付ローテ）
        ├── telegram-U12345678-20260525.jsonl  # Telegram個人会話（後回し・日付ローテ）
        ├── cron-heartbeat.jsonl               # Heartbeat実行履歴ログ
        ├── cron-flush.jsonl                   # Memory flush 履歴ログ
        ├── cron-daily-summary.jsonl           # 日次サマリーログ
        └── session-titles.json                # 各セッションのタイトル管理
```

---

## 2. セッションIDの命名規則

蓄積されるセッションログ（`sessions/*.jsonl`）のファイル名は以下の規則に従います。

```
discord-C{チャンネルID}-{YYYYMMDD}.jsonl    # Discordのチャンネル（初期優先）
telegram-U{ユーザーID}-{YYYYMMDD}.jsonl    # Telegramの個別チャット（後回し）
cron-heartbeat.jsonl                       # Heartbeat実行セッション
cron-flush.jsonl                           # メモリフラッシュセッション
cron-daily-summary.jsonl                   # 日次サマリーセッション
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

### 原子性書き込み (Atomic Write) の実装
電源断時やSSDのクラッシュ時に重要設定ファイル（`config.json` 等）の破損を防ぐため、常に一時ファイルを作成してからリネームする「原子性書き込み」パターンを採用します。

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
