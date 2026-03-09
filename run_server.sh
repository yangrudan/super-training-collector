#!/bin/bash

# 1. 设置环境变量覆盖默认配置
# 指定静态资源（JS/WASM/CSS）的存放目录
export LEPTOS_SITE_ROOT="target/site"

# 监听地址设为 0.0.0.0，允许局域网或公网访问
export LEPTOS_SITE_ADDR="0.0.0.0:3000"

# 设置运行模式为生产环境（PROD），这通常会影响日志级别和资源加载逻辑
export LEPTOS_ENV="PROD"

# 2. 打印提示信息（可选，方便调试）
echo "Starting Leptos server at $LEPTOS_SITE_ADDR..."
echo "Site root is set to: $LEPTOS_SITE_ROOT"

# 3. 执行服务器二进制文件
# 确保 ./server 文件在当前目录下，并且具有执行权限
./server
