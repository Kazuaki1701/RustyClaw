# RustyClaw — 将来拡張仕様（bwrap / HomeAssistant / rig-core）

> [!NOTE]
> **ステータス**: `[将来拡張]`（§13・§15 rig-core は v0.4 完了済み）
> **バージョン**: v0.3（v0.4 完了分を反映）
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

---

## 13. bwrap サンドボックス `[完了済 — v0.4 で context-mode に委譲]`

> **v0.4 対応**: `workspace_execute_script`（bwrap Rust 実装）を削除し、`ctx_execute` ツール経由で context-mode の `PolyglotExecutor` に委譲済み（Phase 01）。以下は設計背景の記録として残す。

### 13.1 採用理由

LLM が自律生成したコードやプロンプトインジェクションからシステムを物理的に守るため、一般権限で動作する超軽量コンテナ化ツール **`bwrap` (Bubblewrap)** を採用。**Spawn-per-call（1 回きりの完全使い捨て環境）** で実行する。

### 13.2 bwrap マウント・隔離戦略

| 設定 | 効果 |
|---|---|
| `--unshare-net` | ネットワーク遮断。機密データ（HA トークン等）の外部流出を物理的に防止 |
| `--ro-bind /usr /usr` 等 | OS システム領域は読み込み専用で安全に共有 |
| `--tmpfs /tmp` | 書き込み領域はメモリ上の使い捨て `tmpfs`。プロセス終了と同時に完全消滅 |

### 13.3 symlink 対策

サンドボックス外を指す絶対パスの symlink は隔離空間内で「リンク切れ」を起こす。
そのため、Rust 側からツールへファイルをマウントする際は必ず **`std::fs::canonicalize` を実行して実体の絶対パスを解決してからバインドする** ことを鉄則とする。

> **v0.5 再検討**: `SecureSandboxExecutor`（Rust 内製）を実装する際に bwrap の隔離設計（`--unshare-net` 等）を再適用する。

---

## 14. HomeAssistant 統合 `[完了済 — v0.4 Phase 50]`

> **v0.4 対応**: Phase 50 にて `220_ha_env_snapshot.sh`（HA skill スクリプト）+ CronService 10 分ポーリング + HeartbeatService スパイク検知を実装済み。TrendAnalyzer は bash リングバッファ（`memory/ha-state.json`）として実装。以下は設計背景の記録として残す。

### 14.1 導入の目的

HA サーバーのセンサーデータ（室温・湿度・CO2・人感等）を「感覚器官」として統合する。
生データをそのまま LLM へ渡すとコンテキストを過度に圧迫し、かつ単一の現在値では「上昇しつつある」という時系列文脈を理解できない。
インメモリ・リングバッファで時系列の「兆候」を算出し、極小のフットプリントで LLM に先回り理解させる。

### 14.2 TrendAnalyzer

センサーデータのスパイクによる誤検知を防ぎ、過去 1 時間（10 分おきに 6 サンプル）の傾きを計算する固定長バッファ。

```rust
pub struct TrendAnalyzer {
    history: VecDeque<SensorPoint>,
    max_samples: usize,           // デフォルト 6（RPi4 メモリ保護）
    stability_threshold: f32,     // トレンド反転判定閾値（例: 0.5）
}

impl TrendAnalyzer {
    pub fn get_trend_arrow(&self) -> &'static str {
        if self.history.len() < 2 { return "→"; }
        let diff = self.history.back().unwrap().value
                 - self.history.front().unwrap().value;
        if diff > self.stability_threshold { "↑" }
        else if diff < -self.stability_threshold { "↓" }
        else { "→" }
    }
}
```

### 14.3 HA エンコーダによるコンテキスト圧縮

毎ターンの `ContextBuilder` で system 領域に動的に埋め込まれる環境サマリー（1 行・約数十トークン）。

```
[HA_ENV|21:05] [Room: 27.5°C↑ | CO2: 1250ppm↑] [Presence: Detected] [Outer: Rain]
```

### 14.4 データ取り込み流量制限 & ルーティング

```
[ HA サーバー ]
│ (State Changed イベント / REST API)
▼
rustyclaw-gateway (ha_client)
  └── 最低 10 分間の時間ベース間引き (Throttling)
       ↓
MessageBus
  ├── TrendAnalyzer へプッシュして傾きを計算
  ├── [通常ターン] ContextBuilder が 1 行サマリーを吸い出し (Lane A)
  └── [スパイク検知] 閾値突破時（例: CO2↑ 1,500ppm 超）
         → HeartbeatService へ緊急フラグ通知
         → 自発投稿（Proactive posts）を強制キック (Lane A)
```

---

## 15. rig-core / rig-fastembed 統合

### 15.1 rig-core `[完了済 — v0.4 実装済]`

> **v0.4 対応**: `rig-core` (0.38, rmcp feature) を導入済み。LLM プロバイダー抽象・ツール定義・MCP クライアント接続に使用中。自製 `LlmProvider` trait からの移行完了。

- **rig-core**: LLM プロバイダー抽象・ツール定義・エージェントループを統一的に扱うフレームワーク。`rustyclaw-providers` の各プロバイダーを `rig::providers` ベースへ移行し、プロバイダー追加コストを削減。

### 15.2 rig-fastembed / ローカル Embedding `[不要 — context-mode FTS5 で代替]`

> **v0.4 対応**: `tantivy` BM25 および `rig-fastembed` を削除。エピソード記憶検索は context-mode の `ctx_search`（SQLite FTS5）に委譲済み。ローカル Embedding レイヤーは現構成では不要。
>
> **v0.5 再検討**: `EmbeddedKnowledgeBase`（Rust 内製）を実装する際に、ベクトル検索の要否を改めて判断する。

```rust
// （参考）ローカル Embedding の将来設計（v0.5 以降）
tokio::task::spawn_blocking(|| {
    let model = EmbeddingModel::new("multilingual-e5-small")?;
    let embeddings = model.embed(chunks)?;
    // SQLite または インメモリベクトルストアへ保存
})
```

> **注意**: ONNX Runtime の初期化コストが高いため、モデルインスタンスは `once_cell::sync::Lazy` 等でプロセス内キャッシュする。毎リクエスト初期化は禁止。

---

## 16. マルチチャンネル対応（LINE + Notifications）`[将来拡張]`

### 16.1 LINE チャンネル導入（Phase 39）

- `rustyclaw-channels` に `LineConnector` を実装。設定スキーマ（`channel_type: "line"`）はすでに定義済み。
- 導入確定後に一般課題へ昇格し、Discord チャンネルの実装を参照して実装する。
- 調査資料: `docs/review/2026-06-03-geminiclaw-nonok-delivery-analysis.md`

### 16.2 Notifications チャンネル

- LINE 導入と同期して設計。システム通知・アラートを専用チャンネルへルーティングする軽量コネクタ。LINE が加わることで `notifications` チャンネルの配信先が増え、価値が高まる。

---

## 17. Upstream 先進機能・外部ツール統合 `[v0.5 再検討]`

> **v0.5 再検討**: PicoClaw Upstream の Hook Manager / Steering / Spawn は v0.5 純 Rust 化（インプロセス化）の設計と合わせて要否・実装方法を改めて判断する。Google Drive ツールはユースケースが明確化した時点で追加する。

### 17.1 Hook Manager / Steering / Spawn（Phase 30）

PicoClaw (Go Upstream) が実装している以下の 3 機能を将来的に取り込む。

- **Hook Manager**: パイプラインの各ステージに差し込める副作用フック（ログ収集・監視・フィルタリング）。
- **Steering 割り込み**: 実行中のパイプラインに外部から指示を注入して応答を誘導する機構。
- **非同期 Spawn**: サブタスクを独立したエージェントとして非同期に生成・管理する機構。

### 17.2 Google Drive / Sheets / Docs ツール

gws CLI 経由で実装可能。Sheets へのデータ書き込み・Docs の参照など、ユースケースが明確になった時点でツールとして追加する。
