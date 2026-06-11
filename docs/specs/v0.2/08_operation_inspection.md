# 08. 稼働点検ガイド

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-06-06  
> **目的**: RustyClaw デーモンの稼働状況を定期的または障害時に点検するための手順書

---

## 1. 点検対象ファイル一覧

```
workspace/
├── sessions/                        # セッション別の会話履歴 (JSONL)
│   ├── cron-heartbeat.jsonl         # Heartbeat の実行履歴（肥大化に注意）
│   ├── cron-daily-summary.jsonl     # Daily Summary の実行履歴
│   ├── discord-<channelId>-<date>.jsonl  # Discord チャンネル別・日付別
│   └── http-dashboard.jsonl         # Dashboard チャットの履歴
├── memory/
│   ├── heartbeat-state.json         # 最後の各チェック実行時刻 + lastUserContact
│   ├── heartbeat-digest.md          # 最新の Heartbeat ダイジェスト（0 byte なら異常）
│   └── logs/YYYY-MM-DD.md          # 日次活動ログ（Heartbeat の詳細記録）
├── MEMORY.md                        # エージェントの長期記憶（5KB 以下が目標）
└── memory.db                        # SQLite（flush カウント・seen_items 等）
```

ログファイル（`~/.rustyclaw/rustyclaw.log`）はデーモン起動後に生成される。

---

## 2. クイック点検（所要 2〜3 分）

### 2-1. Heartbeat の最終実行時刻を確認

```bash
cat workspace/memory/heartbeat-state.json
```

**見るべき点**:
- `activityReview`（最終 Heartbeat 実行時刻）が現在時刻から 40 分以内か
- `lastUserContact`（最後のユーザー応答時刻）が期待通りか
- いずれかが大幅に過去の場合 → Heartbeat が止まっている

**正常例**:
```json
{
  "lastChecks": {
    "activityReview": "2026-05-28T06:55:17+09:00",
    "memoryMaintenance": "2026-05-28T06:55:17+09:00",
    "calendar": "2026-05-28T06:55:17+09:00",
    "email": "2026-05-28T06:55:17+09:00",
    "weather": "2026-05-28T06:55:17+09:00",
    "lastUserContact": "2026-05-28T01:15:46+09:00"
  }
}
```

---

### 2-2. heartbeat-digest.md の中身を確認

```bash
wc -c workspace/memory/heartbeat-digest.md
cat workspace/memory/heartbeat-digest.md
```

**判定**: 0 バイトなら `generate_digest()` またはファイル書き込みに異常あり。

---

### 2-3. 本日の活動ログを確認

```bash
# Heartbeat の実行回数と各時刻を列挙
grep "Heartbeat Patrol" workspace/memory/logs/$(date +%Y-%m-%d).md | head -20

# ユーザーセッションのエントリを列挙
grep "^## " workspace/memory/logs/$(date +%Y-%m-%d).md
```

**見るべき点**:
- 30 分間隔で Heartbeat が記録されているか（40 分以上の空白は異常）
- `Critical` アラートが発報されていないか

---

### 2-4. セッションファイルの状態確認

```bash
ls -lh workspace/sessions/
```

**見るべき点**:
- `cron-heartbeat.jsonl` が異常に大きくないか（1MB 超は要注意）
- Discord セッションファイルの更新時刻がメッセージ受信時刻と整合しているか

---

## 3. 詳細点検（所要 10〜15 分）

### 3-1. Heartbeat 応答の中身を解析

```bash
python3 - <<'EOF'
import json

with open('workspace/sessions/cron-heartbeat.jsonl') as f:
    lines = f.readlines()

total = len(lines)
empty_assistant = sum(
    1 for l in lines
    if json.loads(l).get('role') == 'assistant'
    and json.loads(l).get('content', '') == ''
)
print(f"Total entries  : {total}")
print(f"Assistant empty: {empty_assistant} / {total // 2}")

# 最新5ターンを表示
for line in lines[-10:]:
    obj = json.loads(line)
    role = obj.get('role', '?')
    content = obj.get('content', '')
    if isinstance(content, list):
        content = ' '.join(c.get('text','') for c in content if c.get('type')=='text')
    print(f"[{role}] {str(content)[:120]!r}")
EOF
```

**判定基準**:
- `empty_assistant` が全 assistant ターンの 80% 超 → LLM プロバイダが空応答を返しているか、またはツール呼び出しの処理がループ制限（5回）に達した（または MCP ツール側の問題）
- 最新ターンの content に `HEARTBEAT_OK` が含まれているか確認

---

### 3-2. セッション別の空応答率を一括チェック

```bash
python3 - <<'EOF'
import json, os, glob

for fpath in sorted(glob.glob('workspace/sessions/*.jsonl')):
    fname = os.path.basename(fpath)
    with open(fpath) as f:
        lines = [json.loads(l) for l in f if l.strip()]
    assistants = [l for l in lines if l.get('role') == 'assistant']
    empty = [l for l in assistants if not l.get('content', '')]
    rate = len(empty) / len(assistants) * 100 if assistants else 0
    size = os.path.getsize(fpath)
    print(f"{fname:50s}  {len(lines):4d} entries  empty={len(empty)}/{len(assistants)} ({rate:.0f}%)  {size//1024}KB")
EOF
```

**判定基準**:
- 空応答率が高いセッション → そのセッションでモデルがツール呼び出しのみ生成している
- `http-dashboard.jsonl` で空率が高い → ユーザー操作が成功していない

---

### 3-3. Heartbeat の時間間隔ギャップを検出

```bash
grep -oP '\[\K[0-9]{2}:[0-9]{2}:[0-9]{2}(?=\] Heartbeat Patrol)' \
  workspace/memory/logs/$(date +%Y-%m-%d).md \
  | awk 'NR>1 {
      split(prev, a, ":");  split($0, b, ":");
      prev_min = a[1]*60+a[2]; cur_min = b[1]*60+b[2];
      diff = cur_min - prev_min;
      if (diff < 0) diff += 1440;
      if (diff > 35) printf "GAP %s -> %s (%d min)\n", prev, $0, diff;
    }
    {prev=$0}'
```

**判定基準**:
- 35 分超のギャップがある場合 → gmn_sem 待機（Daily Summary 等との競合。容量は 4 です）または rate limit の疑い

---

### 3-4. Discord セッションで MCP JSON 漏出を検出

```bash
python3 - <<'EOF'
import json, glob

for fpath in sorted(glob.glob('workspace/sessions/discord-*.jsonl')):
    fname = fpath.split('/')[-1]
    with open(fpath) as f:
        for line in f:
            obj = json.loads(line)
            if obj.get('role') == 'assistant':
                content = obj.get('content', '')
                if isinstance(content, list):
                    content = ' '.join(c.get('text','') for c in content if c.get('type')=='text')
                if '```json' in content and ('"action"' in content or '"function"' in content):
                    print(f"[MCP LEAK] {fname}: {content[:200]!r}")
EOF
```

---

### 3-5. SQLite の状態確認

```bash
# flush カウント・タイムスタンプ
sqlite3 workspace/memory.db "SELECT patrol_name, last_run_at FROM patrol_state WHERE patrol_name LIKE 'flush%' ORDER BY patrol_name;"

# lastUserContact
sqlite3 workspace/memory.db "SELECT patrol_name, last_run_at FROM patrol_state WHERE patrol_name = 'lastUserContact';"

# seen_items テーブルの件数
sqlite3 workspace/memory.db "SELECT COUNT(*) FROM seen_items;"
```

---

### 3-6. MEMORY.md のサイズ確認

```bash
wc -c workspace/MEMORY.md
```

**判定基準**: 5,120 バイト（5KB）超は Memory Flush の上書き機能が正常でない可能性。

---

## 4. デーモンログの確認（rustyclaw.log）

デーモン起動後、`~/.rustyclaw/rustyclaw.log` に出力される。

```bash
# 末尾100行（Dashboard の /logs/app に相当）
tail -100 ~/.rustyclaw/rustyclaw.log

# エラーのみ抽出
grep -i "error\|warn\|timeout\|rate.limit" ~/.rustyclaw/rustyclaw.log | tail -30

# LLM プロバイダ接続・フォールバックログ
grep -i "fallback model used\|model failed\|cooled down" ~/.rustyclaw/rustyclaw.log | tail -20

# Heartbeat の成否
grep "Heartbeat LLM\|Heartbeat failed" ~/.rustyclaw/rustyclaw.log | tail -20

# Memory Flush の動作
grep "memory flush" ~/.rustyclaw/rustyclaw.log | tail -20

# gmn_sem の待機・タイムアウト
grep "gmn_sem\|timed out\|permit" ~/.rustyclaw/rustyclaw.log | tail -20
```

---

## 5. 既知の問題パターンと判定フロー

```
症状: チャットに ```json ... ``` ブロックが送信される
  └─ 原因: --no-agent モードで MCP ツール呼び出し JSON が漏出
     └─ 対処: AGENTS.md から GeminiClaw 固有ツール指示を削除（応急）
              または MCP クライアント実装（実装完了、詳細は [07_mcp_plan.md](file:///home/kazuaki/Projects/RustyClaw/docs/archive/plans/07_mcp_plan.md) 参照）

症状: Heartbeat が 40 分以上止まっている
  ├─ heartbeat-state.json の activityReview が古い
  ├─ ログに "timed out (60s) waiting for permit" がある
  │    └─ 原因: gmn_sem の空きスロット不足（Daily Summary 等との一時的な競合）
  │       └─ 対処: 各ジョブの実行時刻スケジュールの見直し・調整
  └─ ログに "rate-limit" がある
       └─ 対処: 指数バックオフによる自動クールダウン完了を待つ

症状: cron-heartbeat.jsonl が 1MB 超
  └─ 原因: 固定 session_id "cron:heartbeat" による履歴の無制限蓄積
     └─ 対処: Heartbeat セッション ID を毎回リセット（task.md Phase 7 参照）

症状: heartbeat-digest.md が 0 バイト
  └─ 調査: generate_digest() の戻り値が空か確認
           HeartbeatService の digest 書き込みパス確認

症状: assistant 応答がすべて空
  └─ 原因: モデルがテキストなしでツール呼び出しのみ生成
     └─ 確認: LLM プロバイダの API レスポンスが空かどうか RUST_LOG=debug 等で確認
```

---

## 6. 定期点検チェックリスト（週次）

| 確認項目 | コマンド | 正常基準 |
|---|---|---|
| Heartbeat 最終実行 | `cat workspace/memory/heartbeat-state.json` | 40 分以内 |
| heartbeat-digest | `wc -c workspace/memory/heartbeat-digest.md` | 0 より大きい |
| MCP JSON 漏出 | 3-4 のスクリプト | 0 件 |
| heartbeat.jsonl 肥大化 | `ls -lh workspace/sessions/cron-heartbeat.jsonl` | 500KB 以下目安 |
| MEMORY.md サイズ | `wc -c workspace/MEMORY.md` | 5KB 以下 |
| Heartbeat ギャップ | 3-3 のスクリプト | 35分以上のギャップなし |
| 空応答率 | 3-2 のスクリプト | cron-heartbeat 以外は 20% 以下 |
| SQLite flush カウント | `sqlite3 workspace/memory.db "SELECT patrol_name, last_run_at FROM patrol_state WHERE patrol_name LIKE 'flush%'"` | flush_ts が最近更新されている |

---

## 7. 関連ドキュメント

- `docs/specs/04_heartbeat_spec.md` — Heartbeat の設計仕様
- `docs/specs/05_gateway_spec.md` — gmn_sem・Lane Queue 設計
- `docs/specs/06_dashboard_spec.md` — Web Dashboard・管理用 API 仕様
- `docs/task.md` — Phase 7: 稼働観察で判明した要修正・要点検項目
