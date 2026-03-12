#!/bin/bash
# 版本发布脚本
# 用法: ./scripts/release.sh [patch|minor|major] 或 ./scripts/release.sh <version>

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"

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
    grep -m1 'version = "' "$CARGO_TOML" | sed 's/.*version = "\([^"]*\)".*/\1/'
}

# 计算新版本
bump_version() {
    local current="$1"
    local bump_type="$2"
    
    IFS='.' read -r major minor patch <<< "$current"
    
    case "$bump_type" in
        major)
            echo "$((major + 1)).0.0"
            ;;
        minor)
            echo "$major.$((minor + 1)).0"
            ;;
        patch)
            echo "$major.$minor.$((patch + 1))"
            ;;
        *)
            echo "$bump_type"  # 直接使用传入的版本号
            ;;
    esac
}

# 更新 Cargo.toml 中的版本
update_version() {
    local new_version="$1"
    sed -i "s/^version = \"[^\"]*\"/version = \"$new_version\"/" "$CARGO_TOML"
}

# 主流程
main() {
    cd "$PROJECT_ROOT"
    
    # 检查参数
    if [ -z "$1" ]; then
        log_error "用法: $0 [patch|minor|major|<version>]"
        exit 1
    fi
    
    # 检查工作区是否干净
    if ! git diff --quiet || ! git diff --cached --quiet; then
        log_error "工作区有未提交的更改，请先提交或暂存"
        exit 1
    fi
    
    local current_version=$(get_current_version)
    local new_version=$(bump_version "$current_version" "$1")
    
    log_info "当前版本: $current_version"
    log_info "新版本: $new_version"
    
    # 确认发布
    read -p "确认发布 v$new_version? [y/N] " confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        log_warn "已取消发布"
        exit 0
    fi
    
    # 更新版本号
    log_info "更新版本号..."
    update_version "$new_version"
    
    # 构建项目
    log_info "构建项目..."
    cargo leptos build --release
    
    # 运行测试
    log_info "运行测试..."
    cargo test --workspace
    
    # 提交版本变更
    log_info "提交版本变更..."
    git add -A
    git commit -m "chore: release v$new_version"
    
    # 创建标签
    log_info "创建 Git 标签..."
    git tag -a "v$new_version" -m "Release v$new_version"
    
    # 推送
    log_info "推送到远程仓库..."
    git push origin main --tags
    
    log_info "✅ 版本 v$new_version 发布成功！"
    log_info "下一步: 运行 ./scripts/deploy.sh v$new_version <目标路径> 进行部署"
}

main "$@"
