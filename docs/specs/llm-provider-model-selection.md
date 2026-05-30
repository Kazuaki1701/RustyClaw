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
| **Hugging Face** | RPD 1,000・5M tokens/月 | hf-inference は旧世代モデルのみ・現代 LLM は $0.10 クレジット必須・実質 context 2k〜4k | ⚠️ 戦略再検討中 |

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

> ⚠️ **2026年実態調査により大幅修正** — 旧来の想定（hf-inference が現代 LLM に対応）は誤りだった

制限は **2層構造** だが、実用上の制約は想定より大きい:

| 層 | サービス | 無料枠 | 実際に対応するモデル |
|---|---------|--------|-----------------|
| Layer 1 | HF Serverless (`hf-inference`) | **5M tokens/月** | **旧世代モデルのみ**（BERT・GPT-2 等 CPU 推論）|
| Layer 2 | Inference Providers（外部ルーター） | **$0.10/月クレジット** | Qwen2.5・Llama 3.2 等の現代 LLM |

**⚠️ `hf-inference` は現代的な instruction-tuned モデルには対応していない。**  
Qwen2.5・Llama 3.2・Gemma 2 などを使うには Layer 2（$0.10 クレジット消費）が必要。

#### Free Tier 全体の共通制限（2026年時点）

| 項目 | 値 | 備考 |
|------|-----|------|
| 月間トークン上限 | **5,000,000** | Layer 1 全体の月次上限 |
| RPD | **1,000** | 1日あたりの最大リクエスト数 |
| RPM | 非公開 | 動的スロットリング（大量連打で 429）|
| 外部プロバイダークレジット | $0.10/月 | Layer 2 使用時のみ消費 |
| HTTP ボディ上限 | 2MB/リクエスト | 超過時 413 エラー |

#### ⚠️ 実質コンテキストウィンドウ制限

Free Tier の serverless API はモデル本来のコンテキストウィンドウを **フルには使えない**。  
API 側で入力を 2k〜4k tokens 程度に切り詰める制限がある。

| model_name | model | 本来の Context | 無料 API での実質制限 | RPD |
|-----------|-------|-------------|-------------------|-----|
| hf-qwen2.5-1.5b | Qwen/Qwen2.5-1.5B-Instruct:hf-inference | 32k | **~2k〜4k** | 1,000 |
| hf-qwen2.5-0.5b | Qwen/Qwen2.5-0.5B-Instruct:hf-inference | 32k | **~2k〜4k** | 1,000 |
| hf-qwen2.5-coder-1.5b | Qwen/Qwen2.5-Coder-1.5B-Instruct:hf-inference | 32k | **~2k〜4k** | 1,000 |
| hf-gemma-2-2b | google/gemma-2-2b-it:hf-inference | 8k | **~2k〜4k** | 1,000 |
| hf-llama-3.2-3b | meta-llama/Llama-3.2-3B-Instruct:hf-inference | 128k | **~4k** | 1,000 |

> **結論**: HF Free Tier（hf-inference）は短い返答・軽量タスクにのみ向く。  
> 長い会話履歴・System Prompt・ツール呼び出しを含む RustyClaw の用途では実用上の制限が大きい。  
> `discord` purpose への採用は **モデル戦略の再検討が必要**（→ 現在 `groq-qwen3-32b` で代替中）。

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

### Hugging Face の使いどころ（⚠️ 戦略再検討中）

**実態調査で判明した制約：**
- `hf-inference` は現代 LLM 非対応（旧世代 CPU 推論専用）
- Free Tier の実質 context は 2k〜4k tokens（モデル本来の 32k〜128k ではない）
- RustyClaw の会話 + System Prompt + ツール呼び出しでは context 超過リスクが高い
- RPD 1,000 は対話用途には十分だが、短文レスポンスのみ実用的

**現状（暫定）：** `discord` purpose は `groq-qwen3-32b` で代替中

| モデル | 当初想定 | 実態 |
|--------|---------|------|
| hf-qwen2.5-1.5b | discord 主力（日本語特化） | context 制限で Discord 対話には不向き |
| hf-qwen2.5-0.5b | 超軽量 | 同上 |
| hf-qwen2.5-coder-1.5b | コード生成 | 短いコードスニペットのみ実用 |
| hf-gemma-2-2b | 日本語対話 | context 制限で不向き |
| hf-llama-3.2-3b | バランス型 | context 制限で不向き |

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
