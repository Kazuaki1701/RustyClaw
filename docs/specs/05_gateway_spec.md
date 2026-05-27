# 05. Gateway・並列制御（Lane Queue）仕様

> [!NOTE]
> **ステータス**: `[ACTIVE]` (最新の真実 - コードと同期中)  
> **最終更新日**: 2026-05-28  
> **対象コード**: `rustyclaw-gateway` の最新実装

## 1. Gateway プロセス構成

`Gateway`（`rustyclaw-gateway` クレート）は、バックグラウンドデーモンとして常駐し、外部チャンネル（Discord等）からの着信を受け付け、エージェントパイプラインをオーケストレーションするコアプロセスです。

```
                       [ 外部チャンネル (Discord / Telegram) ]
                                      │  ▲ (受信・配信)
                                      ▼  │
                              [ ChannelManager ]
                                      │
                 (Message)            ▼ (Publish Message)
              [ MessageBus ] ◄─────────────────► [ CronService ]
                      │
                      ▼ (Dispatch)
               [ LaneRegistry ]
           ┌──────────────────────┐
           │     Lane Queue       │  Semaphore:
           │  ┌────────────────┐  │  - User Permit: 1
           │  │ Lane 1 (User)  │  │  - BG Permit: 1
           │  └────────────────┘  │
           │  ┌────────────────┐  │
           │  │ Lane 2 (BG)    │  │
           │  └────────────────┘  │
           └──────────────────────┘
                      │
                      ▼
                 [ AgentLoop ] ──> [ Pipeline 起動 ]
```

### 構成モジュール

#### ① MessageBus
`tokio::sync::mpsc` および `broadcast` を用いて、非同期な内部イベント（対話メッセージ、cronトリガー、システムエラーなど）を pub/sub 接続します。

#### ② ChannelManager
`Channel` トレイトを実装した Discord（初期優先）や Telegram（後回し）等のコネクタを管理し、着信メッセージの標準化と応答の配信を行います。

#### ③ WatchdogService (systemd連携)
`sd-notify` クレートを使用し、システムがハングアップしていないことを systemd に定期的に通知します（`WatchdogSec=60s`）。

#### ④ HealthServer (HTTPサーバー)
`tokio::net::TcpListener` による軽量 TCP ベース実装（重い Web フレームワーク依存なし）。ポート `8080` で待受。

| エンドポイント | メソッド | 内容 |
|---|---|---|
| `/health` | GET | Liveness プローブ → `200 OK` |
| `/ready` | GET | 起動完了プローブ → `200 READY` |
| `/reload` | GET | 設定ホットリロード（SIGHUP 相当） |
| `/dashboard` または `/` | GET | ブラウザ管理 UI（シングルページ HTML） |
| `/chat` | POST | ダッシュボードからの対話（`{"message":"..."}` → テキスト応答） |
| `/logs/memory` | GET | `workspace/MEMORY.md` 全文 |
| `/logs/heartbeat-digest` | GET | `workspace/memory/heartbeat-digest.md` 全文 |
| `/logs/heartbeat-state` | GET | `workspace/memory/heartbeat-state.json`（pretty-print） |
| `/logs/app` | GET | `~/.rustyclaw/rustyclaw.log` 末尾 100 行 |

**ダッシュボードレイアウト（`/dashboard`）**:
```
┌────────────────────┬──────────────────────────────────┐
│                    │  🟣 MEMORY.md         (flex: 3)  │
│  Chat パネル       ├──────────────┬───────────────────┤
│  (flex: 4)         │ 🟢 hb-digest │ 🟡 hb-state.json │
│                    │  (flex: 6)   │  (flex: 4)        │
│                    ├──────────────┴───────────────────┤
│                    │  🔵 rustyclaw.log     (flex: 4)  │
└────────────────────┴──────────────────────────────────┘
```
- MEMORY.md / heartbeat 系: 5 秒ポーリング
- App ログ: 2 秒ポーリング
- `/chat` セッション ID は `"http-dashboard"` 固定（履歴が蓄積される）

---

## 2. Lane Queue 設計 (並列制御・競合回避)

多数のチャネルからのメッセージや、バックグラウンドでのHeartbeat処理が同時に重なった際、プロセスがフリーズしたり、同一ユーザーに対する応答が混ざったりするのを防ぐための並列制御です。

### 確定パラメータ

| 制御項目 | 値 | 役割 |
|---|:---:|---|
| **ユーザー同時枠** (`user_sem`) | **1** | ユーザー対話セッションを直列化。`MEMORY.md` 等ワークスペースファイルへの並列書き込みによるデータ消失を防止。[^user_sem] |
| **バックグラウンド枠** (`bg_sem`) | **1** | Heartbeat / Daily Summary など非対話型処理用スロット。 |
| **Flush 専用枠** (`flush_sem`) | **1** | `flush_memory()` 専用。セマフォ管理外で走っていた Flush gmn を制限し、意図した上限を超えないよう保護。 |
| **意図した最大同時 gmn 数** | **3** | user(1) + bg(1) + flush(1)。 |
| **同一セッションの直列保証** | **1** | 同一の `sessionId` からのリクエストは必ず直列にキューイングして処理。 |
| **待機タイムアウト** | **60秒** | スロット空き待ちの最大時間。タイムアウト時はエラーを返却。 |

---

## 3. 別レーン（チャネル・タスク）からの話題（TOPICS）混載防止の 4 層構造

複数のユーザー、異なるチャネル（Telegram/Discord）、および自発パトロールタスク（Heartbeat）が同時に処理される際、エージェントの内部文脈や対話トピックが混ざり合うのを防ぐための徹底した隔離アーキテクチャです。

### レイヤー 1：`sessionId` による物理的なコンテキスト隔離
- **仕組み**: メッセージが着信すると、システムは送信元情報に基づいて一意の `sessionId`（例: `telegram-U12345678-20260525`）を特定し、そのIDに対応する対話ファイルのみをスキャン・ロードします。
- **効果**: 内部の `ConversationHistory` は特定のセッションに完全にバインドされてインスタンス化されるため、異なるユーザーやチャネルの話題が物理的に同じプロンプトやコンテキストに混ざり込む余地を完全に排除します。

### レイヤー 2：`LaneRegistry` による同一セッションの直列保証
- **仕組み**: 同一 `sessionId` からの連続したメッセージは、スレッドセーフな `LaneRegistry` によって割り当てられた特定のレーン（内部 `mpsc` チャネル）に送られ、単一の Lane worker によって順番に処理されます。
- **効果**: ユーザーが返答を待たずにメッセージを連投した場合でも、並行処理による会話状態の破壊や応答順の逆転を防止し、会話の流れを一貫した「直列の流れ」として処理します。

### レイヤー 3：Semaphore による優先スロット制限（リソース隔離）
- **仕組み**: ユーザー対話・バックグラウンド・Flush の 3 種類でセマフォスロットを物理的に分割します。
  - `user_sem`: 同時許可数 **1**（ユーザー対話・直列化）
  - `bg_sem`: 同時許可数 **1**（Heartbeat / Daily Summary）
  - `flush_sem`: 同時許可数 **1**（`flush_memory()` 専用）
- **flush_sem の背景**: `trigger_memory_flush_async()` は `tokio::spawn` で直接起動するため `user_sem`/`bg_sem` の外で走る。並列チャット時に flush gmn が制限なく増殖するのを防ぐため専用枠を設けた。
- **Antigravity 2.0 対応**: 合計最大 3 並列 gmn。`GMN_MAX_RETRIES=0` で内部リトライを無効化しているため最悪バーストは 3 リクエスト/秒程度に抑制される。

[^user_sem]: **継続検討課題（2026-05-28）**: `user_sem > 1`（並列化復活）を検討する場合、ワークスペースファイル（`MEMORY.md` / `USER.md` / 日次ログ）への排他制御が前提条件となる。候補手法: run-progress.json によるソフト保護（TOCTOU 問題あり）、またはプロバイダー層でのファイルロック機構。Gemini CLI サブプロセス経由のツール実行を RustyClaw がインターセプトできない構造的制約がある。

### レイヤー 4：SQLite `seen_items` による判定分離
- **仕組み**: Heartbeat などの自発巡回タスクで検知された情報（メールやニュース等）は、直接ユーザーの対話履歴に差し込まれるのではなく、まず SQLite の既読テーブル（`seen_items`）で静かに処理されます。
- **効果**: 緊急度が「Critical（即時声掛け）」に達した場合のみ、特定の `sessionId` に対する正規の割り込みとしてメッセージが配信され、通常の定常情報巡回は静かに `logs/` にのみ出力されます。これにより、余計な「独り言」や話題の混入からユーザーチャットを守ります。

```rust
// Background Laneのキューは最大1件とし、古いHeartbeatの積み重なりを破棄する
let cap = match priority {
    Priority::Normal     => 0,   // 無制限（ユーザー対話）
    Priority::Background => 1,   // 最新の1件のみ保持（古いパトロールは破棄）
};
```


---

## 4. systemd サービス構成 (`/etc/systemd/system/rustyclaw.service`)

Raspberry Pi 4 向けにリソース制限と Watchdog を組み込んだ systemd 設定仕様です。

```ini
[Unit]
Description=RustyClaw AI Agent Gateway
After=network-online.target

[Service]
Type=simple
User=pi
ExecStart=/usr/local/bin/rustyclaw gateway
Restart=on-failure
RestartSec=5s

# リソース制限（4GB RAM環境に対する安全設計：物理RAMの半分以下に制限）
OOMScoreAdjust=-500
MemoryMax=2G

# systemd watchdog 連携
WatchdogSec=60s

[Install]
WantedBy=multi-user.target
```

### Rust側での Watchdog 送信処理
`sd-notify` を利用し、メインループの起動後にバックグラウンドタスクを `spawn` して30秒ごとにシグナルを送信します。

```rust
tokio::spawn(async {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        // Watchdog シグナルを systemd に送信
        let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]);
    }
});
```
