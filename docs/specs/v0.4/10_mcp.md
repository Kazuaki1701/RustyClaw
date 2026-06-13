# RustyClaw — MCP クライアント仕様

> [!NOTE]
> **ステータス**: `[実装済]`（堅牢化・トランスポート拡張は `[将来拡張]`）
> **バージョン**: v0.3
> **最終更新日**: 2026-06-11
> **参照元**: [`00_rustyclaw.md`](00_rustyclaw.md)

---

## 1. 現行実装 `[実装済]`

### 1.1 アーキテクチャ上の位置づけ

`rustyclaw-tools` クレート内の `McpClientHandler` が外部 MCP サーバーとの接続を管理する。
ToolRegistry を通じてインプロセスツールと同一のインターフェースで LLM に公開される。

```
rustyclaw-agent (Pipeline)
└── ExecuteTools
    └── ToolRegistry
        ├── built-in tools（インプロセス）
        └── McpClientHandler（外部 MCP プロキシ）
            └── stdio サブプロセス（rmcp）
```

### 1.2 採用クレート

| クレート | 役割 |
|---|---|
| `rmcp` | Rust 公式 MCP SDK。stdio transport による外部 MCP サーバー接続 |

### 1.3 接続方式（stdio transport）

MCP サーバーを子プロセス（stdio）としてスポーンし、JSON-RPC over stdio で通信する。
プロセスのライフサイクルは `McpClientHandler` が管理する。

**廃止済み方式との対比**:

| 方式 | 採用 | 理由 |
|---|---|---|
| stdio サブプロセス（rmcp） | ✅ 現行 | Rust ネイティブ、外部依存なし |
| Node.js MCP サーバー常駐 | ❌ 廃止 | Node.js 依存、プロセス管理複雑 |
| SSE transport（リモート） | ❌ 未実装 | → §2.3 参照 |

---

## 2. 将来拡張（Phase 26）`[将来拡張]`

> **着手条件**: 外部 MCP サーバーが整備され、実運用で接続断・メモリ圧迫が問題になった時点で着手。

### 2.1 Auto-Reconnect `[将来拡張]`

**課題**: MCP サーバープロセスが OOM やパニックでクラッシュした場合、`McpClientHandler` の接続がそのまま無効になり、次回ツール呼び出しがエラーになる。

**設計方針**:
- ツール呼び出し時に接続状態を検証し、断絶を検知したら即座に再スポーン・再接続を試みる
- 最大リトライ回数（例: 3 回）と Exponential Backoff を設ける
- 再接続失敗時はツール呼び出しをエラーとして LLM に返し、パイプラインは継続する（fail-open）

**実装対象**: `crates/rustyclaw-tools/src/` の `McpClientHandler` / `ToolServerHandle`

### 2.2 Idle Eviction `[将来拡張]`

**課題**: RPi4（RAM 8GB）上で複数の MCP サーバー（Node.js / Python ベース）が常駐すると、長期間使われない場合もメモリを消費し続ける。

**設計方針**:
- `McpClientHandler` に最終使用時刻を記録し、アイドルタイムアウト（例: 30 分）超過で子プロセスを `SIGTERM` → `kill` の順で終了
- 次の呼び出しで再スポーンする（Auto-Reconnect と連携）
- アイドルタイムアウトは設定ファイルから変更可能にする

**実装対象**: `crates/rustyclaw-gateway/src/lib.rs` 内のアイドル監視タスク

### 2.3 SSE Transport `[将来拡張]`

**課題**: 現状は stdio サブプロセスのみ。リモートホスト（別マシン・クラウド）上の MCP サーバーには接続できない。

**設計方針**:
- `rmcp` の SSE / HTTP transport フィーチャーを有効化し、`McpClientHandler` をトランスポート抽象に対応させる
- 設定ファイルでサーバーごとに `transport: "stdio"` / `"sse"` を切り替え可能にする
- SSE 接続は自動再接続（§2.1）と組み合わせて使用する

**前提条件**: リモート MCP サーバーを実際に運用する具体的なユースケースが確定してから着手する。
