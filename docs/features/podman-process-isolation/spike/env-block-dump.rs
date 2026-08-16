// Dumps another process's environment block by walking PEB -> RTL_USER_PROCESS_PARAMETERS.
use std::ffi::c_void;

#[repr(C)]
struct ProcessBasicInformation {
    exit_status: i32,
    _pad: i32,
    peb_base_address: *mut c_void,
    affinity_mask: usize,
    base_priority: i32,
    _pad2: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        h: *mut c_void,
        class: u32,
        info: *mut c_void,
        len: u32,
        ret: *mut u32,
    ) -> i32;
}

extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn ReadProcessMemory(
        h: *mut c_void,
        addr: *const c_void,
        buf: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
}

unsafe fn read<T>(h: *mut c_void, addr: usize) -> T {
    let mut v: T = std::mem::zeroed();
    let mut n = 0usize;
    let ok = ReadProcessMemory(h, addr as *const c_void, &mut v as *mut T as *mut c_void, std::mem::size_of::<T>(), &mut n);
    assert!(ok != 0, "ReadProcessMemory failed at {addr:#x}");
    v
}

fn main() {
    let pid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    unsafe {
        // PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
        let h = OpenProcess(0x0400 | 0x0010, 0, pid);
        assert!(!h.is_null(), "OpenProcess failed");
        let mut pbi: ProcessBasicInformation = std::mem::zeroed();
        let mut ret = 0u32;
        let st = NtQueryInformationProcess(
            h,
            0,
            &mut pbi as *mut _ as *mut c_void,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            &mut ret,
        );
        assert_eq!(st, 0, "NtQueryInformationProcess status {st:#x}");
        let peb = pbi.peb_base_address as usize;
        // PEB.ProcessParameters is at offset 0x20 on x64.
        let params: usize = read(h, peb + 0x20);
        // RTL_USER_PROCESS_PARAMETERS: CurrentDirectory at 0x38 (UNICODE_STRING),
        // Environment pointer at 0x80, EnvironmentSize at 0x3F0.
        let env_ptr: usize = read(h, params + 0x80);
        let env_size: usize = read(h, params + 0x3F0);
        println!("pid {pid} env block {env_ptr:#x} size {env_size}");
        let mut buf = vec![0u8; env_size];
        let mut n = 0usize;
        let ok = ReadProcessMemory(h, env_ptr as *const c_void, buf.as_mut_ptr().cast(), env_size, &mut n);
        assert!(ok != 0, "env read failed");
        let wide: Vec<u16> = buf[..n].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        for entry in wide.split(|&c| c == 0) {
            if entry.is_empty() {
                continue;
            }
            println!("{}", String::from_utf16_lossy(entry));
        }
    }
}
