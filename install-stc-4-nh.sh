#!/bin/bash

# --- 1. 环境预检 ---
# 检查是否为 source 模式，避免退出 Shell
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "[STC]Error: This script must be SOURCED."
    exit 1
fi

# 检查关键环境变量 OUTPUT_DIR 是否存在
if [ -z "$OUTPUT_DIR" ]; then
    echo "[STC]ERROR: Environment variable 'OUTPUT_DIR' is NOT set."
    return 1
fi

# 检查该共享路径在当前节点是否挂载/存在
if [ ! -d "$OUTPUT_DIR" ]; then
    echo "[STC]ERROR: Shared directory '$OUTPUT_DIR' not found."
    echo "[STC]Please ensure the shared file system (Lustre/NFS) is correctly mounted on this node."
    return 1
fi

# --- 2. 确定集群 RANK ---
# 兼容 Slurm (南湖平台常用), PyTorch (RANK), 和 MPI
MY_RANK=${RANK:-${SLURM_PROCID:-${OMPI_COMM_WORLD_RANK:-0}}}

# --- 3. 只有 RANK 0 执行下载到共享路径 ---
if [ "$MY_RANK" -eq 0 ]; then
    echo "[STC][RANK 0] Shared directory detected. Checking for updates..."

    # 使用 -N (仅更新) 和 -P (指定路径)
    wget -N -P "$OUTPUT_DIR" \
            https://gitlab.zhejianglab.com/wangqi/probing-release/-/raw/main/probing-0.2.0alpha1-py3-none-manylinux_2_12_x86_64.manylinux2010_x86_64.whl \
            || echo "[STC][ERROR] Failed to download .whl file (Exit code: $?)"

    wget -N -P "$OUTPUT_DIR" \
            https://gitlab.zhejianglab.com/wangqi/probing-release/-/raw/main/super-training-collector_0.1.1.deb \
            || echo "[STC][ERROR] Failed to download .deb file (Exit code: $?)"

else
    # 非 RANK 0 节点等待共享文件可见 (处理存储系统延迟)
    # 同时判断 .whl 和 .deb 文件是否存在
    TIMEOUT=30
    WHL_FILE="$OUTPUT_DIR/probing-0.2.0alpha1-py3-none-manylinux_2_12_x86_64.manylinux2010_x86_64.whl"
    DEB_FILE="$OUTPUT_DIR/super-training-collector_0.1.1.deb"

    echo "[STC][RANK $MY_RANK] Waiting for shared files to be synchronized..."

    while { [ ! -f "$WHL_FILE" ] || [ ! -f "$DEB_FILE" ]; } && [ $TIMEOUT -gt 0 ]; do
        sleep 1
        ((TIMEOUT--))
    done

    if [ $TIMEOUT -eq 0 ]; then
        echo "[STC][ERROR] Timeout waiting for shared files in $OUTPUT_DIR"
    else
        echo "[STC][RANK $MY_RANK] All files detected."
    fi
fi

# --- 4. 安装与环境变量设置 ---
# 使用刚刚下载到共享目录的文件进行安装
if [ "$MY_RANK" -ne 0 ]; then
    sleep $((RANDOM % 4))
fi
pip install "$OUTPUT_DIR/probing-0.2.0alpha1-py3-none-manylinux_2_12_x86_64.manylinux2010_x86_64.whl" --force-reinstall

# 仅RANK0才需要安装可视化工具
if [ "$MY_RANK" -eq 0 ]; then
    dpkg -i "$OUTPUT_DIR/super-training-collector_0.1.1.deb"

    export LEPTOS_SITE_ROOT="/opt/super-training-collector/site"
    export LEPTOS_SITE_ADDR="0.0.0.0:3000"
    export LEPTOS_ENV="PROD"
    export COLLECTOR_CONFIG_PATH="/opt/super-training-collector/config/collector.json"

    export HANG_CHECK_ENABLED=true
fi

# 所有节点启动探针功能
export NH_PLATFORM=false
export PROBING=1
export PROBING_PORT=9933

# --- 5. 启动 Dashboard ---
if [ "$MY_RANK" -eq 0 ]; then
    if ! pgrep -x "probing-monitor" > /dev/null; then
        # 生产环境务必将日志重定向到 /tmp 或指定的 Log 目录，避免阻塞 stdout
        nohup probing-monitor > /tmp/probing_monitor_$(hostname).log 2>&1 &
        echo "[STC]Probing monitor started on $(hostname)."
    fi
fi

echo "====================== [STC]Environment prepare done! ======================"
