// Samples a target thread's RIP and stack, naming addresses by nearest export in the system DLLs
// (which load at the same base in every process, so our own copies' export tables resolve them).
use std::convert::TryInto;
use std::ffi::c_void;

type H = *mut c_void;

extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> H;
    fn OpenThread(access: u32, inherit: i32, tid: u32) -> H;
    fn SuspendThread(h: H) -> u32;
    fn ResumeThread(h: H) -> u32;
    fn GetThreadContext(h: H, ctx: *mut c_void) -> i32;
    fn ReadProcessMemory(h: H, addr: *const c_void, buf: *mut c_void, size: usize, read: *mut usize) -> i32;
    fn GetModuleHandleA(name: *const u8) -> H;
    fn GetLastError() -> u32;
}

const CONTEXT_AMD64: u32 = 0x0010_0000;
const CONTEXT_CONTROL: u32 = CONTEXT_AMD64 | 0x1;
const CONTEXT_INTEGER: u32 = CONTEXT_AMD64 | 0x2;

#[repr(C, align(16))]
struct Ctx([u8; 1232]);

fn exports(module: &str) -> Vec<(usize, String)> {
    let mut name = module.as_bytes().to_vec();
    name.push(0);
    let base = unsafe { GetModuleHandleA(name.as_ptr()) } as usize;
    if base == 0 {
        return vec![];
    }
    unsafe {
        let pe = base + *((base + 0x3c) as *const u32) as usize;
        // Optional header magic at pe+0x18; export dir RVA at pe+0x88 for PE32+.
        let export_rva = *((pe + 0x88) as *const u32) as usize;
        if export_rva == 0 {
            return vec![];
        }
        let dir = base + export_rva;
        let count = *((dir + 0x18) as *const u32) as usize;
        let names_rva = *((dir + 0x20) as *const u32) as usize;
        let funcs_rva = *((dir + 0x1c) as *const u32) as usize;
        let ords_rva = *((dir + 0x24) as *const u32) as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let name_rva = *((base + names_rva + i * 4) as *const u32) as usize;
            let ord = *((base + ords_rva + i * 2) as *const u16) as usize;
            let func_rva = *((base + funcs_rva + ord * 4) as *const u32) as usize;
            let cstr = (base + name_rva) as *const u8;
            let mut len = 0;
            while *cstr.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf8_lossy(std::slice::from_raw_parts(cstr, len)).into_owned();
            out.push((base + func_rva, format!("{module}!{s}")));
        }
        out.sort_by_key(|(a, _)| *a);
        out
    }
}

fn name_of(table: &[(usize, String)], addr: usize) -> Option<String> {
    let i = table.partition_point(|(a, _)| *a <= addr);
    if i == 0 {
        return None;
    }
    let (a, n) = &table[i - 1];
    if addr - a > 0x8000 {
        return None;
    }
    Some(format!("{n}+0x{:x}", addr - a))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let pid: u32 = args.next().unwrap().parse().unwrap();
    let tid: u32 = args.next().unwrap().parse().unwrap();

    let mut table = Vec::new();
    for m in ["ntdll.dll", "KERNELBASE.dll", "kernel32.dll", "RPCRT4.dll", "ucrtbase.dll", "powrprof.dll", "UMPDC.dll"] {
        table.extend(exports(m));
    }
    table.sort_by_key(|(a, _)| *a);

    unsafe {
        let ph = OpenProcess(0x0400 | 0x0010, 0, pid);
        assert!(!ph.is_null(), "OpenProcess {}", GetLastError());
        // THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION
        let th = OpenThread(0x0002 | 0x0008 | 0x0040, 0, tid);
        assert!(!th.is_null(), "OpenThread {}", GetLastError());

        SuspendThread(th);
        let mut ctx = Ctx([0u8; 1232]);
        // CONTEXT.ContextFlags lives at offset 0x30 on x64.
        *(ctx.0.as_mut_ptr().add(0x30) as *mut u32) = CONTEXT_CONTROL | CONTEXT_INTEGER;
        let ok = GetThreadContext(th, ctx.0.as_mut_ptr().cast());
        let rip = *(ctx.0.as_ptr().add(0xf8) as *const usize);
        let rsp = *(ctx.0.as_ptr().add(0x98) as *const usize);
        ResumeThread(th);
        assert!(ok != 0, "GetThreadContext {}", GetLastError());

        println!("thread {tid}");
        println!("  RIP {rip:#x}  {}", name_of(&table, rip).unwrap_or_else(|| "<unknown>".into()));
        println!("  RSP {rsp:#x}");

        let mut buf = vec![0u8; 16 * 1024];
        let mut n = 0usize;
        ReadProcessMemory(ph, rsp as *const c_void, buf.as_mut_ptr().cast(), buf.len(), &mut n);
        println!("  stack scan ({n} bytes):");
        let mut seen = std::collections::HashSet::new();
        for chunk in buf[..n].chunks_exact(8) {
            let v = usize::from_le_bytes(chunk.try_into().unwrap());
            if let Some(name) = name_of(&table, v) {
                if seen.insert(name.clone()) {
                    println!("    {v:#x}  {name}");
                }
            }
        }
    }
}
