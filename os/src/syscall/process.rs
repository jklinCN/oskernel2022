use crate::config::CLOCK_FREQ;
use crate::fs::{open, OpenFlags};
use crate::mm::{translated_byte_buffer, translated_ref, translated_refmut, translated_str, UserBuffer, MmapProts, MmapFlags};
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next, pid2task, suspend_current_and_run_next, RLimit, RUsage,
    SignalFlags,
};
use crate::timer::{get_time, get_time_ms, get_timeval, tms, TimeVal, NSEC_PER_SEC};
use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::mem::size_of;
pub use crate::task::{CloneFlags, Utsname, UTSNAME};
// use simple_fat32::{CACHEGET_NUM,CACHEHIT_NUM};

/// 结束进程运行然后运行下一程序
pub fn sys_exit(exit_code: i32) -> ! {
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_exit_group(exit_code: i32) -> ! {
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit_group!");
}
/// ### 应用主动交出 CPU 所有权进入 Ready 状态并切换到其他应用
/// - 返回值：总是返回 0。
/// - syscall ID：124
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

/// ### sleep 给定时长（TimeVal格式）
/// - 返回值：总是返回 0。
/// - syscall ID：101
pub fn sys_nanosleep(buf: *const u8) -> isize {
    let tic = get_time_ms();

    let token = current_user_token();
    let len_timeval = translated_ref(token, buf as *const TimeVal);
    let len = len_timeval.sec * 1000 + len_timeval.usec / 1000;
    loop {
        let toc = get_time_ms();
        if toc - tic >= len {
            break;
        }
    }
    0
}

/// ### 获取CPU上电时间 秒+微秒
/// syscall_id：169
/// - 输入参数
///     - `ts`：`TimeVal` 结构体在用户空间的地址
///     - `tz`：表示时区，这里无需考虑，始终为0
/// - 功能：内核根据时钟周期数和时钟频率换算系统运行时间，并写入到用户地址空间
/// - 返回值：正确执行返回 0，出现错误返回 -1。
pub fn sys_gettimeofday(buf: *const u8) -> isize {
    let token = current_user_token();
    let buffers = translated_byte_buffer(token, buf, core::mem::size_of::<TimeVal>());
    let mut userbuf = UserBuffer::new(buffers);
    userbuf.write(get_timeval().as_bytes());
    0
}

pub fn sys_times(buf: *const u8) -> isize {
    let sec = get_time_ms() as isize * 1000;
    let token = current_user_token();
    let buffers = translated_byte_buffer(token, buf, core::mem::size_of::<tms>());
    let mut userbuf = UserBuffer::new(buffers);
    userbuf.write(
        tms {
            tms_stime: sec,
            tms_utime: sec,
            tms_cstime: sec,
            tms_cutime: sec,
        }
        .as_bytes(),
    );
    0
}

//  long clone(unsigned long flags, void *child_stack, int *ptid, int *ctid, unsigned long newtls);

/// ### 当前进程 fork/clone 出来一个子进程。
/// - 参数：
///     - `flags`:
///     - `stack_ptr`
///     - `ptid`
///     - `ctid`
///     - `newtls`
/// - 返回值：对于子进程返回 0，对于当前进程则返回子进程的 PID 。
/// - syscall ID：220
pub fn sys_fork(flags: usize, stack_ptr: usize, _ptid: usize, _ctid: usize, _newtls: usize) -> isize {
    // println!(
    //     "[DEBUG] enter sys_fork: flags:{}, stack_ptr:{}, ptid:{}, ctid:{}, newtls:{}",
    //     flags, stack_ptr, _ptid, _ctid, _newtls
    // );
    let current_task = current_task().unwrap();
    let new_task = current_task.fork(false);

    // let tid = new_task.getpid();
    let flags = CloneFlags::from_bits(flags).unwrap();
    _ = flags;
    // if flags.contains(CloneFlags::CLONE_CHILD_SETTID) && ctid != 0{
    //     new_task.inner_exclusive_access().address.set_child_tid = ctid;
    //     *translated_refmut(new_task.inner_exclusive_access().get_user_token(), ctid as *mut i32) = tid  as i32;
    // }
    // if flags.contains(CloneFlags::CLONE_CHILD_CLEARTID) && ctid != 0{
    //     new_task.inner_exclusive_access().address.clear_child_tid = ctid;
    // }
    // if !flags.contains(CloneFlags::SIGCHLD) {
    //     panic!("sys_fork: FLAG not supported!");
    // }

    if stack_ptr != 0 {
        let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
        trap_cx.set_sp(stack_ptr);
    }
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // trap_handler 已经将当前进程 Trap 上下文中的 sepc 向后移动了 4 字节，
    // 使得它回到用户态之后，会从发出系统调用的 ecall 指令的下一条指令开始执行

    trap_cx.x[10] = 0; // 对于子进程，返回值是0
    add_task(new_task); // 将 fork 到的进程加入任务调度器
    unsafe {
        core::arch::asm!("sfence.vma");
        core::arch::asm!("fence.i");
    }
    new_pid as isize // 对于父进程，返回值是子进程的 PID
}

/// ### 将当前进程的地址空间清空并加载一个特定的可执行文件，返回用户态后开始它的执行。
/// - 参数：
///     - `path` 给出了要加载的可执行文件的名字
///     - `args` 数组中的每个元素都是一个命令行参数字符串的起始地址，以地址为0表示参数尾
///     - 'envs' 环境变量，暂未处理，直接加地址0结束
/// - 返回值：如果出错的话（如找不到名字相符的可执行文件）则返回 -1，否则返回参数个数 `argc`。
/// - syscall ID：221
pub fn sys_exec(path: *const u8, mut args: *const usize, mut _envs: *const usize) -> isize {
    let token = current_user_token();
    // 读取到用户空间的应用程序名称（路径）
    let path = translated_str(token, path);
    let mut args_vec: Vec<String> = Vec::new();
    if args as usize != 0 {
        loop {
            let arg_str_ptr = *translated_ref(token, args);
            if arg_str_ptr == 0 {
                // 读到下一参数地址为0表示参数结束
                break;
            } // 否则从用户空间取出参数，压入向量
            args_vec.push(translated_str(token, arg_str_ptr as *const u8));
            unsafe {
                args = args.add(1);
            }
        }
    }

    // 环境变量
    let mut envs_vec: Vec<String> = Vec::new();
    envs_vec.push("LD_LIBRARY_PATH=/".to_string());
    envs_vec.push("PATH=/".to_string());
    envs_vec.push("ENOUGH=2500".to_string());
    // envs_vec.push("TIMING_0=7".to_string());
    // envs_vec.push("LOOP_O=0.2".to_string());

    let task = current_task().unwrap();
    // println!("[kernel] exec name:{},argvs:{:?}", path, args_vec);
    // println!("time start!{}",get_time_ms());
    if path.ends_with(".sh") {
        let mut new_args = Vec::new();
        new_args.push("./busybox".to_string());
        new_args.push("sh".to_string());
        for i in &args_vec {
            new_args.push(i.clone());
        }
        task.exec(open("/", "busybox", OpenFlags::O_RDONLY).unwrap(), new_args, envs_vec);
        // memory_usage();
        return 0 as isize;
    }

    let inner = task.inner_exclusive_access();
    if let Some(app_inode) = open(inner.current_path.as_str(), path.as_str(), OpenFlags::O_RDONLY) {
        drop(inner);
        task.exec(app_inode, args_vec, envs_vec);
        // memory_usage();
        0 as isize
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
///     - 否则如果要等待的子进程均未结束则返回，则放权等待；
///     - 否则返回结束的子进程的进程 ID。
/// - syscall ID：260
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    // println!("[KERNEL] pid {} waitpid {}",current_task().unwrap().pid.0, pid);
    // crate::task::debug_show_ready_queue();
    let task = current_task().unwrap();
    // ---- access current TCB exclusively
    let inner = task.inner_exclusive_access();

    // 根据pid参数查找有没有符合要求的进程
    if !inner.children.iter().any(|p| pid == -1 || pid as usize == p.getpid()) {
        return -1;
        // ---- release current PCB
    }
    drop(inner);
    loop {
        let mut inner = task.inner_exclusive_access();
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
            // println!("[KERNEL] pid {} waitpid {}",current_task().unwrap().pid.0, pid);
            assert_eq!(Arc::strong_count(&child), 1);
            // 收集的子进程信息返回
            let found_pid = child.getpid();
            // ++++ temporarily access child TCB exclusively
            let exit_code = child.inner_exclusive_access().exit_code;
            // ++++ release child PCB
            // 将子进程的退出码写入到当前进程的应用地址空间中
            if exit_code_ptr as usize != 0 {
                *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code << 8;
            }
            return found_pid as isize;
        } else {
            // 如果找不到的话则放权等待
            drop(inner); // 手动释放 TaskControlBlock 全局可变部分
            suspend_current_and_run_next();
        }
        // ---- release current PCB lock automatically
    }
}

pub fn sys_kill(pid: usize, signal: u32) -> isize {
    // println!("[KERNEL] enter sys_kill: pid:{} send to pid:{}, signal:0x{:x}",current_task().unwrap().pid.0, pid, signal);
    if signal == 0 {
        return 0;
    }
    let signal = 1 << signal;
    if let Some(task) = pid2task(pid) {
        if let Some(flag) = SignalFlags::from_bits(signal) {
            task.inner_exclusive_access().signals |= flag;
            0
        } else {
            panic!("[DEBUG] sys_kill: unsupported signal");
        }
    } else {
        1
    }
}

/// ### 获取系统utsname参数
/// - 参数
///     - `buf`：用户空间存放utsname结构体的缓冲区
/// - 返回值
///     - 0表示正常
/// - syscall_ID: 160
pub fn sys_uname(buf: *const u8) -> isize {
    let token = current_user_token();
    let uname = UTSNAME.lock();
    let mut userbuf = UserBuffer::new(translated_byte_buffer(token, buf, core::mem::size_of::<Utsname>()));
    userbuf.write(uname.as_bytes());
    0
}

// 在进程虚拟地址空间中分配创建一片虚拟内存地址映射
pub fn sys_mmap(addr: usize, length: usize, prot: usize, flags: usize, fd: isize, offset: usize) -> isize {
    if length == 0 {
        panic!("mmap:len == 0");
    }
    let prot = MmapProts::from_bits(prot).expect("unsupported mmap prot");
    let flags = MmapFlags::from_bits(flags).expect("unsupported mmap flags");

    // info!("[KERNEL syscall] enter mmap: addr:0x{:x}, length:0x{:x}, prot:{:?}, flags:{:?},fd:{}, offset:0x{:x}", addr, length, prot, flags, fd, offset);
    
    let task = current_task().unwrap();
    let result_addr = task.mmap(addr, length, prot, flags, fd, offset);
    // info!("[DEBUG] sys_mmap return: 0x{:x}", result_addr);
    return result_addr as isize;
}

pub fn sys_munmap(addr: usize, length: usize) -> isize {
    let task = current_task().unwrap();
    let ret = task.munmap(addr, length);
    ret
}

pub fn sys_sbrk(grow_size: isize, _is_shrink: usize) -> isize {
    let current_va = current_task().unwrap().grow_proc(grow_size) as isize;
    current_va
}

pub fn sys_brk(brk_addr: usize) -> isize {
    // info!("[DEBUG] enter sys_brk: brk_addr:0x{:x}",brk_addr);
    let mut addr_new = 0;
    _ = addr_new;
    if brk_addr == 0 {
        addr_new = sys_sbrk(0, 0) as usize;
    } else {
        let former_addr = current_task().unwrap().grow_proc(0);
        let grow_size: isize = (brk_addr - former_addr) as isize;
        addr_new = current_task().unwrap().grow_proc(grow_size);
    }
    // info!("[DEBUG] sys_brk return: 0x{:x}",addr_new);
    addr_new as isize
}

pub fn sys_prlimit64(_pid: usize, resource: usize, new_limit: *const u8, old_limit: *const u8) -> isize {
    let token = current_user_token();
    // println!("[DEBUG] enter sys_prlimit64: pid:{},resource:{},new_limit:{},old_limit:{}",pid,resource,new_limit as usize,old_limit as usize);
    if old_limit as usize != 0 {
        let mut buf = UserBuffer::new(translated_byte_buffer(token, old_limit as *const u8, size_of::<RLimit>()));
        let task = current_task().unwrap();
        let inner = task.inner_exclusive_access();
        let old_resource = inner.resource[resource];
        let old_limit = RLimit {
            rlim_cur: old_resource.rlim_cur,
            rlim_max: old_resource.rlim_max,
        };
        buf.write(old_limit.as_bytes());
    }

    if new_limit as usize != 0 {
        let buf = translated_byte_buffer(token, new_limit as *const u8, size_of::<RLimit>())
            .pop()
            .unwrap();
        let addr = buf.as_ptr() as *const _ as usize;
        let new_limit = unsafe { &*(addr as *const RLimit) };
        let task = current_task().unwrap();
        let mut inner = task.inner_exclusive_access();
        let rlimit = RLimit {
            rlim_cur: new_limit.rlim_cur,
            rlim_max: new_limit.rlim_max,
        };
        inner.resource[resource] = rlimit;
    }
    0
}

pub fn sys_clock_gettime(_clk_id: usize, ts: *mut u64) -> isize {
    if ts as usize == 0 {
        return 0;
    }
    let token = current_user_token();
    let ticks = get_time();
    let sec = (ticks / CLOCK_FREQ) as u64;
    let nsec = ((ticks % CLOCK_FREQ) * (NSEC_PER_SEC / CLOCK_FREQ)) as u64;
    *translated_refmut(token, ts) = sec;
    *translated_refmut(token, unsafe { ts.add(1) }) = nsec;
    0
}

/// 获取当前正在运行程序的 PID
pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

pub fn sys_getppid() -> isize {
    current_task().unwrap().tgid as isize
}

pub fn sys_geteuid() -> isize {
    0
}

pub fn sys_getegid() -> isize {
    0
}

pub fn sys_gettid() -> isize {
    0
}

pub fn sys_getuid() -> isize {
    0
}

pub fn sys_getpgid() -> isize {
    0
}

pub fn sys_setpgid() -> isize {
    0
}

pub fn sys_ppoll() -> isize {
    1
}

pub fn sys_sysinfo() -> isize {
    0
}

pub fn sys_faccessat() -> isize {
    0
}

pub fn sys_madvise(_addr: *const u8, _length: usize, _advice: usize) -> isize {
    // println!("[DEBUG] enter sys_madvise: addr:{}, length:{}, advice:{}",addr as usize,length,advice);
    0
}

const RUSAGE_SELF: isize = 0;
pub fn sys_getrusage(who: isize, usage: *mut u8) -> isize {
    if who != RUSAGE_SELF {
        panic!("sys_getrusage: \"who\" not supported!");
    }
    let token = current_user_token();
    let mut userbuf = UserBuffer::new(translated_byte_buffer(token, usage, core::mem::size_of::<RUsage>()));
    let rusage = RUsage::new();
    userbuf.write(rusage.as_bytes());
    0
}

pub fn sys_set_tid_address(tidptr: *mut usize) -> isize {
    let token = current_user_token();
    *translated_refmut(token, tidptr) = 0 as usize;
    0
}
