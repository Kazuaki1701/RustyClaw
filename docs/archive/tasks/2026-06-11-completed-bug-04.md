> [!IMPORTANT]
> **ステータス**: `[HISTORICAL]` (過去の完了済みタスク)  
> **完了日**: 2026-06-09（修正コミット）/ 2026-06-11（task.md で確認・アーカイブ）  
> **備考**: 最新の動作仕様については、`docs/specs/` 配下の最新仕様書を参照してください。

# 完了済みタスク — 2026-06-11 (BUG-04)

## バグ修正 (BUG-04)

### BUG-04: bwrap サンドボックスによる gws 認証失敗（Gmail / Calendar アクセス不能）

- **発見日**: 2026-06-09 heartbeat ログ点検
- **重要度**: 🔴 高（Gmail・Google Calendar ツールが全コール失敗）

**現象**  
bwrap サンドボックス内で gws ツール呼び出し時に以下のエラーで認証失敗。

```
error[auth]: Failed to get token: Failed to set permissions on token directory
  '/home/kazuaki/.config/gws': Read-only file system (os error 30)
```

**原因**: bwrap `--ro-bind / /` によりホスト全体が読み取り専用になるため、gws がトークンキャッシュを `~/.config/gws/` に書き込めなかった。

#### BUG-04-a: gws トークンキャッシュディレクトリを writable に変更
- **完了日**: 2026-06-09
- **コミット**: `a51a499` `fix(tools): ISSUE bwrap サンドボックスで ~/.config を RW バインドし gws 認証エラーを修正`
- **概要**: bwrap コマンドに `--bind $HOME/.config $HOME/.config` を追加し、`~/.config` 配下を書き込み可能にオーバーレイ。
- **検証**: 2026-06-11 rp1 上で `gws auth status` が bwrap 内で `token_valid: true` を返すことを確認。
- **関連ファイル**:
  - `crates/rustyclaw-tools/src/lib.rs`（`WorkspaceExecuteScriptTool::call()`）
