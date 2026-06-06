> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の計画書・開発完了済み)  
> **完了日**: 2026-06-04  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# Completed Phase 24 — LLM 接続プロバイダ層の耐障害性（レジリエンス）強化

Groq や Cloudflare Gateway の API エラー、429制限への耐障害性を強化するため、プロバイダレベルでのクールダウン機構（GLOBAL_COOLDOWN の Per-provider 化）および残り時間表示の改善を行いました。

## 完了タスク一覧

- `[x]` **1. LLM プロバイダ層へのネットワークリトライ**
  - `complete_with_fallback()` の多段モデルチェーンが実質的に同等の役割を担っており、追加実装不要と判断。
- `[x]` **2. GLOBAL_COOLDOWN を Per-provider クールダウンへリファクタ（GLOBAL_COOLDOWN 削除）**
  - `PROVIDER_COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>>` による per-provider 管理に変更。`set_provider_cooldown_from_error()` / `set_provider_cooldown()` / `provider_cooldown_remaining()` を実装。`GLOBAL_COOLDOWN` static 変数・`set_global_cooldown_from_error()`・`global_cooldown_remaining()` およびこれらを呼び出す全7箇所を削除（`crates/rustyclaw-providers/src/lib.rs` 他）。
- `[x]` **4. PROVIDER COOLDOWNS パネルの残り時間表示フォーマット改善**
  - 従来の `XXX.Xs` 形式から、人が読みやすい段階的フォーマットに変更。
    - `XdXXh` / `XhXXm` / `XXmXXs` / `XXs`
  - `.prov-secs` 幅を 44px → 52px に拡張（最長 `XXmXXs` = 6文字対応）。
  - 対象: `crates/rustyclaw-gateway/src/health.rs`（CSS `.prov-secs` + JS `secsLabel` 生成ロジック）
- `[x]` **3. `docs/specs/09_geminiclaw_comparison.md` の最新コードとの一致確認・更新** (DoD)
