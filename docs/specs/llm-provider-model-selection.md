# LLM Provider / Model 選定調査

> 調査日: 2026-05-30  
> 対象: Cloudflare Workers AI / OpenRouter Free / Groq Free Tier / Hugging Face Inference

---

## 1. Provider 特性比較

| Provider | 強み | 弱み | 備考 |
|----------|------|------|------|
| **Groq** | 超高速（LPU）・高 RPD | context 131k 固定 | 対話の主力 |
| **Cloudflare** | RPM 300・RPD/TPD 制限なし・256k context (gemma) | neurons 10,000/日上限（計算量制限）| 定期処理向き |
| **OpenRouter** | 1M context・120B 規模モデル | RPD **50**（最大の制約）| 特殊用途限定 |
| **Hugging Face** | 5M tokens/月・日本語特化モデル・3.8B超軽量 | RPM 非公開（動的スロットリング）・$0.10 外部クレジット | 軽量モデル特化 |

---

## 2. レートリミット一覧

### Groq Free Tier

| model_name | model | RPM | RPD | TPM | TPD | Context |
|-----------|-------|-----|-----|-----|-----|---------|
| groq-llama-8b | llama-3.1-8b-instant | 30 | 14,400 | 6,000 | 500,000 | 131k |
| groq-llama-70b | llama-3.3-70b-versatile | 30 | 1,000 | 12,000 | 100,000 | 131k |
| groq-qwen3-32b | qwen/qwen3-32b | 60 | 1,000 | 6,000 | 500,000 | 131k |

### Cloudflare Workers AI Free Tier

制限の仕組みはリクエスト回数・トークン数ではなく **「速度」と「計算量」** の2軸:

| 項目 | 値 | 備考 |
|------|-----|------|
| RPM | **300** | 全モデル共通・1分間のリクエスト上限 |
| RPD | なし | リクエスト回数による日次制限は存在しない |
| TPM / TPD | なし | トークン数制限もない |
| Neurons/日 | **10,000** | 計算量の実質的な日次上限（全プラン共通）|

| model_name | model | RPM | Input neurons/M | Output neurons/M | Context |
|-----------|-------|-----|----------------|-----------------|---------|
| cf-qwen3-30b | @cf/qwen/qwen3-30b-a3b-fp8 | 300 | 4,625 | 30,475 | 32k |
| cf-gemma-4-26b | @cf/google/gemma-4-26b-a4b-it | 300 | 9,091 | 27,273 | 256k |
| cf-granite-micro | @cf/ibm-granite/granite-4.0-h-micro | 300 | 1,542 | 10,158 | 131k |

500 input + 500 output tokens/リクエスト想定での neurons 消費目安:

| モデル | neurons/req | 無料枠でのリクエスト数/日 |
|--------|-------------|------------------------|
| cf-granite-micro | ~5.9 | ~1,709 |
| cf-qwen3-30b | ~17.5 | ~570 |
| cf-gemma-4-26b | ~18.2 | ~550 |

### Hugging Face Inference Free Tier

制限は **2層構造** になっている点に注意:

| 層 | サービス | 無料枠 | 対象モデル |
|---|---------|--------|----------|
| Layer 1 | HF Serverless (`hf-inference`) | **5M tokens/月** | ファイルサイズ 10GB 以下（7〜14B クラス）|
| Layer 2 | Inference Providers（外部ルーター） | **$0.10/月クレジット** | 70B+ など大型モデル（Groq / Together AI 等へルーティング）|

**7〜8B の推奨モデルはすべて Layer 1 に該当する → `:hf-inference` suffix で 5M tokens/月が無料で使える**

| 項目 | 値 | 備考 |
|------|-----|------|
| 月間トークン上限 | **5,000,000** | Layer 1（hf-inference）全体の月次上限 |
| 日次換算 | ~167K tokens/日 | 500 tokens/req 換算で ~330 req/日 |
| HTTP ボディ上限 | 2MB/リクエスト | 超過時 413 エラー |
| RPM | 非公開 | 動的スロットリング（DDoS 検知時に 429）|
| RPD | なし | |
| デフォルト max_tokens | ~500 | 未指定時にカットアウトされるため **明示必須** |
| 外部プロバイダークレジット | $0.10/月 | Layer 2 のみ消費（Layer 1 には不要）|

| model_name | model | Context | 特徴 |
|-----------|-------|---------|------|
| hf-llama-3.1-8b | meta-llama/Llama-3.1-8B-Instruct:hf-inference | 128k | 汎用・安定性最高 |
| hf-qwen2.5-7b | Qwen/Qwen2.5-7B-Instruct:hf-inference | 128k | 日本語特化 |
| hf-qwen2.5-coder-7b | Qwen/Qwen2.5-Coder-7B-Instruct:hf-inference | 128k | コード生成特化 |
| hf-phi3-mini | microsoft/Phi-3-mini-128k-instruct:hf-inference | 128k | 3.8B・超軽量・高速 |

> **注意**: `:cheapest` suffix は Layer 2（$0.10クレジット消費）にルーティングされる。
> 7〜8B クラスには `:hf-inference` を使うこと。

### OpenRouter Free Tier

**完全無料（クレジットカード未登録）の制限:**

| 項目 | 値 |
|------|-----|
| RPM | 20 |
| RPD | **50** |
| TPM / TPD | 制限なし |

| model_name | model | RPM | RPD | Context |
|-----------|-------|-----|-----|---------|
| or-deepseek-v4-flash | deepseek/deepseek-v4-flash:free | 20 | 50 | 1M |
| or-minimax-m2.5 | minimax/minimax-m2.5:free | 20 | 50 | 205k |
| or-gemma-4-31b | google/gemma-4-31b-it:free | 20 | 50 | 262k |
| or-nemotron-120b | nvidia/nemotron-3-super-120b-a12b:free | 20 | 50 | 1M |
| or-gpt-oss-120b | openai/gpt-oss-120b:free | 20 | 50 | 131k |
| or-llama-3.3-free | meta-llama/llama-3.3-70b-instruct:free | 20 | 50 | 131k |

---

## 3. RustyClaw における LLM 用途一覧

| # | purpose | 呼び出し元 | 特性 | 推定頻度/日 |
|---|---------|-----------|------|------------|
| 1 | `default` | `execute` / `execute_stream` | 対話・応答速度優先 | 高（会話の都度）|
| 2 | `tools`（新設） | `execute_with_tools`（非チャンネル） | ツール呼び出し・推論優先 | 中 |
| 3 | `discord`（新設） | Discord メッセージ dispatch | 日本語会話品質＋ツール呼び出し | 中（Discord メッセージ都度）|
| 4 | `line`（新設・予約） | LINE メッセージ dispatch | 日本語特化・LINE 実装後に有効化 | 未定（実装待ち）|
| 5 | `heartbeat`（新設） | `execute_heartbeat` | 大 context・定期実行 | ~48（30分毎）|
| 6 | `summary` | `generate_session_summary` | 構造化テキスト品質優先 | 低（セッション終了時）|
| 7 | `memory` | `flush_memory` | 精度・低コスト | 低（セッション終了時）|

---

## 4. 用途別モデル割り当て（提案）

> Chat Agent ランキング参考（HF DeepSeek-R1-32B/Qwen2.5-7B 🥇 > Groq llama-8b 🥈 > CF gemma-4-26b 🥉 > OR llama-3.3-70b ❌）

| purpose | モデル | Provider | 根拠 |
|---------|--------|---------|------|
| `default` | groq-llama-8b | Groq | 応答速度最優先。RPD 14,400 で対話頻度を吸収 |
| `tools` | groq-qwen3-32b | Groq | 内部ツール実行・推論特化。RPD 1,000 を全振り可能 |
| `discord` | hf-qwen2.5-7b | HF | Chat Agent #1（完全無料・5M tokens/月）・日本語特化。7B のツール呼び出し弱点は許容 |
| `line` | hf-qwen2.5-7b | HF | discord と同モデル。LINE 実装まで予約（`enabled: false`）|
| `heartbeat` | groq-llama-8b | Groq | 48回/日の高頻度実行。CF neurons 超過を回避。default と同モデル共用 |
| `summary` | cf-gemma-4-26b | CF | 1日数回・256k context・neurons 消費小 |
| `memory` | cf-qwen3-30b | CF | 1日数回・超安価・neurons 消費小 |

### Provider 別 日次予算試算

| Provider | 用途 | 消費量/日 | 上限 | 余裕 |
|----------|------|---------|------|------|
| Groq llama-8b | default(~50K) + heartbeat(~144K) | ~194K tokens | TPD 500K | ✓ |
| Groq qwen3-32b | tools(~60K) | ~60K tokens | TPD 500K | ✓ |
| Groq llama-70b | （未使用）| — | TPD 100K | — |
| CF gemma-4-26b | summary(~177 neurons) | ~177 neurons | 10,000/日 | ✓ |
| CF qwen3-30b | memory(~150 neurons) | ~150 neurons | 10,000/日 | ✓ |
| **CF 合計** | summary + memory | **~327 neurons/日** | 10,000/日 | **余裕 97%** |
| HF qwen2.5-7b | discord(~25K tokens) | ~25K tokens/日 | ~167K/日 | ✓ |

> **Groq→HF 全面移行**: 速度劣化・5M tokens/月の予算制約・tools 品質低下の観点から不採用。現状維持。

### Hugging Face の使いどころ

5M tokens/月（≒ 330 req/日）。Discord chat が主用途:

| モデル | 用途 |
|--------|------|
| hf-qwen2.5-7b | **`discord` / `line` purpose 主力**（日本語特化・完全無料）|
| hf-phi3-mini | 超軽量・安価な summary / memory flush 代替候補 |
| hf-llama-3.1-8b | Groq llama-8b と同一モデル（Groq 障害時の最終 fallback）|
| hf-qwen2.5-coder-7b | コード生成タスク（将来の tools 拡張時）|

### OpenRouter の使いどころ

RPD=50 の制約から、定常利用には不向き。以下の特殊用途に限定する:

| モデル | 用途 |
|--------|------|
| or-deepseek-v4-flash | Heartbeat 週次深層スキャン（1M context が必要な場合のみ）|
| or-nemotron-120b | 複雑タスクの特別実行（月数回）|
| or-gemma-4-31b | Groq RPD 枯渇時の summary fallback |
| or-llama-3.3-free | Groq 完全障害時の最終 fallback |
| or-minimax-m2.5 | 保留（日本語品質未検証）|
| or-gpt-oss-120b | 保留（groq-70b と差別化要因薄）|

---

## 5. Provider 分散イメージ（典型的な1日）

```
Groq  █████████████░░░░░░░  default(対話) + heartbeat(定期48回) + tools(ツール)
HF    ████░░░░░░░░░░░░░░░░  discord(日本語チャット) + line(予約)
CF    ██░░░░░░░░░░░░░░░░░░  summary(1日数回) + memory(flush・1日数回)
OR    █░░░░░░░░░░░░░░░░░░░  深層スキャン・fallback のみ
```

---

## 6. 実装に必要な変更（Phase 19 / 未実施）

詳細タスクは `docs/task.md` の Phase 19 を参照。

1. `agents` config に `tools` / `discord` / `line`（予約）/ `heartbeat` purpose を追加
2. Discord メッセージ dispatch を `get_model("discord")` に変更
3. `execute_heartbeat()` を `get_model("heartbeat")` に変更
4. `get_model()` の fallback ロジックに全 purpose を追加（未設定時は `default`）
5. CF・HF モデルを有効化し agents 設定を反映
