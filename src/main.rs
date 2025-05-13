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
mod tui_monitor;

use contract::MiningContract;
use tui_monitor::{MonitorData, start_monitor};

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
// 并行任务数
const PARALLEL_TASKS: usize = 3; // 同时处理的任务数量
// MagnetChain的chainId
const CHAIN_ID: u64 = 114514; // 修正为正确的链ID

// 全局变量
lazy_static::lazy_static! {
    static ref MONITOR_DATA: Arc<MonitorData> = Arc::new(MonitorData::new());
    static ref MONITOR_ENABLED: AtomicBool = AtomicBool::new(false);
}

#[tokio::main]
async fn main() -> Result<()> {
    // 处理命令行参数
    let args: Vec<String> = std::env::args().collect();
    let mut monitor_mode = false;
    
    for arg in args.iter() {
        if arg == "h" || arg == "--monitor" {
            monitor_mode = true;
            break;
        }
    }
    
    if monitor_mode {
        // 启动监控模式
        MONITOR_ENABLED.store(true, Ordering::SeqCst);
        let monitor_data = start_monitor();
        std::mem::forget(monitor_data); // 防止数据被释放
        
        // 循环等待用户输入退出命令
        println!("已进入监控模式，按 'exit' 退出");
        loop {
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() {
                if input.trim() == "exit" {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        println!("正在退出监控模式...");
        return Ok(());
    }
    
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
    let balance = check_wallet_balance(&wallet).await?;
    
    if MONITOR_ENABLED.load(Ordering::SeqCst) {
        // 如果启用了监控，更新钱包余额
        MONITOR_DATA.update_balance(ethers::utils::format_ether(balance).parse::<f64>().unwrap_or(0.0));
    }
    
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
    
    // 如果启用了监控，更新钱包余额
    if MONITOR_ENABLED.load(Ordering::SeqCst) {
        MONITOR_DATA.update_balance(ethers::utils::format_ether(balance).parse::<f64>().unwrap_or(0.0));
    }
    
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
    let stop_mining = Arc::new(AtomicBool::new(false));
    let active_tasks = Arc::new(AtomicUsize::new(0));
    let completed_tasks = Arc::new(AtomicUsize::new(0));
    let total_tasks_count = Arc::new(AtomicUsize::new(0));
    
    // Ctrl+C 处理
    let stop_mining_clone = stop_mining.clone();
    ctrlc::set_handler(move || {
        println!("{}", "\n接收到停止信号，正在安全停止挖矿... / Received stop signal, safely stopping mining...".yellow());
        stop_mining_clone.store(true, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    println!("{}", format!("并行任务数 / Parallel tasks: {}", PARALLEL_TASKS).cyan());
    println!("{}", "按 Ctrl+C 停止挖矿 / Press Ctrl+C to stop mining".yellow());
    
    let mut retry_count = 0;
    let total_mined = Arc::new(AtomicU64::new(0));
    
    loop {
        if stop_mining.load(Ordering::SeqCst) {
            break;
        }
        
        // 检查是否有足够的线程槽用于新任务
        while active_tasks.load(Ordering::SeqCst) < PARALLEL_TASKS {
            if stop_mining.load(Ordering::SeqCst) {
                break;
            }
            
            let task_id = total_tasks_count.fetch_add(1, Ordering::SeqCst);
            active_tasks.fetch_add(1, Ordering::SeqCst);
            let contract_clone = contract.clone();
            let active_tasks_clone = active_tasks.clone();
            let completed_tasks_clone = completed_tasks.clone();
            let total_mined_clone = total_mined.clone();
            let stop_mining_clone = stop_mining.clone();
            
            // 如果启用了监控，添加任务到监控数据
            if MONITOR_ENABLED.load(Ordering::SeqCst) {
                MONITOR_DATA.add_task(task_id);
            }
            
            tokio::spawn(async move {
                let result = mine_once(&contract_clone, task_id).await;
                
                if let Err(e) = result {
                    eprintln!("{}", format!("任务 #{} 失败: {} / Task #{} failed: {}", task_id, e, task_id, e).red());
                    
                    // 如果启用了监控，更新任务状态为失败
                    if MONITOR_ENABLED.load(Ordering::SeqCst) {
                        MONITOR_DATA.complete_task(task_id, false);
                    }
                } else {
                    completed_tasks_clone.fetch_add(1, Ordering::SeqCst);
                    total_mined_clone.fetch_add(1, Ordering::SeqCst);
                    
                    // 如果启用了监控，更新任务状态为成功
                    if MONITOR_ENABLED.load(Ordering::SeqCst) {
                        MONITOR_DATA.complete_task(task_id, true);
                        
                        // 更新余额
                        if let Ok(balance) = contract_clone.client().get_balance(contract_clone.client().address(), None).await {
                            MONITOR_DATA.update_balance(ethers::utils::format_ether(balance).parse::<f64>().unwrap_or(0.0));
                        }
                    }
                    
                    let completed = completed_tasks_clone.load(Ordering::SeqCst);
                    if completed % 5 == 0 {
                        println!(
                            "{}",
                            format!(
                                "已成功完成 {} 个挖矿任务 / Successfully completed {} mining tasks",
                                completed, completed
                            )
                            .green()
                        );
                    }
                }
                
                active_tasks_clone.fetch_sub(1, Ordering::SeqCst);
            });
            
            sleep(Duration::from_millis(100)).await;
        }
        
        sleep(Duration::from_millis(500)).await;
        
        // 每隔一段时间检查一下余额
        let completed = completed_tasks.load(Ordering::SeqCst);
        if completed > 0 && completed % 10 == 0 {
            match check_wallet_balance(&contract.client()).await {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("{}", format!("检查余额错误 / Balance check error: {}", e).yellow());
                }
            }
            
            match check_contract_balance(&contract).await {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("{}", format!("检查合约余额错误 / Contract balance check error: {}", e).yellow());
                }
            }
        }
    }
    
    // 等待所有活跃任务完成
    println!("{}", "等待活跃任务完成 / Waiting for active tasks to complete...".yellow());
    while active_tasks.load(Ordering::SeqCst) > 0 {
        sleep(Duration::from_millis(500)).await;
    }
    
    let completed = completed_tasks.load(Ordering::SeqCst);
    println!(
        "{}",
        format!(
            "挖矿已停止。总共完成 {} 个任务。/ Mining stopped. Completed {} tasks in total.",
            completed, completed
        )
        .green()
    );
    
    Ok(())
}

async fn mine_once<M: Middleware + 'static>(
    contract: &MiningContract<SignerMiddleware<M, LocalWallet>>,
    task_id: usize,
) -> Result<()> {
    let mut retry_count = 0;
    
    // 请求挖矿任务
    loop {
        match contract.request_mining_task().send().await {
            Ok(tx) => {
                let tx_hash = tx.tx_hash();
                println!(
                    "{}",
                    format!(
                        "任务 #{}: 已发送请求挖矿任务交易 / Task #{}: Sent request mining task tx: {}",
                        task_id, task_id, tx_hash
                    )
                    .cyan()
                );
                
                match tx.await {
                    Ok(_) => {
                        println!(
                            "{}",
                            format!(
                                "任务 #{}: 请求挖矿任务交易已确认 / Task #{}: Request mining task tx confirmed",
                                task_id, task_id
                            )
                            .green()
                        );
                        break;
                    }
                    Err(e) => {
                        let result = handle_mining_error(anyhow!("任务 #{}: 请求挖矿任务交易失败 / Task #{}: Request mining task tx failed: {}", task_id, task_id, e), &mut retry_count).await;
                        if result.is_err() {
                            return result;
                        }
                        continue;
                    }
                }
            }
            Err(e) => {
                let result = handle_mining_error(anyhow!("任务 #{}: 发送请求挖矿任务交易失败 / Task #{}: Failed to send request mining task tx: {}", task_id, task_id, e), &mut retry_count).await;
                if result.is_err() {
                    return result;
                }
                continue;
            }
        }
    }
    
    // 获取挖矿任务
    let (nonce, difficulty, active) = match contract.get_my_task().call().await {
        Ok(task) => task,
        Err(e) => {
            return Err(anyhow!("任务 #{}: 获取挖矿任务失败 / Task #{}: Failed to get mining task: {}", task_id, task_id, e));
        }
    };
    
    if !active {
        return Err(anyhow!("任务 #{}: 挖矿任务未激活 / Task #{}: Mining task not active", task_id, task_id));
    }
    
    println!(
        "{}",
        format!(
            "任务 #{}: 获取到新挖矿任务 - Nonce: {}, 难度: {} / Task #{}: Got new mining task - Nonce: {}, Difficulty: {}",
            task_id, nonce, difficulty, task_id, nonce, difficulty
        )
        .green()
    );
    
    // 解决挖矿任务
    let wallet_address = contract.client().address();
    
    // 设置进度条
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% (预计 {eta}) / {spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% (ETA {eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );
    
    // 超时检查
    let start_time = Instant::now();
    let timeout = Duration::from_secs(MINING_TIMEOUT_SECS);
    
    // 求解
    let solution = match tokio::time::timeout(
        timeout,
        mine_solution(nonce, wallet_address, difficulty, task_id),
    )
    .await
    {
        Ok(result) => match result {
            Ok(solution) => {
                pb.finish_and_clear();
                println!(
                    "{}",
                    format!(
                        "任务 #{}: 找到解决方案: {} (耗时: {:?}) / Task #{}: Found solution: {} (Time: {:?})",
                        task_id,
                        solution,
                        start_time.elapsed(),
                        task_id,
                        solution,
                        start_time.elapsed()
                    )
                    .green()
                );
                solution
            }
            Err(e) => {
                pb.finish_and_clear();
                return Err(anyhow!("任务 #{}: 解决挖矿任务失败 / Task #{}: Failed to solve mining task: {}", task_id, task_id, e));
            }
        },
        Err(_) => {
            pb.finish_and_clear();
            return Err(anyhow!("任务 #{}: 挖矿超时 / Task #{}: Mining timed out after {} seconds", task_id, task_id, MINING_TIMEOUT_SECS));
        }
    };
    
    // 提交结果
    retry_count = 0;
    loop {
        match contract.submit_mining_result(solution).send().await {
            Ok(tx) => {
                let tx_hash = tx.tx_hash();
                println!(
                    "{}",
                    format!(
                        "任务 #{}: 已发送提交挖矿结果交易 / Task #{}: Sent submit mining result tx: {}",
                        task_id, task_id, tx_hash
                    )
                    .cyan()
                );
                
                match tx.await {
                    Ok(receipt) => {
                        if let Some(receipt) = receipt {
                            if receipt.status == Some(U64::one()) {
                                println!(
                                    "{}",
                                    format!(
                                        "任务 #{}: 提交挖矿结果交易已确认，获得奖励！/ Task #{}: Submit mining result tx confirmed, reward received!",
                                        task_id, task_id
                                    )
                                    .green()
                                );
                                
                                // 解析事件以获取奖励数量
                                if let Some(logs) = receipt.logs.iter().find(|log| {
                                    log.topics.len() > 1 && log.topics[0] == keccak256("MiningReward(address,uint256)").into()
                                }) {
                                    if logs.data.0.len() >= 32 {
                                        let reward = U256::from_big_endian(&logs.data.0[0..32]);
                                        println!(
                                            "{}",
                                            format!(
                                                "任务 #{}: 挖矿奖励: {} MAG / Task #{}: Mining reward: {} MAG",
                                                task_id,
                                                ethers::utils::format_ether(reward),
                                                task_id,
                                                ethers::utils::format_ether(reward)
                                            )
                                            .green()
                                        );
                                    }
                                }
                                
                                return Ok(());
                            } else {
                                return Err(anyhow!("任务 #{}: 提交挖矿结果交易失败 / Task #{}: Submit mining result tx failed with status: {:?}", task_id, task_id, receipt.status));
                            }
                        } else {
                            return Err(anyhow!("任务 #{}: 提交挖矿结果交易没有收据 / Task #{}: Submit mining result tx has no receipt", task_id, task_id));
                        }
                    }
                    Err(e) => {
                        let result = handle_mining_error(anyhow!("任务 #{}: 提交挖矿结果交易失败 / Task #{}: Submit mining result tx failed: {}", task_id, task_id, e), &mut retry_count).await;
                        if result.is_err() {
                            return result;
                        }
                        continue;
                    }
                }
            }
            Err(e) => {
                let result = handle_mining_error(anyhow!("任务 #{}: 发送提交挖矿结果交易失败 / Task #{}: Failed to send submit mining result tx: {}", task_id, task_id, e), &mut retry_count).await;
                if result.is_err() {
                    return result;
                }
                continue;
            }
        }
    }
}

async fn mine_solution(nonce: U256, address: Address, difficulty: U256, task_id: usize) -> Result<U256> {
    let difficulty_bytes = difficulty.to_string();
    let difficulty_len = difficulty_bytes.len();
    let difficulty_biguint = BigUint::parse_bytes(difficulty_bytes.as_bytes(), 10).ok_or_else(|| {
        anyhow!("任务 #{}: 无法解析难度值 / Task #{}: Cannot parse difficulty", task_id, task_id)
    })?;

    // 显示估计的哈希次数
    let estimated_hashes = 2u128.pow(difficulty_len as u32 * 4) as f64;
    println!(
        "{}",
        format!(
            "任务 #{}: 难度: {} (约 {:.1e} 次哈希) / Task #{}: Difficulty: {} (approx. {:.1e} hashes)",
            task_id, difficulty, estimated_hashes, task_id, difficulty, estimated_hashes
        )
        .cyan()
    );

    // 并行计算哈希
    let num_cpus = num_cpus::get();
    let mut guesses_per_batch = 100_000; // 每个批次的猜测次数
    
    let mut counter = 0u64;
    let mut last_update = Instant::now();
    let start_time = Instant::now();
    
    // 创建进度条
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "任务 #{} 挖矿中 / Task #{} Mining: {spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {percent}% ({per_sec}) / {spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {percent}% ({per_sec})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message(format!("{}", task_id));

    let solution_found = Arc::new(AtomicBool::new(false));
    let solution_value = Arc::new(std::sync::Mutex::new(None));
    
    loop {
        if solution_found.load(Ordering::SeqCst) {
            break;
        }
        
        let mut futures = Vec::with_capacity(num_cpus);
        
        for _ in 0..num_cpus {
            let start_value = counter;
            counter += guesses_per_batch as u64;
            
            let nonce_clone = nonce;
            let address_clone = address;
            let difficulty_biguint_clone = difficulty_biguint.clone();
            let solution_found_clone = solution_found.clone();
            let solution_value_clone = solution_value.clone();
            
            let future = tokio::task::spawn_blocking(move || {
                for i in 0..guesses_per_batch {
                    if solution_found_clone.load(Ordering::SeqCst) {
                        return None;
                    }
                    
                    let guess = U256::from(start_value + i as u64);
                    
                    // 打包数据
                    let packed_data = match solidity_pack_uint_address(nonce_clone, address_clone) {
                        Ok(data) => data,
                        Err(_) => continue,
                    };
                    
                    let packed_with_guess = match solidity_pack_bytes_uint(packed_data, guess) {
                        Ok(data) => data,
                        Err(_) => continue,
                    };
                    
                    // 计算哈希
                    let hash = keccak256(packed_with_guess);
                    
                    // 转换为 BigUint 以进行比较
                    let hash_biguint = BigUint::from_bytes_be(&hash);
                    
                    // 检查是否满足难度要求
                    if hash_biguint < difficulty_biguint_clone {
                        solution_found_clone.store(true, Ordering::SeqCst);
                        let mut solution = solution_value_clone.lock().unwrap();
                        *solution = Some(guess);
                        return Some(guess);
                    }
                    
                    // 更新进度条，但不要太频繁
                    if i % 10000 == 0 && solution_found_clone.load(Ordering::SeqCst) {
                        return None;
                    }
                }
                None
            });
            
            futures.push(future);
        }
        
        // 等待所有批次完成
        let results = join_all(futures).await;
        
        // 检查是否找到解决方案
        for result in results {
            if let Ok(Some(solution)) = result {
                solution_found.store(true, Ordering::SeqCst);
                let mut sol_value = solution_value.lock().unwrap();
                *sol_value = Some(solution);
                break;
            }
        }
        
        // 如果已经找到解决方案，退出循环
        if solution_found.load(Ordering::SeqCst) {
            break;
        }
        
        // 更新进度条
        let elapsed = start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let hashes_per_second = counter as f64 / elapsed;
            let estimated_total_hashes = estimated_hashes.min(1e18); // 限制最大值以避免数值溢出
            let progress_percent = (counter as f64 / estimated_total_hashes * 100.0).min(99.0);
            
            pb.set_position(progress_percent as u64);
            pb.set_message(format!("{:.2}M 哈希/秒 / {:.2}M hashes/s", hashes_per_second / 1_000_000.0, hashes_per_second / 1_000_000.0));
            
            // 如果启用了监控，更新任务进度
            if MONITOR_ENABLED.load(Ordering::SeqCst) {
                MONITOR_DATA.update_task_progress(task_id, progress_percent / 100.0);
            }
        }
        
        // 每隔一段时间调整批处理大小
        if last_update.elapsed().as_secs() >= 5 {
            last_update = Instant::now();
            
            // 调整批处理大小
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 5.0 {
                let hashes_per_second = counter as f64 / elapsed;
                // 每个CPU核心每秒处理的哈希数
                let hashes_per_cpu_per_second = hashes_per_second / num_cpus as f64;
                
                // 调整每批次的猜测次数，使每个批次大约运行0.1秒
                guesses_per_batch = (hashes_per_cpu_per_second * 0.1) as usize;
                guesses_per_batch = guesses_per_batch.max(10_000).min(1_000_000);
            }
        }
    }
    
    pb.finish_and_clear();
    
    // 获取找到的解决方案
    let solution = solution_value.lock().unwrap();
    match *solution {
        Some(value) => Ok(value),
        None => Err(anyhow!("任务 #{}: 未找到解决方案 / Task #{}: No solution found", task_id, task_id)),
    }
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