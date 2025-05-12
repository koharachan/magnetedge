use anyhow::{anyhow, Result};
use colored::*;
use dialoguer::{Input, Select};
use ethers::{
    prelude::*,
    providers::{Http, Provider},
    utils::keccak256,
    abi::Token,
};
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use num_bigint::BigUint;
use std::{
    convert::TryFrom,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::time::sleep;

mod contract;

use contract::MiningContract;

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
const MAX_RETRIES: usize = 5;
const MINING_TIMEOUT_SECS: u64 = 600; // 10分钟
// MagnetChain的chainId
const CHAIN_ID: u64 = 114514; // 修正为正确的链ID

#[tokio::main]
async fn main() -> Result<()> {
    print_welcome_message();

    // 选择RPC节点
    let rpc_url = select_rpc_node()?;
    println!("{}", format!("已选择 RPC / Selected RPC: {}", rpc_url).green());

    // 初始化以太坊提供者
    let provider = Provider::<Http>::try_from(rpc_url)?;
    
    // 显示链ID信息
    match provider.get_chainid().await {
        Ok(chainid) => {
            println!("{}", format!("连接到链ID: {} / Connected to chain ID: {}", chainid, chainid).green());
            if chainid != U256::from(CHAIN_ID) {
                println!("{}", format!("警告：检测到的链ID与设置的不符！ / Warning: Detected chain ID does not match configuration!").yellow());
            }
        },
        Err(e) => {
            println!("{}", format!("无法获取链ID: {} / Could not get chain ID: {}", e, e).yellow());
        }
    }
    
    // 输入私钥并创建钱包
    let wallet = input_private_key(provider).await?;
    let wallet_address = wallet.address();
    println!("{}", format!("钱包地址 / Wallet address: {}", wallet_address).green());

    // 检查钱包余额
    let _balance = check_wallet_balance(&wallet).await?;
    
    // 初始化合约
    let contract = init_contract(wallet).await?;
    
    // 检查合约余额
    check_contract_balance(&contract).await?;
    
    // 开始挖矿循环
    println!("{}", "\n挖矿模式 / Mining Mode:".bold());
    println!("{}", "免费挖矿 (3 MAG 每次哈希) / Free Mining (3 MAG per hash)".cyan());
    println!("{}", "\n开始挖矿 / Starting mining...".bold().green());
    
    start_mining_loop(contract).await?;
    
    Ok(())
}

fn print_welcome_message() {
    println!("{}", " 你好，欢迎使用 Magnet POW 区块链挖矿客户端！ ".bold().on_cyan().black());
    println!("{}", " Hello, welcome to Magnet POW Blockchain Mining Client! ".bold().on_cyan().black());
    println!("{}", "启动挖矿客户端，需要确保钱包里有0.1MAG，如果没有，加入TG群免费领取0.1 MAG空投。".bold().magenta());
    println!("{}", "To start the mining client, ensure your wallet has 0.1 MAG. If not, join the Telegram group for a free 0.1 MAG airdrop.".bold().magenta());
    println!("{}", "TG群链接 / Telegram group link: https://t.me/MagnetPOW".bold().magenta());
    println!("{}", format!("网络信息 / Network Info: 链ID / Chain ID: {}, 货币符号 / Symbol: MAG", CHAIN_ID).cyan());
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

async fn input_private_key<P: JsonRpcClient + 'static + Clone>(provider: Provider<P>) -> Result<SignerMiddleware<Provider<P>, LocalWallet>> {
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
                println!("{}", format!("已设置钱包chainId为: {} / Set wallet chainId to: {}", CHAIN_ID, CHAIN_ID).green());
                
                let client = SignerMiddleware::new(provider.clone(), wallet);
                return Ok(client);
            },
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
                    return Err(anyhow!("达到最大尝试次数，程序退出 / Max attempts reached, exiting."));
                }
            }
        }
    }
    
    Err(anyhow!("无法解析私钥 / Unable to parse private key"))
}

async fn check_wallet_balance<M: Middleware + 'static>(wallet: &SignerMiddleware<M, LocalWallet>) -> Result<U256> {
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
    println!("{}", format!("合约地址 / Contract address: {}", contract_address).cyan());
    
    let contract = MiningContract::new(contract_address, Arc::new(wallet));
    Ok(contract)
}

async fn check_contract_balance<M: Middleware + 'static>(contract: &MiningContract<M>) -> Result<U256> {
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

async fn start_mining_loop<M: Middleware + 'static>(
    contract: MiningContract<SignerMiddleware<M, LocalWallet>>,
) -> Result<()> {
    let mut retry_count = 0;
    let mut rpc_index = 0;
    
    loop {
        match mine_once(&contract).await {
            Ok(_) => {
                retry_count = 0; // 重置重试计数
            }
            Err(err) => {
                let err_str = format!("{:?}", err);
                if err_str.contains("network") || err_str.contains("timeout") || err_str.contains("connection") {
                    // 网络错误，建议用户尝试切换RPC节点
                    println!("{}", format!("网络错误，建议手动重启并选择其他RPC节点 / Network error, suggest restarting with a different RPC node").yellow());
                    rpc_index = (rpc_index + 1) % RPC_OPTIONS.len();
                    let new_rpc = RPC_OPTIONS[rpc_index];
                    println!("{}", format!("推荐的RPC / Recommended RPC: {}", new_rpc).green());
                    
                    // 不直接修改provider，而是继续使用原有合约
                    // 如果连续失败，会通过retry_count退出
                    handle_mining_error(err, &mut retry_count).await?;
                } else {
                    // 其他错误
                    handle_mining_error(err, &mut retry_count).await?;
                }
            }
        }
    }
}

async fn mine_once<M: Middleware + 'static>(
    contract: &MiningContract<SignerMiddleware<M, LocalWallet>>,
) -> Result<()> {
    // 请求新任务
    println!("{}", "请求新挖矿任务 / Requesting new mining task...".cyan());
    
    // 获取当前gas价格
    let gas_price = match contract.client().get_gas_price().await {
        Ok(price) => {
            println!("{}", format!("获取到当前gas价格: {} gwei", ethers::utils::format_units(price, "gwei")?).green());
            price
        },
        Err(e) => {
            println!("{}", format!("获取gas价格失败，使用默认值: {}", e).yellow());
            U256::from(25_000_000_001u64) // 25 gwei 默认值
        }
    };
    
    // 估算gas限制
    let gas_limit = match contract.request_mining_task().estimate_gas().await {
        Ok(limit) => {
            // 增加10%余量 (limit * 110 / 100)
            let adjusted_limit = limit.saturating_mul(U256::from(110)) / U256::from(100);
            println!("{}", format!("估算gas限制: {}, 调整后: {}", limit, adjusted_limit).green());
            adjusted_limit
        },
        Err(e) => {
            println!("{}", format!("估算gas限制失败，使用默认值: {}", e).yellow());
            U256::from(300_000u64) // 使用默认值
        }
    };
    
    // 打印交易发送详情
    println!("{}", format!("准备发送交易：gas限制={}, gas价格={} gwei, chainId={}",
             gas_limit,
             ethers::utils::format_units(gas_price, "gwei")?,
             CHAIN_ID).cyan());
    
    // 发送交易 - 使用多个let绑定来避免临时值被释放
    let task = contract.request_mining_task();
    let task_with_gas = task.gas(gas_limit);
    let task_with_gas_price = task_with_gas.gas_price(gas_price);
    let tx_result = task_with_gas_price.send().await;
        
    let tx = match tx_result {
        Ok(pending_tx) => {
            println!("{}", format!("交易已发送，等待确认 / Transaction sent, waiting for confirmation...").cyan());
            match pending_tx.await {
                Ok(Some(receipt)) => receipt,
                Ok(None) => return Err(anyhow!("交易没有收据 / Transaction has no receipt")),
                Err(e) => {
                    let err_msg = format!("交易确认失败 / Transaction confirmation failed: {:?}", e);
                    return Err(anyhow!(err_msg));
                }
            }
        },
        Err(e) => {
            let err_msg = format!("交易发送失败 / Transaction send failed: {:?}", e);
            return Err(anyhow!(err_msg));
        }
    };
        
    println!(
        "{}",
        format!(
            "任务请求成功 / Task requested successfully, 交易哈希 / Transaction hash: {}",
            tx.transaction_hash
        )
        .green()
    );
    
    // 获取任务
    let task = contract.get_my_task().call().await?;
    
    if !task.2 {
        // 如果任务不活跃
        println!("{}", "没有活跃的挖矿任务 / No active mining task".yellow());
        sleep(Duration::from_secs(5)).await;
        return Err(anyhow!("没有活跃的挖矿任务 / No active mining task"));
    }
    
    let nonce = task.0;
    let difficulty = task.1;
    
    println!(
        "{}",
        format!("任务 / Task: nonce={}, difficulty={}", nonce, difficulty).cyan()
    );
    
    // 获取钱包地址（从合约实例的签名者中提取）
    let wallet_address = contract.client().address();
    
    // 计算解决方案
    println!("{}", "正在计算解决方案 / Calculating solution...".cyan());
    
    let solution = tokio::time::timeout(
        Duration::from_secs(MINING_TIMEOUT_SECS),
        mine_solution(nonce, wallet_address, difficulty),
    )
    .await??;
    
    println!("{}", format!("找到解决方案 / Solution found: {}", solution).green());
    
    // 验证任务是否仍然有效
    let current_task = contract.get_my_task().call().await?;
    if !current_task.2 || current_task.0 != nonce {
        println!("{}", "任务已失效，重新请求 / Task expired, requesting new task...".yellow());
        return Err(anyhow!("任务已失效 / Task expired"));
    }
    
    // 检查合约余额
    let contract_balance = contract.get_contract_balance().call().await?;
    let min_contract_balance = ethers::utils::parse_ether(MIN_CONTRACT_BALANCE)?;
    if contract_balance < min_contract_balance {
        println!("{}", "合约余额不足，无法提交 / Insufficient contract balance, cannot submit.".red());
        return Err(anyhow!("合约余额不足 / Insufficient contract balance"));
    }
    
    // 提交解决方案
    println!("{}", "提交解决方案 / Submitting solution...".cyan());
    
    // 获取当前gas价格（提交时再次更新）
    let gas_price = match contract.client().get_gas_price().await {
        Ok(price) => {
            println!("{}", format!("获取到当前gas价格: {} gwei", ethers::utils::format_units(price, "gwei")?).green());
            price
        },
        Err(e) => {
            println!("{}", format!("获取gas价格失败，使用默认值: {}", e).yellow());
            U256::from(25_000_000_001u64) // 25 gwei 默认值
        }
    };
    
    // 估算提交解决方案的gas限制
    let submit_gas_limit = match contract.submit_mining_result(solution).estimate_gas().await {
        Ok(limit) => {
            // 增加10%余量 (limit * 110 / 100)
            let adjusted_limit = limit.saturating_mul(U256::from(110)) / U256::from(100);
            println!("{}", format!("估算提交gas限制: {}, 调整后: {}", limit, adjusted_limit).green());
            adjusted_limit
        },
        Err(e) => {
            println!("{}", format!("估算提交gas限制失败，使用默认值: {}", e).yellow());
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
            println!("{}", format!("提交交易已发送，等待确认 / Submission transaction sent, waiting for confirmation...").cyan());
            match pending_tx.await {
                Ok(Some(receipt)) => receipt,
                Ok(None) => return Err(anyhow!("提交交易没有收据 / Submission transaction has no receipt")),
                Err(e) => {
                    let err_msg = format!("提交交易确认失败 / Submission confirmation failed: {:?}", e);
                    return Err(anyhow!(err_msg));
                }
            }
        },
        Err(e) => {
            let err_msg = format!("提交交易发送失败 / Submission transaction send failed: {:?}", e);
            return Err(anyhow!(err_msg));
        }
    };
        
    println!(
        "{}",
        format!(
            "提交成功 / Submission successful, 交易哈希 / Transaction hash: {}",
            submit_tx.transaction_hash
        )
        .green()
    );
    
    // 显示余额变化
    let new_balance = contract.client().get_balance(contract.client().address(), None).await?;
    println!(
        "{}",
        format!(
            "当前余额 / Current balance: {} MAG",
            ethers::utils::format_ether(new_balance)
        )
        .green()
    );
    
    Ok(())
}

async fn mine_solution(nonce: U256, address: Address, difficulty: U256) -> Result<U256> {
    // 优化1: 增加线程数量，默认CPU核心数，但最少4个线程
    let num_threads = std::cmp::max(num_cpus::get(), 4);
    let solution_found = Arc::new(AtomicBool::new(false));
    let found_solution = Arc::new(AtomicU64::new(0));
    let total_hashes = Arc::new(AtomicU64::new(0));
    
    // 优化2: 记录上次找到解决方案的统计信息，用于启发式搜索
    static LAST_SUCCESSFUL_THREAD: AtomicUsize = AtomicUsize::new(0);
    static LAST_SOLUTION_RANGE: AtomicU64 = AtomicU64::new(0);
    
    let start_time = Instant::now();
    
    // 设置进度条
    let pb = Arc::new(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    
    // 优化3: 使用更精确的阈值计算
    let threshold = {
        let max_u256 = BigUint::from(2u32).pow(256) - BigUint::from(1u32);
        let diff = BigUint::from(difficulty.as_u128());
        max_u256 / diff
    };
    
    // 预计算编码前缀
    let prefix = solidity_pack_uint_address(nonce, address)?;
    
    // 创建多个挖矿任务，考虑之前的成功记录
    let mut handles = vec![];
    let last_successful_thread = LAST_SUCCESSFUL_THREAD.load(Ordering::Relaxed);
    let last_solution_range = LAST_SOLUTION_RANGE.load(Ordering::Relaxed);
    
    // 预分配缓冲区，提高性能
    let buffer_size = 32 * 1024; // 32KB预分配缓冲区
    
    for thread_id in 0..num_threads {
        let prefix = prefix.clone();
        let solution_found = solution_found.clone();
        let found_solution = found_solution.clone();
        let total_hashes = total_hashes.clone();
        let threshold = threshold.clone();
        let pb = pb.clone();
        
        // 优化4: 启发式搜索策略 - 优先分配给上次成功的线程附近区域
        let start_solution = if last_solution_range > 0 && thread_id == last_successful_thread {
            // 成功线程从上次成功位置附近开始
            let base = last_solution_range.saturating_sub(1000);
            U256::from(base + (thread_id as u64))
        } else {
            // 其他线程正常分布
            U256::from(thread_id as u64)
        };
        
        let handle = tokio::spawn(async move {
            let mut solution = start_solution;
            let step = U256::from(num_threads as u64);
            
            // 优化5: 批处理计算，每次处理多个哈希
            const BATCH_SIZE: usize = 16;
            let mut encoded_buffers: Vec<Vec<u8>> = Vec::with_capacity(BATCH_SIZE);
            let mut solutions: Vec<U256> = Vec::with_capacity(BATCH_SIZE);
            
            // 预分配缓冲区
            for _ in 0..BATCH_SIZE {
                encoded_buffers.push(Vec::with_capacity(buffer_size));
                solutions.push(U256::zero());
            }
            
            // 每10秒显示线程状态
            let thread_start_time = Instant::now();
            let thread_hashes = AtomicU64::new(0);
            
            tokio::spawn({
                let thread_hashes = &thread_hashes;
                let solution_found = solution_found.clone();
                async move {
                    while !solution_found.load(Ordering::Relaxed) {
                        sleep(Duration::from_secs(10)).await;
                        if solution_found.load(Ordering::Relaxed) {
                            break;
                        }
                        
                        let elapsed = thread_start_time.elapsed().as_secs_f64();
                        let hashes = thread_hashes.load(Ordering::Relaxed);
                        let rate = hashes as f64 / elapsed;
                        
                        pb.println(format!(
                            "[线程 {}] 哈希速度: {:.2} H/s, 当前位置: {}",
                            thread_id, rate, solution
                        ));
                    }
                }
            });
            
            while !solution_found.load(Ordering::Relaxed) {
                // 填充批次
                for i in 0..BATCH_SIZE {
                    let current_solution = solution.overflowing_add(U256::from(i)).0;
                    solutions[i] = current_solution;
                    encoded_buffers[i].clear();
                    
                    // 复用缓冲区而不是每次重新分配
                    solidity_pack_bytes_uint_into(&prefix, current_solution, &mut encoded_buffers[i])?;
                }
                
                // 处理批次
                for i in 0..BATCH_SIZE {
                    let hash = keccak256(&encoded_buffers[i]);
                    thread_hashes.fetch_add(1, Ordering::Relaxed);
                    total_hashes.fetch_add(1, Ordering::Relaxed);
                    
                    // 检查哈希值是否满足难度要求
                    let hash_bigint = BigUint::from_bytes_be(hash.as_ref());
                    if hash_bigint <= threshold {
                        // 更新成功统计
                        LAST_SUCCESSFUL_THREAD.store(thread_id, Ordering::Relaxed);
                        LAST_SOLUTION_RANGE.store(solutions[i].as_u64(), Ordering::Relaxed);
                        
                        solution_found.store(true, Ordering::Relaxed);
                        found_solution.store(solutions[i].as_u64(), Ordering::Relaxed);
                        return Ok::<(), anyhow::Error>(());
                    }
                }
                
                // 批量步进
                solution = solution.overflowing_add(U256::from(BATCH_SIZE)).0;
                solution = solution.overflowing_add(step.saturating_mul(U256::from(BATCH_SIZE - 1))).0;
                
                // 每处理一批哈希值让出一次CPU
                tokio::task::yield_now().await;
            }
            
            Ok::<(), anyhow::Error>(())
        });
        
        handles.push(handle);
    }
    
    // 优化6: 更详细的进度显示
    let total_hashes_clone = total_hashes.clone();
    let solution_found_clone = solution_found.clone();
    let pb_clone = pb.clone();
    tokio::spawn(async move {
        let mut last_update = Instant::now();
        let mut last_hash_count = 0;
        
        while !solution_found_clone.load(Ordering::Relaxed) {
            let now = Instant::now();
            let elapsed_total = start_time.elapsed().as_secs_f64();
            let elapsed_since_update = last_update.elapsed().as_secs_f64();
            
            let hash_count = total_hashes_clone.load(Ordering::Relaxed);
            let recent_hashes = hash_count - last_hash_count;
            
            let total_hash_rate = hash_count as f64 / elapsed_total;
            let current_hash_rate = if elapsed_since_update > 0.0 {
                recent_hashes as f64 / elapsed_since_update
            } else {
                0.0
            };
            
            pb_clone.set_message(format!(
                "总哈希数 / Total hashes: {}, 平均速度 / Avg rate: {:.2} H/s, 当前速度 / Current rate: {:.2} H/s",
                hash_count, total_hash_rate, current_hash_rate
            ));
            
            last_update = now;
            last_hash_count = hash_count;
            
            sleep(Duration::from_millis(500)).await;
        }
    });
    
    // 优化7: 等待任务完成时的改进逻辑
    let result_future = join_all(handles);
    
    // 添加超时处理，默认不超过10分钟
    match tokio::time::timeout(Duration::from_secs(MINING_TIMEOUT_SECS), result_future).await {
        Ok(_) => {
            // 正常完成
            pb.finish_and_clear();
            
            if solution_found.load(Ordering::Relaxed) {
                let solution = U256::from(found_solution.load(Ordering::Relaxed));
                return Ok(solution);
            }
            
            Err(anyhow!("未找到解决方案 / No solution found"))
        },
        Err(_) => {
            // 超时
            pb.finish_with_message("挖矿超时，停止尝试 / Mining timeout, stopping attempts");
            Err(anyhow!("挖矿超时 / Mining timeout"))
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
    if *retry_count >= MAX_RETRIES {
        return Err(anyhow!("达到最大重试次数，程序退出 / Max retries reached, exiting."));
    }
    
    println!(
        "{}",
        format!(
            "5秒后重试（第 {}/{} 次） / Retrying in 5 seconds (Attempt {}/{})",
            retry_count, MAX_RETRIES, retry_count, MAX_RETRIES
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