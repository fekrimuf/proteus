//! The game engine as a child program: Godot runs `engine/godot/referee.gd` on an engine file (the tiled glTF an engine gets).
//!
//! Every launch goes through [`crate::lowload`] (low priority, the job memory cap, stopped when `pt` exits) and has a time limit
//! (`PT_GODOT_SECONDS`, default 600): an import that stalls is stopped with the engine it started and reported, so no command waits
//! on it forever. The scorecard's referee runs, the survey (`pt survey`) and the walk-through (`pt walk`) all start the engine here.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const DEFAULT_GODOT: &str = r"D:\Fekri\tools\godot\Godot_v4.7.2-stable_win64_console.exe";

/// The Godot program: `PT_GODOT`, else the machine's default (ENVIRONMENT.md).
pub fn exe() -> PathBuf {
    std::env::var("PT_GODOT").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(DEFAULT_GODOT))
}

/// How long one engine run may take before it is stopped: `PT_GODOT_SECONDS`, default 600.
pub fn limit() -> Duration {
    Duration::from_secs(std::env::var("PT_GODOT_SECONDS").ok().and_then(|s| s.parse::<u64>().ok()).filter(|s| *s > 0).unwrap_or(600))
}

/// A path the way the engine script takes it: absolute, with forward slashes.
pub fn engine_path(p: &Path) -> String {
    std::env::current_dir().unwrap_or_default().join(p).to_string_lossy().replace('\\', "/")
}

/// The command line of one referee run on `glb` with the script's `key=value` options.
pub fn command(glb: &Path, extra: &[&str], windowed: bool) -> Command {
    let mut cmd = Command::new(exe());
    if !windowed {
        cmd.arg("--headless");
    }
    cmd.arg("--path").arg(crate::blender::repo_root().join("engine/godot")).args(["--script", "referee.gd", "--"]).arg(engine_path(glb)).args(extra);
    cmd
}

/// The script's one result line, `REFEREE_JSON {...}`, out of what the engine printed.
pub fn result_line(stdout: &str) -> Option<Value> {
    stdout.lines().find_map(|l| l.strip_prefix("REFEREE_JSON ")).and_then(|j| serde_json::from_str::<Value>(j).ok())
}

/// What a run that was stopped at the time limit reports: not ok, with the reason and the way out in words.
pub fn timed_out(seconds: f64, limit: Duration) -> Value {
    json!({"available": true, "ok": false, "timed_out": true, "seconds": seconds,
           "errors": [format!("the engine did not finish within {} s and was stopped (a stalled import; PT_GODOT_SECONDS changes the limit)", limit.as_secs())]})
}

/// Run the referee script on `glb` and return what it measured. `{"available": false}` when Godot is not installed; a run that
/// passes the time limit is stopped and comes back as [`timed_out`].
pub fn run(glb: &Path, extra: &[&str], windowed: bool) -> Value {
    run_within(glb, extra, windowed, limit())
}

pub fn run_within(glb: &Path, extra: &[&str], windowed: bool, limit: Duration) -> Value {
    if !exe().exists() {
        return json!({"available": false});
    }
    let Ok(out) = crate::lowload::output_within(&mut command(glb, extra, windowed), limit) else { return json!({"available": false}) };
    if out.timed_out {
        return timed_out(out.seconds, limit);
    }
    match result_line(&out.stdout) {
        Some(mut v) => {
            v["available"] = json!(true);
            v["launch_s"] = json!((out.seconds * 100.0).round() / 100.0);
            v["peak_mem_mb"] = json!(crate::lowload::peak_mem_mb().round());
            v
        }
        None => json!({"available": true, "ok": false, "errors": [format!("Godot printed no result (exit {:?})", out.status.and_then(|s| s.code()))]}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_line_is_found_among_the_engine_s_own_printing() {
        let v = result_line("Godot Engine v4.7\nVulkan 1.3\nREFEREE_JSON {\"ok\":true,\"nodes\":3}\n").unwrap_or_default();
        assert_eq!(v["nodes"], 3);
        assert!(result_line("no result here").is_none());
    }

    #[test]
    fn a_stopped_run_is_not_ok_and_says_why() {
        let v = timed_out(600.2, Duration::from_secs(600));
        assert_eq!(v["ok"], false);
        assert_eq!(v["timed_out"], true);
        assert!(v["errors"][0].as_str().is_some_and(|e| e.contains("600 s") && e.contains("PT_GODOT_SECONDS")));
    }

    /// The class: an engine that never comes back. A stand-in program that runs for a minute takes the engine's place; the run
    /// returns at the limit and says it timed out.
    #[cfg(windows)]
    #[test]
    fn an_engine_that_stalls_is_stopped_at_the_limit() {
        let dir = std::env::temp_dir().join(format!("pt_engine_stall_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fake = dir.join("stalled_engine.cmd");
        assert!(std::fs::write(&fake, "@echo off\r\nping -n 60 127.0.0.1 > nul\r\n").is_ok());
        let t = std::time::Instant::now();
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(&fake);
        let out = crate::lowload::output_within(&mut cmd, Duration::from_secs(2));
        assert!(out.is_ok_and(|o| o.timed_out));
        assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
