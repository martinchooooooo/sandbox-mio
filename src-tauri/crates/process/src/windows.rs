//! Win32 handle work behind the process layer: the startup sanitiser that strips console-handle
//! inheritance, and the process-handle duplication that lets a timeout terminate exactly one child.
//! All FFI of the layer lives here.

/// Clears `HANDLE_FLAG_INHERIT` on every inheritable handle this process already owns, so no child
/// can inherit a console handle the launching terminal handed us. Returns how many handles were
/// cleared.
///
/// Load-bearing and ordering-sensitive: creation flags and fully explicit stdio do **not** prevent
/// console-handle inheritance — not even `DETACHED_PROCESS`, the strictest of them
/// (`inheritance-ab-test.rs` in
/// `docs/features/podman-process-isolation/spike/`), and a child spawned before this runs still
/// inherits the poisoned handles — so this must be the first statement of `run()`
/// (`src-tauri/src/lib.rs`).
///
/// Governs only handles present at call time; one created later by a dependency is out of reach
/// (`docs/features/podman-process-isolation/04_improvements.md`).
#[cfg(windows)]
pub fn clear_inheritable_handles() -> usize {
    let pid = unsafe { GetCurrentProcessId() };
    let Some(buf) = system_handle_snapshot() else {
        return 0;
    };
    // SYSTEM_HANDLE_INFORMATION_EX: a `ULONG_PTR` count, a `ULONG_PTR` reserved, then the entries.
    let count = buf[0] as usize;
    let entries =
        unsafe { std::slice::from_raw_parts(buf.as_ptr().add(2) as *const HandleEntry, count) };
    let mut cleared = 0;
    for entry in entries.iter().filter(|e| e.unique_process_id as u32 == pid) {
        let handle = entry.handle_value as *mut std::ffi::c_void;
        let mut flags = 0u32;
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            continue;
        }
        if flags & HANDLE_FLAG_INHERIT != 0
            && unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } != 0
        {
            cleared += 1;
        }
    }
    cleared
}

#[cfg(not(windows))]
pub fn clear_inheritable_handles() -> usize {
    0
}

#[cfg(windows)]
const HANDLE_FLAG_INHERIT: u32 = 0x1;

/// `SystemExtendedHandleInformation`.
#[cfg(windows)]
const SYSTEM_EXTENDED_HANDLE_INFORMATION: u32 = 0x40;

#[cfg(windows)]
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

/// `SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX`.
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    object: usize,
    unique_process_id: usize,
    handle_value: usize,
    granted_access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    handle_attributes: u32,
    reserved: u32,
}

#[cfg(windows)]
#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        info: *mut std::ffi::c_void,
        len: u32,
        ret: *mut u32,
    ) -> i32;
}

#[cfg(windows)]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GetHandleInformation(handle: *mut std::ffi::c_void, flags: *mut u32) -> i32;
    fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
}

/// The system-wide handle table, or `None` if the query never succeeded. `u64`-backed so the entry
/// slice is pointer-aligned, which a `Vec<u8>` does not guarantee. The table grows between the
/// size query and the read, so the doubling retry is the documented way to call this — bounded
/// here rather than open-ended, since failing to sanitise is survivable but hanging startup is not.
#[cfg(windows)]
fn system_handle_snapshot() -> Option<Vec<u64>> {
    let mut words = 1 << 17;
    for _ in 0..8 {
        let mut buf = vec![0u64; words];
        let mut needed = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buf.as_mut_ptr().cast(),
                (words * 8) as u32,
                &mut needed,
            )
        };
        if status == 0 {
            return Some(buf);
        }
        if status != STATUS_INFO_LENGTH_MISMATCH {
            return None;
        }
        words *= 2;
    }
    None
}

/// An independently owned copy of `child`'s process handle, so the timeout branch can terminate the
/// child without racing the blocking thread's ownership of the [`std::process::Child`] — closing
/// that one out from under the wait would risk terminating an unrelated, recycled handle.
/// Non-inheritable, like every other handle this process holds after [`clear_inheritable_handles`].
#[cfg(windows)]
pub(super) fn duplicate_process_handle(
    child: &std::process::Child,
) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};

    const DUPLICATE_SAME_ACCESS: u32 = 0x2;
    let mut duplicate = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            child.as_raw_handle().cast(),
            GetCurrentProcess(),
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    (ok != 0).then(|| unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(duplicate.cast()) })
}

#[cfg(not(windows))]
pub(super) fn duplicate_process_handle(_child: &std::process::Child) -> Option<std::convert::Infallible> {
    None
}

/// Kills exactly the child this app spawned, addressed by its own handle — nothing is matched by
/// name, so no other process (another worktree's, say) is reachable from here.
#[cfg(windows)]
pub(super) fn terminate_process(handle: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    unsafe { TerminateProcess(handle.as_raw_handle().cast(), 1) };
}

#[cfg(not(windows))]
pub(super) fn terminate_process(_handle: &std::convert::Infallible) {}

#[cfg(windows)]
extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn DuplicateHandle(
        source_process: *mut std::ffi::c_void,
        source: *mut std::ffi::c_void,
        target_process: *mut std::ffi::c_void,
        target: *mut *mut std::ffi::c_void,
        desired_access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn TerminateProcess(process: *mut std::ffi::c_void, exit_code: u32) -> i32;
}

/// Ties `child` to this app's lifetime: the OS kills it when the app process ends, however it ends.
///
/// For children the app stops explicitly on a graceful exit (`on_run_event` in
/// `src-tauri/src/bootstrap.rs`) but that must not outlive a crash, a `Ctrl+C` in the launching
/// terminal or a Task Manager kill — none of which run the exit handler, and `kill_on_drop` does not
/// help there either, since no destructor runs. Backed by one process-wide job object with
/// kill-on-close semantics, created on first use and never closed by this app: the job handle dies
/// with the process, which is exactly the trigger. Jobs nest on Windows 8+, so this still works when
/// the app was itself launched inside a job (`tauri dev`, a CI runner).
///
/// Not for children that are meant to outlive the app (the Mutagen daemon, the Podman machine).
/// Children of `child` join the job too, so a binary that spawns a long-lived helper does not fit.
/// Best effort: a failed job call leaves the child unbound, i.e. the previous behaviour, and returns
/// `false`.
#[cfg(windows)]
pub fn bind_to_app_lifetime(child: &tokio::process::Child) -> bool {
    use std::sync::OnceLock;

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

    static JOB: OnceLock<usize> = OnceLock::new();

    let job = *JOB.get_or_init(|| unsafe {
        let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if handle.is_null() {
            return 0;
        }
        let mut info: JobObjectExtendedLimitInformation = std::mem::zeroed();
        info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            handle,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            (&mut info as *mut JobObjectExtendedLimitInformation).cast(),
            std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        );
        if ok == 0 {
            CloseHandle(handle);
            return 0;
        }
        handle as usize
    });
    let bound = match child.raw_handle() {
        Some(process) if job != 0 => unsafe {
            AssignProcessToJobObject(job as *mut std::ffi::c_void, process.cast()) != 0
        },
        _ => false,
    };
    if !bound {
        tracing::warn!(target: "process", "could not bind a child process to the app's lifetime job");
    }
    bound
}

#[cfg(not(windows))]
pub fn bind_to_app_lifetime(_child: &tokio::process::Child) -> bool {
    false
}

/// `JOBOBJECT_BASIC_LIMIT_INFORMATION`.
#[cfg(windows)]
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

/// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`, with its `IO_COUNTERS` member spelled out as six `u64`.
#[cfg(windows)]
#[repr(C)]
struct JobObjectExtendedLimitInformation {
    basic: JobObjectBasicLimitInformation,
    io_counters: [u64; 6],
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
extern "system" {
    fn CreateJobObjectW(attributes: *mut std::ffi::c_void, name: *const u16) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: u32,
        info: *mut std::ffi::c_void,
        len: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::io::AsRawHandle;

    use super::super::SPAWN_GATE;
    use super::*;

    /// The sanitiser's contract in one round trip: a handle deliberately marked inheritable is not
    /// inheritable afterwards. Uses a real file handle rather than a synthetic one, because the
    /// snapshot the sanitiser walks only reports handles the kernel actually holds for us.
    #[test]
    fn clears_the_inherit_flag_on_an_inheritable_handle() {
        let _gate = SPAWN_GATE.blocking_write();
        let path = std::env::temp_dir().join(format!("mio-sanitise-{}.tmp", std::process::id()));
        let file = std::fs::File::create(&path).expect("temp file");
        let handle = file.as_raw_handle().cast();

        assert_ne!(
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
            0,
            "could not mark the handle inheritable"
        );
        let mut flags = 0u32;
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);

        assert!(clear_inheritable_handles() > 0, "sanitiser cleared nothing");

        let mut flags = 0u32;
        assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0, "inherit flag survived the sanitiser");

        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    /// `cargo test` runs inside cargo's own job, so this covers the nested case `tauri dev` and CI
    /// hit, with a second child joining an already non-empty lifetime job.
    #[tokio::test]
    async fn binds_concurrent_children_from_inside_a_job() {
        let _gate = SPAWN_GATE.read().await;
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let spawn = || {
            super::super::detached_command(format!(r"{root}\System32\ping.exe"))
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .expect("spawn ping")
        };
        let (first, second) = (spawn(), spawn());
        assert!(bind_to_app_lifetime(&first), "first child not bound");
        assert!(bind_to_app_lifetime(&second), "second child not bound to the non-empty job");
    }
}

#[cfg(all(windows, target_pointer_width = "64"))]
const _: () = assert!(std::mem::size_of::<JobObjectExtendedLimitInformation>() == 144);
