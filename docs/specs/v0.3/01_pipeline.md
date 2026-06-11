# RustyClaw — パイプライン & Lane Control 仕様

> [!NOTE]
> **ステータス**: `[実装済]`（§4.1 ExecuteTools/bwrap 隔離のみ `[将来拡張]`）
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 4. 4ステージ・パイプライン `[実装済]`

エージェントの 1 思考ターンは以下の 4 ステージを厳格に経由する。

| ステージ | 役割 |
|---|---|
| **ContextBuilder** | システムコンテキスト（人格 4 ファイル）+ 会話履歴 + RAG 記憶をブレンドし、プロンプトを構築 |
| **CallLLM** | `FallbackChain` と SSE ストリーミングによるステートレス LLM 呼び出し |
| **ExecuteTools** | インプロセス ToolRegistry を通じたツール実行。`[将来拡張]` bwrap 隔離空間対応 |
| **PublishResponse** | Discord / Telegram / LINE / Web ダッシュボードへの応答分配 |

### 4.1 LoggableTool による透過ログキャプチャ `[将来拡張]`

rig-core の Tool トレイトを透過ラッパーで拡張。ツール実行のたびに「ツール名」「引数 (JSON)」「出力（エラー含む）」「実行時間」を MessageBus へブロードキャストし、バックグラウンドの AuditorWorker（Lane B）へ蓄積する。Hermes 自己改善 Skills システム（§12）のデータソースとなる。

---

## 7. Lane Control `[実装済]`

### 7.1 思想と目的

RPi4 の 4 コア（Cortex-A72）において、マルチチャンネルからの同時リクエスト・HA イベントスパイク・定時 Cron が衝突しても**「ユーザーへの対話レスポンスを最高位で保護し、CPU ハング・熱暴走を完全に回避する」**ためのリソース分配インフラ。

### 7.2 レーン定義と厳格な分離

```
[ ユーザー発言 / センサー値変化 / 定期 Cron ]
│
▼
┌──────────────────────────────┐
│   MessageBus による交通整理  │
└──────┬────────────────┬──────┘
       │                │
       ▼                ▼
┌──────────────┐  ┌────────────────────────────────┐
│    Lane A    │  │             Lane B             │
│ (対話・応答) │  │ (記憶・Embedding・監査・バッチ) │
└──────┬───────┘  └────────────────┬───────────────┘
       │                           │
       ▼ 【Semaphore limit: 1】    ▼ 【Semaphore limit: 1 / 待ち行列】
  [即時非同期駆動]          [tokio::task::spawn_blocking]
                                   │
                                   ▼ (連続処理時)
                             [200ms 息抜きスリープ]
```

- **Lane A (Interactive Lane)**: セマフォ `limit=1`。ユーザーとのストリーミング対話および `Publish` 専用。対話レイテンシを最高位で保護。
- **Lane B (Background Lane)**: セマフォ `limit=1`。ローカル Embedding・LLM 自己監査・Memory Flush・夜間バッチ専用。`tokio::task::spawn_blocking` で Tokio スレッドプールから完全分離。

### 7.3 待ち行列処理メカニズム

1. **チャネルによる非同期化**: `Publish` 完了後の会話ログは `tokio::sync::mpsc::channel::<MemoryJob>(100)` へ投下。プッシュ自体は数マイクロ秒で完了し、Lane A はすぐに次の発言の待機に戻る。
2. **セマフォによる非同期待機**: キューからポップした Job は Lane B セマフォの Permit を得るまで非同期待機（`.acquire_owned().await`）。同時に CPU を動かす重いタスクは常に 1 つに制限。
3. **息抜きスリープ（サーマルプロテクション）**: MemoryWorker が連続タスクを処理するとき、1 タスク完了・セマフォ解放の直前に `tokio::time::sleep(Duration::from_millis(200))` を挿入。1 コアの 100% 長時間占有によるサーマルスロットリングを防ぐ。

### 7.4 Cron 定期予約の調停ルール

| タスク種別 | レーン | 理由 |
|---|---|---|
| Heartbeat（自発対話） | **Lane A** | ユーザーへの自発投稿はインタラクションと同等の最優先 |
| Daily Summary / 知識の剪定 | **Lane B** | 重いメタ処理は行列末尾にシリアライズ |
| Memory Flush・Skill Flush | **Lane B** | Publish 完了後に kick。対話ループはゼロレイテンシで解放 |
| HA データポーリング | **Lane B**（10 分 Throttling 後） | スパイク防止のため Gateway 層で間引いてから投入 |
