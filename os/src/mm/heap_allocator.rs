/// # 堆空间内存动态分配模块
/// `os/src/mm/heap_allocator.rs`
/// ## 实现功能
/// ```
/// pub fn init_heap()
/// pub fn handle_alloc_error(layout: core::alloc::Layout) -> !
/// ```
//

use crate::config::KERNEL_HEAP_SIZE;
use buddy_system_allocator::LockedHeap;

// 将 buddy_system_allocator 中提供的 LockedHeap 实例化成一个全局变量
// 并使用 alloc 要求的 #[global_allocator] 语义项进行标记
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

/// 用于处理动态内存分配失败的情形,直接panic
#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}
/// 给全局分配器用于分配的一块内存，位于内核的 .bss 段中
static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 在使用任何 alloc 中提供的堆数据结构之前，我们需要先调用 init_heap 函数来给我们的全局分配器一块内存用于分配。
pub fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR
            .lock()// LockedHeap 也是一个被互斥锁 Mutex<T> 保护的类型，
            // 在对它任何进行任何操作之前都要先获取锁以避免其他线程同时对它进行操作导致数据竞争
            .init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
            // 调用 init 方法告知它能够用来分配的空间的起始地址和大小即可
    }
}

/// ### 尝试动态内存分配
/// 其中分别使用智能指针 Box<T> 和向量 Vec<T> 在堆上分配数据并管理它们，
/// 通过 as_ref 和 as_ptr 方法可以分别看到它们指向的数据的位置，能够确认它们的确在位于 .bss 段的堆上。
#[allow(unused)]
pub fn heap_test() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    extern "C" {
        fn sbss();
        fn ebss();
    }
    let bss_range = sbss as usize..ebss as usize;
    let a = Box::new(5);
    assert_eq!(*a, 5);
    assert!(bss_range.contains(&(a.as_ref() as *const _ as usize)));
    drop(a);
    let mut v: Vec<usize> = Vec::new();
    for i in 0..500 {
        v.push(i);
    }
    for (i, val) in v.iter().take(500).enumerate() {
        assert_eq!(*val, i);
    }
    assert!(bss_range.contains(&(v.as_ptr() as usize)));
    drop(v);
    println!("heap_test passed!");
}
