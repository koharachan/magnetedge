@echo off
:: 构建所有平台的二进制文件（Windows版本）

echo ===== 开始多平台构建 =====

:: 确保目标目录存在
if not exist dist mkdir dist

:: 1. 构建Windows x86_64版本
echo 正在构建Windows x86_64版本...
cargo build --release --target x86_64-pc-windows-msvc
copy target\x86_64-pc-windows-msvc\release\pow-client.exe dist\

:: 注意: 以下需要安装正确的交叉编译工具链才能在Windows上执行
:: 如果没有安装，可以通过GitHub Actions进行构建

:: 2. 构建Linux x86_64版本（需要cross等工具）
echo 如需构建Linux版本，请使用GitHub Actions或安装cross工具

:: 3. 构建Termux ARM64版本
echo 要在Termux (ARM64) 上构建，请直接在Termux中运行以下命令:
echo cargo build --release
echo.
echo 或使用Linux环境的交叉编译:
echo cargo build --release --target aarch64-linux-android

echo ===== 构建完成 =====
echo 二进制文件已保存到 .\dist 目录
dir dist 