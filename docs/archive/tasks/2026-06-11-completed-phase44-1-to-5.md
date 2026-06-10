> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-11  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-11 (Phase 44-1 〜 44-5)

## LLM I/O 最適化と Dashboard 遅延削減（Phase 44）

### Phase 44-1: Dashboard のタイムアウト調整
- **完了日**: 2026-06-10
- **概要**: Dashboard チャットのタイムアウトを 300 s → 120 s に短縮し、`AbortSignal.any()` で手動キャンセルとタイムアウトを統合。1回リトライとレスポンスキャッシュ（TTL 5分）を実装し、遅延時の体感を改善。
- **関連計画書**: `docs/plans/2026-06-10-phase44-1-dashboard-timeout.md`

### Phase 44-2: リクエストサイズ削減
- **完了日**: 2026-06-10
- **概要**: `build_system_context()` で SOUL.md / USER.md を 3000 文字に圧縮する `truncate_context_content()` ヘルパーを追加。`dump_llm_io` にコンパクト版 `last_request.json`（< 5 KB、コンテンツ先頭 500 文字プレビュー）の書き出しを追加。
- **関連計画書**: `docs/plans/2026-06-10-phase44-2-request-size-reduction.md`

### Phase 44-3: システムプロンプトの固定化
- **完了日**: 2026-06-11
- **概要**: `build_system_context()` から `[now: timestamp]` 注入を除去し、`execute()` / `execute_with_rig_agent()` / `execute_stream()` の呼び出し元へ移動。`build_heartbeat_context()` と同パターンを統一し、Groq Implicit Prefix Caching の効果を最大化。
- **関連計画書**: `docs/plans/2026-06-10-phase44-3-system-prompt-stabilization.md`

### Phase 44-4: ダンプロジックのプロバイダ層への集約
- **完了日**: 2026-06-11
- **概要**: エージェント層に残っていた NOP の `dump_request`/`dump_response` 関数定義と 5 つの呼び出し箇所を除去（付随する未使用変数 `initial_messages` も削除）。ダンプロジックがプロバイダ層に完全集約された。
- **関連計画書**: `docs/plans/2026-06-11-phase44-4-5-dump-cleanup.md`

### Phase 44-5: エラーハンドリングとディレクトリ作成
- **完了日**: 2026-06-11
- **概要**: `dump_llm_io` でディレクトリ作成失敗時に `tracing::error!` + early return していた箇所を `tracing::warn!` + `dated_ok` フラグに変更。dated ファイルをスキップしても `last_request.json` は必ず書き続けるよう修正。テスト `test_dump_llm_io_writes_last_request_even_when_dated_dir_fails` を追加。
- **関連計画書**: `docs/plans/2026-06-11-phase44-4-5-dump-cleanup.md`
