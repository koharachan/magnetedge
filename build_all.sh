#!/bin/bash
# 构建所有平台的二进制文件

set -e

echo "===== 开始多平台构建 ====="

# 确保目标目录存在
mkdir -p ./dist

# 1. 构建Windows x86_64版本
echo "正在构建Windows x86_64版本..."
cargo build --release --target x86_64-pc-windows-msvc
cp target/x86_64-pc-windows-msvc/release/pow-client.exe ./dist/

# 2. 构建Linux x86_64版本
echo "正在构建Linux x86_64版本..."
cargo build --release --target x86_64-unknown-linux-gnu
cp target/x86_64-unknown-linux-gnu/release/pow-client ./dist/pow-client-linux-x86_64

# 3. 构建Linux ARM64版本
echo "正在构建Linux ARM64版本..."
cargo build --release --target aarch64-unknown-linux-gnu
cp target/aarch64-unknown-linux-gnu/release/pow-client ./dist/pow-client-linux-arm64

echo "===== 构建完成 ====="
echo "所有二进制文件已保存到 ./dist 目录"
ls -la ./dist 