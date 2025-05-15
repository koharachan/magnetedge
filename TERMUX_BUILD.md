# Termux (ARM64) 构建指南

本文档提供在Android手机上使用Termux应用构建和运行POW-Client-Rust的详细步骤。

## 前期准备

1. 在手机上安装[Termux](https://termux.dev/en/)应用
2. 在Termux中安装基础开发环境

```bash
pkg update && pkg upgrade
pkg install rust git build-essential make
```

## 克隆项目

```bash
git clone https://github.com/MagnetPOW/POW-Client-Rust.git
cd POW-Client-Rust
```

## 构建项目

在Termux中，直接使用Cargo进行构建：

```bash
cargo build --release
```

构建完成后，可执行文件将位于`target/release/pow-client`

## 运行客户端

```bash
./target/release/pow-client
```

## 性能优化建议

在Termux/ARM64环境中：

1. 使用优化的线程数 - ARM处理器核心通常比桌面CPU少，建议设置合理的线程数
2. 监控温度 - 挖矿会产生大量热量，请留意手机发热情况
3. 使用外部电源 - 挖矿耗电较大，建议连接电源
4. 后台运行 - 可以使用tmux或screen工具让挖矿在后台进行：

```bash
pkg install tmux
tmux new -s mining
# 在tmux会话中启动挖矿客户端
./target/release/pow-client
# 使用Ctrl+b然后按d分离会话
# 使用以下命令重新连接会话
tmux attach -t mining
```

## 常见问题

1. 构建失败？ - 确保已安装所有必要的开发包
2. 性能不佳？ - 尝试减少线程数(`--threads`参数)
3. 应用崩溃？ - 检查日志，可能是由于内存不足或设备过热

## 注意事项

移动设备通常功耗和散热有限，长时间高负载运行可能导致设备过热或电池损耗，请谨慎使用。 