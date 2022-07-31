//! Process management syscalls

use crate::mm::{MapPermission, translated_byte_buffer, translated_refmut, translated_str, VirtAddr};
use crate::task::{add_task, current_task, current_user_token, exit_current_and_run_next, suspend_current_and_run_next, take_current_task, TaskControlBlock, TaskStatus};
use crate::timer::{get_time_milli, get_time_us};
use alloc::sync::Arc;
use core::mem::size_of;

use crate::config::MAX_SYSCALL_NUM;
use crate::fs::{open_file, OpenFlags};
use crate::sbi::shutdown;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    println!("[kernel] Application exited with code {}", exit_code);
    if current_task().unwrap().pid.0==0 {
        println!("pid 0 cannot quit");
        shutdown();
    }
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

/// Syscall Fork which returns 0 for child process and child_pid for parent process
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

/// Syscall Exec which accepts the elf path
pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(),OpenFlags::RDONLY) {
        let data=inode.read_all();
        let task = current_task().unwrap();
        task.exec(data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    // find a child process

    // ---- access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB lock exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after removing from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child TCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB lock automatically
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let buffers = translated_byte_buffer(current_user_token(), _ts as *mut u8, size_of::<TimeVal>());
    assert_eq!(1, buffers.len());
    let ts = unsafe { (buffers[0].as_ptr() as *mut TimeVal).as_mut().unwrap() };
    let us = get_time_us();
    ts.sec = us / 1_000_000;
    ts.usec = us % 1_000_000;
    0
}


// YOUR JOB: 实现sys_set_priority，为任务添加优先级
pub fn sys_set_priority(prio: isize) -> isize {
    if prio<=1{
        return -1
    }
    let task = current_task().unwrap();
    // find a child process

    // ---- access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    inner.priority=prio as usize;
    return prio ;
}



//
// YOUR JOB: 实现 sys_spawn 系统调用
// ALERT: 注意在实现 SPAWN 时不需要复制父进程地址空间，SPAWN != FORK + EXEC 
pub fn sys_spawn(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(),OpenFlags::RDONLY) {
        let data=inode.read_all();
        let parent = current_task().unwrap();
        let new_task=Arc::new( TaskControlBlock::new(data.as_slice(),Some(&parent)));
        let new_pid = new_task.pid.0;
        add_task(new_task.clone());
        parent.inner_exclusive_access().children.push(new_task);
        println!("[kernel] Spawned task {}, path={}", new_pid,path);
        new_pid as isize
    } else {
        println!("[kernel] Spawn failed, path={}", path);
        -1
    }
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, _len: usize, mut _port: usize) -> isize {
    let task = current_task().unwrap();

    // ---- access current TCB exclusively
    let mut task_inner = task.inner_exclusive_access();
    // println!("_start={:#x},_len={:#x},_port={}", _start, _len, _port);
    let start_va = VirtAddr::from(_start);
    if !start_va.aligned() {
        return -1;
    }
    _port = _port << 1;//第0位没有使用
    if (_port & !0x07) != 0 {
        return -1;
    }
    if (_port & 0x07) == 0 {
        return -1;
    }
    let end_va: VirtAddr = (_start + _len).into();

    if task_inner.memory_set.is_mapped(start_va, end_va) {
        println!("error already mapped");
        return -1;
    }
    let perm = MapPermission::from_bits_truncate(_port as u8);
    task_inner.memory_set.insert_framed_area(_start.into(),
                                          (_start + _len).into(),
                                          perm | MapPermission::U)  ;
    if get_current_pid() == 27 {
        // println!("memory_set={:?}", task_inner.memory_set);
        // println!("port={:#x},perm={:?},perm2={:#x}", _port, perm, perm2.bits());
    }
    0
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    let start_va = VirtAddr::from(_start);
    let end_va: VirtAddr = (_start + _len).into();
    let task = current_task().unwrap();

    // ---- access current TCB exclusively
    let mut tcb = task.inner_exclusive_access();
    if !start_va.aligned() {
        return -1;
    }
    if !tcb.memory_set.is_mapped(start_va, end_va) {
        return -1;
    }
    // println!("munmap start_va={:#x},end_va={:#x}", start_va.0, end_va.0);
    if get_current_pid() == 27 {
        // println!("unmap before memory_set={:?}", tcb.memory_set);
        // println!("port={:#x},perm={:?},perm2={:#x}", _port, perm, perm2.bits());
    }
    if !tcb.memory_set.remove_framed_area(start_va,
                                          end_va) {
        return -1;
    }
    if get_current_pid() == 27 {
        // println!("unmap after memory_set={:?}", tcb.memory_set);
        // println!("port={:#x},perm={:?},perm2={:#x}", _port, perm, perm2.bits());
    }
    println!("munmap success");
    0
}

pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let buffers = translated_byte_buffer(current_user_token(), ti as *mut u8, size_of::<TaskInfo>());
    assert_eq!(1, buffers.len());
    let mut ti = unsafe { (buffers[0].as_ptr() as *mut TaskInfo).as_mut().unwrap() };
    let task = current_task().unwrap();

    // ---- access current TCB exclusively
    let mut tcb = task.inner_exclusive_access();
    let time = get_time_milli() - tcb.first_start_time;
    ti.status = TaskStatus::Running;
    ti.time = time;
    ti.syscall_times = tcb.syscall_times;
    0
}


pub fn get_current_pid()->usize{
    let task=current_task();
    task.unwrap().pid.0
}