# Phase 43-A 設計書: RAG 最適化 Heartbeat — 旧 Context 削減策の廃止と再設計

**日付**: 2026-06-07
**案件番号**: Phase 43-A
**ステータス**: 設計承認済み

---

## 概要

RAG 導入前に実施した Context Window 削減策（MEMORY.md サイズ上限・flush Δ チェック・top_k 抑制）を Heartbeat パスから廃止し、RAG 環境下に最適化された構成に置き換える。同時に `chunk_memory_md()` のチャンク戦略を改善し、section ヘッダー付き・隣接バレット結合で RAG 検索精度を底上げする。

Dashboard・Discord パスへの展開は後続フェーズ（43-B・43-C）で実施する。

---

## 背景

| 旧施策 | 導入時の理由 | RAG 導入後の問題 |
|---|---|---|
| MEMORY.md 5000 byte 上限 | インライン全文注入時の context 爆発防止 | RAG ではチャンク単位で部分注入するため制限不要。上限により情報が損失する |
| flush 出力への `truncate_70_20` | MEMORY.md サイズ超過フェイルセーフ | 上限撤廃に伴い不要 |
| flush Δ チェック（6 msg 未満スキップ） | flush 頻度抑制 | RAG 品質はフラッシュ頻度に比例するため逆効果 |
| `heartbeat_top_k` デフォルト 2 | Heartbeat の context 過剰防止（保守値） | チャンク品質向上後は 2 では不足 |
| `chunk_memory_md` section ヘッダーなし | 実装コスト削減 | section 情報がチャンクに含まれず検索精度が低い |
| Heartbeat での USER.md 除外 | context 節約 | USER.md の関連部分を RAG 経由で召喚すれば節約不要 |

---

## アーキテクチャ

### 変更ファイル

| ファイル | 変更内容 |
|---|---|
| `crates/rustyclaw-agent/src/lib.rs` | `flush_memory`: 上限・truncate・Δ チェック撤廃 |
| `crates/rustyclaw-agent/src/lib.rs` | `chunk_memory_md`: section prefix + 隣接バレット結合 |
| `crates/rustyclaw-agent/src/lib.rs` | `execute_heartbeat`: `heartbeat_top_k` デフォルトを 3 に変更 |
| `crates/rustyclaw-agent/src/lib.rs` | `ingest_static_documents`: USER.md を RAG コーパスに追加 |

---

## 詳細設計

### 1. MEMORY.md サイズ上限の撤廃

#### 1-1. `flush_memory()` のフェイルセーフ truncate 削除

変更前（`crates/rustyclaw-agent/src/lib.rs` の flush_memory 内）:
```rust
// LLM 出力が 5000 bytes を超える場合は truncate
let final_content = if new_memory_text.len() > 5000 {
    truncate_70_20(&new_memory_text, 5000)
} else {
    new_memory_text
};
```

変更後:
```rust
let final_content = new_memory_text;
```

#### 1-2. LLM プロンプトの文字数指示を削除

flush_memory() が LLM に渡すプロンプト内の以下の 2 行を削除する:
- `"   - Stays strictly under 5KB (≤ 5000 characters)"` （ルール説明行）
- `"- Keep MEMORY.md total under 5000 characters."` （制約指示行）

---

### 2. flush Δ チェックの緩和

変更前:
```rust
// 6 メッセージ未満はスキップ
if delta_messages.len() < 6 {
    return Ok(());
}
```

変更後:
```rust
// 3 メッセージ未満はスキップ（最低限の delta を確保）
if delta_messages.len() < 3 {
    return Ok(());
}
```

時間ゲート（15 分未満の再実行スキップ）は維持する。

---

### 3. `chunk_memory_md()` の再設計

#### 現状

```rust
// bullet 行単位で分割、512 chars、section ヘッダーなし
// チャンク例: "**Identity & Mission:** 名前は Gemi..."
```

#### 新設計

**アルゴリズム:**
1. MEMORY.md を行ごとに走査する
2. `#` で始まる行を現在の `section_name` として記録する（チャンクには含めない）
3. `- ` または `* ` で始まる行をバレットとして収集する
4. 隣接するバレットを 800 chars 以内で結合する（800 chars を超える場合は新チャンクを開始）
5. 各チャンク先頭に `[{section_name}] ` プレフィクスを付与する
6. `section_name` が未設定の場合は `[General]` を使用する

**実装:**
```rust
pub fn chunk_memory_md(content: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_section = "General".to_string();
    let mut current_bullets: Vec<String> = Vec::new();
    let mut current_len = 0usize;
    const MAX_CHUNK: usize = 800;

    let flush = |section: &str, bullets: &mut Vec<String>, chunks: &mut Vec<String>| {
        if !bullets.is_empty() {
            let body = bullets.join("\n");
            chunks.push(format!("[{}] {}", section, body));
            bullets.clear();
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            // セクション切り替え: 現在のバレット群をフラッシュ
            flush(&current_section, &mut current_bullets, &mut chunks);
            current_section = trimmed.trim_start_matches('#').trim().to_string();
            if current_section.is_empty() {
                current_section = "General".to_string();
            }
            current_len = 0;
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if current_len + trimmed.len() > MAX_CHUNK && !current_bullets.is_empty() {
                // サイズ超過: 現在のチャンクをフラッシュして新チャンクを開始
                flush(&current_section, &mut current_bullets, &mut chunks);
                current_len = 0;
            }
            current_bullets.push(trimmed.to_string());
            current_len += trimmed.len() + 1;
        }
    }
    flush(&current_section, &mut current_bullets, &mut chunks);
    chunks
}
```

**チャンク例:**
```
// 変更前
"**Identity & Mission:** 名前は Gemi..."

// 変更後
"[Identity & Mission] - **Name:** 名前は Gemi...\n- **Core Role:** K様の専属個人秘書..."
```

---

### 4. `heartbeat_top_k` デフォルト値の変更

`execute_heartbeat()` 内の `unwrap_or(2)` を `unwrap_or(3)` に変更する。

groq-llama-8b の TPM=6,000 に対し、top_k=3 では 1 回あたり約 4,744 tokens（≒79%）となり、
同一分内に他の Groq 呼び出しが重なっても安全マージンを確保できる。

変更前:
```rust
let hb_top_k = self
    .config
    .embedding
    .as_ref()
    .and_then(|e| e.heartbeat_top_k)
    .unwrap_or(2);
```

変更後:
```rust
let hb_top_k = self
    .config
    .embedding
    .as_ref()
    .and_then(|e| e.heartbeat_top_k)
    .unwrap_or(3);
```

Step 2・Step 6 の専用クエリも同じ `hb_top_k` を使用するため、自動的に 3 に統一される。

---

### 5. USER.md を RAG コーパスに追加

`ingest_static_documents()` のスキャン対象に `USER.md` を追加する。

変更前（`ingest_static_documents` 冒頭のファイルリスト）:
```rust
let mut files = vec![workspace_dir.join("AGENTS.md")];
```

変更後:
```rust
let mut files = vec![workspace_dir.join("AGENTS.md"), workspace_dir.join("USER.md")];
```

**効果:** `execute_heartbeat()` の Step 2 クエリ `"user interests hobbies routine habits long-term memory"` が USER.md の関連チャンクを自然に召喚する。`build_heartbeat_context()` への直接注入は行わない。

---

## Heartbeat 実行フロー（変更後）

```
【事前処理 — 変更なし】
① generate_digest()           heartbeat-digest.md 生成
② is_step5_allowed()          声かけ許可確認
③ run_weather_patrol()        気象情報取得
④ build_heartbeat_rag_query() digest 末尾10行 → RAG クエリ
⑤ heartbeat_prompt 組立

【execute_heartbeat】
⑥ build_heartbeat_context()  SOUL.md + HEARTBEAT.md
⑦ [now: timestamp] 付与
⑧ RAG 注入 top_k=3（変更）
   ├─ クエリ①: digest末尾10行
   ├─ クエリ②: STEP2_RAG_QUERY（USER.md チャンクも召喚）
   └─ クエリ③: STEP6_RAG_QUERY
⑨ LLM 実行（Step 1〜6）

【バックグラウンド】
⑩ flush_memory()             Δ≥3 msg かつ ≥15 min（変更）
   └─ MEMORY.md 再整理（サイズ上限なし・truncate 廃止）（変更）
⑪ ingest_memory_md()         section prefix + 800 chars 結合チャンク（変更）
```

---

## 廃止施策と保持施策

### 廃止（本フェーズ対象）

| 施策 | 廃止内容 |
|---|---|
| MEMORY.md 5000 byte 上限 | `flush_memory()` の truncate 処理と LLM プロンプトの文字数指示を削除 |
| `truncate_70_20` (MEMORY.md 用) | flush 出力への適用を削除 |
| flush Δ チェック（6 msg） | 3 msg に緩和 |
| `heartbeat_top_k` デフォルト 2 | 3 に引き上げ（TPM=6,000 に対し ~79% で安全マージン確保） |

### 保持（変更なし）

| 施策 | 理由 |
|---|---|
| flush 時間ゲート（15 分） | 安全弁として継続 |
| flush Context 安全チェック（80% ガード） | トークン爆発防止として継続 |
| `get_history_message_limit` / `trim_to_last` | 会話履歴圧縮は RAG と独立 |
| `trim_heartbeat_messages` | Heartbeat の 1 世代圧縮は引き続き有効 |
| `strip_comments` | RAG と無関係、品質向上施策として継続 |
| `heartbeat-digest.md` 生成 | 時系列オリエンテーション用として維持（RAG と補完関係） |

---

## エラーハンドリング

- MEMORY.md が大幅に肥大化した場合: flush の Context 安全チェック（80% ガード）が引き続き保護する
- USER.md が存在しない場合: `ingest_static_documents()` の既存 fail-open 動作（ファイルが存在しなければスキップ）により問題なし
- chunk が空の場合: 既存の `if chunks.is_empty()` ガードがそのまま機能する

---

## テスト方針

### `rustyclaw-agent`

- `test_chunk_memory_md_section_prefix`: section ヘッダーが `[SectionName]` として付与されること
- `test_chunk_memory_md_adjacent_bullets_merged`: 隣接バレットが 800 chars 以内で結合されること
- `test_chunk_memory_md_split_on_overflow`: 800 chars 超過時に新チャンクが開始されること
- `test_chunk_memory_md_no_section`: section なしの場合に `[General]` プレフィクスが付与されること
- `test_flush_memory_no_truncation`: flush 出力が 5000 chars を超えても truncate されないこと（モック）
- 既存の `test_build_heartbeat_context_is_static` は変更なし

### 統合確認

- `cargo build --all` / `cargo test --all` / `cargo clippy --all-targets`
- Heartbeat 実行後に `memory_embeddings` テーブルで `source='memory'` のチャンクに `[SectionName]` prefix が含まれることを SQLite で確認

---

## スコープ外（後続フェーズ）

- Dashboard パスの最適化（Phase 43-B）
- Discord パスの最適化（Phase 43-C）
- `truncate_70_20` 関数自体の削除（全パス廃止後に実施）
