// user/src/lib.rs
// 用户模式下程序的主文件

#![no_std]
#![feature(linkage)]    // 为了支持软链接操作而加入
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

#[macro_use]
pub mod console;
mod lang_items;

use buddy_system_allocator::LockedHeap;

const USER_HEAP_SIZE: usize = 16384;

static mut HEAP_SPACE: [u8; USER_HEAP_SIZE] = [0; USER_HEAP_SIZE];

#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "C" fn _start() -> ! {
    unsafe {    // 初始化一个由伙伴系统控制的堆空间
        HEAP.lock()
            .init(HEAP_SPACE.as_ptr() as usize, USER_HEAP_SIZE);
    }
    // 调用main函数得到一个类型为i32的返回值
    // 最后调用用户库提供的 exit 接口退出应用程序
    // 并将 main 函数的返回值告知批处理系统
    exit(main());   
    panic!("unreachable after sys_exit!");
}

// 我们使用 Rust 的宏将其函数符号 main 标志为弱链接。
// 这样在最后链接的时候，虽然在 lib.rs 和 bin 目录下的某个应用程序
// 都有 main 符号，但由于 lib.rs 中的 main 符号是弱链接，
// 链接器会使用 bin 目录下的应用主逻辑作为 main 。
// 这里我们主要是进行某种程度上的保护，如果在 bin 目录下找不到任何 main ，
// 那么编译也能够通过，但会在运行时报错。
#[linkage = "weak"]
#[no_mangle]
fn main() -> i32 {
    panic!("Cannot find main!");
}

mod syscall;
use syscall::*;

/// ### 从文件描述符读取字符到缓冲区
/// - `fd` : 文件描述符
///     - 0表示标准输入
/// - `buf`: 缓冲区起始地址
pub fn read(fd: usize, buf: &mut [u8]) -> isize {
    sys_read(fd, buf)
}
/// ### 打印输出
/// - `fd` : 文件描述符
///     - 1表示标准输出
/// - `buf`: 缓冲区起始地址
pub fn write(fd: usize, buf: &[u8]) -> isize {
    sys_write(fd, buf)
}
pub fn exit(exit_code: i32) -> isize {
    sys_exit(exit_code)
}
pub fn yield_() -> isize {
    sys_yield()
}
pub fn get_time() -> isize {
    sys_get_time()
}
pub fn getpid() -> isize {
    sys_getpid()
}
/// ### 系统调用 `sys_fork` 的封装
/// - 返回值：对于子进程返回 0，对于当前进程则返回子进程的 PID
pub fn fork() -> isize {
    sys_fork()
}
/// ### 系统调用 `sys_exec` 的封装
/// - 参数 path 必须在最后加 \0
pub fn exec(path: &str) -> isize {
    sys_exec(path)
}
/// ### 当前进程等待一个子进程变为僵尸进程，回收其全部资源并收集其返回值
/// - 返回值：
///     - 如果要等待的子进程不存在则返回 -1；
///     - 否则如果要等待的子进程均未结束则返回 -2；
///     - 否则返回结束的子进程的进程 ID
pub fn wait(exit_code: &mut i32) -> isize {
    loop {  // 循环检查，后期会修改为阻塞的
        match sys_waitpid(-1, exit_code as *mut _) {
            -2 => { //要等待的子进程存在但它却尚未退出
                yield_();
            }
            // -1 or a real pid
            exit_pid => return exit_pid,
        }
    }
}
/// ### 等待一个进程标识符的值为 `pid` 的子进程结束
pub fn waitpid(pid: usize, exit_code: &mut i32) -> isize {
    loop {  // 循环检查，后期会修改为阻塞的
        match sys_waitpid(pid as isize, exit_code as *mut _) {
            -2 => { // 要等待的子进程存在但它却尚未退出
                yield_();
            }
            // -1 or a real pid
            exit_pid => return exit_pid,
        }
    }
}
/// ### 通过 `sys_yield` 放弃CPU一段时间
pub fn sleep(period_ms: usize) {
    let start = sys_get_time();
    while sys_get_time() < start + period_ms as isize {
        sys_yield();
    }
}
