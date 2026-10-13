//! `pt gate`: the one command that says whether a change may be committed, the same locally, in an agent's hands and in CI.
//!
//! Steps, in order (stop at the first failure, name it):
//!
//! 1. `build`: the whole workspace compiles in release;
//! 2. `lint`: clippy on all production code (libraries and binaries) with two rules that are written down and now enforced:
//!    * the banned-methods list in `clippy.toml` (the standard library's `sin`, `cos`, `exp`, `powf`, ... differ between operating
//!      systems, so cooked geometry must use `pt_core::math`; DECISIONS D2);
//!    * no crash-prone shortcuts (`unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`), set per crate by
//!      `[workspace.lints.clippy]` in the root `Cargo.toml` (DECISIONS B19). Test code may use them; a production line that truly
//!      cannot fail carries `#[allow(clippy::...)]` with the reason;
//! 3. `docs`: the documentation harness (`pt docs-check`);
//! 4. `tests`: every test, which includes the recipes against their golden hashes and the thread-count independence of the cook.
//!
//! CI runs the same steps (`.github/workflows/ci.yml`), so "it passed on my machine" and "it passed in CI" mean the same thing.

use crate::registry::{Command as Cmd, Kind, Param};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

pub const GATE: Cmd = Cmd {
    name: "gate",
    tool: "gate",
    summary: "The checks every commit must pass, in order: build, lint (the platform-independent maths rule and the ban on crash-prone shortcuts, enforced by clippy on all production code), the documentation harness, and the tests. Stops at the first failing step and names it. Takes minutes: it builds and tests the workspace.",
    heavy: true,
    params: &[
        Param::flag("quick", Kind::Bool, "leave out the tests (build, lint and docs only)"),
        Param::flag("only", Kind::Str, "run only these steps, comma-separated: build, lint, docs, tests"),
    ],
    run: gate_cmd,
};

const STEPS: &[&str] = &["build", "lint", "docs", "tests"];

fn root() -> PathBuf {
    for c in [".", "../.."] {
        let p = PathBuf::from(c);
        if p.join("Cargo.toml").is_file() && p.join("crates").is_dir() {
            return p;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cargo() -> std::ffi::OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Run `cargo ARGS` at low priority (the owner works on this machine), output passed through.
fn run_cargo(args: &[&str]) -> Result<(), String> {
    let mut c = Command::new(cargo());
    c.args(args).current_dir(root());
    let mut child = crate::lowload::spawn_light(&mut c).map_err(|e| format!("cannot start cargo: {e}"))?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`cargo {}` failed ({status})", args.join(" ")))
    }
}

/// The clippy run of the `lint` step.
const LINT_ARGS: &[&str] = &["clippy", "--release", "--workspace", "--lib", "--bins", "--", "-A", "clippy::all", "-D", "clippy::disallowed_methods"];

pub fn gate_cmd(args: &[String]) -> Result<(), String> {
    let quick = args.iter().any(|a| a == "--quick");
    let only: Option<Vec<String>> = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
    if let Some(o) = &only {
        if let Some(bad) = o.iter().find(|s| !STEPS.contains(&s.as_str())) {
            return Err(format!("unknown step '{bad}'; the steps are: {}", STEPS.join(", ")));
        }
    }
    let wanted = |s: &str| match &only {
        Some(o) => o.iter().any(|x| x == s),
        None => !(quick && s == "tests"),
    };
    let all = Instant::now();
    for step in STEPS.iter().filter(|s| wanted(s)) {
        println!("== {step}");
        let t = Instant::now();
        let r = match *step {
            "build" => run_cargo(&["build", "--release", "--workspace"]),
            "lint" => run_cargo(LINT_ARGS),
            "docs" => crate::docs::check_cmd(&[]),
            _ => run_cargo(&["test", "--release", "--workspace"]),
        };
        match r {
            Ok(()) => println!("== {step}: ok ({:.1} s)", t.elapsed().as_secs_f64()),
            Err(e) => return Err(format!("gate stopped at step '{step}': {e}")),
        }
    }
    println!("gate: all steps passed ({:.1} s)", all.elapsed().as_secs_f64());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_step_is_refused_with_the_list() {
        let e = gate_cmd(&["--only".into(), "build,sing".into()]).unwrap_err();
        assert!(e.contains("unknown step 'sing'") && e.contains("lint") && e.contains("docs"), "{e}");
    }

    #[test]
    fn the_lint_step_enforces_the_maths_rule_and_the_workspace_declares_the_crash_ban() {
        assert!(LINT_ARGS.contains(&"clippy::disallowed_methods"), "the maths rule is enforced");
        let cargo_toml = std::fs::read_to_string(root().join("Cargo.toml")).unwrap_or_default();
        for lint in ["unwrap_used", "expect_used", "panic", "unreachable", "todo", "unimplemented"] {
            assert!(cargo_toml.contains(&format!("{lint} = \"deny\"")), "[workspace.lints.clippy] must deny {lint}");
        }
        // every crate opts in, so a new crate cannot start outside the rule
        let mut missing = vec![];
        for e in std::fs::read_dir(root().join("crates")).into_iter().flatten().flatten() {
            let m = std::fs::read_to_string(e.path().join("Cargo.toml")).unwrap_or_default();
            if !m.contains("[lints]") || !m.contains("workspace = true") {
                missing.push(e.file_name().to_string_lossy().into_owned());
            }
        }
        assert!(missing.is_empty(), "these crates do not inherit the workspace lints: {missing:?} (add `[lints]` / `workspace = true`)");
        let clippy = std::fs::read_to_string(root().join("clippy.toml")).unwrap_or_default();
        assert!(clippy.contains("disallowed-methods") && clippy.contains("allow-unwrap-in-tests = true"), "clippy.toml keeps the banned list and lets tests unwrap");
    }
}
