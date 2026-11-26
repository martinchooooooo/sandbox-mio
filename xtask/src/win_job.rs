//! A Windows job object that kills every process still assigned to it the instant this binary's
//! last handle to the job closes — which the OS does automatically on any exit path (normal
//! return, panic, Ctrl+C, or a hard crash of the tool itself). Every process `run`/`try_run`
//! spawns joins the job (child processes inherit job membership from their parent, transitively),
//! so a crash deep in a `pnpm` -> `node` -> `wdio` -> `vite`/`tauri build` chain — observed as
//! `pnpm` exiting with `0xc0000005` mid-`e2e` run — no longer orphans the still-running
//! grandchildren to keep building/testing in the background after the tool has already panicked.
//!
//! Under `cargo xtask` the tool is cargo's child, so `cargo.exe` stays outside the job: it is the
//! tool's own process and its children that the job covers.

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::sync::atomic::{AtomicIsize, Ordering};

const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

#[repr(C)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[repr(C)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

extern "system" {
    fn CreateJobObjectW(lp_job_attributes: *mut c_void, lp_name: *const u16) -> *mut c_void;
    fn SetInformationJobObject(
        h_job: *mut c_void,
        job_object_information_class: u32,
        lp_job_object_information: *mut c_void,
        cb_job_object_information_length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(h_job: *mut c_void, h_process: *mut c_void) -> i32;
}

static JOB: AtomicIsize = AtomicIsize::new(0);

/// Creates the job with kill-on-close. A no-op (leaving `JOB` at 0, so
/// `assign` harmlessly skips) if either Windows API call fails — cleanup then falls back to
/// whatever each tool's own teardown does, same as before this existed.
pub fn setup() {
    let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
    if handle.is_null() {
        return;
    }
    let mut info: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
    info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            handle,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        )
    };
    if ok != 0 {
        JOB.store(handle as isize, Ordering::SeqCst);
    }
}

/// Assigns `child` to the job from `setup()`; a no-op if `setup()` was never called or failed.
pub fn assign(child: &Child) {
    let job = JOB.load(Ordering::SeqCst);
    if job == 0 {
        return;
    }
    unsafe {
        AssignProcessToJobObject(job as *mut c_void, child.as_raw_handle() as *mut c_void);
    }
}
