> [!NOTE]
> **ステータス**: `[ACTIVE]` (移植進捗とコードレベルの比較仕様)  
> **最終更新日**: 2026-06-04  
> **対象コード**: `crates/rustyclaw-agent/`, `crates/rustyclaw-gateway/`

# GeminiClaw vs RustyClaw コードレベル比較 & 移植進捗レポート

本ドキュメントは、TypeScript 版エージェントである **GeminiClaw** と、Rust への移植版である **RustyClaw** のアーキテクチャおよびソースコードレベルの実装差分を整理し、未移植機能（ギャップ）と今後の実装指針を記録する技術仕様・比較書である。

---

## 1. アーキテクチャおよび主要コンポーネント比較

| 比較軸 | GeminiClaw (TypeScript) | RustyClaw (Rust) | 設計上の意図・メリット |
| :--- | :--- | :--- | :--- |
| **言語・ランタイム** | Bun / Node.js (V8) | Rust (`tokio` 非同期ランタイム) | Raspberry Pi 4 (8GB) の CPU/メモリリソース最適化、シングルバイナリ化。 |
| **LLM 接続方式** | **ACP (Agent Control Protocol)**<br>Gemini CLI を stdio JSON-RPC サブプロセスとして制御 | **LlmProvider (直接 HTTP SSE)**<br>`reqwest` + `rustls` を使用した直接のステートレス接続 | 外部プロセス起動の遅延および一時ファイル・プロセスの競合によるデッドロックリスクの完全排除。 |
| **プロセス・デーモン制御** | **PM2**<br>PM2による起動・管理 | **systemd**<br>・systemdによる定常起動・ライフサイクル管理を採用 | ホストOS標準のsystemdでデーモンプロセス管理を行うため、RustyClaw自身には不要な二重実装を行わない。 |
| **並列・排他制御** | Inngest / 自作プロセスプール | `tokio::sync::Semaphore` / Lane Registry | インプロセスで完結する軽量でスレッド安全な同時実行制御。 |
| **状態永続化** | `heartbeat-state.json` (ファイル) | SQLite WAL モード (`deadpool-sqlite`) ＋ JSONL | 電源断に対する堅牢性 (atomic write + SQLite WAL) の向上。 |
| **全文検索 (RAG)** | QMD (外部プロセス) | `tantivy` (インプロセス BM25 検索) | 外部プロセス依存を排除した純 Rust によるローカル検索。 |

---

## 2. ソースコードレベルの比較詳細

### ① ContextBuilder (システムプロンプト構築)
*   **GeminiClaw (`src/agent/context-builder.ts`):**
    Gemini CLI の `@filename` 自動インポート仕様に準拠するため、`SOUL.md` や `MEMORY.md` などの参照を含む**完全に静的な `GEMINI.md`** を事前にディスクへ書き出し、実行毎の動的情報（trigger、history、directives など）のみを `-p` 引数にインジェクトする設計。
*   **RustyClaw (`crates/rustyclaw-agent/src/lib.rs`):**
    インプロセスで動的プロンプトを合成する。毎実行時に `SOUL.md`, `AGENTS.md`, `MEMORY.md`, `USER.md` を読み込み、`strip_comments` を通して `//` で始まるコメント行を除去した上でメモリ上で結合し、直接 LLM API の `system` メッセージに格納する。
    また、Heartbeat 実行時には専用の軽量コンテキスト（`SOUL.md`, `MEMORY.md`, `HEARTBEAT.md` のみ）を構築する `build_heartbeat_context` が明確に独立した関数として定義されている。

### ② Session Continuation (日またぎ文脈引き継ぎ)
*   **GeminiClaw (`src/agent/session/continuation.ts`):**
    前日の `.md` サマリーファイルの中身を**正規表現でパース**し、構造化データ（TL;DR テキストと、`## Topics` 内の `- **トピック**: 要約`）としてオブジェクトに分解した上で、再度組み立てて注入する。
    ```typescript
    const tldrMatch = content.match(/## TL;DR\n([\s\S]*?)(?=\n## |$)/);
    const topicsMatch = content.match(/## Topics\n([\s\S]*?)(?=\n## |$)/);
    if (topicsMatch?.[1]) {
        for (const m of topicsMatch[1].matchAll(/^- \*\*(.+?)\*\*:\s*(.+)$/gm)) {
            topics.push({ topic: m[1], summary: m[2] });
        }
    }
    ```
*   **RustyClaw (`crates/rustyclaw-agent/src/lib.rs` : `get_session_continuation_context`):**
    正規表現によるパースは行わず、前日の個別セッションサマリー（または `daily-summary.md`）の**全体テキストを丸ごとそのまま読み込んで結合**する。
    ```rust
    if specific_summary_path.exists() {
        if let Ok(c) = std::fs::read_to_string(&specific_summary_path) {
            summary_content = c;
        }
    }
    ```
    これにより、LLM のサマリー出力フォーマットが微妙に揺れた場合でもパースエラーにならず文脈引き継ぎ自体が成功する、シンプルかつ頑強な設計になっている。

### ③ 圧縮アルゴリズム (`truncateWithContext`)
*   **GeminiClaw (`src/agent/context-builder.ts`):**
    文字数 (`maxChars`) を基準にし、頭 70%、尾 20%、省略マーク 10% の `string.substring()` で単純に切り詰める。
*   **RustyClaw (`crates/rustyclaw-agent/src/lib.rs` : `truncate_70_20`):**
    バイト数 (`max_bytes`) を基準にする。Rust の UTF-8 文字列境界を考慮した slice 処理を行うことで、マルチバイト文字（日本語）が境界で破損してパニックするのを防ぎつつ、厳密なバイト単位制御を行っている。
    ```rust
    fn truncate_70_20(content: &str, max_bytes: usize) -> String {
        if content.len() <= max_bytes { return content.to_string(); }
        let head_end = (max_bytes as f64 * 0.7) as usize;
        let tail_len = (max_bytes as f64 * 0.2) as usize;
        let tail_start = content.len().saturating_sub(tail_len);
        let omitted = content.len() - head_end - tail_len;
        format!(
            "{}\n\n[...{} bytes omitted...]\n\n{}",
            &content[..head_end], // UTF-8境界安全性に配慮が必要
            omitted,
            &content[tail_start..],
        )
    }
    ```

---

## 3. 移植済み機能（ギャップの解消）

### 【移植完了】Proactive Posts 注入
Heartbeat が自発的に送った Discord 等のメッセージを、翌日の会話セッション開始時に「会話履歴外の自分の発言」としてシステムプロンプトに差し戻す機能を完全実装しました。これにより、自分が自発的に発言した内容の文脈を次の対話で正しく認識できるようになりました。

#### 実装仕様:
1.  **対象コード**: `crates/rustyclaw-agent/src/lib.rs` の `execute` および `execute_with_tools` 内の `process_proactive_posts` ヘルパー関数。
2.  **スキャンの仕組み**:
    *   `SessionLogger::load_history` でセッション履歴（JSONL）を読み込む。
    *   `trigger == "proactive"` (自発投稿) かつ、最後にユーザーが発言したタイムスタンプ以降に記録されたエントリーをフィルタリングして会話履歴（`history.messages`）から除外（二重参照防止）。
    *   抽出した直近 5 件の発言を以下の Markdown フォーマットで `system_context` に動的に注入する。

```markdown
### Your Previous Posts in This Channel
You posted these messages (not in your conversation history):
- [YYYY-MM-DD HH:MM:SS]: (自発発言内容の先頭300文字...)
```

### 【並行制御完了】インプロセス非同期パスロックと並行数 4 へのスケーリング (Phase 2)
同一セッション workspace ファイル（`MEMORY.md`, `USER.md` 等）への並列アクセスによる競合や上書き破損を防止するため、`rustyclaw-storage` に `once_cell` を用いた「インプロセス非同期パスロック (`PATH_LOCKS`)」を実装しました。読み込みは並行（Shared）、書き込み（排他）は自動で `atomic_write` 内にて制御されます。この安全性確保により、Gateway 内のグローバル実行セマフォ容量を `1` から `4` に安全に拡張しました。

### 【シークレット疎結合化】$vault: 動的環境変数インジェクションと多層フォールバック
カスタムスクリプト（Garminバイタル取得、KaraKeepクリーンアップ等）から、環境依存の平文シークレット解決ロジックを完全に排除し、安全なシークレット注入システムを Rust 側で実装しました。

#### 実装仕様:
1. **対象コード**: `crates/rustyclaw-tools/src/lib.rs` 内の `WorkspaceExecuteScriptTool::execute()`
2. **解決システムと多層フォールバック**:
   * `$vault:key_name` プレフィックスを検知すると、以下の優先順位で動的にシークレットを解決・デコードして環境変数に注入する。
     1. 暗号化 Vault データベース (`vault.enc`) のロードと復号。
     2. 平文の `vault.json` からのロード（下位互換フォールバック）。
     3. UNIX環境変数（キー名を大文字化・アンダースコア化したもの、例: `TEST_VAL`）からのロード。
     4. いずれも解決できない場合は安全にエラーを返して即時中断（方式Aフェイルファスト）。
   * これにより、シークレットが設定されていない環境でも安全にフォールバックし、かつディスク上に平文ファイルがなくても安全に動作します。
3. **スキルのリファクタリング**:
   * `vitals-coach` や `karakeep` スキルのローカルスクリプトから Python などの不要なシークレットパースコードを完全削除。
   * スキル定義 `SKILL.md` の `env` パラメータとして `$vault:homeassistant-token` などのバインドを定義し、トークン効率を高めつつ完全なポータビリティ化を達成。

### 【LLM 耐障害性】Per-provider クールダウン (Phase 24)

GeminiClaw には 429 エラー発生時のバックオフ機構が実装されている。RustyClaw では `GLOBAL_COOLDOWN` 変数による全体停止方式から、プロバイダ単位のクールダウン管理へ移行完了。

#### 実装仕様:
1. **対象コード**: `crates/rustyclaw-providers/src/lib.rs`
2. **変更内容**:
   - `PROVIDER_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>>` による per-provider 管理に変更。
   - `set_provider_cooldown_from_error()` / `set_provider_cooldown()` / `provider_cooldown_remaining()` の3関数を実装。
   - `GLOBAL_COOLDOWN` static 変数と旧関数群をすべて削除。
3. **ダッシュボード連携**: `health.rs` の PROVIDER COOLDOWNS パネルで残り時間を `XdXXh` / `XhXXm` / `XXmXXs` / `XXs` の段階フォーマットで表示。

---

## 4. 移植・改修実績

本調査結果に基づき、以下のタスクを完了しました：

1.  **Proactive Posts 注入の実装** (`crates/rustyclaw-agent/src/lib.rs`) ✅ 完了
    *   自発メッセージの差分ロードおよびプロンプトへの差し戻しロジック、およびユニットテストを追加。
2.  **heartbeat-digest.md の増分ロード不全・ツール対話抽出バグの改修** (`crates/rustyclaw-gateway/src/heartbeat.rs`) ✅ 完了
    *   ログ差分増分スキャンの境界タイムスタンプのバグ、およびツール呼び出し中の最終テキスト返答の抽出ロジックを修正。動的なLocalタイムスタンプ表示および構造化マークダウンヘッダー出力を追加。
3.  **tantivy 検索および Obsidian 書き込みツールの LLM 公開** (`crates/rustyclaw-tools/src/lib.rs`) ✅ 完了
    *   `MemorySearchTool` (tantivy) と `ObsidianWriteTool` の実装とツール登録、およびテストスイートを完備。
4.  **インプロセス非同期パスロックの導入と並行スロット 4 への拡張** (`crates/rustyclaw-storage/src/lib.rs`, `crates/rustyclaw-gateway/src/lib.rs`) ✅ 完了
    *   共有ファイルの並列アクセスを調停する非同期 `RwLock` マップを実装し、Gateway セマフォを容量 4 に拡張、デプロイ完了。

