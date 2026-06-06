# 06. Web Dashboard・管理用 API 仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-06-06  
> **対象コード**: `rustyclaw-gateway` 内の HealthServer およびダッシュボード実装

## 1. Web Dashboard 概要

HealthServer (ポート `8080` で動作) は、Liveness/Readiness プローブのほかに、ブラウザ経由でシステム状態をリアルタイムに確認・管理できる「Web Dashboard」を提供します。

### ダッシュボード用 API エンドポイント

| エンドポイント | メソッド | 内容 |
|---|---|---|
| `/dashboard` または `/` | GET | ブラウザ管理 UI（シングルページ HTML） |
| `/chat` | POST | ダッシュボードからの対話（`{"message":"..."}` → テキスト応答） |
| `/logs/memory` | GET | `workspace/MEMORY.md` 全文 |
| `/logs/heartbeat-digest` | GET | `workspace/memory/heartbeat-digest.md` 全文 |
| `/logs/heartbeat-state` | GET | `workspace/memory/heartbeat-state.json`（pretty-print） |
| `/logs/app` | GET | `~/.rustyclaw/rustyclaw.log` 末尾 100 行 |
| `/api/queue` | GET | gmn API Pipeline Queue の現在状態（JSON） |
| `/api/neurons` | GET | Cloudflare Neurons クォータ使用状況 JSON（`neurons_used`, `quota_limit`, `remaining`, `reset_in`, `next_reset_jst`） |
| `/api/schedule` | GET | cron.json 内の有効ジョブの次回実行予定リスト（JSON） |
| `/api/concurrency` | GET | プロバイダ別（groq/cloudflare/openrouter/gmn）のクールダウン残り時間情報（JSON） |
| `/api/usage/summary` | GET | トークン使用量の集計サマリー（JSON、パラメータ: `since`） |
| `/api/usage/timeline` | GET | トークン使用量の時間別・期間別推移データ（JSON、パラメータ: `gran`, `from`） |
| `/api/usage/by-trigger` | GET | トリガー別のトークン使用量内訳（JSON、パラメータ: `since`） |
| `/api/llm/dates` | GET | 指定カテゴリ（`?cat=<category>`）の通信ログが存在する日付リスト（JSON） |
| `/api/llm/times` | GET | 指定カテゴリ・日付（`?cat=<category>&date=<date>`）の通信ログの時刻リスト（JSON） |
| `/api/llm/io` | GET | 指定カテゴリ・日付・時刻（`?cat=<category>&date=<date>&time=<time>`）のLLM APIリクエスト＆レスポンス（JSON。日時省略時は最新ログを取得） |

---

## 2. ダッシュボード画面設計

### レイアウト構成 (`/dashboard`)

```
┌────────────────────┬──────────────────────────────────────┐
│                    │  🟡 Neurons クォータ（Cloudflare）    │
│  Chat パネル       │  🔄 gmn API Pipeline Queue           │
│  (flex: 4)         ├────────────────────────────────────  │
│                    │  🟣 MEMORY.md                        │
│                    ├──────────────────────────────────────┤
│                    │ 🔵 LLM API INSPECTOR                 │
│                    │ (tools/discord/memory等11タブ切り替え)  │
│                    ├──────────────────────────────────────┤
│                    │  🔵 rustyclaw.log                    │
└────────────────────┴──────────────────────────────────────┘
```

#### 各コンポーネントの挙動:
- **MEMORY.md / heartbeat 系**: 5 秒毎に自動ポーリングしてデータを更新します。
- **App ログ (`rustyclaw.log`)**: 2 秒毎に最新ログを自動ポーリングして表示します。
- **Neurons / Queue パネル / LLM Inspector**: 5 秒毎に自動ポーリングします（`/api/neurons`・`/api/queue`・`/api/llm/io`）。
- **チャット機能**: 対話セッション ID は `"http-dashboard"` 固定として機能し、ブラウザからの入力履歴が蓄積されます。

---

## 3. セキュリティポリシー（XSS ＆ パストラバーサル防御）

ダッシュボードは LAN 公開・認証なしの前提で運用されるため、外部由来データをブラウザ上でレンダリングする際の XSS 防御、および API エンドポイントにおけるパストラバーサル防御を徹底します。

### ① XSS 防御 (DOM レンダリング方針)
外部由来のテキストデータを DOM に挿入する際は、以下のレンダリング方針を厳守します。
- **チャットバブル**: ユーザー入力および LLM の応答テキストは、直接 `.innerHTML` を使わず、必ず `.textContent` に代入し、CSS で `white-space: pre-wrap` を指定して安全に描画します。
- **ログ・キュー・スケジュール表示**: `.innerHTML` のテンプレートリテラルに動的文字列（ログ本文・`session_id`・`description`・ジョブ名・モデル名など）を補間する場合は、必ず共通の `escapeHtml()` ヘルパー関数を通して特殊文字（`& < > " '`）を安全に文字実体参照にエスケープしてから出力します。

#### HTML エスケープヘルパーの例:
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

### ② パストラバーサル防御 (ホワイトリスト検証)
- `/api/llm/io` エンドポイントがクエリパラメータ `?cat=<category>` を受け取る際、ディレクトリトラバーサルなどのパストラバーサル脆弱性を防ぐため、入力されたカテゴリ名が規定の 11 用途カテゴリ（`tools`, `discord`, `memory`, `default` など）のホワイトリストに含まれていることをサーバー側で検証し、不適合な文字列は 400 Bad Request 等で遮断します。
