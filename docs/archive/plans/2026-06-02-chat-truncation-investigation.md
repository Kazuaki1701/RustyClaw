# 【調査報告＆課題】LLM応答の途切れ（Truncation）による処理中断問題

**作成日:** 2026-06-02  
**ステータス:** 課題として起票（Open）  
**影響範囲:** RustyClaw ゲートウェイの全ユーザーセッションおよび自動化（Cron）ジョブ

---

## 1. 発生事象 (Issue Description)

2026-06-02 10:13 頃、ダッシュボードでのチャットセッション（`http-dashboard`）において、カレンダーの予定更新処理（`_AI-AGENT Test` の時間を1時間遅らせる）の実行時に、アシスタントの応答が突然途切れて処理が中断しました。

### 具体的なログと不整合
1. **チャットセッションの途切れ**:  
   [http-dashboard.jsonl](file:///home/kazuaki/Projects/RustyClaw/production/workspace/sessions/http-dashboard.jsonl) の末尾を確認したところ、以下のように思考プロセス（Thought）の末尾が `"Let'"` で終わっており、最終的なツール呼び出し（`tool_calls`）を発行しないままセッションが強制終了していました。
   ```json
   {"role":"assistant","content":"thought\nThe user wants to update... Let'","tool_calls":[],"timestamp":"2026-06-02T10:13:38...Z"}
   ```
2. **実データの未更新**:  
   カレンダーの実データ上では、`_AI-AGENT Test` イベントは `10:00〜11:00` のままであり、更新は実行されていませんでした。
3. **活動ログのハルシネーション（誤認記録）**:  
   一方で、その直後のメモリフラッシュ処理により、[2026-06-02.md](file:///home/kazuaki/Projects/RustyClaw/production/workspace/memory/logs/2026-06-02.md) には *“新しいupdate機能を利用し、開始時間を1時間遅らせて変更する作業を成功させた”* と記録されていました。アシスタント自身が「実行しようとしていた計画」を「成功した事実」として誤って日記に要約記録してしまったものと考えられます。

---

## 2. 根本原因 (Root Cause)

1. **`max_tokens` の枯渇**:  
   使用されていた LLM (Gemma 4 等の推論対応モデル) が、回答を決定するための思考プロセス (thought) を詳細に出力した結果、`config/config.json` で指定されていた `max_tokens` の制限値（**2048トークン**）に達し、出力が途中で強制的に遮断されました。
2. **自動継続機能の欠如**:  
   現状のシステム（[lib.rs:LlmResponse](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-providers/src/lib.rs#L270-L279)）には API から返される `finish_reason` を保持するフィールドが存在せず、エージェントの実行ループ（[lib.rs:execute_with_tools](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs#L930)）でも `finish_reason: length` (トークン不足による中途終了) を検知して自動で続きを再要求・レスポンス結合する仕組みがないため、そのまま中途半端な状態で処理が完了とみなされていました。

---

## 3. 対策案 (Proposed Solutions)

### 対策A. 【即効策】`max_tokens` の引き上げ (設定変更)
*   **内容**: `config/config.json` において、推論の思考プロセスが長い `cf-gemma-4-26b` やその他の主要モデルの `max_tokens` 設定を一律 `2048` から `4096`（または `8192`）に拡張する。
*   **期待効果**: ほとんどのケースでトークン不足による突然の遮断を防げる。

### 対策B. 【恒久策】`finish_reason` 検知と自動継続の実装 (コード修正)
*   **内容**: 
    1. `LlmResponse` 構造体および OpenAI/Cloudflare プロバイダレスポンス構造体に `finish_reason: Option<String>` を追加し、デシリアライズ時に取得する。
    2. エージェントの実行ループで `finish_reason == Some("length")` を検知した際、前回の部分的応答をメッセージ履歴にマージして続きを自動で再リクエスト（レスポンスの自動結合）する。
*   **期待効果**: 出力上限に達した場合でも、自動的に最後まで応答が結合され、処理の途切れやツールの未実行が完全に防止される。

### 対策C. 【プロンプト策】思考プロセスの簡潔化指示 (プロンプト調整)
*   **内容**: システムプロンプトにおいて、thought タグ内での冗長な記述を避け、思考プロセスを簡潔にまとめ、速やかにツール呼び出しに移るように明示的に制約を加える。
*   **期待効果**: 出力トークン量を削減できるが、推論能力が低下する副作用の懸念がある。

---

## 4. 課題（Todo）リスト

- [ ] **Task 1: `config.json` の `max_tokens` を 4096 に引き上げる (対策A)**
  * 対象: [config.json](file:///home/kazuaki/Projects/RustyClaw/config/config.json) の主要モデルの `max_tokens` 定義
- [ ] **Task 2: `LlmResponse` および `OpenAiChoice` への `finish_reason` 取得実装 (対策B-1)**
  * 対象: [crates/rustyclaw-providers/src/lib.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-providers/src/lib.rs)
- [ ] **Task 3: `execute_with_tools` ループ内での自動継続（レスポンス結合）ロジックの追加 (対策B-2)**
  * 対象: [crates/rustyclaw-agent/src/lib.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-agent/src/lib.rs)
