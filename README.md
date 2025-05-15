# Magnet POW 区块链挖矿客户端 (Rust版)

Magnet POW区块链挖矿客户端的Rust实现，性能更优，资源占用更低。

## 特点

- 高性能Rust实现，比原JavaScript版本快数倍
- 多线程挖矿，充分利用CPU资源
- 美观的命令行界面
- 支持多RPC节点切换
- 稳定的错误处理和自动重试
- 支持多平台：Windows、Linux和ARM64 Linux(如手机)

## 依赖项

- Rust 1.70+

## 安装方法

### 直接下载预编译版本

1. 前往[Releases页面](https://github.com/koharachan/magnetedge/releases)
2. 下载适合您操作系统的最新版本
3. 解压后直接运行可执行文件

### 从源码编译

```bash
# 克隆仓库并切换目录
git clone https://github.com/koharachan/magnetedge.git && cd magnetedge

# 编译
cargo build

# 运行程序
./target/release/pow-client
```

## 使用说明

1. 启动程序后，选择RPC节点
2. 输入您的私钥
3. 程序会自动检查余额并开始挖矿
4. 挖矿成功会自动获取奖励

## GitHub Actions自动构建

本项目使用GitHub Actions自动构建多平台可执行文件：
- Windows (x86_64)
- Linux (x86_64)
- Linux ARM64 (适用于ARM设备，如手机)

每当推送带有`v`前缀的标签（如`v0.1.0`）时，会自动触发构建流程并发布Release。

```bash
# 创建新版本并推送标签
git tag v0.1.0
git push origin v0.1.0
```

您也可以使用项目根目录下的脚本在本地构建所有平台版本：
- Linux/macOS: `./build_all.sh`
- Windows: `build_all.bat`

## 技术说明

- 使用`ethers-rs`库处理区块链交互
- 使用`tokio`进行异步操作
- 多线程POW挖矿算法
- 使用`colored`和`indicatif`提供友好的终端界面

## 安全提示

- 请确保私钥安全，不要在不信任的设备上使用
- 建议为挖矿创建单独的钱包
- 本程序不会存储或传输您的私钥

## Telegram群

如需该代币帮助或讨论，请加入官方Telegram群：[@MagnetPOW](https://t.me/MagnetPOW)
或 [@koharachans](https://t.me/koharachans)

## 许可证

MIT License 
