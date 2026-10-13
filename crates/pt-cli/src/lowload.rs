//! Keep heavy child programs (Blender, Godot, the glTF validator, the judge server) from taking over the owner's PC.
//!
//! On Windows every child starts at below-normal priority and is put in a job object that (1) kills it when `pt` exits or is
//! killed, so nothing is left running at wrap-up, and (2) for Blender, Godot and the validator caps the memory of the job, so a
//! runaway import fails by itself instead of swapping the whole machine. Limits: `PT_HEAVY_MEM_GB` (default 8, 0 = no cap) and
//! `PT_HEAVY_THREADS` (default: all cores but two). Other systems just run the command.
//!
//! A child can also be given a time limit ([`output_within`]): one that has not finished is stopped together with everything it
//! started ([`kill_tree`]), so a stalled import ends by itself. The game engine always runs this way (`crate::engine`).

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

/// What a run under a time limit gave back.
pub struct Timed {
    /// `None` when the run was stopped at the limit.
    pub status: Option<std::process::ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub seconds: f64,
}

/// A file a child writes its output to. A pipe would have to be drained while waiting; a file lets the wait poll the clock.
pub fn sink_file(tag: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    std::env::temp_dir().join(format!("pt_child_{}_{}_{tag}.txt", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

/// Like [`output`], with a time limit: a child that has not finished after `limit` is stopped with everything it started, and the
/// result says so. A stalled import then ends by itself instead of holding the command (and the machine) for as long as it likes.
pub fn output_within(cmd: &mut Command, limit: std::time::Duration) -> io::Result<Timed> {
    let (out, err) = (sink_file("out"), sink_file("err"));
    cmd.stdin(Stdio::null()).stdout(std::fs::File::create(&out)?).stderr(std::fs::File::create(&err)?);
    let started = std::time::Instant::now();
    let mut child = spawn(cmd)?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => {
                kill_tree(&mut child);
                break None;
            }
        }
    };
    let read = |p: &std::path::Path| {
        let text = std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let _ = std::fs::remove_file(p);
        text
    };
    Ok(Timed { status, stdout: read(&out), stderr: read(&err), timed_out: status.is_none(), seconds: started.elapsed().as_secs_f64() })
}

/// Stop a child and every program it started (Godot's console program is a launcher: stopping it alone leaves the engine running).
pub fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The most memory the capped children of this `pt` held together so far, in megabytes (0 where it is not measured).
pub fn peak_mem_mb() -> f64 {
    #[cfg(windows)]
    {
        win::peak_job_bytes() as f64 / 1_048_576.0
    }
    #[cfg(not(windows))]
    {
        0.0
    }
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
        fn QueryInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
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

    static CAPPED: OnceLock<(usize, Job)> = OnceLock::new();
    static FREE: OnceLock<Job> = OnceLock::new();

    /// The peak memory of the capped job (`PeakJobMemoryUsed`): every capped child of this `pt` together.
    pub fn peak_job_bytes() -> usize {
        let Some((_, job)) = CAPPED.get() else { return 0 };
        if job.0 == 0 {
            return 0;
        }
        unsafe {
            let mut l: ExtendedLimits = std::mem::zeroed();
            if QueryInformationJobObject(job.0 as *mut c_void, EXTENDED_LIMIT_INFORMATION, &mut l as *mut _ as *mut c_void, std::mem::size_of::<ExtendedLimits>() as u32, std::ptr::null_mut()) != 0 {
                l.peak_job_memory
            } else {
                0
            }
        }
    }

    pub fn confine(child: &Child, cap: usize) {
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

    #[cfg(windows)]
    #[test]
    fn a_child_that_finishes_in_time_reports_what_it_printed() {
        let r = output_within(Command::new("cmd").args(["/C", "echo", "done"]), std::time::Duration::from_secs(30)).unwrap();
        assert!(!r.timed_out && r.status.is_some_and(|s| s.success()));
        assert!(r.stdout.contains("done"), "{}", r.stdout);
    }

    /// The class: a child program that never finishes (a stalled import). Without the limit this test would wait for the full
    /// minute the child sleeps; with it the child, and the program the child started, are gone after the limit.
    #[cfg(windows)]
    #[test]
    fn a_stalled_child_is_stopped_at_the_time_limit_with_what_it_started() {
        let t = std::time::Instant::now();
        // `cmd` starts `ping`, which runs for a minute: the launcher and what it launched, like Godot's console program
        let r = output_within(Command::new("cmd").args(["/C", "echo started & ping -n 60 127.0.0.1 > nul"]), std::time::Duration::from_secs(2)).unwrap();
        assert!(r.timed_out && r.status.is_none());
        assert!(t.elapsed().as_secs() < 20, "stopped at the limit, not after the child's own minute: {:?}", t.elapsed());
        assert!(r.stdout.contains("started"), "what it printed before the stop is kept: {}", r.stdout);
    }
}
