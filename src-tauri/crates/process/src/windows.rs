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
}
