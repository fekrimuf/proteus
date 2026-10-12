//! Keep heavy child programs (Blender, Godot, the glTF validator, the judge server) from taking over the owner's PC.
//!
//! On Windows every child starts at below-normal priority and is put in a job object that (1) kills it when `pt` exits or is
//! killed, so nothing is left running at wrap-up, and (2) for Blender, Godot and the validator caps the memory of the job, so a
//! runaway import fails by itself instead of swapping the whole machine. Limits: `PT_HEAVY_MEM_GB` (default 8, 0 = no cap) and
//! `PT_HEAVY_THREADS` (default: all cores but two). Other systems just run the command.

use std::io;
use std::process::{Child, Command, Output, Stdio};

/// Threads a heavy child may use: all cores but two (at least one), or `PT_HEAVY_THREADS`.
pub fn threads() -> usize {
    std::env::var("PT_HEAVY_THREADS").ok().and_then(|s| s.parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(2).max(1))
}

/// Memory cap in bytes for the capped job; 0 means none.
fn mem_cap_bytes() -> usize {
    let gb = std::env::var("PT_HEAVY_MEM_GB").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(8.0);
    (gb.max(0.0) * 1_073_741_824.0) as usize
}

/// Start `cmd` at low priority inside the memory-capped job.
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    start(cmd, true)
}

/// Start `cmd` at low priority and kill it when `pt` exits, without a memory cap (the judge server maps a large model).
pub fn spawn_uncapped(cmd: &mut Command) -> io::Result<Child> {
    start(cmd, false)
}

/// Like `Command::output`, through [`spawn`].
pub fn output(cmd: &mut Command) -> io::Result<Output> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    spawn(cmd)?.wait_with_output()
}

fn start(cmd: &mut Command, capped: bool) -> io::Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x4000;
        cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
    }
    let child = cmd.spawn()?;
    #[cfg(windows)]
    win::confine(&child, if capped { mem_cap_bytes() } else { 0 });
    #[cfg(not(windows))]
    let _ = capped;
    Ok(child)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::OnceLock;

    #[repr(C)]
    struct BasicLimits {
        per_process_user_time: i64,
        per_job_user_time: i64,
        flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }
    #[repr(C)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }
    extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    }
    const EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const LIMIT_JOB_MEMORY: u32 = 0x200;
    const LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;

    /// A job handle, kept open for the life of `pt` (closing it, which Windows does at exit, kills what is inside).
    struct Job(usize);

    fn make(cap: usize) -> Job {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return Job(0);
            }
            let mut l: ExtendedLimits = std::mem::zeroed();
            l.basic.flags = LIMIT_KILL_ON_JOB_CLOSE | if cap > 0 { LIMIT_JOB_MEMORY } else { 0 };
            l.job_memory = cap;
            SetInformationJobObject(job, EXTENDED_LIMIT_INFORMATION, &mut l as *mut _ as *mut c_void, std::mem::size_of::<ExtendedLimits>() as u32);
            Job(job as usize)
        }
    }

    pub fn confine(child: &Child, cap: usize) {
        static CAPPED: OnceLock<(usize, Job)> = OnceLock::new();
        static FREE: OnceLock<Job> = OnceLock::new();
        let job = if cap > 0 { &CAPPED.get_or_init(|| (cap, make(cap))).1 } else { FREE.get_or_init(|| make(0)) };
        if job.0 != 0 {
            unsafe {
                AssignProcessToJobObject(job.0 as *mut c_void, child.as_raw_handle() as *mut c_void);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_leave_room_for_the_owner() {
        let n = threads();
        assert!(n >= 1);
        assert!(n <= std::thread::available_parallelism().map_or(2, |n| n.get()));
    }

    #[cfg(windows)]
    #[test]
    fn a_confined_child_runs_and_reports() {
        let out = output(Command::new("cmd").args(["/C", "echo", "hello"])).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }
}
