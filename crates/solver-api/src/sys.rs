//! Operating system calls for the runner: process group, hard memory limit,
//! memory probe and machine RAM. No crate is used. The calls are declared here.
//!
//! Unix: the child leads a new process group. Linux also gets a hard address
//! space limit (`RLIMIT_AS`). macOS has no limit that the kernel enforces, so
//! only the probe watches the memory there.
//! Windows: the child runs in a job object with a process memory limit.

use std::process::{Child, Command};

/// Address space limit for a solver with `threads` threads: the resident limit
/// plus 256 MiB and 64 MiB for each thread.
#[cfg_attr(
    not(all(target_os = "linux", target_pointer_width = "64")),
    allow(dead_code)
)]
pub fn address_space_limit(resident_limit: u64, threads: usize) -> u64 {
    resident_limit
        .saturating_add(256 << 20)
        .saturating_add((threads as u64).saturating_mul(64 << 20))
}

/// Thread count of the solver: `FH_THREADS` when set and above 0, else the CPU count.
#[cfg_attr(
    not(all(target_os = "linux", target_pointer_width = "64")),
    allow(dead_code)
)]
fn solver_threads() -> usize {
    std::env::var("FH_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .or_else(|| std::thread::available_parallelism().ok().map(|n| n.get()))
        .unwrap_or(1)
}

/// Handle for the process tree of a child.
pub struct Group {
    pid: u32,
    #[cfg(windows)]
    job: isize,
}

/// Memory of the process tree of `pid`, in bytes of resident memory.
pub type Probe = fn(u32) -> Result<u64, String>;

#[cfg(unix)]
mod os {
    use super::*;
    use std::os::unix::process::CommandExt;

    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    extern "C" {
        fn setrlimit(resource: i32, limit: *const [u64; 2]) -> i32;
    }

    pub fn prepare(cmd: &mut Command, hard_limit: Option<u64>) {
        cmd.process_group(0);
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        if let Some(bytes) = hard_limit {
            // RLIMIT_AS is 9 on Linux. Address space is not resident memory:
            // malloc arenas and thread stacks reserve address space per thread.
            // SAFETY: `setrlimit` is async-signal-safe.
            let limit = [address_space_limit(bytes, solver_threads()); 2];
            unsafe {
                cmd.pre_exec(move || {
                    if setrlimit(9, &limit) == 0 {
                        Ok(())
                    } else {
                        Err(std::io::Error::last_os_error())
                    }
                });
            }
        }
        #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
        let _ = hard_limit;
    }

    pub fn attach(child: &Child, _hard_limit: Option<u64>) -> Group {
        Group { pid: child.id() }
    }

    pub fn kill_group(g: &Group) {
        // A negative pid names the process group.
        unsafe {
            kill(-(g.pid as i32), 9);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn probe(pgid: u32) -> Result<u64, String> {
        let page = 4096;
        let mut total = 0_u64;
        let dir = std::fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?;
        for entry in dir.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            // The process can end while we read. Skip it then.
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            // Fields after the name in parentheses: state ppid pgrp ... rss at 21.
            let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
                continue;
            };
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.get(2).and_then(|v| v.parse::<u32>().ok()) == Some(pgid) {
                total += f.get(21).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * page;
            }
        }
        Ok(total)
    }

    #[cfg(not(target_os = "linux"))]
    pub fn probe(pgid: u32) -> Result<u64, String> {
        let out = Command::new("ps")
            .args(["-A", "-o", "pgid=", "-o", "rss="])
            .output()
            .map_err(|e| format!("cannot run ps: {e}"))?;
        if !out.status.success() {
            return Err("ps failed".into());
        }
        let mut kib = 0_u64;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let mut it = line.split_whitespace();
            if let (Some(g), Some(r)) = (it.next(), it.next()) {
                if g.parse::<u32>().ok() == Some(pgid) {
                    kib += r.parse::<u64>().unwrap_or(0);
                }
            }
        }
        Ok(kib * 1024)
    }

    #[cfg(target_os = "linux")]
    pub fn total_memory() -> Option<u64> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib * 1024)
    }

    #[cfg(target_os = "macos")]
    pub fn total_memory() -> Option<u64> {
        extern "C" {
            fn sysctlbyname(
                name: *const std::ffi::c_char,
                old: *mut std::ffi::c_void,
                oldlen: *mut usize,
                new: *mut std::ffi::c_void,
                newlen: usize,
            ) -> i32;
        }
        let mut value = 0_u64;
        let mut len = std::mem::size_of::<u64>();
        let rc = unsafe {
            sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut value as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0 && value > 0).then_some(value)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn total_memory() -> Option<u64> {
        None
    }
}

/// The Windows structures of the calls. Tests compile them on every system, so that a
/// test can check their sizes.
#[cfg(any(windows, test))]
#[allow(dead_code)]
mod win {
    /// `JOBOBJECT_BASIC_LIMIT_INFORMATION`.
    #[repr(C)]
    pub struct BasicLimit {
        pub per_process_time: i64,
        pub per_job_time: i64,
        pub flags: u32,
        pub min_ws: usize,
        pub max_ws: usize,
        pub active_limit: u32,
        pub affinity: usize,
        pub priority: u32,
        pub scheduling: u32,
    }

    /// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`.
    #[repr(C)]
    pub struct ExtendedLimit {
        pub basic: BasicLimit,
        pub io: [u64; 6],
        pub process_memory: usize,
        pub job_memory: usize,
        pub peak_process: usize,
        pub peak_job: usize,
    }

    /// `PROCESS_MEMORY_COUNTERS`.
    #[repr(C)]
    pub struct MemoryCounters {
        pub cb: u32,
        pub page_faults: u32,
        pub peak_working_set: usize,
        pub working_set: usize,
        pub rest: [usize; 6],
    }

    /// `MEMORYSTATUSEX`: 2 DWORD and 7 DWORDLONG fields.
    #[repr(C)]
    pub struct MemoryStatus {
        pub length: u32,
        pub load: u32,
        pub total_phys: u64,
        pub rest: [u64; 6],
    }
}

#[cfg(windows)]
mod os {
    use super::win::*;
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> isize;
        fn SetInformationJobObject(job: isize, class: i32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
        fn TerminateJobObject(job: isize, code: u32) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn K32GetProcessMemoryInfo(process: isize, c: *mut MemoryCounters, cb: u32) -> i32;
        fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
    }

    pub fn prepare(_cmd: &mut Command, _hard_limit: Option<u64>) {}

    /// Put the child in a job object. The job has the memory limit, and it kills
    /// every process in it when the runner closes the handle. A process that the
    /// child starts before this call is not in the job.
    pub fn attach(child: &Child, hard_limit: Option<u64>) -> Group {
        let mut group = Group {
            pid: child.id(),
            job: 0,
        };
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job == 0 {
                return group;
            }
            let mut info: ExtendedLimit = std::mem::zeroed();
            // KILL_ON_JOB_CLOSE, and PROCESS_MEMORY when there is a limit.
            info.basic.flags = 0x2000;
            if let Some(bytes) = hard_limit {
                info.basic.flags |= 0x100;
                info.process_memory = usize::try_from(bytes).unwrap_or(usize::MAX);
            }
            let size = std::mem::size_of::<ExtendedLimit>() as u32;
            SetInformationJobObject(job, 9, (&info as *const ExtendedLimit).cast(), size);
            AssignProcessToJobObject(job, child.as_raw_handle() as isize);
            group.job = job;
        }
        group
    }

    pub fn kill_group(g: &Group) {
        unsafe {
            if g.job != 0 {
                TerminateJobObject(g.job, 1);
            }
        }
    }

    pub fn close(g: &Group) {
        unsafe {
            if g.job != 0 {
                CloseHandle(g.job);
            }
        }
    }

    pub fn probe(pid: u32) -> Result<u64, String> {
        unsafe {
            // PROCESS_QUERY_LIMITED_INFORMATION (0x1000) | PROCESS_VM_READ (0x10).
            let h = OpenProcess(0x1010, 0, pid);
            if h == 0 {
                return Err("OpenProcess failed".into());
            }
            let mut c: MemoryCounters = std::mem::zeroed();
            c.cb = std::mem::size_of::<MemoryCounters>() as u32;
            let ok = K32GetProcessMemoryInfo(h, &mut c, c.cb);
            CloseHandle(h);
            if ok == 0 {
                return Err("GetProcessMemoryInfo failed".into());
            }
            Ok(c.working_set as u64)
        }
    }

    pub fn total_memory() -> Option<u64> {
        unsafe {
            let mut s: MemoryStatus = std::mem::zeroed();
            s.length = std::mem::size_of::<MemoryStatus>() as u32;
            (GlobalMemoryStatusEx(&mut s) != 0).then_some(s.total_phys)
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod os {
    use super::*;
    pub fn prepare(_: &mut Command, _: Option<u64>) {}
    pub fn attach(child: &Child, _: Option<u64>) -> Group {
        Group { pid: child.id() }
    }
    pub fn kill_group(_: &Group) {}
    pub fn probe(_: u32) -> Result<u64, String> {
        Err("no memory probe for this operating system".into())
    }
    pub fn total_memory() -> Option<u64> {
        None
    }
}

/// Set up `cmd` so that the child leads its own process tree.
pub fn prepare(cmd: &mut Command, hard_limit: Option<u64>) {
    os::prepare(cmd, hard_limit);
}

/// Take hold of the process tree after the spawn.
pub fn attach(child: &Child, hard_limit: Option<u64>) -> Group {
    os::attach(child, hard_limit)
}

impl Group {
    /// Kill every process of the tree.
    pub fn kill(&self) {
        os::kill_group(self);
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }
}

#[cfg(windows)]
impl Drop for Group {
    fn drop(&mut self) {
        os::close(self);
    }
}

/// The memory probe of this operating system.
pub fn default_probe() -> Probe {
    os::probe
}

/// Physical memory of the machine in bytes, or `None` if the system does not tell.
pub fn total_memory() -> Option<u64> {
    os::total_memory()
}

#[cfg(all(test, target_pointer_width = "64"))]
mod win_tests {
    use super::win::*;
    use std::mem::size_of;

    #[test]
    fn the_windows_structures_have_the_sizes_of_the_api() {
        assert_eq!(size_of::<BasicLimit>(), 64);
        assert_eq!(size_of::<ExtendedLimit>(), 144);
        assert_eq!(size_of::<MemoryCounters>(), 72);
        assert_eq!(size_of::<MemoryStatus>(), 64);
    }
}

#[cfg(test)]
mod limit_tests {
    use super::address_space_limit;

    #[test]
    fn allowance_scales_with_threads() {
        assert_eq!(address_space_limit(1 << 30, 1), (1 << 30) + (320 << 20));
        assert_eq!(
            address_space_limit(1 << 30, 64),
            (1 << 30) + (256 << 20) + (64 * (64 << 20))
        );
        assert_eq!(address_space_limit(u64::MAX, 8), u64::MAX);
    }
}
