# RustyClaw ログ点検レポート — 2026-06-05

> 作成日: 2026-06-05  
> 調査範囲: `production/logs/rustyclaw.log.2026-06-05` (稼働状況) / `production/workspace/`

---

## 1. ログ全体サマリー

| 区分 | 内容 |
|---|---|
| 🔴 要対応 | **`seen_items` 機能未実装** による重複検知と Heartbeat 頻発（詳細 2-2） |
| 🟠 機能障害 | **Memory Flush 失敗 (コンテキスト制限超過)** が `http-dashboard` セッションで多発（詳細 2-1、**※コード修正済**） |
| 🟡 軽微 | Discord WebSocket Reset (自動回復、プロトコルリセット) が 18:58 に発生 |
| ✅ 正常 | Heartbeat Patrol 最終実行（23:56 JST、直近 4 分以内）、`heartbeat-digest.md` 生成（2.1 KB）、全テスト通過 |

---

## 2. 個別問題詳細

### 2-1. memory flush 失敗 (コンテキスト制限超過) 🔴 【※修正済】

- **現象**: 22:37 以降、`http-dashboard` セッションにおいて以下の警告ログが毎ターン記録され、長期記憶 `MEMORY.md` への書き込み（Flush）がスキップされていた。
  ```
  WARN rustyclaw_agent: memory flush: skipping — estimated tokens exceeds model context limit session=http-dashboard estimated_tokens=27749 ctx_limit=13107
  ```
- **根本原因**: 
  - `rustyclaw-gateway` がリクエスト処理時に `skills::inject_skill_content` を呼び出し、`## Available Skills` や各スキルのインストラクションをユーザープロンプトの末尾・先頭に自動注入していた。
  - 注入後の bloated なテキストがそのまま `execute_with_rig_agent` に渡され、セッション履歴（`http-dashboard.jsonl`）に `role: "user"` としてそのまま保存されていた。
  - 結果、1ターンあたり 2.6KB 〜 4.2KB のドキュメントテキストが履歴ファイルに累積。Memory Flush 実行時には、これら大量の静的ドキュメントが何重にも重複してロードされ、LLM のコンテキスト上限（13k tokens）を瞬く間に超えていた。
- **対処内容**:
  - `execute_with_rig_agent` に元の生メッセージ (`raw_user_message`) とスキル注入後のメッセージ (`injected_user_message`) を別々に渡すようにシグネチャを変更。
  - セッション履歴ログへの保存や RAG ベクトル検索にはオリジナルの `raw_user_message` を使用し、LLM / rig agent の実行時チャットにのみ `injected_user_message` を送るよう分離。
  - これによりセッション履歴のトークン爆発が解消され、今後の対話では正常に Memory Flush が作動する状態になった。

### 2-2. seen_items が一切機能していない問題（既読管理の完全欠落） 🔴 【要対応】

- **現象**: Heartbeat Patrol が実行されるたびに、楽天カードの利用確定お知らせ（5/29, 5/30）や NUROモバイル、八王子東高校同窓会支援といった同一のメールを毎回「新着の Important」として検知し、ユーザーへ Discord を通じた Proactive Speak（声掛け通知）を 30分おきに送り続けている。
  - これにより Heartbeat が `HEARTBEAT_OK` で無音終了することがなくなり、一日中 Discord 送信ログとセッションログが肥大化し続けている。
- **根本原因**:
  - `docs/specs/04_heartbeat_spec.md` には「既読管理は `seen_items` テーブルで行います」との記載があるが、**ゲートウェイおよびエージェントのコードベース（テストを除く）で `is_item_seen` および `mark_item_seen` が一度も呼び出されていない**。
  - SQLite の `seen_items` テーブルは完全に空（`COUNT(*) = 0`）のままであった。
- **今後の推奨アクション**:
  - Heartbeat 時に `gmail` や `calendar` 等のスキルツールを実行した際、検知した項目（ID またはタイムスタンプ+件名のハッシュ）を `is_item_seen` でチェックし、未読の場合のみ Proactive / Alert に乗せる。
  - Alert/Proactive として処理した項目は、`mark_item_seen` を呼び出して既読データベースへ登録するロジックを gateway または agent 内に追加する必要がある。

---

## 3. 定期点検チェックリスト結果 (2026-06-05)

| 確認項目 | コマンド・確認結果 | 正常基準 | 判定 |
|---|---|---|:---:|
| Heartbeat 最終実行 | `heartbeat-state.json` で `2026-06-05T23:56:09+09:00` | 20分以内 | ✅ 正常 |
| heartbeat-digest | `heartbeat-digest.md` の容量が 2149 bytes | 0より大きい | ✅ 正常 |
| MCP JSON 漏出 | Discord セッションの JSON 漏出検知スクリプト実行結果: 0 件 | 0件 | ✅ 正常 |
| heartbeat.jsonl 肥大化 | `cron-heartbeat.jsonl` の容量が 1.5 MB | 500KB 以下目安 | ❌ 肥大化 |
| MEMORY.md サイズ | `MEMORY.md` の容量が 4846 bytes | 5KB 以下 | ✅ 正常 (近接) |
| 空応答率 | `cron-heartbeat`: 37%、`http-dashboard`: 34% | cron-heartbeat 以外 20% 以下 | ⚠️ ツール使用に伴う許容値 |

---

## 4. 対策と今後の課題

1. **`cron-heartbeat.jsonl` の自動クルーニング/ローテーション**:
   - `cron-heartbeat.jsonl` が 1.5MB まで肥大化している（目標 500KB）。Phase 27 の「Cron セッションおよびログの自動プルーニング」に沿って、古い実行履歴の自動クローニングおよびローテーションロジックを実装することを推奨する。
2. **`seen_items` による既読フィルタリングの実装**:
   - 外部メール着信チェック (Step 3) の Proactive 処理部分で、既読データベースを活用する仕組みを実装して無用な通知ループを遮断する。
