# RustyClaw — LLM Config 制限適用 調査・設計メモ

> [!NOTE]
> **ステータス**: `[実装完了]`
> **バージョン**: v0.4
> **最終更新日**: 2026-06-12（Phase 51-1 全サブタスク完了）
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md) / [`docs/task.md`](../../task.md)

---

## 1. 背景・問題意識

### 1.1 実装前の状態（コード精査 2026-06-12）

| 問題 | 内容 |
|---|---|
| `LlmModelConfig` に制限情報なし | `context_window` / `rpm` / `tpm` 等は `ModelEntry`（raw config）にしか存在せず、パイプラインが毎回 `model_list` を再サーチしていた |
| 件数ベースの粗い履歴制限 | `get_history_message_limit()` が 30/50/80/100/120 のステップ関数——トークン推計に基づいていなかった |
| rpm / tpm が未適用 | config に定義済みだがパイプライン全体で enforce されていなかった |
| 事前トークン推計なし | system prompt + history + user message の合計を送信前に検証する仕組みがなかった |

---

## 2. Phase 51-1 実装内容（2026-06-12 完了）

### 2.1 `LlmModelConfig` フィールド追加（`rustyclaw-config`）

```rust
pub struct LlmModelConfig {
    // （既存フィールド省略）
    pub context_window_tokens: usize,  // parse_context_window() で解決済み
    pub rpm: Option<u64>,
    pub rpd: Option<u64>,
    pub tpm: Option<u64>,
    pub tpd: Option<u64>,
}
```

`resolve_model()` / `get_model()` で `ModelEntry` から確定・伝播させることにより、各利用箇所での `model_list` 再サーチを廃止。

### 2.2 `parse_context_window()` を `rustyclaw-config` に移動

`rustyclaw-agent` にあった private 関数を `pub fn` として `rustyclaw-config` に移動し、共有化。

```rust
// "131k" → 134_144、"1M" → 1_048_576、None → 4_096（デフォルト）
pub fn parse_context_window(context_window: Option<&str>) -> usize
```

### 2.3 `get_history_message_limit()` トークン予算ベースに置き換え

```rust
// 旧: ステップ関数（30/50/80/100/120）
// 新: トークン予算ベース
fn get_history_message_limit(&self, purpose: &str) -> usize {
    let cw = self.config.get_model(purpose).context_window_tokens;
    ((cw * 65 / 100) / 350).clamp(20, 150)
}
```

| context_window | 旧（件数） | 新（件数） |
|---|---|---|
| 4k (4,096) | — | 20（下限クランプ） |
| 16k | 30 | 30 |
| 32k | 50 | 60 |
| 64k | 80 | 121 |
| 131k | 100 | 150（上限クランプ） |
| 256k+ | 120 | 150（上限クランプ） |

### 2.4 `RateLimiter` 追加（`rustyclaw-agent`）

`Pipeline` に `Arc<RateLimiter>` を追加。per-model 60秒窓でリクエスト数・トークン数を追跡。

- **rpm 超過**: warn ログ + 残り時間スリープ（ソフトリミット）
- **tpm 超過**: warn ログのみ（スリープなし）

適用箇所: `execute_with_rig_agent()` / `execute_stream()`

---

## 3. context_window デフォルト値

`context_window` 未指定モデルのフォールバック値の変遷：

| バージョン | デフォルト | 根拠 |
|---|---|---|
| Phase 51-1 実装直後 | 32,768 | 旧 `parse_context_window(None)` と同値 |
| **同日修正（現在）** | **4,096** | lms-gemma-4-12b の実制限に合わせ保守的に設定 |

**方針**: 未知モデルは *過小見積もり* が安全（context overflow 防止）。config に `"context_window"` を明示すれば即座に上書き可能。

---

## 4. 小コンテキストモデル（4096 tokens）の制約分析

### 4.1 トークン収支

4096 tokens 環境での実際のバジェット内訳（推計）：

```
システムプロンプト（SOUL.md + USER.md + MEMORY.md）: ~1,500 tokens
ユーザーメッセージ:                                    ~200 tokens
max_tokens（レスポンス枠）:                          ~2,048 tokens
────────────────────────────────────────────────────────────────
合計:  ~3,748 tokens  →  履歴に使える余地: ~250 tokens（≈ 0〜1 往復）
```

### 4.2 現実装の問題点

`get_history_message_limit()` の下限クランプ 20 件は 4096 モデルでは逆効果：

```
20 件 × 350 tokens/件 ≈ 7,000 tokens  >>  4,096 tokens（モデル上限）
```

`context_window_tokens` が十分小さいとき下限を緩和する必要がある（→ §6 残課題）。

### 4.3 対応アプローチ候補

| 案 | 内容 | 実装コスト |
|---|---|---|
| **A. 静的カット** | `max_tokens` を 512〜1024 に下げ、クランプ下限を 2 まで下げる | 低 |
| **B. トークン予算アセンブリ** | `chars × 1.5` でシステムプロンプト＋履歴を実測しながら組み立て | 中〜高 |
| **C. 軽量プロンプトモード** | 小コンテキストモデルでは SOUL.md のみ注入・他省略 | 中 |

---

## 5. Purpose 分類の現状と問題点

### 5.1 現在の purpose 一覧

| purpose | トリガー | 会話履歴 | システムプロンプト |
|---|---|---|---|
| `default` | CLI / API | 必要 | フル |
| `discord` | Discord チャット | 必要 | フル |
| `line` | LINE チャット | 必要 | フル |
| `tools` | rig ツール呼出（呼び出し元依存） | 状況次第 | 不明 |
| `heartbeat` | 定時タスク | 不要 | 最小限 |
| `patrol` | 定時監視 | 不要 | 最小限 |
| `memory` | MEMORY.md 更新 | 不要 | 不要（変換） |
| `summary` | セッション要約 | 不要 | 不要（変換） |

### 5.2 問題点

1. **`tools` の位置づけが曖昧** — ツール実行がサブタスクか会話の継続かでコンテキスト要件が異なる。
2. **`discord` / `line` が `default` と実質同一** — チャンネル別モデルの割り当てには使えるが、コンテキスト管理ポリシーとしての差異がない。
3. **分類軸が「チャンネル別」** — コンテキスト管理の観点では「タスク種別」で整理した方が自然。

### 5.3 タスク種別による再分類（設計案）

```
stateful_chat  →  default / discord / line
                  （履歴必要・フルプロンプト・大容量モデル推奨）

scheduled_task →  heartbeat / patrol
                  （履歴不要・SOUL.md のみ・4k モデルで十分）

transform      →  memory / summary
                  （履歴不要・プロンプト不要・入力→出力変換のみ）
```

**結論**: 現行の purpose 分類はモデル割り当てには十分。コンテキスト管理ポリシーを自動適用するには、`context_window_tokens` 閾値ベースの判定を追加する方が purpose 数を増やさずに済む。

---

## 6. 残課題

### 6.1 ✅ 小コンテキストモデル向け下限クランプの修正（完了）

```rust
// 現状（問題あり）
((cw * 65 / 100) / 350).clamp(20, 150)

// 改善案: context_window が小さい場合は下限も縮小
let raw = (cw * 65 / 100) / 350;
let min = if cw <= 8_192 { 2 } else { 20 };
raw.clamp(min, 150)
```

### 6.2 ✅ システムプロンプトの context_window 対応（完了）

`build_system_context()` に context_window_tokens を渡し、小コンテキストモデルでは注入ファイルを絞る。

```rust
// 案: 閾値（例: 8192）以下なら SOUL.md のみ注入
if context_window_tokens <= 8_192 {
    // SOUL.md のみ
} else {
    // SOUL.md + USER.md + MEMORY.md + proactive-posts
}
```

### 6.3 トークン予算アセンブリ（低優先・v0.4 Context 最適化と統合）

`70/20/10` 戦略（`docs/specs/v0.3/02_memory.md §5.3`）の実装と統合。`ContextBuilder` が各コンポーネントの推計トークン数を合算し、予算内に収まる量だけ組み立てる。

---

## 7. 関連ファイル

| ファイル | 内容 |
|---|---|
| `crates/rustyclaw-config/src/lib.rs` | `LlmModelConfig` / `parse_context_window()` |
| `crates/rustyclaw-agent/src/lib.rs` | `RateLimiter` / `get_history_message_limit()` / `flush_memory()` |
| `docs/specs/v0.3/02_memory.md §5.3` | 70/20/10 コンテキスト戦略仕様 |
| `docs/specs/v0.4/91_context_upstream_comparison.md` | Upstream 実装比較（Heartbeat Digest・ContextBuilder 等） |
| `docs/task.md` | Phase 51-1 定義・残課題リスト |
