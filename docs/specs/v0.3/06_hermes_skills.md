# RustyClaw — Hermes 自己改善 Skills システム仕様

> [!NOTE]
> **ステータス**: `[将来拡張]`
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

---

## 12. Hermes 自己改善 Skills システム `[将来拡張]`

### 12.1 思想と位置づけ

通常の Skill が外部環境（HA・OS）を操作する道具であるのに対し、本拡張は**「エージェントが自分自身の手続き知識（`workspace/skills/self_improved/`）を直接書き換えるための高位の道具」**として定義する。
PicoClaw 標準仕様の静的 Skill 機構を内包しつつ、Hermes Agent 由来の「動的結晶化」へと独自拡張する。

### 12.2 ディレクトリ分離構造

```
workspace/skills/
├── standard/                  # 人間が記述・固定する静的 Skill
│   ├── home_assistant.md
│   └── secure_bash.md
└── self_improved/             # エージェントが自律生成・修正する動的 Skill
    ├── error_recovery_rpi4.md # ハングアップ復旧手順書（自動生成）
    └── ha_retry_handler.md    # HA 通信瞬断リトライ手順書（自動生成）
```

- **ロードフェーズ（ContextBuilder）**: `standard/` と `self_improved/` の双方から、RAG 経由で現在のタスクに適合する Markdown を Top-N 抽出し、プロンプトへ透過的にブレンド。
- **フラッシュフェーズ（Post-run）**: Publish 完了をトリガーに AuditorWorker（Lane B）が `self_improved/` のみを動的改変。

### 12.3 振り返り監査（Skill Generation）

`LoggableTool` が蓄積した実行ログを元に、AuditorWorker が以下の 3 条件を LLM に自律審査させる。

1. タスクが最終的に成功したか
2. 将来再利用できる汎用的なアプローチか
3. 複数ステップを要した複雑な知識か

条件を満たした場合、`CREATE`（新規）または `UPDATE`（差分更新）を指示する専用 JSON を出力させ、後続の PatchMerger へ渡す。

### 12.4 Search & Replace パッチマージ

LLM にファイル全体の再執筆をさせず、部分的な修正パッチ（`SEARCH/REPLACE` ブロック）を出力させ、Rust 側の `PatchMerger` でアトミックにマージする。

```text
<<<<<< SEARCH
## 陥りがちな罠と対策
- [エラー] APIトークンの期限切れによる401エラー。
======
## 陥りがちな罠と対策
- [エラー] APIトークンの期限切れによる401エラー。
- [新規追加] 接続が瞬断した場合は、10秒待って最大3回リトライすること。
>>>>>> REPLACE
```

**安全性**: 既存ファイル内に `SEARCH` ブロックの文字列が完全一致で存在しない場合、`PatchMerger` が処理を拒否する。これにより既存 Skill の破損・白紙化リスクを防ぐ。

### 12.5 隠しメタツール

通常の対話ターンからは厳格に隠蔽され、`Post-run` フェーズの AuditorWorker（Lane B）からのみ駆動される 2 つのツール。

1. **`create_new_skill(name, markdown_content)`**: `workspace/skills/self_improved/{name}.md` を新規作成。PicoClaw 互換の標準フォーマットを厳守。
2. **`patch_existing_skill(target_file, search_replace_patch)`**: PatchMerger を呼び出し、`SEARCH/REPLACE` ブロックによる安全な差分パッチマージを適用。

### 12.6 自動生成 Skill テンプレート規格

```markdown
---
id: skill_{{skill_name}}
type: self_improved
trigger_condition: "{{RAG 検索用キーワード（エラー・要求のキーワード）}}"
last_updated: {{YYYY-MM-DD}}
---
# Skill: {{スキルの明快なタイトル}}

## 1. 概要・発動シチュエーション
- この手順書は、{{エラーやハングアップ・要求}}の際に適用する。
- 目的: {{最終的に達成すべきゴール}}

## 2. 実践的実行手順 (Runbook / Recipes)
エージェントは、この状況に直面した際、以下のシーケンスを正確に再現すること。
1. `{{ツール名_1}}` を実行し、引数に `{{想定パラメータ}}` を渡す。
2. 出力に `{{特定のエラー文}}` が含まれる場合、`{{ツール名_2}}` でリカバリせよ。

## 3. 陥りがちな罠と検証チェックリスト
- [注意] {{過去の失敗ログから判明したやってはいけない操作}}
- [ ] チェック: 実行後、HAのステータスまたはログが `{{正常値}}` に戻っていることを確認する。
```

### 12.7 Skill GC（コンパクション・忘却）

内製 `CronService` の `daily-summary` cron 実行時に、LLM 自身に以下の「知識の剪定」を実行させる。

- **コンパクション**: `self_improved/` 内に類似目的の Skill が複数乱立した場合、1 つの高位 Skill へマージし古いファイルを削除。
- **忘却（デリート）**: 過去 30 日間のセッションで 1 度も RAG からヒットされなかった動的 Skill は一時的な揮発性知識とみなし、物理ファイルを自動削除してインデックスから抹消。

---

## 将来拡張（Phase 30-4）`[将来拡張]`

### ClawHub 互換の動的 Skill ダウンローダー

`rustyclaw skills install <skill-name>` サブコマンドと `workspace/skills/` へのリモート展開ロジックの実装。

- **前提**: 外部 Skill Hub（ClawHub 相当）が現時点で存在しないため保留。外部 Hub が整備された時点で着手する。
- **想定実装**: Hub URL からスキル定義 Markdown をダウンロードし、`skills/standard/` または `skills/self_improved/` へ配置して RAG インデックスを更新する。
