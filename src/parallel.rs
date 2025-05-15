use ethers::types::{Address, U256};
use num_bigint::BigUint;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::hash_engine::{compute_hash_batch, encode_solution, keccak256_opt};

/// 高性能的并行挖矿管理器
#[derive(Clone)]
pub struct ParallelMiner {
    pub num_threads: usize,
    pub batch_size: usize,
    pub buffer_size: usize,
    pub total_hashes: Arc<AtomicU64>,
    pub solution_found: Arc<AtomicBool>,
    pub found_solution: Arc<AtomicU64>,
    pub last_successful_thread: Arc<AtomicUsize>,
    pub last_solution_range: Arc<AtomicU64>,
}

impl ParallelMiner {
    /// 创建新的并行挖矿管理器
    pub fn new(threads: usize) -> Self {
        let batch_size = 32; // 更大的批量处理
        let buffer_size = 64 * 1024; // 64KB预分配缓冲区

        ParallelMiner {
            num_threads: threads,
            batch_size,
            buffer_size,
            total_hashes: Arc::new(AtomicU64::new(0)),
            solution_found: Arc::new(AtomicBool::new(false)),
            found_solution: Arc::new(AtomicU64::new(0)),
            last_successful_thread: Arc::new(AtomicUsize::new(0)),
            last_solution_range: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 使用并行策略进行Keccak256挖矿
    pub fn mine(&self, nonce: U256, address: Address, difficulty: U256) -> Option<U256> {
        // 计算阈值
        let threshold = {
            let max_u256 = BigUint::from(2u32).pow(256) - BigUint::from(1u32);
            let diff = BigUint::from(difficulty.as_u128());
            max_u256 / diff
        };

        // 预计算前缀
        let mut prefix_buffer = Vec::with_capacity(52); // 32字节nonce + 20字节地址
        let mut buffer = [0u8; 32];
        nonce.to_big_endian(&mut buffer);
        prefix_buffer.extend_from_slice(&buffer);
        prefix_buffer.extend_from_slice(address.as_bytes());

        // 共享数据
        let threshold = Arc::new(threshold);
        let prefix = Arc::new(prefix_buffer);
        let solution_found = self.solution_found.clone();

        // 最后成功的线程和范围
        let last_thread = self.last_successful_thread.load(Ordering::Relaxed);
        let last_range = self.last_solution_range.load(Ordering::Relaxed);

        // 使用rayon进行并行计算
        let result = (0..self.num_threads)
            .into_par_iter()
            .map(|thread_id| {
                if solution_found.load(Ordering::Relaxed) {
                    return None;
                }

                // 每个线程的计数器
                let thread_hashes = Arc::new(AtomicU64::new(0));

                // 确定起始点，优先考虑上次成功的范围
                let start_solution = if last_range > 0 && thread_id == last_thread {
                    // 从上次成功的区域开始
                    let base = last_range.saturating_sub(2000);
                    U256::from(base + (thread_id as u64))
                } else {
                    // 其他线程均匀分布
                    U256::from(thread_id as u64)
                };

                // 设置步进值 - 暂时不使用但保留
                let _step = U256::from(self.num_threads as u64);

                // 预分配缓冲区
                let mut encoded_buffers = Vec::with_capacity(self.batch_size);
                let mut solutions = Vec::with_capacity(self.batch_size);

                for _ in 0..self.batch_size {
                    encoded_buffers.push(Vec::with_capacity(self.buffer_size));
                    solutions.push(U256::zero());
                }

                // 搜索解决方案
                let mut solution = start_solution;
                while !solution_found.load(Ordering::Relaxed) {
                    // 准备批次数据
                    for i in 0..self.batch_size {
                        let current_solution = solution + U256::from(i);
                        solutions[i] = current_solution;
                        encoded_buffers[i].clear();
                        encode_solution(&prefix, current_solution, &mut encoded_buffers[i]);
                    }

                    // 计算和检查批次
                    if let Some(found) = compute_hash_batch(
                        &encoded_buffers,
                        &solutions,
                        &threshold,
                        &self.total_hashes,
                        &thread_hashes,
                        &solution_found,
                    ) {
                        self.last_successful_thread
                            .store(thread_id, Ordering::Relaxed);
                        self.last_solution_range
                            .store(found.as_u64(), Ordering::Relaxed);
                        solution_found.store(true, Ordering::Relaxed);
                        self.found_solution.store(found.as_u64(), Ordering::Relaxed);
                        return Some(found);
                    }

                    // 更新解决方案步进
                    solution = solution + U256::from(self.batch_size * self.num_threads as usize);

                    // 偶尔让出一下CPU，避免100%占用导致热量问题
                    if thread_hashes.load(Ordering::Relaxed) % 1_000_000 == 0 {
                        std::thread::yield_now();
                    }
                }

                None
            })
            .find_any(|result| result.is_some())
            .flatten();

        if let Some(solution) = result {
            return Some(solution);
        }

        // 如果有解决方案但上面没找到，可能是其他线程设置了标志
        if solution_found.load(Ordering::Relaxed) {
            return Some(U256::from(self.found_solution.load(Ordering::Relaxed)));
        }

        None
    }

    /// 重置矿工状态
    pub fn reset(&self) {
        self.solution_found.store(false, Ordering::Relaxed);
        self.total_hashes.store(0, Ordering::Relaxed);
        self.found_solution.store(0, Ordering::Relaxed);
    }

    /// 获取哈希计算速率
    pub fn get_hash_rate(&self, elapsed_secs: f64) -> f64 {
        let count = self.total_hashes.load(Ordering::Relaxed);
        count as f64 / elapsed_secs
    }
}
