// Lists a target process's handles, naming File objects via a watchdog thread so a handle whose
// name query itself blocks (a synchronous handle with pending I/O) is reported rather than hanging.
use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

type H = *mut c_void;

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

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
    fn NtQueryObject(h: H, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> H;
    fn GetCurrentProcess() -> H;
    fn DuplicateHandle(sp: H, sh: H, tp: H, th: *mut H, access: u32, inherit: i32, opts: u32) -> i32;
    fn CloseHandle(h: H) -> i32;
    fn GetFileType(h: H) -> u32;
}

fn query_name(dup: usize) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        let mut ret = 0u32;
        // ObjectNameInformation = 1
        let st = unsafe { NtQueryObject(dup as H, 1, buf.as_mut_ptr().cast(), buf.len() as u32, &mut ret) };
        if st != 0 {
            let _ = tx.send(format!("<query status {st:#x}>"));
            return;
        }
        // UNICODE_STRING { u16 len; u16 max; u32 pad; *wchar buf }
        let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
        let ptr = usize::from_le_bytes(buf[8..16].try_into().unwrap()) as *const u16;
        let s = unsafe { std::slice::from_raw_parts(ptr, len / 2) };
        let _ = tx.send(String::from_utf16_lossy(s));
    });
    rx.recv_timeout(Duration::from_millis(800)).ok()
}

fn type_name(dup: usize) -> String {
    let mut buf = vec![0u8; 4096];
    let mut ret = 0u32;
    // ObjectTypeInformation = 2
    let st = unsafe { NtQueryObject(dup as H, 2, buf.as_mut_ptr().cast(), buf.len() as u32, &mut ret) };
    if st != 0 {
        return format!("<type status {st:#x}>");
    }
    let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    let ptr = usize::from_le_bytes(buf[8..16].try_into().unwrap()) as *const u16;
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len / 2) })
}

fn main() {
    let pid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    unsafe {
        let mut size = 1 << 20;
        let mut buf: Vec<u8>;
        loop {
            buf = vec![0u8; size];
            let mut ret = 0u32;
            // SystemExtendedHandleInformation = 0x40
            let st = NtQuerySystemInformation(0x40, buf.as_mut_ptr().cast(), size as u32, &mut ret);
            if st == 0 {
                break;
            }
            size *= 2;
            assert!(size < (1 << 30), "handle snapshot too large");
        }
        let count = usize::from_le_bytes(buf[0..8].try_into().unwrap());
        let entries = std::slice::from_raw_parts(buf.as_ptr().add(16) as *const HandleEntry, count);

        let ph = OpenProcess(0x0040 /*DUP_HANDLE*/, 0, pid);
        assert!(!ph.is_null(), "OpenProcess failed");
        let me = GetCurrentProcess();

        for e in entries.iter().filter(|e| e.unique_process_id as u32 == pid) {
            let mut dup: H = std::ptr::null_mut();
            let ok = DuplicateHandle(ph, e.handle_value as H, me, &mut dup, 0, 0, 0x0002 /*SAME_ACCESS*/);
            if ok == 0 {
                println!("handle {:#x}: <dup failed>", e.handle_value);
                continue;
            }
            let t = type_name(dup as usize);
            let ft = GetFileType(dup);
            let name = query_name(dup as usize).unwrap_or_else(|| "<NAME QUERY HUNG>".into());
            if t == "File" || name.contains("HUNG") {
                println!("handle {:#x} type={t} GetFileType={ft} name={name}", e.handle_value);
            }
            CloseHandle(dup);
        }
    }
}
