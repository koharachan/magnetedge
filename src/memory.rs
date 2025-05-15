use std::alloc::{alloc, dealloc, Layout};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 全局注册jemalloc作为内存分配器 (仅在非Windows且非Android平台)
#[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

// 对于Termux/Android平台使用系统默认分配器
#[cfg(target_os = "android")]
#[inline(always)]
pub fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
}

/// 预分配的缓冲池，减少频繁的内存分配
pub struct BufferPool<T> {
    buffers: Vec<UnsafeCell<Vec<T>>>,
    in_use: AtomicUsize,
}

impl<T> BufferPool<T> {
    /// 释放一个缓冲区
    fn release(&self, _idx: usize) {
        self.in_use.fetch_sub(1, Ordering::Release);
    }
}

impl<T: Clone + Default> BufferPool<T> {
    /// 创建一个新的缓冲池
    pub fn new(count: usize, capacity: usize) -> Self {
        let mut buffers = Vec::with_capacity(count);
        for _ in 0..count {
            buffers.push(UnsafeCell::new(Vec::with_capacity(capacity)));
        }

        BufferPool {
            buffers,
            in_use: AtomicUsize::new(0),
        }
    }

    /// 获取一个缓冲区
    pub fn get(&self) -> Option<PoolBuffer<T>> {
        let current = self.in_use.fetch_add(1, Ordering::Acquire);
        if current < self.buffers.len() {
            // 安全: 我们通过原子计数确保每个缓冲区只被一个线程访问
            let buffer_ptr = unsafe { &mut *self.buffers[current].get() };
            buffer_ptr.clear();

            Some(PoolBuffer {
                buffer: unsafe { NonNull::new_unchecked(buffer_ptr as *mut Vec<T>) },
                pool: self,
                idx: current,
                _marker: PhantomData,
            })
        } else {
            self.in_use.fetch_sub(1, Ordering::Release);
            None
        }
    }
}

unsafe impl<T: Send> Send for BufferPool<T> {}
unsafe impl<T: Sync> Sync for BufferPool<T> {}

/// 从缓冲池获取的缓冲区
pub struct PoolBuffer<'a, T> {
    buffer: NonNull<Vec<T>>,
    pool: &'a BufferPool<T>,
    idx: usize,
    _marker: PhantomData<&'a mut Vec<T>>,
}

impl<'a, T> std::ops::Deref for PoolBuffer<'a, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        unsafe { self.buffer.as_ref() }
    }
}

impl<'a, T> std::ops::DerefMut for PoolBuffer<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.buffer.as_mut() }
    }
}

impl<'a, T> Drop for PoolBuffer<'a, T> {
    fn drop(&mut self) {
        self.pool.release(self.idx);
    }
}

/// 在高性能场景中的内存对齐辅助函数
#[inline(always)]
pub fn aligned_alloc(size: usize, align: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, align).unwrap();
    unsafe { alloc(layout) }
}

#[inline(always)]
pub fn aligned_dealloc(ptr: *mut u8, size: usize, align: usize) {
    let layout = Layout::from_size_align(size, align).unwrap();
    unsafe { dealloc(ptr, layout) }
}
