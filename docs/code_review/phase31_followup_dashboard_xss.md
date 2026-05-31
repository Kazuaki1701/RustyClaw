# Phase 31 Code Review 追補 — ダッシュボード DOM-XSS 横展開修正

本書は `docs/code_review/` 配下の Phase 31 各レビュー結果を**再点検**した際に発見した、
レビューの見落とし項目と、その改善実装の記録である。

## 再点検サマリ

3文書（Steps1-3 / Steps4-6 / Steps7-8）の全指摘を実コードと照合した結果：

- **全指摘は技術的に妥当**であり、過剰・誤りはなかった。
- **解決策もすべて実コードに正しく反映済み**であることを確認（index 追加・字句比較・
  DST `.earliest()`・パストラバーサルのホワイトリスト検証・`textContent` 化など）。
- ただし Steps4-6 の **DOM-XSS 修正がチャットバブル1か所に限定**されており、
  **同一クラスの未修正箇所**が残っていた（下記）。

## 発見した未修正の脆弱性

Steps4-6 では `addBubble` の `.innerHTML` → `.textContent` 化で XSS を修正したが、
同じく外部由来データを `.innerHTML` テンプレートへ補間する箇所が残存していた。
ダッシュボードは LAN 公開・認証なし（`192.168.1.12:8080`）のため実害がある。

| 箇所 | 補間値 | 由来 | 重大度 |
|---|---|---|---|
| `health.rs` ログビューア | ログ本文 `msg` | カレンダー名・メール件名・Discord 本文・LLM 応答・ツール結果 | **重要** |
| `health.rs` キューパネル | `session_id` / `description` | セッション・タスク記述 | 中 |
| `health.rs` スケジュール | `s.name` / `s.trigger_type` | cron.json ジョブ名 | 軽微 |
| `health.rs` モデル内訳 | モデル名 `m` | config / プロバイダ応答 | 軽微 |

例：`tracing::info!("Calendar resolved: '{}'...", name)` の `name` は外部カレンダー由来で、
`<img src=x onerror=...>` を含むとログビューアの `.innerHTML` で実行される。

## 改善実装

共通 `escapeHtml()` ヘルパを1個追加し、上記すべての外部由来補間箇所へ適用した。
ts / lvl / pill ラベル等の機械生成値はエスケープ不要のため対象外。

```javascript
function escapeHtml(s){return String(s??'').replace(/[&<>"']/g,
  c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))}
```

あわせて Steps7-8 で延期判断された無制限 `tokio::spawn`（GWS カレンダー解決, `lib.rs:725`）に、
将来の並列度制限方針（`Semaphore` / `buffer_unordered`）を NOTE コメントとして明記した。

## 検証

- `cargo build -p rustyclaw-gateway`：成功（既存の無関係警告のみ）
- `cargo test -p rustyclaw-gateway`：**14 passed; 0 failed**

> [!NOTE]
> Phase 31 の既存レビュー指摘はすべて妥当かつ解決済み。本追補で同一クラスの横展開漏れ
> （ダッシュボード DOM-XSS）を解消し、Phase 31 のセキュリティ対応を完結とする。
