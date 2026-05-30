#!/bin/bash
# ==============================================================================
# RustyClaw Cross-Compile & Deploy Script for Raspberry Pi 4 (rp1)
# ==============================================================================
# このスクリプトは、開発機 (x64) から RPi4 (aarch64) へのクロスコンパイル、
# バイナリの識別リネーム (x64 / rpi4)、および RPi4 (rp1) へのデプロイを自動化します。
# 本番専用ディレクトリ `production/` を活用した最新設計に対応しています。
# ==============================================================================

set -e

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

# 1. 開発機 (x64) 向けリリースビルド
echo -e "\n${YELLOW}[1/4] 開発機 (x64) 向けローカルリリースビルドを実行中...${NC}"
cargo build --release

# バイナリの複製・リネーム
cp "$PROJECT_ROOT/target/release/rustyclaw-cli" "$PROD_BIN_DIR/rustyclaw-x64"
echo -e "${GREEN}✓ 開発機用バイナリを作成しました: production/bin/rustyclaw-x64${NC}"

# 2. RPi4 (aarch64) 向けクロスビルド
echo -e "\n${YELLOW}[2/4] RPi4 (aarch64) 向けクロスコンパイルを実行中...${NC}"
check_command cross
cross build --release --target aarch64-unknown-linux-gnu

# バイナリの複製・リネーム
cp "$PROJECT_ROOT/target/aarch64-unknown-linux-gnu/release/rustyclaw-cli" "$PROD_BIN_DIR/rustyclaw-rpi4"
echo -e "${GREEN}✓ RPi4用バイナリを作成しました: production/bin/rustyclaw-rpi4${NC}"

# 3. RPi4 (rp1) へのデプロイ
echo -e "\n${YELLOW}[3/4] RPi4 (rp1) 上へのバイナリ配置を自動実行中...${NC}"

# SSH 接続確認
if ! ssh -q rp1 exit; then
    echo -e "${RED}エラー: 'ssh rp1' 接続に失敗しました。SSH 設定または RPi4 の電源を確認してください。${NC}"
    exit 1
fi

# NAS 共有経由でバイナリを ~/.local/bin/rustyclaw に配置
# （~/.rustyclaw は production/ への symlink のため、バイナリは ~/.local/bin/ に置く）
ssh rp1 "sudo systemctl stop rustyclaw && \
         mkdir -p $TARGET_RPI_DIR && \
         cp ~/Projects/RustyClaw/production/bin/rustyclaw-rpi4 $TARGET_RPI_DIR/rustyclaw && \
         chmod +x $TARGET_RPI_DIR/rustyclaw"

echo -e "${GREEN}✓ RPi4 側の ~/.local/bin/rustyclaw を更新しました。${NC}"

# ~/.rustyclaw -> production/ symlink の確認（初回以降は不要）
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
