use anyhow::{anyhow, Result};
use colored::*;
use dialoguer::{Input, Select};
use ethers::{
    abi::Token,
    prelude::*,
    providers::{Http, Provider},
};
use indicatif::{ProgressBar, ProgressStyle};
use parking_lot::Mutex;
use std::{
    convert::TryFrom,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::time::sleep;

mod contract;
mod hash_engine;
mod memory;
mod parallel;

use contract::MiningContract;
use parallel::ParallelMiner;

// 定义常量
const CONTRACT_ADDRESS: &str = "0x51e0ab7f7db4a2bf4500dfa59f7a4957afc8c02e";
const RPC_OPTIONS: [&str; 4] = [
    "https://node1.magnetchain.xyz",
    "https://node2.magnetchain.xyz",
    "https://node3.magnetchain.xyz",
    "https://node4.magnetchain.xyz",
];
const MIN_WALLET_BALANCE: f64 = 0.1;
const MIN_CONTRACT_BALANCE: f64 = 3.0;
const MINING_TIMEOUT_SECS: u64 = 600; // 10分钟
                                      // MagnetChain的chainId
const CHAIN_ID: u64 = 114514; // 修正为正确的链ID

// 全局矿工实例
lazy_static::lazy_static! {
    static ref GLOBAL_MINER: Mutex<Option<ParallelMiner>> = Mutex::new(None);
}

#[cfg(target_os = "android")]
fn is_running_in_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

#[tokio::main]
async fn main() -> Result<()> {
    print_welcome_message();

    // 选择RPC节点
    let rpc_url = select_rpc_node()?;
    println!(
        "{}",
        format!("已选择 RPC / Selected RPC: {}", rpc_url).green()
    );

    // 初始化以太坊提供者
    let provider = Provider::<Http>::try_from(rpc_url)?;

    // 显示链ID信息
    match provider.get_chainid().await {
        Ok(chainid) => {
            println!(
                "{}",
                format!(
                    "连接到链ID: {} / Connected to chain ID: {}",
                    chainid, chainid
                )
                .green()
            );
            if chainid != U256::from(CHAIN_ID) {
                println!("{}", format!("警告：检测到的链ID与设置的不符！ / Warning: Detected chain ID does not match configuration!").yellow());
            }
        }
        Err(e) => {
            println!(
                "{}",
                format!("无法获取链ID: {} / Could not get chain ID: {}", e, e).yellow()
            );
        }
    }

    // 输入私钥并创建钱包
    let wallet = input_private_key(provider).await?;
    let wallet_address = wallet.address();
    println!(
        "{}",
        format!("钱包地址 / Wallet address: {}", wallet_address).green()
    );

    // 检查钱包余额
    let _balance = check_wallet_balance(&wallet).await?;

    // 初始化合约
    let contract = init_contract(wallet).await?;

    // 检查合约余额
    check_contract_balance(&contract).await?;

    // 设置并行任务数
    let parallel_tasks = input_parallel_tasks()?;

    // 初始化全局挖矿实例
    let thread_count = std::cmp::max(num_cpus::get() + 2, 6); // 使用CPU核心数+2，最少6个线程
    *GLOBAL_MINER.lock() = Some(ParallelMiner::new(thread_count));

    println!(
        "{}",
        format!(
            "已初始化挖矿引擎：使用{}个线程 / Mining engine initialized: using {} threads",
            thread_count, thread_count
        )
        .green()
    );

    // 开始挖矿循环
    println!("{}", "\n挖矿模式 / Mining Mode:".bold());
    println!(
        "{}",
        "免费挖矿 (3 MAG 每次哈希) / Free Mining (3 MAG per hash)".cyan()
    );
    println!("{}", "\n开始挖矿 / Starting mining...".bold().green());

    start_mining_loop(contract, parallel_tasks).await?;

    #[cfg(target_os = "android")]
    if is_running_in_termux() {
        println!("{}", "检测到Termux环境，已启用相应优化".green().bold());
        // 在Termux环境中，我们可能需要调整线程数以避免过热
        let cpu_count = num_cpus::get();
        let suggested_threads = std::cmp::max(1, cpu_count.saturating_sub(1));

        if thread_count > suggested_threads && thread_count == num_cpus::get() {
            println!(
                "{}",
                format!(
                    "建议: 在Termux环境中，建议使用 {} 个线程以减少发热",
                    suggested_threads
                )
                .yellow()
                .bold()
            );
        }
    }

    Ok(())
}

fn print_welcome_message() {
    println!(
        "{}",
        " 你好，欢迎使用 Magnet POW 区块链挖矿客户端！ "
            .bold()
            .on_cyan()
            .black()
    );
    println!(
        "{}",
        " Hello, welcome to Magnet POW Blockchain Mining Client! "
            .bold()
            .on_cyan()
            .black()
    );
    println!(
        "{}",
        "启动挖矿客户端，需要确保钱包里有0.1MAG，如果没有，加入TG群免费领取0.1 MAG空投。"
            .bold()
            .magenta()
    );
    println!("{}", "To start the mining client, ensure your wallet has 0.1 MAG. If not, join the Telegram group for a free 0.1 MAG airdrop.".bold().magenta());
    println!(
        "{}",
        "TG群链接 / Telegram group link: https://t.me/MagnetPOW"
            .bold()
            .magenta()
    );
    println!(
        "{}",
        format!(
            "网络信息 / Network Info: 链ID / Chain ID: {}, 货币符号 / Symbol: MAG",
            CHAIN_ID
        )
        .cyan()
    );
}

fn select_rpc_node() -> Result<&'static str> {
    println!("{}", "\n选择 RPC 节点 / Select RPC Node:".bold());

    for (i, rpc) in RPC_OPTIONS.iter().enumerate() {
        println!("{}", format!("{}. {}", i + 1, rpc).cyan());
    }

    let selection = Select::new()
        .with_prompt("选择节点 / Select node")
        .items(&RPC_OPTIONS)
        .default(0)
        .interact()?;

    Ok(RPC_OPTIONS[selection])
}

async fn input_private_key<P: JsonRpcClient + 'static + Clone>(
    provider: Provider<P>,
) -> Result<SignerMiddleware<Provider<P>, LocalWallet>> {
    let max_attempts = 3;
    let mut attempts = 0;

    while attempts < max_attempts {
        let private_key: String = Input::new()
            .with_prompt("\n请输入私钥 / Enter private key (starts with 0x)")
            .validate_with(|input: &String| -> Result<(), &str> {
                if input.starts_with("0x") && input.len() == 66 && hex::decode(&input[2..]).is_ok() {
                    Ok(())
                } else {
                    Err("私钥格式错误：需以0x开头，后面跟64位十六进制字符 / Invalid private key: Must start with 0x followed by 64 hexadecimal characters")
                }
            })
            .interact()?;

        match private_key.parse::<LocalWallet>() {
            Ok(mut wallet) => {
                // 设置钱包的chainId
                wallet = wallet.with_chain_id(CHAIN_ID);
                println!(
                    "{}",
                    format!(
                        "已设置钱包chainId为: {} / Set wallet chainId to: {}",
                        CHAIN_ID, CHAIN_ID
                    )
                    .green()
                );

                let client = SignerMiddleware::new(provider.clone(), wallet);
                return Ok(client);
            }
            Err(e) => {
                attempts += 1;
                eprintln!(
                    "{}",
                    format!(
                        "私钥解析错误 / Private key parsing error: {}. 还剩 {} 次尝试。 / {} attempts left.",
                        e,
                        max_attempts - attempts,
                        max_attempts - attempts
                    )
                    .red()
                );

                if attempts == max_attempts {
                    return Err(anyhow!(
                        "达到最大尝试次数，程序退出 / Max attempts reached, exiting."
                    ));
                }
            }
        }
    }

    Err(anyhow!("无法解析私钥 / Unable to parse private key"))
}

async fn check_wallet_balance<M: Middleware + 'static>(
    wallet: &SignerMiddleware<M, LocalWallet>,
) -> Result<U256> {
    let balance = wallet.get_balance(wallet.address(), None).await?;
    println!(
        "{}",
        format!(
            "当前余额 / Current balance: {} MAG",
            ethers::utils::format_ether(balance)
        )
        .green()
    );

    let min_balance = ethers::utils::parse_ether(MIN_WALLET_BALANCE)?;
    if balance < min_balance {
        return Err(anyhow!(
            "钱包余额不足 / Insufficient balance: {} MAG (需要至少 {} MAG / Requires at least {} MAG)\n请通过 Telegram 群领取免费 MAG 或充值 / Please claim free MAG via Telegram or fund the wallet.",
            ethers::utils::format_ether(balance),
            MIN_WALLET_BALANCE,
            MIN_WALLET_BALANCE
        ));
    }

    Ok(balance)
}

async fn init_contract<M: Middleware + 'static>(
    wallet: SignerMiddleware<M, LocalWallet>,
) -> Result<MiningContract<SignerMiddleware<M, LocalWallet>>> {
    let contract_address = CONTRACT_ADDRESS.parse::<Address>()?;

    // 显示当前钱包信息和设置
    println!("{}", format!("钱包信息 / Wallet info:").cyan());
    println!("{}", format!("地址 / Address: {}", wallet.address()).cyan());
    println!("{}", format!("链ID / Chain ID: {}", CHAIN_ID).cyan());
    println!(
        "{}",
        format!("合约地址 / Contract address: {}", contract_address).cyan()
    );

    let contract = MiningContract::new(contract_address, Arc::new(wallet));
    Ok(contract)
}

async fn check_contract_balance<M: Middleware + 'static>(
    contract: &MiningContract<M>,
) -> Result<U256> {
    let contract_balance = contract.get_contract_balance().call().await?;
    println!(
        "{}",
        format!(
            "池中余额 / Pool balance: {} MAG",
            ethers::utils::format_ether(contract_balance)
        )
        .green()
    );

    let min_contract_balance = ethers::utils::parse_ether(MIN_CONTRACT_BALANCE)?;
    if contract_balance < min_contract_balance {
        return Err(anyhow!(
            "合约余额不足 / Insufficient contract balance: {} MAG (需要至少 {} MAG / Requires at least {} MAG)\n请联系 Magnet 链管理员充值合约 / Please contact Magnet chain admin to fund the contract.",
            ethers::utils::format_ether(contract_balance),
            MIN_CONTRACT_BALANCE,
            MIN_CONTRACT_BALANCE
        ));
    }

    Ok(contract_balance)
}

fn input_parallel_tasks() -> Result<usize> {
    let cpu_cores = num_cpus::get();

    println!(
        "{}",
        format!(
            "\n系统检测到 {} 个CPU核心 / System detected {} CPU cores",
            cpu_cores, cpu_cores
        )
        .cyan()
    );

    // 推荐并行任务数为CPU核心数的75%
    let recommended = std::cmp::max(1, (cpu_cores as f32 * 0.75) as usize);

    // 优先使用推荐值
    let default_tasks = recommended;

    let parallel_tasks: usize = Input::new()
        .with_prompt(format!(
            "请输入并行挖矿任务数量 (建议: {}) / Enter parallel mining tasks (recommended: {})",
            recommended, recommended
        ))
        .default(default_tasks)
        .validate_with(|input: &usize| -> Result<(), &str> {
            if *input > 0 && *input <= cpu_cores * 2 {
                Ok(())
            } else {
                Err("任务数必须大于0且不超过CPU核心数的2倍 / Tasks must be > 0 and <= 2x CPU cores")
            }
        })
        .interact()?;

    println!(
        "{}",
        format!(
            "设置并行任务数为: {} / Set parallel tasks to: {}",
            parallel_tasks, parallel_tasks
        )
        .green()
    );

    // 确保系统有足够资源
    if parallel_tasks > cpu_cores {
        println!(
            "{}",
            format!(
                "警告: 任务数超过CPU核心数，可能导致性能下降 / Warning: Tasks exceed CPU cores, may cause performance degradation",
            )
            .yellow()
        );
    }

    Ok(parallel_tasks)
}

async fn start_mining_loop<M: Middleware + 'static>(
    contract: MiningContract<SignerMiddleware<M, LocalWallet>>,
    parallel_tasks: usize,
) -> Result<()> {
    println!(
        "{}",
        format!(
            "启动 {} 个并行挖矿任务 / Starting {} parallel mining tasks",
            parallel_tasks, parallel_tasks
        )
        .cyan()
    );

    // 启动多个并行任务处理器
    let mut task_handles = Vec::new();
    for task_id in 0..parallel_tasks {
        let contract_clone = contract.clone();

        let handle = tokio::spawn(async move {
            // 每个任务独立记录重试次数
            let mut local_retry_count = 0;

            // 任务主循环
            loop {
                println!(
                    "{}",
                    format!(
                        "任务 #{}: 开始处理新挖矿任务 / Task #{}: Starting new mining task",
                        task_id, task_id
                    )
                    .cyan()
                );

                match mine_once(&contract_clone, task_id).await {
                    Ok(_) => {
                        local_retry_count = 0; // 重置重试计数
                        println!(
                            "{}",
                            format!(
                                "任务 #{}: 成功完成 / Task #{}: Successfully completed",
                                task_id, task_id
                            )
                            .green()
                        );
                    }
                    Err(err) => {
                        let err_str = format!("{:?}", err);
                        println!(
                            "{}",
                            format!(
                                "任务 #{}: 出错 / Task #{}: Error: {}",
                                task_id, task_id, err_str
                            )
                            .yellow()
                        );

                        if err_str.contains("network")
                            || err_str.contains("timeout")
                            || err_str.contains("connection")
                        {
                            // 网络错误处理
                            println!(
                                "{}",
                                format!(
                                    "任务 #{}: 网络错误 / Task #{}: Network error",
                                    task_id, task_id
                                )
                                .yellow()
                            );
                            local_retry_count += 1;
                        } else {
                            // 其他错误
                            local_retry_count += 1;
                        }

                        println!("{}", format!("任务 #{}: 5秒后重试 (第{})/ Task #{}: Retrying in 5 seconds (Attempt {})", 
                                            task_id, local_retry_count, task_id, local_retry_count).yellow());
                        sleep(Duration::from_secs(5)).await;
                    }
                }

                // 短暂延迟，避免连续请求
                sleep(Duration::from_secs(2)).await;
            }
        });

        task_handles.push(handle);
    }

    // 等待所有任务完成（实际上不会完成，除非出错）
    for handle in task_handles {
        if let Err(e) = handle.await {
            eprintln!("{}", format!("任务出错: {} / Task error: {}", e, e).red());
        }
    }

    // 所有任务都结束时，返回错误
    Err(anyhow!(
        "所有挖矿任务都已终止 / All mining tasks have terminated"
    ))
}

async fn mine_once<M: Middleware + 'static>(
    contract: &MiningContract<SignerMiddleware<M, LocalWallet>>,
    task_id: usize,
) -> Result<()> {
    // 请求新任务
    println!(
        "{}",
        format!(
            "任务 #{}: 请求新挖矿任务 / Task #{}: Requesting new mining task...",
            task_id, task_id
        )
        .cyan()
    );

    // 获取当前gas价格
    let gas_price = match contract.client().get_gas_price().await {
        Ok(price) => {
            println!(
                "{}",
                format!(
                    "任务 #{}: 获取到当前gas价格: {} gwei",
                    task_id,
                    ethers::utils::format_units(price, "gwei")?
                )
                .green()
            );
            price
        }
        Err(e) => {
            println!(
                "{}",
                format!("任务 #{}: 获取gas价格失败，使用默认值: {}", task_id, e).yellow()
            );
            U256::from(25_000_000_001u64) // 25 gwei 默认值
        }
    };

    // 估算gas限制
    let gas_limit = match contract.request_mining_task().estimate_gas().await {
        Ok(limit) => {
            // 增加10%余量 (limit * 110 / 100)
            let adjusted_limit = limit.saturating_mul(U256::from(110)) / U256::from(100);
            println!(
                "{}",
                format!(
                    "任务 #{}: 估算gas限制: {}, 调整后: {}",
                    task_id, limit, adjusted_limit
                )
                .green()
            );
            adjusted_limit
        }
        Err(e) => {
            println!(
                "{}",
                format!("任务 #{}: 估算gas限制失败，使用默认值: {}", task_id, e).yellow()
            );
            U256::from(300_000u64) // 使用默认值
        }
    };

    // 打印交易发送详情
    println!(
        "{}",
        format!(
            "任务 #{}: 准备发送交易：gas限制={}, gas价格={} gwei, chainId={}",
            task_id,
            gas_limit,
            ethers::utils::format_units(gas_price, "gwei")?,
            CHAIN_ID
        )
        .cyan()
    );

    // 发送交易 - 使用多个let绑定来避免临时值被释放
    let task = contract.request_mining_task();
    let task_with_gas = task.gas(gas_limit);
    let task_with_gas_price = task_with_gas.gas_price(gas_price);
    let tx_result = task_with_gas_price.send().await;

    let tx = match tx_result {
        Ok(pending_tx) => {
            println!("{}", format!("任务 #{}: 交易已发送，等待确认 / Task #{}: Transaction sent, waiting for confirmation...", task_id, task_id).cyan());
            match pending_tx.await {
                Ok(Some(receipt)) => receipt,
                Ok(None) => {
                    return Err(anyhow!(
                        "任务 #{}: 交易没有收据 / Task #{}: Transaction has no receipt",
                        task_id,
                        task_id
                    ))
                }
                Err(e) => {
                    let err_msg = format!(
                        "任务 #{}: 交易确认失败 / Task #{}: Transaction confirmation failed: {:?}",
                        task_id, task_id, e
                    );
                    return Err(anyhow!(err_msg));
                }
            }
        }
        Err(e) => {
            let err_msg = format!(
                "任务 #{}: 交易发送失败 / Task #{}: Transaction send failed: {:?}",
                task_id, task_id, e
            );
            return Err(anyhow!(err_msg));
        }
    };

    println!(
        "{}",
        format!(
            "任务 #{}: 任务请求成功 / Task #{}: Task requested successfully, 交易哈希 / Transaction hash: {}",
            task_id, task_id, tx.transaction_hash
        )
        .green()
    );

    // 获取任务
    let task = contract.get_my_task().call().await?;

    if !task.2 {
        // 如果任务不活跃
        println!(
            "{}",
            format!(
                "任务 #{}: 没有活跃的挖矿任务 / Task #{}: No active mining task",
                task_id, task_id
            )
            .yellow()
        );
        sleep(Duration::from_secs(5)).await;
        return Err(anyhow!(
            "任务 #{}: 没有活跃的挖矿任务 / Task #{}: No active mining task",
            task_id,
            task_id
        ));
    }

    let nonce = task.0;
    let difficulty = task.1;

    println!(
        "{}",
        format!(
            "任务 #{}: 任务 / Task #{}: Task: nonce={}, difficulty={}",
            task_id, task_id, nonce, difficulty
        )
        .cyan()
    );

    // 获取钱包地址（从合约实例的签名者中提取）
    let wallet_address = contract.client().address();

    // 计算解决方案
    println!(
        "{}",
        format!(
            "任务 #{}: 正在计算解决方案 / Task #{}: Calculating solution...",
            task_id, task_id
        )
        .cyan()
    );

    let solution = tokio::time::timeout(
        Duration::from_secs(MINING_TIMEOUT_SECS),
        mine_solution(nonce, wallet_address, difficulty, task_id),
    )
    .await??;

    println!(
        "{}",
        format!(
            "任务 #{}: 找到解决方案 / Task #{}: Solution found: {}",
            task_id, task_id, solution
        )
        .green()
    );

    // 验证任务是否仍然有效
    let current_task = contract.get_my_task().call().await?;
    if !current_task.2 || current_task.0 != nonce {
        println!(
            "{}",
            format!(
                "任务 #{}: 任务已失效，重新请求 / Task #{}: Task expired, requesting new task...",
                task_id, task_id
            )
            .yellow()
        );
        return Err(anyhow!(
            "任务 #{}: 任务已失效 / Task #{}: Task expired",
            task_id,
            task_id
        ));
    }

    // 检查合约余额
    let contract_balance = contract.get_contract_balance().call().await?;
    let min_contract_balance = ethers::utils::parse_ether(MIN_CONTRACT_BALANCE)?;
    if contract_balance < min_contract_balance {
        println!("{}", format!("任务 #{}: 合约余额不足，无法提交 / Task #{}: Insufficient contract balance, cannot submit.", task_id, task_id).red());
        return Err(anyhow!(
            "任务 #{}: 合约余额不足 / Task #{}: Insufficient contract balance",
            task_id,
            task_id
        ));
    }

    // 提交解决方案
    println!(
        "{}",
        format!(
            "任务 #{}: 提交解决方案 / Task #{}: Submitting solution...",
            task_id, task_id
        )
        .cyan()
    );

    // 获取当前gas价格（提交时再次更新）
    let gas_price = match contract.client().get_gas_price().await {
        Ok(price) => {
            println!(
                "{}",
                format!(
                    "任务 #{}: 获取到当前gas价格: {} gwei",
                    task_id,
                    ethers::utils::format_units(price, "gwei")?
                )
                .green()
            );
            price
        }
        Err(e) => {
            println!(
                "{}",
                format!("任务 #{}: 获取gas价格失败，使用默认值: {}", task_id, e).yellow()
            );
            U256::from(25_000_000_001u64) // 25 gwei 默认值
        }
    };

    // 估算提交解决方案的gas限制
    let submit_gas_limit = match contract.submit_mining_result(solution).estimate_gas().await {
        Ok(limit) => {
            // 增加10%余量 (limit * 110 / 100)
            let adjusted_limit = limit.saturating_mul(U256::from(110)) / U256::from(100);
            println!(
                "{}",
                format!(
                    "任务 #{}: 估算提交gas限制: {}, 调整后: {}",
                    task_id, limit, adjusted_limit
                )
                .green()
            );
            adjusted_limit
        }
        Err(e) => {
            println!(
                "{}",
                format!("任务 #{}: 估算提交gas限制失败，使用默认值: {}", task_id, e).yellow()
            );
            U256::from(300_000u64) // 使用默认值
        }
    };

    // 发送提交交易 - 使用多个let绑定来避免临时值被释放
    let submit_task = contract.submit_mining_result(solution);
    let submit_task_with_gas = submit_task.gas(submit_gas_limit);
    let submit_task_with_gas_price = submit_task_with_gas.gas_price(gas_price);
    let submit_result = submit_task_with_gas_price.send().await;

    let submit_tx = match submit_result {
        Ok(pending_tx) => {
            println!("{}", format!("任务 #{}: 提交交易已发送，等待确认 / Task #{}: Submission transaction sent, waiting for confirmation...", task_id, task_id).cyan());
            match pending_tx.await {
                Ok(Some(receipt)) => receipt,
                Ok(None) => {
                    return Err(anyhow!(
                    "任务 #{}: 提交交易没有收据 / Task #{}: Submission transaction has no receipt",
                    task_id,
                    task_id
                ))
                }
                Err(e) => {
                    let err_msg = format!("任务 #{}: 提交交易确认失败 / Task #{}: Submission confirmation failed: {:?}", task_id, task_id, e);
                    return Err(anyhow!(err_msg));
                }
            }
        }
        Err(e) => {
            let err_msg = format!(
                "任务 #{}: 提交交易发送失败 / Task #{}: Submission transaction send failed: {:?}",
                task_id, task_id, e
            );
            return Err(anyhow!(err_msg));
        }
    };

    println!(
        "{}",
        format!(
            "任务 #{}: 提交成功 / Task #{}: Submission successful, 交易哈希 / Transaction hash: {}",
            task_id, task_id, submit_tx.transaction_hash
        )
        .green()
    );

    // 显示余额变化
    let new_balance = contract
        .client()
        .get_balance(contract.client().address(), None)
        .await?;
    println!(
        "{}",
        format!(
            "任务 #{}: 当前余额 / Task #{}: Current balance: {} MAG",
            task_id,
            task_id,
            ethers::utils::format_ether(new_balance)
        )
        .green()
    );

    Ok(())
}

async fn mine_solution(
    nonce: U256,
    address: Address,
    difficulty: U256,
    task_id: usize,
) -> Result<U256> {
    let start_time = Instant::now();

    // 设置进度条
    let pb = Arc::new(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    // 获取全局挖矿实例
    let miner = {
        let mut miner_guard = GLOBAL_MINER.lock();
        if let Some(ref mut miner) = *miner_guard {
            miner.reset(); // 重置状态
            miner.clone()
        } else {
            return Err(anyhow!("挖矿引擎未初始化"));
        }
    };

    // 启动进度显示线程
    let total_hashes = miner.total_hashes.clone();
    let solution_found = miner.solution_found.clone();
    let pb_clone = pb.clone();

    tokio::spawn(async move {
        let mut last_update = Instant::now();
        let mut last_hash_count = 0;

        while !solution_found.load(Ordering::Relaxed) {
            let now = Instant::now();
            let elapsed_total = start_time.elapsed().as_secs_f64();
            let elapsed_since_update = last_update.elapsed().as_secs_f64();

            let hash_count = total_hashes.load(Ordering::Relaxed);
            let recent_hashes = hash_count - last_hash_count;

            let total_hash_rate = hash_count as f64 / elapsed_total;
            let current_hash_rate = if elapsed_since_update > 0.0 {
                recent_hashes as f64 / elapsed_since_update
            } else {
                0.0
            };

            pb_clone.set_message(format!(
                "任务 #{}: 总哈希数 / Task #{}: Total hashes: {}, 平均速度 / Avg rate: {:.2} H/s, 当前速度 / Current rate: {:.2} H/s",
                task_id, task_id, hash_count, total_hash_rate, current_hash_rate
            ));

            last_update = now;
            last_hash_count = hash_count;

            sleep(Duration::from_millis(500)).await;

            // 检查超时条件
            if elapsed_total > MINING_TIMEOUT_SECS as f64 {
                break;
            }
        }
    });

    // 使用高性能并行挖矿器处理
    let mining_future = tokio::task::spawn_blocking(move || miner.mine(nonce, address, difficulty));

    // 添加超时处理
    match tokio::time::timeout(Duration::from_secs(MINING_TIMEOUT_SECS), mining_future).await {
        Ok(Ok(result)) => {
            // 完成挖矿
            pb.finish_and_clear();

            if let Some(solution) = result {
                return Ok(solution);
            }

            Err(anyhow!(
                "任务 #{}: 未找到解决方案 / Task #{}: No solution found",
                task_id,
                task_id
            ))
        }
        Ok(Err(e)) => {
            pb.finish_with_message(format!(
                "任务 #{}: 挖矿过程发生错误: {} / Task #{}: Mining error: {}",
                task_id, e, task_id, e
            ));
            Err(anyhow!(
                "任务 #{}: 挖矿错误 / Task #{}: Mining error: {}",
                task_id,
                task_id,
                e
            ))
        }
        Err(_) => {
            // 超时
            pb.finish_with_message(format!(
                "任务 #{}: 挖矿超时，停止尝试 / Task #{}: Mining timeout, stopping attempts",
                task_id, task_id
            ));
            Err(anyhow!(
                "任务 #{}: 挖矿超时 / Task #{}: Mining timeout",
                task_id,
                task_id
            ))
        }
    }
}

// 优化的内存复用版本solidity_pack_bytes_uint
fn solidity_pack_bytes_uint_into(bytes: &[u8], num: U256, output: &mut Vec<u8>) -> Result<()> {
    // 确保有足够的容量
    let required_capacity = bytes.len() + 32;
    if output.capacity() < required_capacity {
        output.reserve(required_capacity - output.capacity());
    }

    // 添加bytes，保持原始长度
    output.extend_from_slice(bytes);

    // 添加uint256，固定32字节长度
    let mut buffer = [0u8; 32];
    num.to_big_endian(&mut buffer);
    output.extend_from_slice(&buffer);

    Ok(())
}

async fn handle_mining_error(error: anyhow::Error, retry_count: &mut usize) -> Result<()> {
    eprintln!("{}", format!("挖矿错误 / Mining error: {}", error).red());

    *retry_count += 1;

    println!(
        "{}",
        format!(
            "5秒后重试（第 {} 次） / Retrying in 5 seconds (Attempt {})",
            retry_count, retry_count
        )
        .yellow()
    );

    sleep(Duration::from_secs(5)).await;
    Ok(())
}

// 替换旧的encode_packed函数，添加与JavaScript一致的实现
// 特定的solidityPack实现，对应JS版本中的ethers.utils.solidityPack(['uint256', 'address'], [nonce, address])
fn solidity_pack_uint_address(num: U256, addr: Address) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(32 + 20);

    // 添加uint256，固定32字节长度
    let mut buffer = [0u8; 32];
    num.to_big_endian(&mut buffer);
    result.extend_from_slice(&buffer);

    // 添加address，20字节
    result.extend_from_slice(addr.as_bytes());

    Ok(result)
}

// 特定的solidityPack实现，对应JS版本中的ethers.utils.solidityPack(['bytes', 'uint256'], [prefix, solution])
fn solidity_pack_bytes_uint(bytes: Vec<u8>, num: U256) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(bytes.len() + 32);

    // 添加bytes，保持原始长度
    result.extend_from_slice(&bytes);

    // 添加uint256，固定32字节长度
    let mut buffer = [0u8; 32];
    num.to_big_endian(&mut buffer);
    result.extend_from_slice(&buffer);

    Ok(result)
}

// 保留原函数，但只用于其他场景
fn encode_packed(tokens: &[Token]) -> Result<Vec<u8>> {
    let mut result = Vec::new();

    for token in tokens {
        match token {
            Token::Address(addr) => {
                result.extend_from_slice(addr.as_bytes());
            }
            Token::Uint(value) => {
                let mut buffer = [0u8; 32];
                value.to_big_endian(&mut buffer);

                // 跳过前面的零
                let mut start = 0;
                while start < 32 && buffer[start] == 0 {
                    start += 1;
                }

                if start == 32 {
                    // 如果值为0，则添加单个0字节
                    result.push(0);
                } else {
                    // 否则添加非零部分
                    result.extend_from_slice(&buffer[start..]);
                }
            }
            Token::Bytes(bytes) => {
                result.extend_from_slice(bytes);
            }
            _ => {
                return Err(anyhow!("不支持的类型 / Unsupported type"));
            }
        }
    }

    Ok(result)
}
