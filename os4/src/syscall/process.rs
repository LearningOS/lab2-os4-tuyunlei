//! Process management syscalls

use core::intrinsics::size_of;
use core::slice;
use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE};
use crate::mm::{copy_data_into_space, frame_alloc};
use crate::task::{current_task_mmap, current_task_munmap, current_user_token, exit_current_and_run_next, get_current_task_info, suspend_current_and_run_next, TaskStatus};
use crate::timer::get_time_us;

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
    info!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(ts_ptr: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    unsafe { copy_data_into_space(&ts, current_user_token(), ts_ptr) };
    0
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    if current_task_mmap(start, len, port).is_some() {
        0
    } else {
        -1
    }
}

pub fn sys_munmap(start: usize, len: usize) -> isize {
    if current_task_munmap(start, len).is_some() {
        0
    } else {
        -1
    }
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti_ptr: *mut TaskInfo) -> isize {
    let task_info = get_current_task_info();
    unsafe { copy_data_into_space(&task_info, current_user_token(), ti_ptr) };
    0
}

