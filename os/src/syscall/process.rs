/// # 进程控制模块
/// `os/src/syscall/process.rs`
/// ## 实现功能
/// ```
/// pub fn sys_exit(exit_code: i32) -> !
/// pub fn sys_yield() -> isize
/// pub fn sys_get_time() -> isize
/// pub fn sys_getpid() -> isize
/// pub fn sys_fork() -> isize
/// pub fn sys_exec(path: *const u8) -> isize
/// pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize
/// ```
//

use crate::loader::get_app_data_by_name;
use crate::mm::{translated_refmut, translated_str};
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next,
};
use crate::timer::get_time_ms;
use alloc::sync::Arc;

/// 结束进程运行然后运行下一程序
pub fn sys_exit(exit_code: i32) -> ! {
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// ### 应用主动交出 CPU 所有权进入 Ready 状态并切换到其他应用
/// - 返回值：总是返回 0。
/// - syscall ID：124
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

/// 获取CPU上电时间（单位：ms）
pub fn sys_get_time() -> isize {
    get_time_ms() as isize
}

/// 获取当前正在运行程序的 PID
pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

/// ### 当前进程 fork 出来一个子进程。
/// - 返回值：对于子进程返回 0，对于当前进程则返回子进程的 PID 。
/// - syscall ID：220
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // trap_handler 已经将当前进程 Trap 上下文中的 sepc 向后移动了 4 字节，
    // 使得它回到用户态之后，会从发出系统调用的 ecall 指令的下一条指令开始执行

    // 对于子进程，返回值是0
    trap_cx.x[10] = 0;
    // 将 fork 到的进程加入任务调度器
    add_task(new_task);
    // 对于父进程，返回值是子进程的 PID
    new_pid as isize
}

/// ### 将当前进程的地址空间清空并加载一个特定的可执行文件，返回用户态后开始它的执行。
/// - 参数：path 给出了要加载的可执行文件的名字；
/// - 返回值：如果出错的话（如找不到名字相符的可执行文件）则返回 -1，否则不应该返回。
/// - syscall ID：221
pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    // 读取到用户空间的应用程序名称（路径）
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// ### 当前进程等待一个子进程变为僵尸进程，回收其全部资源并收集其返回值。
/// - 参数：
///     - pid 表示要等待的子进程的进程 ID，如果为 -1 的话表示等待任意一个子进程；
///     - exit_code 表示保存子进程返回值的地址，如果这个地址为 0 的话表示不必保存。
/// - 返回值：
///     - 如果要等待的子进程不存在则返回 -1；
///     - 否则如果要等待的子进程均未结束则返回 -2；
///     - 否则返回结束的子进程的进程 ID。
/// - syscall ID：260
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    // ---- access current TCB exclusively
    let mut inner = task.inner_exclusive_access();

    // 根据pid参数查找有没有符合要求的进程
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }

    // 查找所有符合PID要求的处于僵尸状态的进程，如果有的话还需要同时找出它在当前进程控制块子进程向量中的下标
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB lock exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    }); 


    if let Some((idx, _)) = pair {
        // 将子进程从向量中移除并置于当前上下文中
        let child = inner.children.remove(idx);
        // 确认这是对于该子进程控制块的唯一一次强引用，即它不会出现在某个进程的子进程向量中，
        // 更不会出现在处理器监控器或者任务管理器中。当它所在的代码块结束，这次引用变量的生命周期结束，
        // 将导致该子进程进程控制块的引用计数变为 0 ，彻底回收掉它占用的所有资源，
        // 包括：内核栈和它的 PID 还有它的应用地址空间存放页表的那些物理页帧等等
        assert_eq!(Arc::strong_count(&child), 1);
        // 收集的子进程信息返回
        let found_pid = child.getpid();
        // ++++ temporarily access child TCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        // 将子进程的退出码写入到当前进程的应用地址空间中
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2  // 如果找不到的话直接返回 -2
    }
    // ---- release current PCB lock automatically
}