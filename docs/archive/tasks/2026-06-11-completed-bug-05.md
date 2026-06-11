> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-11（vault 登録確認・アーカイブ）  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-11 (BUG-05)

## バグ修正 (BUG-05)

### BUG-05: Obsidian トークン未設定（Obsidian ツール全機能不能）

- **発見日**: 2026-06-11 workspace/MEMORY.md 点検
- **重要度**: 🟡 中（Obsidian ノート参照・書き込みが全て不能）

**現象**  
Obsidian ツール呼び出し時に認証エラーが発生し、ノートの読み取り・書き込みが不能。

**原因**: `$vault:obsidian-api-key` が vault に未登録だった（task.md 起票時点の認識）。

#### BUG-05-a: vault への obsidian-api-key 登録
- **完了日**: 2026-06-11（vault 登録済みを確認）
- **概要**: `obsidian-api-key` は既に vault.enc に登録済みで値も設定されていた。rp1 から `http://192.168.1.2:27123/vault/` へ接続確認（HTTP 200）。Obsidian Local REST API は正常に応答。
- **対象**: `production/config/vault.enc`（`rustyclaw vault set obsidian-api-key <KEY>` で設定済み）
