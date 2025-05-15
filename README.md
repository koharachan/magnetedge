# Magnet POW 区块链挖矿客户端 (Rust版)

Magnet POW区块链挖矿客户端的Rust实现，性能更优，资源占用更低。

## 特点

- 高性能Rust实现，比原JavaScript版本快数倍
- 多线程挖矿，充分利用CPU资源
- 美观的命令行界面
- 支持多RPC节点切换
- 稳定的错误处理和自动重试
- 支持多平台：Windows、Linux、ARM64 Linux(如手机)和Android(Termux)

## 依赖项

- Rust 1.70+

## 安装方法

### 直接下载预编译版本

1. 前往[Releases页面](https://github.com/hotianbexuanto/POW-Client-Rust/releases)
2. 下载适合您操作系统的最新版本
3. 解压后直接运行可执行文件

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/hotianbexuanto/POW-Client-Rust.git
cd POW-Client-Rust

# 编译发布版本
cargo build --release

# 运行程序
./target/release/pow-client
```

### 在Termux (Android手机)上构建与运行

我们现在支持在手机Termux环境中编译和运行客户端！详情请查看[Termux构建指南](TERMUX_BUILD.md)。

简要步骤：
```bash
pkg install rust git build-essential make
git clone https://github.com/hotianbexuanto/POW-Client-Rust.git
cd POW-Client-Rust
cargo build --release
./target/release/pow-client
```

## 使用说明

1. 启动程序后，选择RPC节点
2. 输入您的私钥（以0x开头）
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

您也可以手动触发工作流程：
1. 在GitHub仓库页面点击"Actions"选项卡
2. 选择"Build and Release"工作流
3. 点击"Run workflow"按钮

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

如需帮助或讨论，请加入Telegram群：https://t.me/MagnetPOW

## 许可证

MIT License 

## 高性能挖矿优化版本

本项目已进行了高性能优化，使用以下技术实现最高计算效率：

- 使用 `rayon` 进行并行计算
- 使用 `tinykeccak` 高性能哈希计算
- 使用 `jemalloc` 高效内存分配器
- 启用CPU特定指令集（AVX、SSE等）
- 批量哈希计算，减少内存分配
- 精细的内存管理和缓冲池
- 编译器优化：LTO、单元生成、内联等

### 编译高性能版本

使用以下命令编译最高性能版本：

```bash
# Windows
cargo build --release

# Linux/macOS
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

编译后的二进制文件位于 `target/release/pow-client` (Linux/macOS) 或 `target/release/pow-client.exe` (Windows)。

### 运行性能优化建议

- 关闭不必要的后台程序以释放CPU资源
- 确保系统有足够的冷却能力
- 适当调整线程数，考虑CPU温度和功耗
- 在性能模式下运行操作系统
- 对于多CPU系统，建议使用 `taskset` (Linux) 或 `start /affinity` (Windows) 绑定到特定CPU
- 如果可能，考虑超频CPU以获得更好性能

### 性能监控

程序运行时会显示哈希速率信息，可以通过以下方式监控：

- 查看控制台输出的哈希速率
- 使用系统监控工具观察CPU利用率
- 监控温度，避免过热
- 比较不同配置下的性能 