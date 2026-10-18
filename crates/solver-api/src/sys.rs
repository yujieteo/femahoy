//! Operating system calls for the runner: process group, hard memory limit,
//! memory probe and machine RAM. No crate is used. The calls are declared here.
//!
//! Unix: the child leads a new process group. Linux also gets a hard address
//! space limit (`RLIMIT_AS`). macOS has no limit that the kernel enforces, so
//! only the probe watches the memory there.

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

/// Soft and hard `RLIMIT_AS` for the child. A process without privilege cannot raise
/// its hard limit, so the request stops at the hard limit that the runner inherited.
#[cfg_attr(
    not(all(target_os = "linux", target_pointer_width = "64")),
    allow(dead_code)
)]
fn address_limits(requested: u64, inherited_hard: u64) -> [u64; 2] {
    [requested.min(inherited_hard); 2]
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

/// Resident memory in bytes from the text of `/proc/<pid>/status`. `VmRSS` is in kB for
/// every page size. A process without memory (a kernel thread or a zombie) has no line.
#[cfg(any(target_os = "linux", test))]
fn vm_rss_bytes(status: &str) -> u64 {
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        .map_or(0, |kib| kib.saturating_mul(1024))
}

/// Handle for the process tree of a child.
pub struct Group {
    pid: u32,
}

/// Memory of the process tree in bytes: physical footprint on macOS, resident elsewhere.
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
        fn getrlimit(resource: i32, limit: *mut [u64; 2]) -> i32;
        fn setrlimit(resource: i32, limit: *const [u64; 2]) -> i32;
    }

    pub fn prepare(cmd: &mut Command, hard_limit: Option<u64>) {
        cmd.process_group(0);
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        if let Some(bytes) = hard_limit {
            // RLIMIT_AS is 9 on Linux. Address space is not resident memory:
            // malloc arenas and thread stacks reserve address space per thread.
            // RLIM_INFINITY is all ones, so an unknown limit does not clamp.
            let mut inherited = [u64::MAX; 2];
            // SAFETY: `inherited` is valid for the write of two numbers.
            if unsafe { getrlimit(9, &mut inherited) } != 0 {
                inherited = [u64::MAX; 2];
            }
            let limit = address_limits(address_space_limit(bytes, solver_threads()), inherited[1]);
            // SAFETY: `setrlimit` is async-signal-safe.
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
            // Fields after the name in parentheses: state ppid pgrp ...
            let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
                continue;
            };
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.get(2).and_then(|v| v.parse::<u32>().ok()) == Some(pgid) {
                // Not the rss of `stat`: it counts pages, and the page size is not
                // always 4 KiB (for example 16 KiB or 64 KiB on arm64).
                let status = std::fs::read_to_string(entry.path().join("status"));
                total += vm_rss_bytes(&status.unwrap_or_default());
            }
        }
        Ok(total)
    }

    #[cfg(target_os = "macos")]
    pub fn probe(pgid: u32) -> Result<u64, String> {
        extern "C" {
            fn proc_listpids(
                kind: u32,
                group: u32,
                buffer: *mut std::ffi::c_void,
                size: i32,
            ) -> i32;
            fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut std::ffi::c_void) -> i32;
        }
        // libproc.h: PROC_PGRP_ONLY = 2. Retry when the list fills the buffer.
        let mut pids = vec![0_i32; 32];
        loop {
            let size = i32::try_from(pids.len() * std::mem::size_of::<i32>())
                .map_err(|_| "process group is too large".to_string())?;
            // SAFETY: the buffer holds `size` bytes, with the alignment of pid_t.
            let used = unsafe { proc_listpids(2, pgid, pids.as_mut_ptr().cast(), size) };
            if used <= 0 {
                // libproc returns 0 on error too. The runner checks for child exit.
                return Err(format!(
                    "proc_listpids returned no processes: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if used < size {
                pids.truncate(used as usize / std::mem::size_of::<i32>());
                break;
            }
            pids.resize(pids.len() * 2, 0);
        }
        let mut total = 0_u64;
        for pid in pids.into_iter().filter(|&pid| pid > 0) {
            // sys/resource.h: rusage_info_v2 has a 16-byte UUID and 18 u64 fields.
            // ri_phys_footprint is field 8 after the UUID. It includes compressed pages.
            let mut usage = [0_u64; 20];
            // SAFETY: the buffer matches the size and alignment of rusage_info_v2.
            if unsafe { proc_pid_rusage(pid, 2, usage.as_mut_ptr().cast()) } != 0 {
                let error = std::io::Error::last_os_error();
                // ESRCH: the process ended between the list and the memory query.
                if error.raw_os_error() == Some(3) {
                    continue;
                }
                return Err(format!("proc_pid_rusage({pid}): {error}"));
            }
            total = total.saturating_add(usage[9]);
        }
        Ok(total)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

#[cfg(not(unix))]
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

/// The memory probe of this operating system.
pub fn default_probe() -> Probe {
    os::probe
}

/// Physical memory of the machine in bytes, or `None` if the system does not tell.
pub fn total_memory() -> Option<u64> {
    os::total_memory()
}

#[cfg(test)]
mod probe_tests {
    use super::vm_rss_bytes;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_probe_reads_footprint_instead_of_rss() {
        use std::os::unix::process::CommandExt;
        extern "C" {
            fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut std::ffi::c_void) -> i32;
        }
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        // SDK rusage_info_v2: UUID (16 bytes), then 18 u64 fields.
        let mut usage = [0_u64; 20];
        let rc = unsafe { proc_pid_rusage(child.id() as i32, 2, usage.as_mut_ptr().cast()) };
        let actual = super::os::probe(child.id());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(rc, 0);
        let footprint = usage[9];
        assert!(footprint > 0);
        assert_eq!(actual.unwrap(), footprint);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_probe_counts_other_members_of_the_group() {
        use std::os::unix::process::CommandExt;
        let mut leader = std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let own = super::os::probe(leader.id()).unwrap();
        let mut member = std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(leader.id() as i32)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let both = super::os::probe(leader.id());
        member.kill().unwrap();
        member.wait().unwrap();
        leader.kill().unwrap();
        leader.wait().unwrap();
        assert!(both.unwrap() > own + own / 2);
        assert!(super::os::probe(u32::MAX).is_err());
    }

    #[test]
    fn the_resident_size_comes_from_vm_rss_in_kib() {
        let status =
            "Name:\tfh-solve-ccx\nVmPeak:\t  9999 kB\nVmRSS:\t    2048 kB\nRssAnon:\t1 kB\n";
        assert_eq!(vm_rss_bytes(status), 2048 * 1024);
        assert_eq!(vm_rss_bytes("Name:\tkthreadd\nState:\tS (sleeping)\n"), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_probe_counts_this_process() {
        extern "C" {
            fn getpgrp() -> i32;
        }
        let own = vm_rss_bytes(&std::fs::read_to_string("/proc/self/status").unwrap());
        let group = super::os::probe(unsafe { getpgrp() } as u32).unwrap();
        // The memory can change between the two reads.
        assert!(own > 0 && group >= own / 2, "group {group}, own {own}");
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

    #[test]
    fn the_limit_never_asks_for_more_than_the_inherited_hard_limit() {
        use super::address_limits;
        let gib = 1u64 << 30;
        // A child cannot raise its hard limit, so a request above it must not fail the spawn.
        assert_eq!(address_limits(2 * gib, gib), [gib; 2]);
        assert_eq!(address_limits(gib, 2 * gib), [gib; 2]);
        // RLIM_INFINITY is all ones on Linux.
        assert_eq!(address_limits(gib, u64::MAX), [gib; 2]);
    }

    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    #[test]
    fn a_child_starts_under_a_lower_inherited_hard_limit() {
        use std::os::unix::process::CommandExt;
        use std::process::Command;
        const NAME: &str = "sys::limit_tests::a_child_starts_under_a_lower_inherited_hard_limit";
        const INNER: &str = "FH_TEST_INHERITED_HARD_LIMIT";
        extern "C" {
            fn setrlimit(resource: i32, limit: *const [u64; 2]) -> i32;
        }
        let gib = 1u64 << 30;
        if std::env::var_os(INNER).is_some() {
            // This copy of the test has a hard limit of 4 GiB. The request with its
            // allowance is above it.
            let mut cmd = Command::new("true");
            super::prepare(&mut cmd, Some(4 * gib));
            let status = cmd.status().expect("the child starts");
            assert!(status.success());
            return;
        }
        let limit = [4 * gib; 2];
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", NAME, "--test-threads", "1"])
            .env(INNER, "1");
        // SAFETY: `setrlimit` is async-signal-safe.
        unsafe {
            cmd.pre_exec(move || {
                if setrlimit(9, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        let out = cmd.output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && text.contains("1 passed"), "{text}");
    }
}
