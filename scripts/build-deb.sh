#!/bin/bash
set -e

# Super Training Collector - DEB Package Builder
# 
# 此脚本仅打包已构建好的产物，不执行编译，适合在生产环境使用。
# 编译请在开发机器上运行: cargo leptos build --release
#
# Usage: ./scripts/build-deb.sh [--with-build]
#   --with-build  同时执行编译（仅限开发机器使用）

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
VERSION="0.1.0"
PACKAGE_NAME="super-training-collector"
BUILD_DIR="$PROJECT_ROOT/build-deb"
DEB_ROOT="$BUILD_DIR/${PACKAGE_NAME}_${VERSION}"

WITH_BUILD=false
for arg in "$@"; do
    case $arg in
        --with-build)
            WITH_BUILD=true
            shift
            ;;
    esac
done

echo "=== Super Training Collector DEB Package Builder ==="

# Step 0: Optional build (only with --with-build flag)
if [ "$WITH_BUILD" = true ]; then
    echo "[0/5] Building release binary (--with-build mode)..."
    echo "WARNING: This will consume significant CPU/memory resources."
    read -p "Continue? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 1
    fi
    cd "$PROJECT_ROOT"
    if ! command -v cargo-leptos &> /dev/null; then
        echo "ERROR: cargo-leptos not found. Please install it first:"
        echo "  cargo install cargo-leptos"
        exit 1
    fi
    cargo leptos build --release
fi

# Step 1: Verify required files exist
echo "[1/5] Verifying build artifacts..."
REQUIRED_FILES=(
    "$PROJECT_ROOT/target/release/server"
    "$PROJECT_ROOT/target/site/pkg/super-trainning-collector.wasm"
    "$PROJECT_ROOT/target/site/pkg/super-trainning-collector.js"
)
for f in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$f" ]; then
        echo "ERROR: Required file not found: $f"
        echo ""
        echo "Please build the project first on a development machine:"
        echo "  cargo leptos build --release"
        echo ""
        echo "Or use --with-build flag (not recommended for production servers):"
        echo "  ./scripts/build-deb.sh --with-build"
        exit 1
    fi
done
echo "All required artifacts found."

# Step 2: Create DEB directory structure
echo "[2/5] Creating package structure..."
if [ -d "$BUILD_DIR" ]; then
    echo "Removing existing build directory: $BUILD_DIR"
    rm -rf "$BUILD_DIR"
fi
mkdir -p "$DEB_ROOT/DEBIAN"
mkdir -p "$DEB_ROOT/opt/$PACKAGE_NAME/site/pkg"
mkdir -p "$DEB_ROOT/opt/$PACKAGE_NAME/config"
mkdir -p "$DEB_ROOT/etc/systemd/system"

# Step 3: Copy files
echo "[3/5] Copying files..."
# Binary
cp "$PROJECT_ROOT/target/release/server" "$DEB_ROOT/opt/$PACKAGE_NAME/"
chmod 755 "$DEB_ROOT/opt/$PACKAGE_NAME/server"

# Static assets
cp -r "$PROJECT_ROOT/target/site/"* "$DEB_ROOT/opt/$PACKAGE_NAME/site/"

# Config files
if [ -d "$PROJECT_ROOT/config" ]; then
    cp -r "$PROJECT_ROOT/config/"* "$DEB_ROOT/opt/$PACKAGE_NAME/config/" 2>/dev/null || true
fi

# DEBIAN control files
cp "$PROJECT_ROOT/debian/control" "$DEB_ROOT/DEBIAN/"
cp "$PROJECT_ROOT/debian/postinst" "$DEB_ROOT/DEBIAN/"
cp "$PROJECT_ROOT/debian/prerm" "$DEB_ROOT/DEBIAN/"
chmod 755 "$DEB_ROOT/DEBIAN/postinst"
chmod 755 "$DEB_ROOT/DEBIAN/prerm"

# Systemd service
cp "$PROJECT_ROOT/debian/super-training-collector.service" "$DEB_ROOT/etc/systemd/system/"

# Step 4: Calculate installed size
echo "[4/5] Calculating package size..."
INSTALLED_SIZE=$(du -sk "$DEB_ROOT" | cut -f1)
sed -i "s/^Version:.*/Version: $VERSION/" "$DEB_ROOT/DEBIAN/control"
echo "Installed-Size: $INSTALLED_SIZE" >> "$DEB_ROOT/DEBIAN/control"

# Step 5: Build the DEB package
echo "[5/5] Building DEB package..."
cd "$BUILD_DIR"
dpkg-deb --build "${PACKAGE_NAME}_${VERSION}"
mv "${PACKAGE_NAME}_${VERSION}.deb" "$PROJECT_ROOT/"

# Cleanup
rm -rf "$BUILD_DIR"

echo ""
echo "=== Build Complete ==="
echo "Package: $PROJECT_ROOT/${PACKAGE_NAME}_${VERSION}.deb"
echo ""
echo "Install with: sudo dpkg -i ${PACKAGE_NAME}_${VERSION}.deb"
echo "Service management:"
echo "  sudo systemctl status super-training-collector"
echo "  sudo systemctl restart super-training-collector"
echo "  sudo journalctl -u super-training-collector -f"
