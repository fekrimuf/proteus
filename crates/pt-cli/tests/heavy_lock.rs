//! Only one heavy child runs on the machine at a time, across `pt` processes (`lowload`'s machine-wide lock).
//!
//! The stand-in programs are this test program itself, started again with a role in the environment: a `job` takes the place of
//! one `pt` (it starts one heavy child through `lowload`), a `sleep` takes the place of the heavy child (it logs when it starts
//! and ends). The tests need no Blender or Godot and run the same on every system.

use pt_cli::lowload;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pt_heavy_lock_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn me(role: &str, test: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", test, "--nocapture", "--test-threads=1"]).env("PT_TEST_ROLE", role);
    c
}

/// The stand-in heavy child: logs `start TAG ms`, sleeps, logs `end TAG ms`.
#[test]
fn helper_sleep() {
    if std::env::var("PT_TEST_ROLE").as_deref() != Ok("sleep") {
        return;
    }
    let (log, tag) = (std::env::var("PT_TEST_LOG").unwrap_or_default(), std::env::var("PT_TEST_TAG").unwrap_or_default());
    let note = |what: &str| {
        if !log.is_empty() {
            use std::io::Write;
            let _ = std::fs::OpenOptions::new().create(true).append(true).open(&log).and_then(|mut f| writeln!(f, "{what} {tag} {}", now_ms()));
        }
    };
    note("start");
    std::thread::sleep(Duration::from_millis(std::env::var("PT_TEST_SLEEP_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(100)));
    note("end");
}

/// The stand-in `pt`: starts one heavy child through `lowload` and fails if that fails.
#[test]
fn helper_job() {
    if std::env::var("PT_TEST_ROLE").as_deref() != Ok("job") {
        return;
    }
    let mut child = me("sleep", "helper_sleep");
    let out = lowload::output(&mut child).unwrap();
    eprintln!("job done");
    assert!(out.status.success());
}

struct Job {
    child: Child,
}

fn start_job(dir: &Path, tag: &str, sleep_ms: u64, wait: &str) -> Job {
    let mut c = me("job", "helper_job");
    c.env("PT_HEAVY_LOCK_DIR", dir.join("lock")).env("PT_HEAVY_WAIT_SECONDS", wait).env("PT_TEST_LOG", dir.join("log.txt")).env("PT_TEST_TAG", tag).env("PT_TEST_SLEEP_MS", sleep_ms.to_string());
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    Job { child: c.spawn().unwrap() }
}

impl Job {
    fn finish(self) -> (bool, String) {
        let o = self.child.wait_with_output().unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stderr).into_owned())
    }
}

fn log(dir: &Path) -> Vec<(String, String, u128)> {
    std::fs::read_to_string(dir.join("log.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            Some((p.next()?.to_string(), p.next()?.to_string(), p.next()?.parse().ok()?))
        })
        .collect()
}

fn when(dir: &Path, what: &str, tag: &str) -> Option<u128> {
    log(dir).into_iter().find(|(w, t, _)| w == what && t == tag).map(|e| e.2)
}

fn wait_for(dir: &Path, what: &str, tag: &str) {
    let t = Instant::now();
    while when(dir, what, tag).is_none() {
        assert!(t.elapsed() < Duration::from_secs(60), "{what} {tag} never logged");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The class: two `pt` processes that each start a heavy child. Without the lock both children run at once.
#[test]
fn a_second_heavy_start_waits_until_the_first_child_has_ended() {
    let dir = scratch("overlap");
    let a = start_job(&dir, "A", 2000, "120");
    wait_for(&dir, "start", "A");
    let b = start_job(&dir, "B", 200, "120");
    let ((a_ok, _), (b_ok, b_err)) = (a.finish(), b.finish());
    assert!(a_ok && b_ok, "{b_err}");
    let (a_end, b_start) = (when(&dir, "end", "A").unwrap(), when(&dir, "start", "B").unwrap());
    assert!(b_start >= a_end, "B started at {b_start} while A ran until {a_end}");
    assert!(b_err.contains("waiting for the machine"), "the waiter says what it waits for: {b_err}");
    assert!(b_err.contains("pid "), "and who holds it: {b_err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A holder that is killed (not ended) must not leave a lock behind: the waiter goes on.
#[test]
fn a_holder_that_is_killed_lets_the_waiter_go_on() {
    let dir = scratch("killed");
    let mut a = start_job(&dir, "A", 20_000, "120");
    wait_for(&dir, "start", "A");
    let b = start_job(&dir, "B", 100, "120");
    std::thread::sleep(Duration::from_millis(1000));
    assert!(when(&dir, "start", "B").is_none(), "B must be waiting while A lives");
    let t = Instant::now();
    a.child.kill().unwrap();
    let _ = a.child.wait();
    let (ok, err) = b.finish();
    assert!(ok, "{err}");
    assert!(t.elapsed() < Duration::from_secs(15), "B went on soon after the kill: {:?}", t.elapsed());
    assert!(when(&dir, "end", "B").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The wait has a limit; 0 means do not wait. Both fail with a message that names the holder.
#[test]
fn the_wait_is_limited_and_zero_fails_at_once() {
    let dir = scratch("limit");
    let a = start_job(&dir, "A", 8000, "120");
    wait_for(&dir, "start", "A");
    let t = Instant::now();
    let (ok, err) = start_job(&dir, "B", 100, "1").finish();
    assert!(!ok && err.contains("PT_HEAVY_WAIT_SECONDS=1") && err.contains("pid "), "{err}");
    assert!(t.elapsed() < Duration::from_secs(7), "{:?}", t.elapsed());
    let (ok, err) = start_job(&dir, "C", 100, "0").finish();
    assert!(!ok && err.contains("PT_HEAVY_WAIT_SECONDS=0"), "{err}");
    assert!(when(&dir, "start", "B").is_none() && when(&dir, "start", "C").is_none());
    assert!(a.finish().0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// One process that starts several heavy children, one after another and one inside another (the scorecard, the judge server
/// alive across calls), must not wait for itself, even with PT_HEAVY_WAIT_SECONDS=0.
#[test]
fn one_process_with_several_heavy_children_does_not_block_itself() {
    let dir = scratch("self");
    std::env::set_var("PT_HEAVY_LOCK_DIR", dir.join("lock"));
    std::env::set_var("PT_HEAVY_WAIT_SECONDS", "0");
    let t = Instant::now();
    for _ in 0..2 {
        let mut c = me("sleep", "helper_sleep");
        assert!(lowload::output(&mut c).unwrap().status.success());
    }
    assert!(!lowload::is_held(), "nothing is held between children");
    let mut long = me("sleep", "helper_sleep");
    long.env("PT_TEST_SLEEP_MS", "1500").stdout(Stdio::null()).stderr(Stdio::null());
    let mut server = lowload::spawn_uncapped(&mut long).unwrap();
    assert!(lowload::is_held());
    let mut other = me("sleep", "helper_sleep");
    assert!(lowload::output(&mut other).unwrap().status.success(), "a child while the server lives");
    assert!(lowload::is_held(), "still held for as long as the server lives");
    let _ = server.wait();
    drop(server);
    assert!(!lowload::is_held(), "let go with the last child");
    assert!(t.elapsed() < Duration::from_secs(30));
    let _ = std::fs::remove_dir_all(&dir);
}
