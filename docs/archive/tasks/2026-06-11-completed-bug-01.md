> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-11  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-11 (BUG-01)

## バグ修正 (BUG-01)

### BUG-01: LLM 全モデル失敗（`all models failed`）による深夜 Agent 停止

- **発見日**: 2026-06-11 ログ点検
- **重要度**: 🔴 高（heartbeat 停止・4セッションサマリー未保存）

**現象**  
深夜 02:21〜05:51（JST）にかけて heartbeat・summary パーパス向けの全 LLM モデルが連続失敗し、以下 5 セッションが実行不能になった。

- `02:21` cron:heartbeat
- `02:23` cron:session-summary:cron:topic-patrol-explore
- `04:03` cron:session-summary:cron:karakeep-cleanup
- `04:49` cron:session-summary:cron:karakeep-recommendation
- `05:51` cron:session-summary:cron:topic-patrol-deliver

**原因**: Groq / Cloudflare Workers AI の外部プロバイダー障害。フォールバック先が 2 モデルしかなく、両方が同一時間帯に障害を受けた。

#### BUG-01-a: config.local-llm.json 全 purpose にフォールバック追加
- **完了日**: 2026-06-11
- **概要**: `config.local-llm.json` の全 purpose を lms-* 主力 + groq フォールバック構成に変更。`global_fallback_model_name` を `groq-llama-8b` に更新。`default` / `tools` も配列化しフォールバックを追加。
- **関連ファイル**:
  - `production/config/config.local-llm.json`

#### BUG-01-b: 全モデル失敗時の Discord アラート実装
- **完了日**: 2026-06-11
- **概要**: heartbeat 失敗パス（non-rate-limit）に `SystemEvent::SystemError` publish を追加。`Gateway::run()` に `SystemError` subscriber ループを追加し、"all models failed" を含むエラーを Discord home channel へ `⚠️ LLM 全モデル失敗` アラートとして通知。
- **関連ファイル**:
  - `crates/rustyclaw-gateway/src/lib.rs`

#### BUG-01-c: 過去の失敗セッションサマリー確認
- **対応**: 不要（skip）
