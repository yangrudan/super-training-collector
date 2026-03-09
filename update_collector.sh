#!/bin/bash

############ **************** 使用说明 ************************************############
#
############# 将此文件拷贝到干净的目录下, 在对应目录的collector目录进行拷贝############

# 1. 定义源路径（建议加引号）
SRC_DIR="/home/yang/135-data/data/workspace/super-trainning-collector"
# 2. 定义目标根路径（既然你在本地，建议用绝对路径或明确的相对路径）
DEST_DIR="./collector"

# 确保目标父目录存在
mkdir -p "$DEST_DIR"

# --- 第一部分：处理 server 二进制 ---
# 先删除旧的，确保环境干净
rm -rf "$DEST_DIR/server"
# 注意：这里直接拷贝到 $DEST_DIR 下
cp -r "$SRC_DIR/target/release/server" "$DEST_DIR/"

# --- 第二部分：处理 site 静态资源 ---
# 清空或删除旧的 site 目录
rm -rf "$DEST_DIR/target/site"
mkdir -p "$DEST_DIR/target/site"
# 使用 /. 拷贝内容，避免目录嵌套
cp -r "$SRC_DIR/target/site/." "$DEST_DIR/target/site/"

# --- 第三部分： 处理配置文件---
# 先删除旧的，确保环境干净
rm -rf "$DEST_DIR/config"
mkdir -p "$DEST_DIR/config"
# 注意：这里直接拷贝到 $DEST_DIR/config 下
cp -r "$SRC_DIR/config/." "$DEST_DIR/config"

echo "部署完成！"
