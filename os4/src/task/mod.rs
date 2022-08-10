//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

use alloc::vec::Vec;

use lazy_static::*;
use riscv::paging::PageTable;

pub use context::TaskContext;
pub use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};
use crate::config::PAGE_SIZE;

use crate::loader::{get_app_data, get_num_app};
use crate::mm::{MapPermission, MemorySet, VirtAddr, VirtPageNum, VPNRange};
use crate::sync::UPSafeCell;
use crate::syscall::TaskInfo;
use crate::timer::get_time_ms;
use crate::trap::TrapContext;

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager(UPSafeCell<TaskManagerInner>);

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    num_app: usize,
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task_id: usize,
}

pub static TASK_MANAGER: TaskManager = TaskManager(
    unsafe {
        UPSafeCell::new(TaskManagerInner {
            num_app: 0,
            tasks: Vec::new(),
            current_task_id: 0,
        })
    }
);

impl TaskManager {
    fn init_task_manager(&self) {
        let mut inner = self.0.exclusive_access();

        let num_app = get_num_app();
        inner.num_app = num_app;
        for i in 0..num_app {
            inner.tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
    }

    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        self.init_task_manager();

        let mut inner = self.0.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        next_task.start_time_ms = get_time_ms();
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.0.exclusive_access();
        let current = inner.current_task_id;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.0.exclusive_access();
        let task = inner.current_task_mut();
        task.task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.0.exclusive_access();
        let current = inner.current_task_id;
        (current + 1..current + inner.num_app + 1)
            .map(|id| id % inner.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.0.exclusive_access();
        inner.current_task().get_user_token()
    }

    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &mut TrapContext {
        let inner = self.0.exclusive_access();
        inner.current_task().get_trap_cx()
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next_id) = self.find_next_task() {
            let mut inner = self.0.exclusive_access();
            let current_id = inner.current_task_id;
            let next_task = &mut inner.tasks[next_id];
            next_task.task_status = TaskStatus::Running;
            if next_task.start_time_ms == 0 {
                next_task.start_time_ms = get_time_ms();
            }
            inner.current_task_id = next_id;
            let current_task_cx_ptr = &mut inner.tasks[current_id].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next_id].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    fn get_current_task_info(&self) -> TaskInfo {
        let inner = self.0.exclusive_access();
        let task = &inner.tasks[inner.current_task_id];
        let current_time_ms = get_time_ms();
        debug!("[kernel] start={} current={} delta={}", task.start_time_ms, current_time_ms, current_time_ms - task.start_time_ms);

        TaskInfo {
            status: task.task_status,
            syscall_times: task.syscall_times,
            time: current_time_ms - task.start_time_ms,
        }
    }

    fn increase_syscall_times(&self, syscall_id: usize) {
        let mut inner = self.0.exclusive_access();
        let task = inner.current_task_mut();
        task.syscall_times[syscall_id] += 1;
    }

    fn decrease_syscall_times(&self, syscall_id: usize) {
        let mut inner = self.0.exclusive_access();
        let task = inner.current_task_mut();
        assert_ne!(task.syscall_times[syscall_id], 0);
        task.syscall_times[syscall_id] -= 1;
    }

    fn mmap(&self, start: usize, len: usize, port: usize) -> Option<()> {
        if start & (PAGE_SIZE - 1) != 0 {
            debug!("[kernel] [app {}] start not aligned, mmap failed", get_current_task_id());
            return None;
        }
        if port & !0b111 != 0 || port & 0b111 == 0 {
            debug!("[kernel] [app {}] port `{:#b}` is illegal, mmap failed", port, get_current_task_id());
            return None;
        }
        let start_va = VirtAddr::from(start);
        let end_va: VirtAddr = VirtAddr::from(start + len).ceil().into();

        let mut inner = self.0.exclusive_access();
        let memory_set = &mut inner.current_task_mut().memory_set;
        if memory_set.is_conflict(start_va, end_va) {
            debug!("[kernel] [app {}] memory conflicted, mmap failed", inner.current_task_id);
            return None;
        }
        let permission = MapPermission::from_bits((port << 1) as u8)? | MapPermission::U;
        memory_set.insert_framed_area(start_va, end_va, permission)?;
        Some(())
    }

    fn munmap(&self, start: usize, len: usize) -> Option<()> {
        let start_va = VirtAddr::from(start);
        let end_va: VirtAddr = VirtAddr::from(start + len).ceil().into();

        let mut inner = self.0.exclusive_access();
        let memory_set = &mut inner.current_task_mut().memory_set;
        memory_set.unmap_area(start_va, end_va)
    }
}

impl TaskManagerInner {
    #[inline]
    fn current_task(&self) -> &TaskControlBlock {
        &self.tasks[self.current_task_id]
    }

    #[inline]
    fn current_task_mut(&mut self) -> &mut TaskControlBlock {
        &mut self.tasks[self.current_task_id]
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
#[inline]
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

#[inline]
pub fn increase_syscall_times(syscall_id: usize) {
    TASK_MANAGER.increase_syscall_times(syscall_id)
}

#[inline]
pub fn decrease_syscall_times(syscall_id: usize) {
    TASK_MANAGER.decrease_syscall_times(syscall_id)
}

#[inline]
pub fn get_current_task_info() -> TaskInfo {
    TASK_MANAGER.get_current_task_info()
}

#[inline]
pub fn current_task_mmap(start: usize, len: usize, port: usize) -> Option<()> {
    TASK_MANAGER.mmap(start, len, port)
}

#[inline]
pub fn current_task_munmap(start: usize, len: usize) -> Option<()> {
    TASK_MANAGER.munmap(start, len)
}

#[inline]
pub fn get_current_task_id() -> usize {
    TASK_MANAGER.0.exclusive_access().current_task_id
}
