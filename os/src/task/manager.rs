/// # 任务管理器

use super::TaskControlBlock;
use spin::Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use lazy_static::*;

/// ### FIFO 任务管理器
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// 将一个任务加入队尾
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// 从队头中取出一个任务
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
}

lazy_static! {
    pub static ref TASK_MANAGER: Mutex<TaskManager> =
        Mutex::new(TaskManager::new()) ;
    pub static ref PID2TCB: Mutex<BTreeMap<usize, Arc<TaskControlBlock>>> =
        Mutex::new(BTreeMap::new());
}

/// 将一个任务加入到全局 `FIFO 任务管理器` `TASK_MANAGER` 就绪队列的队尾
pub fn add_task(task: Arc<TaskControlBlock>) {
    PID2TCB
        .lock()
        .insert(task.getpid(), Arc::clone(&task));
    TASK_MANAGER.lock().add(task);
}

/// 从全局变量 `TASK_MANAGER` 就绪队列的队头中取出一个任务
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.lock().fetch()
}

/// 通过PID获取对应的进程控制块
pub fn pid2task(pid: usize) -> Option<Arc<TaskControlBlock>> {
    let map = PID2TCB.lock();
    map.get(&pid).map(Arc::clone)
}

pub fn remove_from_pid2task(pid: usize) {
    let mut map = PID2TCB.lock();
    if map.remove(&pid).is_none() {
        panic!("cannot find pid {} in pid2task!", pid);
    }
}

#[allow(unused)]
pub fn debug_show_ready_queue() {
    for task in TASK_MANAGER.lock().ready_queue.iter() {
        let inner = task.inner_exclusive_access();
        println!("pid = {}, signals: {:?}", task.pid.0, inner.signals);
    }
}