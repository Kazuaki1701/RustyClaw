# デプロイ後ログ点検ガイド

> [!NOTE]
> **ステータス**: `[ACTIVE]`
> **最終更新日**: 2026-06-13
> **用途**: AGENT.md §Step 5「デプロイ後の動作確認」で参照する手順書

---

## 1. システムジャーナル確認（systemd）

```bash
# 最新 100 行を確認（本番 rp1）
ssh rp1 'journalctl --user -u rustyclaw -n 100 --no-pager'

# ERROR / WARN / panic のみ抽出
ssh rp1 'journalctl --user -u rustyclaw -n 200 --no-pager | grep -E "ERROR|WARN|panic|thread.*main"'

# リアルタイム追尾（動作確認後に Ctrl-C）
ssh rp1 'journalctl --user -u rustyclaw -f'
```

**確認ポイント**:
- `ERROR` / `panic` が 0 件であること
- サービスが `active (running)` 状態であること（`systemctl --user status rustyclaw`）

---

## 2. LLM 入出力ログ確認

セッションログは `production/workspace/memory/logs/YYYY-MM-DD.md` に日次で蓄積される。

```bash
# 本日分のログ末尾を確認
ssh rp1 "tail -100 ~/.rustyclaw/workspace/memory/logs/\$(date +%Y-%m-%d).md"

# セッション一覧
ssh rp1 'ls -lt ~/.rustyclaw/workspace/sessions/ | head -10'

# 最新セッションの内容確認
ssh rp1 'ls -t ~/.rustyclaw/workspace/sessions/ | head -1 | xargs -I{} cat ~/.rustyclaw/workspace/sessions/{}'
```

**確認ポイント**:
- 直近セッションにレスポンスが正常に記録されていること
- `[error]` や `rate_limit` の記録がないこと

---

## 3. クイックチェック（1 コマンド）

```bash
ssh rp1 'systemctl --user status rustyclaw --no-pager && journalctl --user -u rustyclaw -n 50 --no-pager | grep -c ERROR | xargs -I{} echo "ERROR count: {}"'
```

出力例（正常時）:
```
● rustyclaw.service - RustyClaw AI Agent
   Active: active (running) since ...
ERROR count: 0
```

---

## 4. 問題発生時の対応

| 症状 | 対応 |
|---|---|
| `ERROR rate_limit` | config.json の rpm/tpm 設定確認 → rate_limiter sleep が機能しているか確認 |
| `panic` / `thread 'main' panicked` | `journalctl` 全文を取得してバックトレース解析 |
| サービスが `failed` | `systemctl --user restart rustyclaw` 後に再確認。解消しない場合はバイナリを前バージョンに差し戻す |
| セッションログが空 | `workspace_dir` のパス設定確認（config.json または環境変数） |
