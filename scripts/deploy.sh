#!/bin/bash
# ==============================================================================
# RustyClaw Cross-Compile & Deploy Script for Raspberry Pi 4 (rp1)
# ==============================================================================
# このスクリプトは、開発機 (x64) から RPi4 (aarch64) へのクロスコンパイル、
# バイナリの識別リネーム (x64 / rpi4)、RPi4 (rp1) へのデプロイ、および
# context-mode (bun + Node.js) のセットアップ確認を自動化します。
# 本番専用ディレクトリ `production/` を活用した最新設計に対応しています。
# ==============================================================================

set -e

# オプション解析
BUILD_X64=false
for arg in "$@"; do
    case "$arg" in
        --x64) BUILD_X64=true ;;
    esac
done

# ディレクトリ設定
PROJECT_ROOT="/home/kazuaki/Projects/RustyClaw"
PROD_DIR="$PROJECT_ROOT/production"
PROD_BIN_DIR="$PROD_DIR/bin"
TARGET_RPI_DIR="~/.local/bin"

# 色出力用
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# コマンドの存在チェック
function check_command() {
    if ! command -v "$1" &> /dev/null; then
        echo -e "${RED}エラー: コマンド '$1' が見つかりません。インストールしてください。${NC}"
        exit 1
    fi
}

# 表示ヘッダー
echo -e "${BLUE}========================================================${NC}"
echo -e "${GREEN}🦖 RustyClaw Auto-Deploy Pipeline (with production/)${NC}"
echo -e "${BLUE}========================================================${NC}"

# production/bin ディレクトリの確保
mkdir -p "$PROD_BIN_DIR"

# 1. 開発機 (x64) 向けリリースビルド（--x64 指定時のみ）
if [ "$BUILD_X64" = true ]; then
    echo -e "\n${YELLOW}[x64] 開発機 (x64) 向けローカルリリースビルドを実行中...${NC}"
    cargo build --release
    cp "$PROJECT_ROOT/target/release/rustyclaw-cli" "$PROD_BIN_DIR/rustyclaw-x64"
    echo -e "${GREEN}✓ 開発機用バイナリを作成しました: production/bin/rustyclaw-x64${NC}"
fi

# 2. RPi4 (aarch64) 向けクロスビルド
echo -e "\n${YELLOW}[1/4] RPi4 (aarch64) 向けクロスコンパイルを実行中...${NC}"
check_command aarch64-linux-gnu-gcc
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    cargo build --release --target aarch64-unknown-linux-gnu

# バイナリの複製・リネーム
cp "$PROJECT_ROOT/target/aarch64-unknown-linux-gnu/release/rustyclaw-cli" "$PROD_BIN_DIR/rustyclaw-rpi4"
echo -e "${GREEN}✓ RPi4用バイナリを作成しました: production/bin/rustyclaw-rpi4${NC}"

# 3. RPi4 (rp1) へのデプロイ
echo -e "\n${YELLOW}[2/4] RPi4 (rp1) 上へのバイナリ配置を自動実行中...${NC}"

# SSH 接続確認
if ! ssh -q rp1 exit; then
    echo -e "${RED}エラー: 'ssh rp1' 接続に失敗しました。SSH 設定または RPi4 の電源を確認してください。${NC}"
    exit 1
fi

# NAS 共有経由でバイナリを ~/.local/bin/rustyclaw に配置
ssh rp1 "sudo systemctl stop rustyclaw && \
         mkdir -p $TARGET_RPI_DIR && \
         cp ~/Projects/RustyClaw/production/bin/rustyclaw-rpi4 $TARGET_RPI_DIR/rustyclaw && \
         chmod +x $TARGET_RPI_DIR/rustyclaw"

echo -e "${GREEN}✓ RPi4 側の ~/.local/bin/rustyclaw を更新しました。${NC}"

# 4. context-mode セットアップ確認（bun + Node.js 22 + context-mode npm パッケージ）
echo -e "\n${YELLOW}[3/4] context-mode セットアップ確認中...${NC}"

ssh rp1 'bash -s' << 'REMOTE_SETUP'
set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# --- bun ---
if [ -f "$HOME/.bun/bin/bun" ]; then
    BUN_VER=$("$HOME/.bun/bin/bun" --version 2>/dev/null || echo "unknown")
    echo -e "${GREEN}✓ bun ${BUN_VER} (既インストール)${NC}"
else
    echo -e "${YELLOW}  bun が見つかりません。インストールします...${NC}"
    curl -fsSL https://bun.sh/install | bash
    echo -e "${GREEN}✓ bun インストール完了${NC}"
fi

BUN="$HOME/.bun/bin/bun"

# --- nvm + Node.js 22 ---
export NVM_DIR="$HOME/.nvm"
if [ ! -f "$NVM_DIR/nvm.sh" ]; then
    echo -e "${YELLOW}  nvm が見つかりません。インストールします...${NC}"
    curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
    echo -e "${GREEN}✓ nvm インストール完了${NC}"
fi

source "$NVM_DIR/nvm.sh"

# Node.js v22 がインストール済みか確認
NODE22=$(ls "$NVM_DIR/versions/node/" 2>/dev/null | grep "^v22" | sort -V | tail -1 || true)
if [ -z "$NODE22" ]; then
    echo -e "${YELLOW}  Node.js v22 が見つかりません。インストールします...${NC}"
    nvm install 22
    NODE22=$(ls "$NVM_DIR/versions/node/" | grep "^v22" | sort -V | tail -1)
    echo -e "${GREEN}✓ Node.js ${NODE22} インストール完了${NC}"
else
    echo -e "${GREEN}✓ Node.js ${NODE22} (既インストール)${NC}"
fi

# --- context-mode npm パッケージ ---
BUNDLE="$NVM_DIR/versions/node/$NODE22/lib/node_modules/context-mode/cli.bundle.mjs"
if [ -f "$BUNDLE" ]; then
    # バージョンを cli.bundle.mjs の中から取得
    CM_VER=$(grep -o '"version":"[^"]*"' "$BUNDLE" | head -1 | cut -d'"' -f4 || echo "unknown")
    echo -e "${GREEN}✓ context-mode ${CM_VER} (既インストール): ${BUNDLE}${NC}"
else
    echo -e "${YELLOW}  context-mode が見つかりません。npm でインストールします...${NC}"
    # npm は nvm 経由で使用
    nvm use 22
    npm install -g context-mode
    BUNDLE="$NVM_DIR/versions/node/$NODE22/lib/node_modules/context-mode/cli.bundle.mjs"
    echo -e "${GREEN}✓ context-mode インストール完了${NC}"
fi

# --- 動作確認（bun で MCP initialize 送受信）---
echo -e "${YELLOW}  context-mode 起動テスト (bun)...${NC}"
mkdir -p /tmp/ctx-test/.context-mode
RESPONSE=$(echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"deploy-check","version":"0.1"}}}' \
    | CONTEXT_MODE_DIR=/tmp/ctx-test/.context-mode \
      CONTEXT_MODE_PLATFORM=custom-rustyclaw \
      timeout 5 "$BUN" "$BUNDLE" 2>/dev/null | head -1 || true)

if echo "$RESPONSE" | grep -q '"context-mode"'; then
    echo -e "${GREEN}✓ context-mode MCP 起動テスト OK${NC}"
else
    echo -e "${YELLOW}⚠ context-mode 起動テストの応答が取得できませんでした（サービス起動後に自動リトライされます）${NC}"
fi

REMOTE_SETUP

# 5. symlink 確認とサービスの再起動
echo -e "\n${YELLOW}[4/4] symlink 確認とサービスの再起動を実行中...${NC}"

ssh rp1 "if [ ! -L ~/.rustyclaw ]; then \
             echo '⚠ ~/.rustyclaw が symlink ではありません。手動で再構成が必要です。'; \
             echo '  run: ln -s ~/Projects/RustyClaw/production ~/.rustyclaw'; \
         else \
             echo '✓ ~/.rustyclaw -> \$(readlink ~/.rustyclaw)'; \
         fi"

# デーモンの再起動
if ssh rp1 "sudo systemctl restart rustyclaw" &> /dev/null; then
    echo -e "${GREEN}✓ RustyClaw サービスを正常に再起動しました！${NC}"
else
    echo -e "${YELLOW}注意: 'rustyclaw' サービスの再起動に失敗しました（サービス未登録または権限不足）。${NC}"
    echo -e "${YELLOW}手動で再起動する場合は RPi4 上で 'sudo systemctl restart rustyclaw' を実行してください。${NC}"
fi

echo -e "\n${BLUE}========================================================${NC}"
echo -e "${GREEN}🦖 本番用（production/）の全デプロイが正常に完了しました！${NC}"
echo -e "${BLUE}========================================================${NC}"
