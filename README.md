# RustyClaw

> A Rust-native AI agent runtime for Raspberry Pi 4.  
> Raspberry Pi 4 向け、Rust 製セルフホスト AI エージェントランタイム。

---

## What is RustyClaw?

RustyClaw is a self-hosted AI agent runtime written in Rust, designed to run continuously on a Raspberry Pi 4. It fuses the three-layer architecture of [PicoClaw](https://github.com/sipeed/picoclaw) (Go) with the memory management and Heartbeat system of GeminiClaw (TypeScript) into a single, opinionated implementation.

RustyClaw は Rust で書かれたセルフホスト型 AI エージェントランタイムです。Go 製 PicoClaw のアーキテクチャと TypeScript 製 GeminiClaw のメモリ管理・Heartbeat 設計を融合させた独自実装で、Raspberry Pi 4 上で常時稼働することを想定しています。

---

## Architecture

```
rustyclaw-cli  (binary entry point)
    └── rustyclaw-gateway
          ├── MessageBus        (tokio mpsc + broadcast)
          ├── AgentLoop         → LaneRegistry (per-purpose semaphore)
          ├── HeartbeatService  (HEARTBEAT.md-driven proactive actions)
          ├── CronService       (built-in scheduler, no external daemon)
          ├── ChannelManager    (Discord, LINE, …)
          └── HealthServer      (HTTP /health /ready /reload)
                └── rustyclaw-agent  (4-stage pipeline)
                      ├── ContextBuilder   (SOUL / MEMORY / SESSION)
                      ├── CallLLM          (FallbackChain + SSE streaming)
                      ├── ExecuteTools     (in-process + MCP proxy)
                      └── PublishResponse
                            └── rustyclaw-providers
                                  └── OpenAI-compatible HTTP (rustls, no OpenSSL)
```

---

## Key Features

- **Heartbeat system** — Scheduled, self-driven actions defined in `HEARTBEAT.md`; keeps the agent proactive without user input
- **3-layer memory** — Short-term conversation history, mid-term rolling summaries, and long-term `MEMORY.md` with SQLite + tantivy BM25 search
- **Lane-based concurrency** — Per-purpose `Semaphore` in `LaneRegistry`; prevents runaway parallel calls
- **Multi-channel** — Discord and LINE connectors; easily extensible via the `Channel` trait
- **Web dashboard** — Built-in single-page app for live monitoring and stats (`/monitor`, `/stats`)
- **Skills system** — Hierarchical `SKILL.md` loader, compatible with PicoClaw's skill format
- **aarch64-first** — Cross-compiled for `aarch64-unknown-linux-gnu`; OpenSSL-free (`rustls`)
- **Hot reload** — `SIGHUP` reloads config and workspace without restarting the process

---

## Getting Started

### Prerequisites

- Rust 2024 edition toolchain (`rustup`)
- For Raspberry Pi 4 deployment: `aarch64-unknown-linux-gnu` target and cross-linker

```bash
rustup target add aarch64-unknown-linux-gnu
# Ubuntu/Debian
sudo apt install gcc-aarch64-linux-gnu
```

### Build (x86_64 / development)

```bash
cargo build --release -p rustyclaw-cli
```

### Run

```bash
./target/release/rustyclaw-cli \
  --config config/config.json \
  --workspace workspace/ \
  gateway
```

Start without a live LLM provider (no API calls, useful for local testing):

```bash
./target/release/rustyclaw-cli --no-agent gateway
```

---

## Documentation

| Document | 内容 |
|---|---|
| [Architecture spec](docs/specs/01_architecture.md) | 全体アーキテクチャ・開発環境 |
| [Agent pipeline](docs/specs/02_agent_pipeline.md) | パイプライン・LLM プロバイダ仕様 |
| [Workspace / storage](docs/specs/03_workspace_spec.md) | ワークスペースファイル・ストレージ仕様 |
| [Heartbeat system](docs/specs/04_heartbeat_spec.md) | Heartbeat 自発行動システム仕様 |
| [Gateway & concurrency](docs/specs/05_gateway_spec.md) | Gateway・Lane Queue 仕様 |
| [Web dashboard](docs/specs/06_dashboard_spec.md) | ダッシュボード・管理 API 仕様 |
| [Deploy to Raspberry Pi 4](docs/README.md#4-デプロイssh-接続手順rpi4--rp1) | クロスビルド・デプロイ手順 |
| [Operation guide](docs/specs/08_operation_inspection.md) | 稼働点検ガイド・コマンド集 |
| [Git collaboration rules](docs/specs/10_git_collaboration_rules.md) | GitHub 共同開発・運用ガイドライン仕様 |
