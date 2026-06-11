# RustyClaw — Upstream 設計方針比較

> **関連**: [`99_hermes_agent_featured_spec.md §1.2`](99_hermes_agent_featured_spec.md)  
> **更新日**: 2026-06-11

---

## 1. PicoClaw (Go)

### 1.1 採用したもの

| 要素 | RustyClaw での実装 |
|---|---|
| Workspace ファイル体系（SOUL.md / AGENTS.md / MEMORY.md / USER.md） | そのまま流用 |
| Gateway + MessageBus + AgentLoop の 3 層構造 | Rust / Tokio で実装 |
| Pipeline の 4 ステージ（ContextBuilder → CallLLM → ExecuteTools → Publish） | そのまま実装 |
| CronService（内製スケジューラー） | 内製実装 |
| Skills システム（SKILL.md 階層ロード） | そのまま実装 |
| Health HTTP エンドポイント | `/health` / `/ready` / `/reload` として実装 |
| Hot Reload（SIGHUP で設定再読み込み） | そのまま実装 |

### 1.2 変更・代替したもの

| PicoClaw 要素 | 変更内容・理由 |
|---|---|
| ACP（子プロセス stdio JSON-RPC） | LLM を直接 HTTP 呼び出しに変更（外部依存排除） |
| Inngest スケジューラー | 内製 CronService で代替（外部サービス不要化） |
| QMD 外部プロセス（全文検索） | `tantivy`（純 Rust BM25）で内製化 |
| ACP プロセスプール | `LaneRegistry` + `Semaphore` で代替（Tokio ネイティブ） |
| Docker / Seatbelt サンドボックス | `bwrap`（Bubblewrap）で代替（軽量・root 不要） |

---

## 2. GeminiClaw (TypeScript)

### 2.1 採用したもの

| 要素 | RustyClaw での実装 |
|---|---|
| メモリ 3 層設計（短期 / 中期 / 長期） | §5 メモリ管理として実装 |
| Post-run memory flush（セッション後 LLM 抽出 → MEMORY.md） | Pipeline 完了後に非同期 kick |
| Session Continuation（日またぎの文脈引き継ぎ） | §9 ④ として実装 |
| HEARTBEAT.md による Heartbeat システム | GeminiClaw 原版を流用（§10） |
| heartbeat-digest.md の自動生成（Heartbeat pre-run） | §10.4 として実装 |
| heartbeat-state.json による各チェックの時刻管理 | SQLite `patrol_state` テーブルで管理 |
| memory/logs/YYYY-MM-DD.md（日次活動ログ） | そのまま実装 |
| Daily summary cron | そのまま実装 |
| Proactive posts 注入（自発投稿を「自分の投稿」として記録） | §9 ⑤ として実装 |
| 会話継続感を作る 6 技法 | §9 として実装 |
| `truncateWithContext`（Sliding Window 圧縮） | §5.2 として実装 |

### 2.2 採用しなかったもの

| GeminiClaw 要素 | 理由 |
|---|---|
| ACP（子プロセス stdio JSON-RPC） | LLM を直接 HTTP 呼び出しに変更 |
| Inngest スケジューラー | 外部サービス依存を排除 |
| QMD 外部プロセス | 純 Rust の tantivy で内製化 |
| ACP プロセスプール | Tokio ネイティブの LaneRegistry で代替 |
| Node.js / TypeScript スタック | Rust に統一 |

---

## 3. Hermes Agent (Nous Research)

### 3.1 採用したもの

| 要素 | RustyClaw での実装 |
|---|---|
| 自己改善 Skills（動的生成・修正） | §12 Hermes 自己改善 Skills システム |
| 3 層メモリ制約（永続的事実 / 手続き的スキル / エピソード記憶） | §5.1 3 層メモリ設計 |
| 自己監査ループ（AuditorWorker） | §12.3 振り返り監査 |
| Search & Replace パッチマージ（`PatchMerger`） | §12.4 |
| 自動生成 Skill テンプレート規格 | §12.6 |
| Skill GC（コンパクション・忘却） | §12.7 |

### 3.2 採用しなかったもの

| Hermes Agent 要素 | 理由 |
|---|---|
| エージェントループ全体の置き換え | PicoClaw ベースのループを維持しつつ選択的に統合 |
| Hermes 専用プロンプトテンプレート | RustyClaw 独自の SOUL.md / AGENTS.md 体系を維持 |
| ツール定義の Hermes 形式 | 内製 `ToolDef` 形式を維持（rig-core 統合後に再検討） |
