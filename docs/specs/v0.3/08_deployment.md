# RustyClaw — 運用・デプロイ手順

> [!NOTE]
> **ステータス**: 運用ドキュメント
> **バージョン**: v0.3
> **最終更新日**: 2026-06-12（Phase 50: HA セットアップ手順追加）
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

---

## 16. 運用・デプロイ

### 16.1 デプロイ先への SSH 接続

```bash
ssh rp1
```

| 項目 | 値 |
|---|---|
| SSH エイリアス | `rp1` |
| Hostname | `RaspberryPi.local`（解決不可時は `192.168.1.12`） |
| ユーザー / Arch | `kazuaki` / `aarch64` |
| バイナリ配置先 | `~/.local/bin/rustyclaw` |
| 本番ルート | `~/.rustyclaw` → NAS 共有 `production/`（symlink） |

### 16.2 aarch64 クロスビルド

```bash
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --target aarch64-unknown-linux-gnu -p rustyclaw-cli
# 成果物: target/aarch64-unknown-linux-gnu/release/rustyclaw-cli
```

### 16.3 デプロイ（推奨: 自動）

```bash
./scripts/deploy.sh
# x64/aarch64 ビルド → production/bin/ 配置 → rp1 へ転送 → サービス再起動まで実行
```

**config profile 切り替え**:

```bash
# 本番（クラウド LLM 主力）
cd production/config && ln -sfn config.cloud-llm.json config.json
# 開発（ローカル LLM 主力）
cd production/config && ln -sfn config.local-llm.json config.json
```

### 16.4 サービス管理

```bash
ssh rp1 'sudo systemctl status  rustyclaw'
ssh rp1 'sudo systemctl restart rustyclaw'
ssh rp1 'journalctl --user -u rustyclaw -f'
```

**デプロイ前検証（実 API 不要）**:

```bash
rustyclaw --config /tmp/verify/config.json --workspace /tmp/verify/workspace --no-agent gateway
curl -s http://127.0.0.1:8080/api/concurrency
```

### 16.5 systemd サービス設定

```ini
[Unit]
Description=RustyClaw AI Agent
After=network-online.target

[Service]
Type=simple
User=kazuaki
ExecStart=/home/kazuaki/.local/bin/rustyclaw gateway
Restart=on-failure
RestartSec=5s
OOMScoreAdjust=-500
MemoryMax=2G
WatchdogSec=60s

[Install]
WantedBy=multi-user.target
```

```rust
// systemd watchdog 通知（main 起動後に spawn）
tokio::spawn(async {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]);
    }
});
```

### 16.6 Hot Reload

`SIGHUP` シグナルを受信するとプロセスを再起動することなく `workspace/` の設定ファイルおよび各種 Markdown プロンプトを安全にリロードする。
ダッシュボードの `/reload` エンドポイントからも同等の操作が可能。

---

## 16.7 Node.js ≥ 22.5 インストール（context-mode 前提）

context-mode は `node:sqlite` 内蔵の Node.js ≥ 22.5 が必要。

```bash
# nvm 経由（推奨）
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
source ~/.bashrc
nvm install 22
node --version  # v22.x.x

# context-mode インストール
npm install -g context-mode
context-mode --version
```

動作確認（手動起動テスト）:

```bash
mkdir -p /tmp/ctx-test/.context-mode
CONTEXT_MODE_DIR=/tmp/ctx-test/.context-mode \
  CONTEXT_MODE_PLATFORM=custom-rustyclaw \
  context-mode
# JSON-RPC 入力待ちになれば OK（Ctrl+C で終了）
```

---

## 16.8 HomeAssistant 連携セットアップ（Phase 50）

HA トークンを vault に登録し、config.json でエンドポイントを指定する。

### トークン登録（vault）

```bash
# vault に HOMEASSISTANT_TOKEN を追加（config.json には "$vault:HOMEASSISTANT_TOKEN" と記述）
rustyclaw vault set HOMEASSISTANT_TOKEN <HA_LONG_LIVED_ACCESS_TOKEN>
```

HA の Long-Lived Access Token は `http://<HA_HOST>:8123/profile` → Security タブで発行。

### config.json 設定例

```json
{
  "tools": {
    "home-assistant": {
      "enabled": true,
      "endpoint": "http://192.168.1.30:8123",
      "token": "$vault:HOMEASSISTANT_TOKEN"
    }
  }
}
```

`endpoint` のデフォルト値は `http://192.168.1.30:8123`。HA ホストが異なる場合のみ指定する。

### 動作確認

```bash
# 1. HA エンドポイント疎通確認
ssh rp1 'wget -qO- --header "Authorization: Bearer <TOKEN>" http://192.168.1.30:8123/api/'

# 2. スナップショットスクリプト手動実行
ssh rp1 'bash ~/.rustyclaw/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh'
cat ~/.rustyclaw/workspace/memory/ha-env-summary.txt
cat ~/.rustyclaw/workspace/memory/ha-state.json | python3 -m json.tool

# 3. スパイク検知テスト（exit code 確認）
ssh rp1 'bash ~/.rustyclaw/workspace/skills/home-assistant-rest-api/scripts/220_ha_env_snapshot.sh --check-spike; echo "exit: $?"'
```

### HA 連携の環境変数継承チェーン

```
vault → config.json: "token": "$vault:HOMEASSISTANT_TOKEN"
  → Rust (inject_vault_to_env): HOMEASSISTANT_TOKEN 環境変数にセット
  → Rust (lib.rs): HOMEASSISTANT_ENDPOINT 環境変数にセット（config.endpoint から）
  → context-mode Node.js: 親プロセスから ENV 継承
  → bash スクリプト: $HOMEASSISTANT_TOKEN / ${HOMEASSISTANT_ENDPOINT:-http://...} 参照
```

---

## 将来拡張 `[将来拡張]`

### 本番環境の自動バックアップ体制

`production/workspace/`（`memory.db`・`sessions/*.jsonl`・`patrol/findings.md` 等）を NAS（QNAP 等）へ定時 rsync する自動バックアップ体制の整備。
