# RustyClaw — 将来拡張仕様（bwrap / HomeAssistant / rig-core）

> [!NOTE]
> **ステータス**: `[将来拡張]`
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw_hermes_featured.md`](00_rustyclaw_hermes_featured.md)

---

## 13. bwrap サンドボックス `[将来拡張]`

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

---

## 14. HomeAssistant 統合 `[将来拡張]`

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

## 15. rig-core / rig-fastembed 統合 `[将来拡張]`

### 15.1 採用理由

- **rig-core**: LLM プロバイダー抽象・ツール定義・エージェントループを統一的に扱うフレームワーク。現状の自製 `LlmProvider` trait を段階的に置き換え、プロバイダー追加コストを削減する。
- **rig-fastembed**: `fastembed`（ONNX Runtime ベース）のラッパー。`multilingual-e5-small` 等のローカル Embedding モデルをプロセス内で実行し、外部 API なしでセマンティック検索を実現する。

### 15.2 移行方針

- `rustyclaw-providers` の各プロバイダーを `rig::providers` ベースへ段階的に移行。
- `LlmProvider` trait は互換ラッパーとして当面維持し、移行期間中の既存コードへの影響を最小化する。

### 15.3 ローカル Embedding

現状の tantivy BM25 全文検索に加え、`rig-fastembed` によるベクトル検索レイヤーを追加。

```rust
// Lane B の spawn_blocking 内で実行（ONNX 演算の Tokio スレッドプール汚染を防ぐ）
tokio::task::spawn_blocking(|| {
    let model = EmbeddingModel::new("multilingual-e5-small")?;
    let embeddings = model.embed(chunks)?;
    // SQLite または インメモリベクトルストアへ保存
})
```

> **注意**: ONNX Runtime の初期化コストが高いため、モデルインスタンスは `once_cell::sync::Lazy` 等でプロセス内キャッシュする。毎リクエスト初期化は禁止。
