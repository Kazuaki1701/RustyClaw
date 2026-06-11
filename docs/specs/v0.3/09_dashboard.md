# RustyClaw — Web Dashboard 仕様

> [!NOTE]
> **ステータス**: `[実装済]`
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **対象コード**: `rustyclaw-gateway` 内の HealthServer およびダッシュボード実装
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 1. Web Dashboard 概要

HealthServer（ポート `8080`）は、Liveness/Readiness プローブのほかに、ブラウザ経由でシステム状態をリアルタイムに確認・管理できる Web Dashboard を提供する。

### API エンドポイント一覧

| エンドポイント | メソッド | 内容 |
|---|---|---|
| `/dashboard` または `/` | GET | ブラウザ管理 UI（シングルページ HTML） |
| `/chat` | POST | ダッシュボードからの対話（`{"message":"..."}` → テキスト応答） |
| `/health` | GET | Liveness プローブ |
| `/ready` | GET | Readiness プローブ |
| `/reload` | GET | workspace 設定のホットリロード（SIGHUP 相当） |
| `/logs/memory` | GET | `workspace/MEMORY.md` 全文 |
| `/logs/heartbeat-digest` | GET | `workspace/memory/heartbeat-digest.md` 全文 |
| `/logs/heartbeat-state` | GET | `workspace/memory/heartbeat-state.json`（pretty-print） |
| `/logs/app` | GET | アプリログ末尾 100 行 |
| `/api/queue` | GET | Pipeline Queue の現在状態（JSON） |
| `/api/neurons` | GET | Cloudflare Neurons クォータ使用状況 JSON |
| `/api/schedule` | GET | cron.json 内の有効ジョブの次回実行予定リスト（JSON） |
| `/api/concurrency` | GET | プロバイダ別のクールダウン残り時間情報（JSON） |
| `/api/usage/summary` | GET | トークン使用量の集計サマリー（JSON、パラメータ: `since`） |
| `/api/usage/timeline` | GET | トークン使用量の時間別・期間別推移データ（JSON、パラメータ: `gran`, `from`） |
| `/api/usage/by-trigger` | GET | トリガー別のトークン使用量内訳（JSON、パラメータ: `since`） |
| `/api/llm/dates` | GET | 指定カテゴリのログが存在する日付リスト（`?cat=<category>`） |
| `/api/llm/times` | GET | 指定カテゴリ・日付のログ時刻リスト（`?cat=<category>&date=<date>`） |
| `/api/llm/io` | GET | LLM API リクエスト＆レスポンス（省略時は最新ログを取得） |

---

## 2. ダッシュボード画面設計

### レイアウト構成 (`/dashboard`)

```
┌────────────────────┬──────────────────────────────────────┐
│                    │  🟡 Neurons クォータ（Cloudflare）    │
│  Chat パネル       │  🔄 Pipeline Queue                   │
│  (flex: 4)         ├────────────────────────────────────  │
│                    │  🟣 MEMORY.md                        │
│                    ├──────────────────────────────────────┤
│                    │ 🔵 LLM API INSPECTOR                 │
│                    │ （カテゴリタブ切り替え）               │
│                    ├──────────────────────────────────────┤
│                    │  🔵 アプリログ                        │
└────────────────────┴──────────────────────────────────────┘
```

### 各コンポーネントの挙動

- **MEMORY.md / heartbeat 系**: 5 秒毎に自動ポーリング
- **アプリログ**: 2 秒毎に最新ログを自動ポーリング
- **Neurons / Queue / LLM Inspector**: 5 秒毎に自動ポーリング
- **チャット機能**:
  - セッション ID は `"http-dashboard-{timestamp}"` 形式（ナノ秒精度）
  - サーバー側タイムアウト: `CHAT_TIMEOUT_SECS = 120`
  - クライアント側タイムアウト: `CHAT_TIMEOUT_MS = 120_000`（タイムアウト時は自動リトライ 1 回）
  - LLM からの最終応答を 5 分間キャッシュし、同一メッセージの再送時は即座に返却

---

## 3. セキュリティポリシー（XSS ＆ パストラバーサル防御）

LAN 公開・認証なしの前提で運用されるため、以下を徹底する。

### ① XSS 防御（DOM レンダリング方針）

- **チャットバブル**: ユーザー入力および LLM 応答テキストは `.innerHTML` を避け `.textContent` に代入、CSS で `white-space: pre-wrap` を指定
- **ログ・キュー等**: `.innerHTML` テンプレートリテラルに動的文字列を補間する場合は必ず `escapeHtml()` を通す

```javascript
function escapeHtml(str) {
    if (!str) return '';
    return str.replace(/[&<>"']/g, function(m) {
        switch (m) {
            case '&': return '&amp;';
            case '<': return '&lt;';
            case '>': return '&gt;';
            case '"': return '&quot;';
            case "'": return '&#39;';
            default: return m;
        }
    });
}
```

### ② パストラバーサル防御（ホワイトリスト検証）

`/api/llm/io` の `?cat=<category>` パラメータは、規定の用途カテゴリ（`tools`, `discord`, `memory`, `default` 等）のホワイトリストと照合し、不適合な文字列は 400 Bad Request で遮断する。

---

## 4. ダッシュボードチャットの RAG 連携

### ① heartbeat-digest.md の動的注入

ダッシュボードからのチャットセッションと検知した場合、`workspace/memory/heartbeat-digest.md` が存在すれば system プロンプトの `## Latest Heartbeat Digest` セクションとして自動注入する。

### ② dashboard_top_k の適用

設定ファイルの `embedding.dashboard_top_k`（推奨値: `8`）を RAG 検索の取得件数として適用（未設定時は `top_k: 5` にフォールバック）。

### ③ cron セッションサマリーの自動 RAG インジェスト

以下のホワイトリスト登録 cron ジョブ完了時に、実行サマリーを自動的に `memory.db` へインジェストする。

- `cron:karakeep-cleanup` / `cron:karakeep-recommendation`
- `cron:topic-patrol-explore` / `cron:topic-patrol-deliver`
- `cron:vitals-morning` / `cron:vitals-night`
- `cron:daily-briefing`

---

## 5. 将来拡張 `[将来拡張]`

### Phase 44-6: ストリーミング応答とキャッシュ

- `/chat` エンドポイントで `complete_stream` を使用し、SSE（Server-Sent Events）でトークンをリアルタイムにブラウザへストリーミングする。
- 定型レポートはローカルキャッシュ（`memory/debug/llm/cache`）に保存し、同一リクエスト時はキャッシュを返却する（キャッシュ無効化設計を慎重に行うこと）。
- **前提**: 44-1〜44-5 施策で十分な遅延削減を達成してから着手する。

### Phase 44-7: パフォーマンステストとベンチマーク

- 改修前後でレスポンスタイムスタンプを `dashboard_response_analysis.md` に記録し、**50% 以上の遅延削減**を目標として検証。
- CI にベンチマークスクリプトを追加し、毎プルリクでパフォーマンス回帰を検知する体制を整備。

### ISSUE-25: STOP 制御のセキュリティ強化

- 現在の `●ACTIVE → daemon STOP` 制御は無認証 LAN へのプロセス停止操作を露出している。
- **前提**: LAN 内認証の仕組みと、`START` 操作との非対称性（STOP のみ許可・START は不許可）のセキュリティポリシーを確定してから実装する。
