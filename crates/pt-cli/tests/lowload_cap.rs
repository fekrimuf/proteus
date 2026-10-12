//! The memory cap really stops a runaway child (own test binary, so the cap is read from this file's environment).
#![cfg(windows)]

use std::process::Command;

const ALLOC: &str = "try { $a = New-Object byte[] SIZE; Write-Output ('len=' + $a.Length) } catch { Write-Output 'failed' }";

fn script(size: u64) -> String {
    ALLOC.replace("SIZE", &size.to_string())
}

#[test]
fn a_child_over_the_cap_fails_and_a_small_one_runs() {
    // control: without the cap an 800 MB array is allocated (briefly)
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-Command", &script(800_000_000)]);
    let free = c.output().unwrap();
    assert!(String::from_utf8_lossy(&free.stdout).contains("len=800000000"), "control run should allocate: {}", String::from_utf8_lossy(&free.stdout));

    std::env::set_var("PT_HEAVY_MEM_GB", "0.4");
    let capped = |size: u64| {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script(size)]);
        String::from_utf8_lossy(&pt_cli::lowload::output(&mut c).unwrap().stdout).to_string()
    };
    assert!(capped(20_000_000).contains("len=20000000"), "a 20 MB child should run under the cap");
    let big = capped(800_000_000);
    assert!(!big.contains("len=800000000"), "an 800 MB child should be stopped by a 0.4 GB cap, got: {big}");
}
