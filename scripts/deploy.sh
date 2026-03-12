#!/bin/bash
# 版本化部署脚本
# 用法: ./scripts/deploy.sh [version] [target_dir]
# 示例: ./scripts/deploy.sh v0.1.0 /path/to/deploy/collector

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# 获取当前版本
get_current_version() {
    grep -m1 'version = "' "$PROJECT_ROOT/Cargo.toml" | sed 's/.*version = "\([^"]*\)".*/\1/'
}

# 默认值
VERSION="${1:-v$(get_current_version)}"
DEST_DIR="${2:-./collector}"

# 源路径
SRC_DIR="$PROJECT_ROOT"
SERVER_BIN="$SRC_DIR/target/release/server"
SITE_DIR="$SRC_DIR/target/site"
CONFIG_DIR="$SRC_DIR/config"

main() {
    log_info "开始部署 Super-Trainning-Collector"
    log_info "版本: $VERSION"
    log_info "目标目录: $DEST_DIR"
    
    # 检查源文件
    if [ ! -f "$SERVER_BIN" ]; then
        log_error "找不到 server 二进制文件: $SERVER_BIN"
        log_error "请先运行: cargo leptos build --release"
        exit 1
    fi
    
    if [ ! -d "$SITE_DIR" ]; then
        log_error "找不到静态资源目录: $SITE_DIR"
        exit 1
    fi
    
    # 创建目标目录
    mkdir -p "$DEST_DIR"
    mkdir -p "$DEST_DIR/target/site"
    mkdir -p "$DEST_DIR/config"
    
    # 备份旧版本（如果存在）
    if [ -f "$DEST_DIR/VERSION" ]; then
        OLD_VERSION=$(cat "$DEST_DIR/VERSION")
        log_info "备份旧版本: $OLD_VERSION"
        BACKUP_DIR="$DEST_DIR.backup.$OLD_VERSION.$(date +%Y%m%d%H%M%S)"
        cp -r "$DEST_DIR" "$BACKUP_DIR"
        log_info "备份已保存到: $BACKUP_DIR"
    fi
    
    # 部署 server 二进制
    log_info "部署 server 二进制..."
    rm -f "$DEST_DIR/server"
    cp "$SERVER_BIN" "$DEST_DIR/server"
    chmod +x "$DEST_DIR/server"
    
    # 部署静态资源
    log_info "部署静态资源..."
    rm -rf "$DEST_DIR/target/site"
    mkdir -p "$DEST_DIR/target/site"
    cp -r "$SITE_DIR/." "$DEST_DIR/target/site/"
    
    # 部署配置文件（仅首次或强制）
    if [ ! -f "$DEST_DIR/config/collector.json" ] || [ "$FORCE_CONFIG" = "1" ]; then
        log_info "部署配置文件..."
        cp -r "$CONFIG_DIR/." "$DEST_DIR/config/"
    else
        log_warn "配置文件已存在，跳过（设置 FORCE_CONFIG=1 强制覆盖）"
    fi
    
    # 复制启动脚本
    log_info "部署启动脚本..."
    cp "$SRC_DIR/run_server.sh" "$DEST_DIR/"
    chmod +x "$DEST_DIR/run_server.sh"
    
    # 写入版本信息
    echo "$VERSION" > "$DEST_DIR/VERSION"
    cat > "$DEST_DIR/BUILD_INFO" << EOF
Version: $VERSION
Build Date: $(date -Iseconds)
Git Commit: $(cd "$SRC_DIR" && git rev-parse --short HEAD 2>/dev/null || echo "unknown")
Git Branch: $(cd "$SRC_DIR" && git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
EOF
    
    log_info "✅ 部署完成！"
    log_info ""
    log_info "启动服务:"
    log_info "  cd $DEST_DIR && ./run_server.sh"
    log_info ""
    log_info "版本信息:"
    cat "$DEST_DIR/BUILD_INFO"
}

main
