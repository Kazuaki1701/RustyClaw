# LLM Provider / Model 選定調査

> 調査日: 2026-05-30（最終更新: cheahjs/free-llm-api-resources 参照）  
> 対象: Groq / Cloudflare Workers AI / OpenRouter / Hugging Face / Google AI Studio / Cerebras

---

## 1. Provider 特性比較

| Provider | 強み | 弱み | 備考 |
|----------|------|------|------|
| **Groq** | 超高速（LPU）・高 RPD（llama-8b: 14,400）| context 131k 固定 | 対話の主力 |
| **Cloudflare** | RPM 300・RPD/TPD 制限なし・256k context | neurons 10,000/日上限 | 少回数・長文向き |
| **OpenRouter** | 1M context・大型モデル無料 | RPD **50**（最大の制約）| 特殊用途限定 |
| **Hugging Face** | RPD 1,000・5M tokens/月 | hf-inference は旧世代のみ・現代 LLM は $0.10 クレジット必須・実質 context 2k〜4k | ⚠️ 戦略再検討中 |
| **Google AI Studio** ★新規 | Gemma 3 が 14,400 RPD・15K TPM・無料 | 日本から利用時データ学習対象（EU 除外）| `discord` 候補 |
| **Cerebras** ★新規 | 14,400 RPD・1M tokens/日・gpt-oss-120B 無料 | モデル数少ない | Groq の分散先候補 |

---

## 2. レートリミット一覧

### Groq Free Tier

> 出典: cheahjs/free-llm-api-resources + Groq 公式ドキュメント（2026-05-30 確認）

| model_name | model | RPM | RPD | TPM | TPD | Context | Max Completion |
|-----------|-------|-----|-----|-----|-----|---------|---------------|
| groq-llama-8b | llama-3.1-8b-instant | 30 | **14,400** | 6,000 | 500,000 | **131,072** | 131,072 |
| groq-llama-70b | llama-3.3-70b-versatile | 30 | 1,000 | 12,000 | 100,000 | **131,072** | 32,768 |
| groq-qwen3-32b | qwen/qwen3-32b | 60 | 1,000 | 6,000 | 500,000 | **131,072** | 40,960 |
| （参考）groq-llama4-scout | meta-llama/llama-4-scout-17b-16e-instruct | 30 | 1,000 | **30,000** | — | — | — |
| （参考）groq-gpt-oss-120b | openai/gpt-oss-120b | 30 | 1,000 | 8,000 | — | — | — |
| （参考）groq-gpt-oss-20b | openai/gpt-oss-20b | 30 | 1,000 | 8,000 | — | — | — |

### Cloudflare Workers AI Free Tier

制限の仕組みはリクエスト回数・トークン数ではなく **「速度」と「計算量」** の2軸:

| 項目 | 値 | 備考 |
|------|-----|------|
| RPM | **300** | 全モデル共通 |
| RPD | なし | リクエスト回数制限なし |
| TPM / TPD | なし | トークン数制限なし |
| Neurons/日 | **10,000** | 計算量の日次上限 |

> 出典: CF 公式ドキュメント pricing ページ（2026-05-30 確認）

| model_name | model | RPM | Input neurons/M | Output neurons/M | Context |
|-----------|-------|-----|----------------|-----------------|---------|
| cf-qwen3-30b | @cf/qwen/qwen3-30b-a3b-fp8 | 300 | 4,625 | 30,475 | **32,768** |
| cf-gemma-4-26b | @cf/google/gemma-4-26b-a4b-it | 300 | 9,091 | 27,273 | **256,000** |
| cf-granite-micro | @cf/ibm-granite/granite-4.0-h-micro | 300 | 1,542 | 10,158 | **131,000** |

500 input + 500 output tokens/req 想定での neurons 消費目安:

| モデル | neurons/req | 無料枠でのリクエスト数/日 |
|--------|-------------|------------------------|
| cf-granite-micro | ~5.9 | ~1,709 |
| cf-qwen3-30b | ~17.5 | ~570 |
| cf-gemma-4-26b | ~18.2 | ~550 |

### Google AI Studio Free Tier ★新規

> データ学習: **日本からの利用はデータ学習対象**（UK/CH/EEA/EU のみ除外）

OpenAI 互換エンドポイント: `https://generativelanguage.googleapis.com/v1beta/openai/`

| モデル | RPM | RPD | TPM | Context |
|--------|-----|-----|-----|---------|
| Gemma 3 27B Instruct | 30 | **14,400** | 15,000 | — |
| Gemma 3 12B Instruct | 30 | **14,400** | 15,000 | — |
| Gemma 3 4B Instruct | 30 | **14,400** | 15,000 | — |
| Gemma 3 1B Instruct | 30 | **14,400** | 15,000 | — |
| Gemini 2.5 Flash | 10 | 20 | 250,000 | — |
| Gemini 3.1 Flash-Lite | 15 | 500 | 250,000 | — |

> Groq と同等の RPD（14,400）を持つ Gemma 3 27B が注目株。`discord` purpose 候補。

### Cerebras Free Tier ★新規

OpenAI 互換エンドポイント: `https://api.cerebras.ai/v1`

| モデル | RPM | RPH | RPD | TPM | TPD |
|--------|-----|-----|-----|-----|-----|
| Llama 3.1 8B | 30 | 900 | **14,400** | 60,000 | 1,000,000 |
| gpt-oss-120b | 30 | 900 | **14,400** | 60,000 | 1,000,000 |

> Groq と同等 RPD かつ **TPM/TPD が大幅に余裕あり**。Groq の分散先として最適。

### Hugging Face Inference Free Tier

> ⚠️ **2026年実態調査により大幅修正** — 旧来の想定（hf-inference が現代 LLM に対応）は誤りだった

制限は **2層構造** だが、実用上の制約は想定より大きい:

| 層 | サービス | 無料枠 | 実際に対応するモデル |
|---|---------|--------|-----------------|
| Layer 1 | HF Serverless (`hf-inference`) | **5M tokens/月** | **旧世代モデルのみ**（BERT・GPT-2 等 CPU 推論）|
| Layer 2 | Inference Providers（外部ルーター） | **$0.10/月クレジット** | Qwen2.5・Llama 3.2 等の現代 LLM |

| 項目 | 値 | 備考 |
|------|-----|------|
| 月間トークン上限 | **5,000,000** | Layer 1 全体の月次上限 |
| RPD | **1,000** | 1日あたりの最大リクエスト数 |
| RPM | 非公開 | 動的スロットリング |
| 外部プロバイダークレジット | $0.10/月 | Layer 2 使用時のみ消費 |
| 実質 context | **2k〜4k tokens** | モデル本来の値に関わらずAPI側で切り詰め |

> 出典: HuggingFace model config.json 実測（2026-05-30）。Qwen2.5-xB は 32,768 tokens（≠ 128k）。

| model_name | model | 本来の Context | 無料 API の実質制限 | RPD |
|-----------|-------|-------------|-----------------|-----|
| hf-qwen2.5-1.5b | Qwen/Qwen2.5-1.5B-Instruct:hf-inference | **32,768** | ~2k〜4k | 1,000 |
| hf-qwen2.5-0.5b | Qwen/Qwen2.5-0.5B-Instruct:hf-inference | **32,768** | ~2k〜4k | 1,000 |
| hf-qwen2.5-coder-1.5b | Qwen/Qwen2.5-Coder-1.5B-Instruct:hf-inference | **32,768** | ~2k〜4k | 1,000 |
| hf-gemma-2-2b | google/gemma-2-2b-it:hf-inference | **8,192** | ~2k〜4k | 1,000 |
| hf-llama-3.2-3b | meta-llama/Llama-3.2-3B-Instruct:hf-inference | **131,072** | ~4k | 1,000 |

⚠️ 現代 LLM（Qwen2.5・Llama 3.2 等）は Layer 1 非対応。RustyClaw の用途（長い System Prompt + 会話履歴 + ツール呼び出し）では context 超過リスクが高く、実用困難。

### OpenRouter Free Tier

**完全無料（クレジットカード未登録）の制限:**

| 項目 | 値 | 備考 |
|------|-----|------|
| RPM | 20 | |
| RPD | **50** | $10 lifetime topup で最大 1,000/日に拡張可 |
| TPM / TPD | 制限なし | |

**現在の無料モデル一覧（2026年時点）:**

> 出典: OpenRouter API `/v1/models`（2026-05-30 実測）

| model_name（config） | model | Context | 特徴 |
|-------------------|-------|---------|------|
| or-deepseek-v4-flash | deepseek/deepseek-v4-flash:free | **1,048,576** | |
| or-minimax-m2.5 | minimax/minimax-m2.5:free | **204,800** | |
| or-gemma-4-31b | google/gemma-4-31b-it:free | **262,144** | |
| or-nemotron-120b | nvidia/nemotron-3-super-120b-a12b:free | **1,000,000** | |
| or-gpt-oss-120b | openai/gpt-oss-120b:free | **131,072** | |
| or-llama-3.3-free | meta-llama/llama-3.3-70b-instruct:free | **131,072** | |
| ★未追加 | qwen/qwen3-coder:free | **1,048,576** | Qwen3 Coder |
| ★未追加 | qwen/qwen3-next-80b-a3b-instruct:free | **262,144** | 80B MoE 無料 |
| ★未追加 | moonshotai/kimi-k2.6:free | **262,144** | |
| ★未追加 | openai/gpt-oss-20b:free | **131,072** | |
| ★未追加 | google/gemma-4-26b-a4b-it:free | **262,144** | CF と同モデル |
| ★未追加 | nvidia/nemotron-3-nano-30b-a3b:free | **256,000** | Nano 30B MoE |

---

## 3. RustyClaw における LLM 用途一覧

| # | purpose | 呼び出し元 | 特性 | 推定頻度/日 |
|---|---------|-----------|------|------------|
| 1 | `default` | `execute` / `execute_stream` | 対話・応答速度優先 | 高（会話の都度）|
| 2 | `tools` | `execute_with_tools`（非チャンネル） | ツール呼び出し・推論優先 | 中 |
| 3 | `discord` | Discord メッセージ dispatch | 日本語会話品質＋ツール呼び出し | 中（Discord メッセージ都度）|
| 4 | `line`（予約） | LINE メッセージ dispatch | 日本語特化・LINE 実装後に有効化 | 未定 |
| 5 | `heartbeat` | `execute_heartbeat` | 定期実行 | ~48（30分毎）|
| 6 | `summary` | `generate_session_summary` | 構造化テキスト品質優先 | 低（セッション終了時）|
| 7 | `memory` | `flush_memory` | 精度・低コスト | 低（セッション終了時）|

---

## 4. 用途別モデル割り当て（現状 + 再検討候補）

### 現在の暫定設定

| purpose | モデル | Provider | 状態 |
|---------|--------|---------|------|
| `default` | groq-llama-8b | Groq | ✅ 稼働中 |
| `tools` | groq-qwen3-32b | Groq | ✅ 稼働中 |
| `discord` | groq-qwen3-32b | Groq | ⚠️ 暫定（HF 戦略失敗のため）|
| `line` | hf-qwen2.5-1.5b | HF | ⏸ disabled（予約のみ）|
| `heartbeat` | groq-llama-8b | Groq | ✅ 稼働中 |
| `summary` | cf-gemma-4-26b | CF | ✅ 稼働中（429 は日次リセット）|
| `memory` | cf-qwen3-30b | CF | ✅ 稼働中（同上）|

### `discord` purpose の再検討候補

| 候補モデル | Provider | RPD | 日本語品質 | ツール呼び出し | 懸念 |
|-----------|---------|-----|-----------|-------------|------|
| groq-qwen3-32b（現状） | Groq | 1,000 | ◎ | ◎ | tools と RPD を共有 |
| **Gemma 3 27B**（Google AI Studio） | GAS | **14,400** | ◎ | ○ | 日本からのデータ学習対象 |
| **Cerebras Llama 3.1 8B** | Cerebras | **14,400** | ○ | ○ | Groq llama-8b と同一モデル |
| HF 小型モデル | HF | 1,000 | △ | △ | context 実質 2k〜4k で不向き |

### Provider 別 日次予算試算（現状）

| Provider | 用途 | 消費量/日 | 上限 | 余裕 |
|----------|------|---------|------|------|
| Groq llama-8b | default(~50K) + heartbeat(~144K) | ~194K tokens | TPD 500K | ✓ |
| Groq qwen3-32b | tools + discord | ~120K tokens | TPD 500K | ✓ |
| CF gemma-4-26b | summary (~177 neurons) | ~177 neurons | 10,000/日 | ✓ |
| CF qwen3-30b | memory (~150 neurons) | ~150 neurons | 10,000/日 | ✓ |
| **CF 合計** | | **~327 neurons/日** | 10,000/日 | **余裕 97%** |

### OpenRouter の使いどころ

RPD=50 の制約から定常利用には不向き。特殊用途に限定:

| モデル | 用途 |
|--------|------|
| or-deepseek-v4-flash | Heartbeat 週次深層スキャン（1M context が必要な場合のみ）|
| or-nemotron-120b | 複雑タスクの特別実行（月数回）|
| or-gemma-4-31b | Groq RPD 枯渇時の summary fallback |
| or-llama-3.3-free | Groq 完全障害時の最終 fallback |
| ★未検討: qwen3-next-80b | 80B MoE 無料・高品質タスク向け |

---

## 5. Provider 分散イメージ（典型的な1日・現状）

```
Groq  █████████████░░░░░░░  default(対話) + heartbeat(定期48回) + tools + discord(暫定)
CF    ██░░░░░░░░░░░░░░░░░░  summary(1日数回) + memory(flush)
OR    █░░░░░░░░░░░░░░░░░░░  深層スキャン・fallback のみ
HF    ░░░░░░░░░░░░░░░░░░░░  全モデル disabled（戦略再検討中）
GAS   ░░░░░░░░░░░░░░░░░░░░  未導入（discord 候補）
CRB   ░░░░░░░░░░░░░░░░░░░░  未導入（Groq 分散候補）
```

---

## 6. 実装済み変更（Phase 19）

- ✅ `AgentsConfig` に tools / discord / line / heartbeat purpose を追加
- ✅ `execute_heartbeat()` → `get_model("heartbeat")`
- ✅ `execute_with_tools()` に purpose 引数追加、Discord は `"discord"` を渡す
- ✅ config.json の agents に全 7 purpose を設定
- ⚠️ `discord` は HF 戦略失敗のため `groq-qwen3-32b` で暫定稼働中

## 7. 今後の検討課題

1. **`discord` purpose の最終決定**: Google AI Studio (Gemma 3 27B) または Cerebras の導入検討
2. **Google AI Studio 導入可否**: データ学習ポリシー（日本からの利用はデータ学習対象）の許容判断
3. **Cerebras 導入**: Groq のバックアップとして有力。同一モデル（Llama 3.1 8B）をより高い TPM で利用可能
4. **OpenRouter 新モデル追加**: qwen3-coder / qwen3-next-80b / kimi-k2.6 等の config 登録
5. **HF 最終方針**: $0.10 クレジット消費を許容するか、HF を断念するか
