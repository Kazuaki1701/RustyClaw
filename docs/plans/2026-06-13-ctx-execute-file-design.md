# `ctx_execute_file`（ストリーム抽出によるノイズ排除）詳細設計 ＆ ユースケース計画書

> **ステータス**: `[PLANNING]`  
> **作成日**: 2026-06-13  
> **関連**: [`docs/plans/2026-06-13-context-optimization-proposal.md`](2026-06-13-context-optimization-proposal.md)  
> **案件番号**: `Phase 53-1` (将来実装候補)

---

## 1. 設計思想：なぜ `ctx_execute_file` か
従来のファイル操作（`workspace_read`）は、ファイルの中身を丸ごと LLM のコンテキスト窓（RAM）に展開する「**LLMファースト（生データ読み込み）**」でした。
本アプローチは、サンドボックス内部で軽量なパーサーやシェルスクリプトを実行し、結果だけを LLM に戻す「**コードファースト（ストリーム抽出）**」へ移行するものです。

```
【従来：LLMファースト】
[巨大なファイル (1MB)] ──(workspace_read)──► [LLM コンテキスト窓 (パンク)] ──► [LLMが探して分析]

【今回：コードファースト】
[巨大なファイル (1MB)] 
       │ (filePath 指定でバインド)
       ▼
[ctx_execute_file (Bubblewrap サンドボックス)] ◄── [フィルタスクリプト (grep/awk/python)]
       │ (実行 & 出力)
       ▼
[抽出結果 (2KB)] ────────────────────────────► [LLM コンテキスト窓 (極小)] ──► [意思決定]
```

これにより、Raspberry Pi 4 のメモリ消費を節約し、LLM の推論時間（トークン数に比例する遅延）を極限まで短縮します。

---

## 2. 具体的ユースケース ＆ スクリプト仕様

### 2.1 ユースケースA：巨大なシステムログ・障害ログのピンポイント解析
- **背景**: システム障害時、`journalctl` 出力や本番ログファイル (`rustyclaw.log`) は数十万行に達します。これを直接 LLM に読ませることは不可能です。
- **解決策**: `ctx_execute_file` にログファイルを渡し、特定のエラーパターンや指定された時間帯の前後行のみを `grep` / `sed` / `awk` で切り出して還流させます。

#### スクリプトプロトタイプ (Tool Call):
```json
{
  "name": "ctx_execute_file",
  "arguments": {
    "filePath": "production/logs/rustyclaw.log",
    "script": "grep -n -C 5 -E 'PANIC|ERROR|std::panic' \"$FILE\" | tail -n 100"
  }
}
```
*※ `context-mode` 内部で、環境変数 `$FILE` に対象ファイルのサンドボックス内マウントパスが自動で割り当てられます。*

#### 削減期待値:
- **生データサイズ**: 10 MB (約 2,500,000 トークン)
- **抽出後サイズ**: 8 KB (約 2,000 トークン)
- **コンテキスト削減率**: **99.9%**

---

### 2.2 ユースケースB：AST（抽象構文木）ベースのコード片抽出
- **背景**: 数千行のソースコード（例: `rustyclaw-gateway/src/lib.rs`）の一部をリファクタリング、または仕様確認したい場合、ファイル全体を読む必要はありません。
- **解決策**: 対象関数や構造体の定義部分だけを、`sed` または Python の AST パーサーなどを用いて抽出し、関数のシグネチャとボディのみを還流させます。

#### スクリプトプロトタイプ (Tool Call):
```json
{
  "name": "ctx_execute_file",
  "arguments": {
    "filePath": "crates/rustyclaw-gateway/src/lib.rs",
    "script": "sed -n '/pub fn build_system_context/,/^    }/p' \"$FILE\""
  }
}
```

#### 削減期待値:
- **生データサイズ**: 100 KB (2,000行 ≒ 25,000 トークン)
- **抽出後サイズ**: 2.5 KB (50行 ≒ 600 トークン)
- **コンテキスト削減率**: **97.6%**

---

### 2.3 ユースケースC：巨大 CSV / JSON データセット of インメモリ集計
- **背景**: センサーデータ履歴や家計簿データ（数万行の CSV）から、「CO2濃度が1000ppmを超えた時間帯の一覧」を取得したい場合、全データをプロンプトに入れるとトークン超過します。
- **解決策**: サンドボックス内で Python の `csv` モジュールや SQLite のインメモリ DB 機能を用いてクエリを実行し、結果（集計値やトップ10レコード）のみを還流させます。

#### スクリプトプロトタイプ (Tool Call):
```json
{
  "name": "ctx_execute_file",
  "arguments": {
    "filePath": "production/workspace/memory/ha-state-history.csv",
    "script": "python3 -c \"\nimport csv, json\nwith open('$FILE') as f:\n    reader = csv.DictReader(f)\n    # CO2が1000を超えた記録をフィルタして最新10件をJSON化\n    spikes = [r for r in reader if float(r.get('co2', 0)) > 1000.0]\n    print(json.dumps(spikes[-10:], indent=2))\n\""
  }
}
```

#### 削減期待値:
- **生データサイズ**: 500 KB (10,000行 ≒ 120,000 トークン)
- **抽出後サイズ**: 1.2 KB (JSON ≒ 300 トークン)
- **コンテキスト削減率**: **99.7%**

---

## 3. システム統合設計（RustyClaw への実装）

### 3.1 `rustyclaw-agent` ToolRegistry への登録
現在、`ExternalMcpController` で spawn された `context-mode` から自動的に MCP ツール定義が読み込まれますが、エージェント（LLM）に対して `ctx_execute_file` ツールを積極的に露出するように `AGENTS.md` の記述を更新します。

#### `AGENTS.md` への追加プロンプト例:
```markdown
- **ctx_execute_file** — ファイルに対してスクリプト（grep/awk/python等）を実行し、必要な行や統計データのみを抽出します。
  - **重要ルール**: 1,000行を超えるファイル、ログファイル、またはCSV/JSONの特定レコードを探索する場合は、`workspace_read` でファイル全体を読むのではなく、必ず `ctx_execute_file` を用いて、必要な行のみに前処理（フィルタリング）を行ってください。これにより、コンテキストの枯渇（トークン死）を防ぎます。
```

### 3.2 セキュアサンドボックスバインドの最適化
- bubblewrap による隔離実行時、処理対象の `filePath` のみが読み取り専用（`--ro-bind`）でマウントされます。
- 一時領域（`/tmp` 等）はメモリ上の `tmpfs` として隔離され、ネットワークアクセスも `--unshare-net` により完全に遮断されます。
- 万が一、LLM が外部への機密データ送信や悪意のあるホスト操作をコード内に含めても、サンドボックス環境により完全に無力化されます。

---

## 4. 将来の実装ロードマップ（Phase 53-1）

1. **Step 1 (Tool Discovery)**: `rustyclaw-agent` のテストコードで `ctx_execute_file` が MCP クライアント経由で正しく呼べることを検証。
2. **Step 2 (Prompt Instruction)**: `AGENTS.md` に `ctx_execute_file` の使用指針（1,000行以上のファイル読み込み制限）を追加。
3. **Step 3 (Evaluation)**: パトロール時に `203_ha_summary.sh` などのログ点検タスクで本ツールが自律選択され、コンテキスト量が削減されることを検証。
