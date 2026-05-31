# 実装計画: フェーズ 22 — GeminiClaw ギャップの回復

この文書では、RustyClaw コードベースで **フェーズ 22: GeminiClaw 移植の回収（Proactive Posts / heartbeat-digest 等）** を実装するための具体的な技術分析、対応方針、コードレベルの設計、検証計画の概要を説明します。

---

## 1. 概要と目標

フェーズ 22 の目的は、以下の点で元の TypeScript アップストリーム (**GeminiClaw**) との機能同等性を回復することです。
1. **プロアクティブな投稿 (自分で開始した音声通話) プロンプト インジェクション**: Discord/Telegram から自分で開始したメッセージを後続のユーザー ターンのシステム プロンプトに再挿入し、エージェントが積極的に発言した内容を覚えられるようにします。
2. **堅牢な「heartbeat-digest.md」生成**: ハートビートの増分および詳細なアクティビティ スキャンにより、ダイアログが正しく集約され、リアルタイム タイムスタンプがフォーマットされ、アシスタントの応答をドロップすることなくツール呼び出しセッションが処理されるようにします。
3. **ネイティブ LLM ツールの公開**: Tantivy ベースの「MemorySearchTool」と Obsidian REST ベースの「ObsidianWriteTool」が完全に実装、登録され、LLM を使用できる状態になっていることを確認します。

---



＃＃＃２．１．プロアクティブポストインジェクション（No.1）

#### ギャップ:
GeminiClaw では、ハートビートがユーザーに積極的に話しかけるとき (雨が降っていることをユーザーに通知するなど)、セッション ログ エントリが `trigger: 'proactive'` で保存されます。次回のユーザー操作時に、プロンプト ビルダーはセッション JSONL をスキャンし、*最後のユーザー メッセージ以降* に送信されたプロアクティブな投稿を抽出し、生の会話履歴から除外して、専用のシステム プロンプト ブロックにフォーマットします。
```マークダウン
### このチャンネルの以前の投稿
あなたは次のメッセージを投稿しました (会話履歴にはありません):
- [YYYY-MM-DD HH:MM:SS]: (プロアクティブな投稿が切り詰められました...)
「」
RustyClaw では:
- `rustyclaw-providers` の `Message` 構造体には `trigger` フィールドや `timestamp` フィールドがありません。
- プロアクティブな投稿は、これらのメタデータ タグのない通常のアシスタント メッセージとして `sessions/<session_id>.jsonl` に追加されます。
- `execute`/`execute_with_tools` で履歴をロードする場合、プロアクティブな投稿はフィルターで除外されず、生の履歴に混ざってしまい、書式設定と LLM のパフォーマンスが中断されます。

#### 対応ポリシー:
1. **`Message` を拡張します**: オプションの `trigger` および `timestamp` フィールドを `crates/rustyclaw-providers/src/lib.rs` の `Message` 構造体に追加します。
2. **自動タイムスタンプ ロガー**: `crates/rustyclaw-storage/src/lib.rs` の `SessionLogger::append_message` を変更して、`timestamp` フィールドが `None` の場合は `chrono::Local::now().to_rfc3339()` を自動的に埋めます。
3. **プロアクティブ ハートビートにタグを付ける**: `crates/rustyclaw-gateway/src/heartbeat.rs` の `HeartbeatService::process_heartbeat_response` を更新して、追加されたアシスタントに `trigger: Some("proactive".to_string())` と `timestamp: Some(chrono::Local::now().to_rfc3339())` を明示的に設定します。メッセージ。
4. **エージェントレベルのフィルターとインジェクト**: `crates/rustyclaw-agent/src/lib.rs` を更新します (`execute` および `execute_with_tools`):
- セッション履歴内の最後のユーザー メッセージのタイムスタンプを特定します。
- プロアクティブ メッセージをフィルターで除外します (`trigger == Some("proactive")` および `timestamp > last_user_timestamp`)。
- フィルタリングされたプロアクティブ エントリ (最後の 5 件、300 文字に切り詰められたもの) を「### このチャンネルの以前の投稿」マークダウン セクションにフォーマットし、「system_context」に追加します。
- 残ったメッセージはきれいな会話履歴となり、重複を防ぎます。

---

＃＃＃２．２． `heartbeat-digest.md`点検・修正(その2)

#### ギャップ:
1. **増分スキップのバグ**: `heartbeat.rs` の `generate_digest` では、セッションが `last_run_at` 以降に変更されていない場合、たとえ `existing_digest_map` にセッションが含まれていない場合でも (例: ダイジェスト ファイルが空、欠落、または切り詰められていた場合)、ファイルの読み取りをスキップします。キーがマップにない場合は、ファイルを読み取る必要があります。
2. **ダイアログ抽出のバグ (ツール呼び出しループ)**: パーサーは、「user」の直後に「assistant」が続くという厳密な交互を想定しています。ただし、ツールを使用した実行の順序は次のとおりです。
`user` -> `assistant` (ツール呼び出し、空のコンテンツ) -> `tool` -> `assistant` (最終的なテキストコンテンツ)。
パーサーは、空のツール呼び出しアシスタント メッセージと一致し、最後のアシスタント応答をスキップして、空のダイジェストを生成して処理を進めます。
3. **ハードコードされたタイムスタンプ**: 「メッセージ」に「タイムスタンプ」が欠如しているため、生成されたダイジェストではダミーの「[--:--]」プレフィックスが使用されます。
4. **ヘッダーのフォーマット**: `# Heartbeat Digest なしでダイジェストを書き込みます

` ヘッダー。

#### 対応ポリシー:
1. **堅牢なダイアログ パーサー**: `generate_digest` 内のターン抽出プログラムを書き換えて、各 `user` プロンプトを見つけ、先読みして次のユーザー プロンプト (またはファイルの終わり) を見つけ、中間範囲をスキャンして **空ではないコンテンツを含む最後のアシスタント メッセージ**をターンの応答として抽出します。
2. **動的タイムスタンプ**: 各メッセージの「タイムスタンプ」を解析し、ローカル タイムゾーンの「[HH:MM]」としてフォーマットし、解析が失敗した場合は「[--:--]」にフォールバックします。
3. **マップ解析の安全性**: セッション名の前にあるタイムスタンプ形式 (`[XX:XX] ` または `[--:--] `) を解析する柔軟な検索により、マップ解析を強化します。
4. **正しい増分スキャン ロジック**: `!is_deep_scan && !is_modified_since_last &&existing_digest_map.contains_key(&session_id)` が完全に満たされている場合にのみ `existing_digest_map` エントリを再利用するようにループを変更します。キーがマップにない場合は、必ずファイルを読んでダイジェストが完全であることを確認してください。
5. **フォーマットされたヘッダー**: `# Heartbeat Digest を追加します

` を出力ファイルの先頭に追加します。

---

＃＃＃２．３．タンティヴィと黒曜石ツールの暴露 (その 3)

#### 現在のステータスの確認:
RustyClaw コードベースを検査すると、両方のツールがすでに完全に実装され、登録されています。
- **Tantivy Search**: `rustyclaw-tools` の `MemorySearchTool` は、`crates/rustyclaw-gateway/src/lib.rs` の 709 行目に `memory_search` という名前で登録されています。
- **Obsidian Write**: `rustyclaw-tools` の `ObsidianWriteTool` は、`crates/rustyclaw-gateway/src/lib.rs` の 687 行目に `obsidian_write_note` という名前で登録されています (`config.tools.obsidian.enabled` で有効になります)。

#### 行動計画:
- 構成構造が正しいことを確認します。
- 単体テストを通じて、両方のツールが正常に登録されていることを確認します。
- 両方のコンポーネントが完全に実装され統合されているため、No. 3 についてはさらにコードを変更する必要はありません。

---

## 3. 段階的な実装シーケンス

「人魚」
グラフTD
A["ステップ 1: メッセージ構造体にトリガー/タイムスタンプを追加する"] --> B["ステップ 2: SessionLogger にタイムスタンプを自動入力する"]
B --> C[「ステップ 3: HeartbeatService でプロアクティブな投稿にタグを付ける」]
C --> D[「ステップ 4: エージェント パイプラインにフィルタリングとプロンプト インジェクションを実装する」]
D --> E[「ステップ 5:generate_digest でダイアログ ターン抽出を書き直す」]
E --> F[「ステップ 6:generate_digest での増分マップ解析を修正する」]
F --> G[「ステップ 7: すべてのテストを実行して検証する」]
「」

### ステップ 1: `rustyclaw-providers` — `Message` 構造体を更新する
ファイル: [lib.rs](file:///home/kazuaki/Projects/RustyClaw/crates/rustyclaw-providers/src/lib.rs)

「trigger」フィールドと「timestamp」フィールドを追加します。
「錆びる」
#[派生(デバッグ、クローン、シリアル化、逆シリアル化、PartialEq、Eq、デフォルト)]
パブ構造体メッセージ {
パブの役割: 文字列、
パブのコンテンツ: 文字列、
#[serde(skip_serializing_if = "オプション::is_none")]
パブ名: Option<String>、
#[serde(skip_serializing_if = "オプション::is_none")]
pub tool_calls: Option<Vec<ToolCall>>,
#[serde(skip_serializing_if = "オプション::is_none")]
pub tool_call_id: オプション<String>、
#[serde(skip_serializing_if = "オプション::is_none")]
パブトリガー: Option<String>、
#[serde(skip_serializing_if = "オプション::is_none")]
パブのタイムスタンプ: Option<String>、
}
「」

