use ethers::types::U256;
use num_bigint::BigUint;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tiny_keccak::{Hasher, Keccak};

/// 高性能的Keccak256哈希计算
/// 使用tiny-keccak库，性能比ethers::utils::keccak256高很多
#[inline(always)]
pub fn keccak256_opt(input: &[u8]) -> [u8; 32] {
    let mut keccak = Keccak::v256();
    let mut result = [0u8; 32];
    keccak.update(input);
    keccak.finalize(&mut result);
    result
}

/// 优化的批量哈希计算
/// 计算多个哈希，并检查是否满足阈值要求
pub fn compute_hash_batch(
    encoded_buffers: &[Vec<u8>],
    solutions: &[U256],
    threshold: &BigUint,
    total_hashes: &Arc<AtomicU64>,
    thread_hashes: &Arc<AtomicU64>,
    solution_found: &Arc<AtomicBool>,
) -> Option<U256> {
    for (i, buffer) in encoded_buffers.iter().enumerate() {
        if solution_found.load(Ordering::Relaxed) {
            return None;
        }

        // 使用高性能keccak实现
        let hash = keccak256_opt(buffer);

        // 计数器更新
        thread_hashes.fetch_add(1, Ordering::Relaxed);
        total_hashes.fetch_add(1, Ordering::Relaxed);

        // 检查哈希值是否满足难度要求，使用大整数比较
        let hash_bigint = BigUint::from_bytes_be(&hash);
        if hash_bigint <= *threshold {
            return Some(solutions[i]);
        }
    }

    None
}

/// 计算一个batch内所有解决方案的哈希值，利用SIMD处理
#[allow(dead_code)]
pub fn compute_solutions_batch<F>(_solutions: &[U256], _prefix: &[u8], _callback: F)
where
    F: FnMut(U256, [u8; 32]),
{
    // 未来可以添加SIMD版本的批量处理，此处暂时只是布局
    unimplemented!("待实现SIMD版本")
}

/// 预分配一组编码缓冲区
pub fn create_encoding_buffers(count: usize, capacity: usize) -> Vec<Vec<u8>> {
    let mut buffers = Vec::with_capacity(count);
    for _ in 0..count {
        buffers.push(Vec::with_capacity(capacity));
    }
    buffers
}

/// 优化版本的编码函数，减少内存分配
pub fn encode_solution(prefix: &[u8], solution: U256, buffer: &mut Vec<u8>) {
    buffer.clear();
    buffer.extend_from_slice(prefix);

    let mut solution_bytes = [0u8; 32];
    solution.to_big_endian(&mut solution_bytes);
    buffer.extend_from_slice(&solution_bytes);
}
