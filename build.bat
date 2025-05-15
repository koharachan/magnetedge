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

echo ===== 构建完成 =====
echo 二进制文件已保存到 .\dist 目录
dir dist 