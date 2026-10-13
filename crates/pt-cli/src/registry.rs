//! The one list of commands.
//!
//! Every command that works on files (a recipe and a scene, a frame, the documents) is declared once, beside its code, as a
//! [`Command`]: a name, a one-line summary and its parameters. Three things read that declaration and nothing else:
//!
//! * the command line (`pt NAME ...`): [`run_cli`] refuses a mistyped option or a wrong number of arguments and names what is
//!   accepted, then runs the command;
//! * the agent's tool server (`tools/list`, `tools/call`): [`tool_defs`] builds each tool's schema from the parameters and
//!   [`call_tool`] runs the same command in a child `pt` process, so a command that prints, panics or runs for minutes cannot
//!   corrupt or take down the live session;
//! * the documentation: [`markdown`] renders `docs/generated/commands.md`, which `pt docs-check` keeps current.
//!
//! To add a command: write its `fn(&[String]) -> Result<(), String>`, declare it with a `pub const` [`Command`] beside it, and add
//! one line to [`ALL`]. It is then on the command line, in the agent's tool list and in the generated docs.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::{Command as Proc, Stdio};

/// The JSON type of a parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Str,
    Int,
    Num,
    /// A flag with no value: present or absent.
    Bool,
    /// Several values: a run of positional arguments, or a flag given more than once.
    List,
}

impl Kind {
    fn json_type(self) -> &'static str {
        match self {
            Kind::Str => "string",
            Kind::Int => "integer",
            Kind::Num => "number",
            Kind::Bool => "boolean",
            Kind::List => "array",
        }
    }
}

/// One parameter of a command. Positional parameters come first on the command line, in declaration order; the others are
/// options, written `--name VALUE` (`--name` alone for a [`Kind::Bool`]), with `_` in the name written as `-`.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub positional: bool,
    pub doc: &'static str,
}

impl Param {
    /// A required positional argument.
    pub const fn pos(name: &'static str, kind: Kind, doc: &'static str) -> Param {
        Param { name, kind, required: true, positional: true, doc }
    }
    /// An optional positional argument (left out, it is skipped).
    pub const fn opt_pos(name: &'static str, kind: Kind, doc: &'static str) -> Param {
        Param { name, kind, required: false, positional: true, doc }
    }
    /// An optional `--name` option.
    pub const fn flag(name: &'static str, kind: Kind, doc: &'static str) -> Param {
        Param { name, kind, required: false, positional: false, doc }
    }
    /// A required `--name` option.
    pub const fn req_flag(name: &'static str, kind: Kind, doc: &'static str) -> Param {
        Param { name, kind, required: true, positional: false, doc }
    }
    /// The command-line spelling of an option.
    pub fn option(&self) -> String {
        format!("--{}", self.name.replace('_', "-"))
    }
}

/// A command, declared once.
pub struct Command {
    /// The command-line name (`pt NAME`).
    pub name: &'static str,
    /// The agent tool's name.
    pub tool: &'static str,
    pub summary: &'static str,
    /// Takes the machine (the GPU or several GB of memory for minutes): the tool description says so, and it runs at low priority.
    pub heavy: bool,
    pub params: &'static [Param],
    pub run: fn(&[String]) -> Result<(), String>,
}

/// Every registered command. One line per command; keep it in alphabetical order of the command-line name.
pub const ALL: &[&Command] = &[
    &crate::docs::DOCS_CHECK,
    &crate::edits::EDIT,
    &crate::gate::GATE,
    &crate::grounding::GROUND_CHECK,
    &crate::recipes::RUN_ALL,
    &crate::blender::RENDER_SCENE,
    &crate::grounding::REVIEW_TILES,
    &crate::scorecard::SCORECARD,
    &crate::docs::STATUS,
    &crate::world::WORLD,
];

/// The command with this command-line name.
pub fn find(name: &str) -> Option<&'static Command> {
    ALL.iter().copied().find(|c| c.name == name)
}

impl Command {
    fn positionals(&self) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(|p| p.positional)
    }

    fn options(&self) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(|p| !p.positional)
    }

    /// `pt NAME <a> [b] --opt VALUE [--flag]`
    pub fn usage(&self) -> String {
        let mut s = format!("pt {}", self.name);
        for p in self.positionals() {
            let dots = if p.kind == Kind::List { "..." } else { "" };
            s.push_str(&if p.required { format!(" <{}{dots}>", p.name) } else { format!(" [{}{dots}]", p.name) });
        }
        for p in self.options() {
            let body = if p.kind == Kind::Bool { p.option() } else { format!("{} {}", p.option(), p.name.to_uppercase()) };
            s.push_str(&if p.required { format!(" {body}") } else { format!(" [{body}]") });
        }
        s
    }

    fn accepted_options(&self) -> String {
        let o: Vec<String> = self.options().map(Param::option).collect();
        if o.is_empty() {
            "no options".into()
        } else {
            o.join(", ")
        }
    }

    /// Refuse a command line that does not fit the declaration: an unknown option (a typo), an option without its value, a
    /// number that is not one, a missing required option, or the wrong number of positional arguments.
    pub fn check_argv(&self, argv: &[String]) -> Result<(), String> {
        let mut seen: Vec<&str> = Vec::new();
        let mut n_pos = 0usize;
        let mut i = 0usize;
        while let Some(a) = argv.get(i) {
            if !a.starts_with("--") {
                n_pos += 1;
                i += 1;
                continue;
            }
            let Some(p) = self.options().find(|p| p.option() == *a) else {
                return Err(format!("unknown option '{a}' for 'pt {}'; accepted: {}\nusage: {}", self.name, self.accepted_options(), self.usage()));
            };
            seen.push(p.name);
            if p.kind == Kind::Bool {
                i += 1;
                continue;
            }
            let Some(v) = argv.get(i + 1) else {
                return Err(format!("option '{a}' needs a value\nusage: {}", self.usage()));
            };
            let bad = match p.kind {
                Kind::Int => v.parse::<i64>().is_err(),
                Kind::Num => v.parse::<f64>().is_err(),
                _ => false,
            };
            if bad {
                return Err(format!("option '{a}' needs {} (a number), got '{v}'\nusage: {}", if p.kind == Kind::Int { "a whole number" } else { "a number" }, self.usage()));
            }
            i += 2;
        }
        let min = self.positionals().filter(|p| p.required).count();
        let max = if self.positionals().any(|p| p.kind == Kind::List) { usize::MAX } else { self.positionals().count() };
        if n_pos < min || n_pos > max {
            return Err(format!("{n_pos} positional argument(s) given, expected {}\nusage: {}", if min == max { min.to_string() } else if max == usize::MAX { format!("at least {min}") } else { format!("{min} to {max}") }, self.usage()));
        }
        if let Some(p) = self.options().find(|p| p.required && !seen.contains(&p.name)) {
            return Err(format!("missing required option {}\nusage: {}", p.option(), self.usage()));
        }
        Ok(())
    }

    /// The agent tool's JSON schema, built from the parameters.
    pub fn tool_def(&self) -> Value {
        let mut props = Map::new();
        for p in self.params {
            let mut d = Map::new();
            d.insert("type".into(), json!(p.kind.json_type()));
            if p.kind == Kind::List {
                d.insert("items".into(), json!({}));
            }
            d.insert("description".into(), json!(p.doc));
            props.insert(p.name.into(), Value::Object(d));
        }
        let required: Vec<&str> = self.params.iter().filter(|p| p.required).map(|p| p.name).collect();
        let heavy = if self.heavy { " HEAVY: takes the machine (the GPU, or several GB of memory, for minutes). It runs at low priority under a memory cap and needs no permission; run one such job at a time and leave none running." } else { "" };
        json!({
            "name": self.tool,
            "description": format!("{}{heavy} Same as `{}`.", self.summary, self.usage()),
            "inputSchema": {"type": "object", "properties": props, "required": required},
        })
    }

    /// The tool's JSON arguments as a command line. An unknown argument, a missing required one or a wrong type is refused with
    /// what is accepted.
    pub fn argv_from_json(&self, args: &Value) -> Result<Vec<String>, String> {
        let empty = Map::new();
        let obj = match args {
            Value::Null => &empty,
            Value::Object(o) => o,
            _ => return Err(format!("'{}' takes its arguments as a JSON object", self.tool)),
        };
        let accepted = || self.params.iter().map(|p| p.name).collect::<Vec<_>>().join(", ");
        if let Some(k) = obj.keys().find(|k| !self.params.iter().any(|p| p.name == k.as_str())) {
            return Err(format!("unknown argument '{k}' for '{}'; accepted: {}", self.tool, if self.params.is_empty() { "none".to_string() } else { accepted() }));
        }
        let mut argv: Vec<String> = Vec::new();
        for p in self.positionals().chain(self.options()) {
            let v = match obj.get(p.name) {
                None | Some(Value::Null) => {
                    if p.required {
                        return Err(format!("missing required argument '{}' for '{}' ({}); accepted: {}", p.name, self.tool, p.doc, accepted()));
                    }
                    continue;
                }
                Some(v) => v,
            };
            let wrong = |what: &str| format!("argument '{}' of '{}' must be {what}, got {v}", p.name, self.tool);
            let tokens: Vec<String> = match p.kind {
                Kind::Str => vec![v.as_str().ok_or_else(|| wrong("a string"))?.to_string()],
                Kind::Int => vec![(if v.is_i64() || v.is_u64() { v.to_string() } else { return Err(wrong("a whole number")) })],
                Kind::Num => vec![if v.is_number() { v.to_string() } else { return Err(wrong("a number")) }],
                Kind::Bool => {
                    if v.as_bool().ok_or_else(|| wrong("true or false"))? {
                        argv.push(p.option());
                    }
                    continue;
                }
                Kind::List => v
                    .as_array()
                    .ok_or_else(|| wrong("an array"))?
                    .iter()
                    .map(|x| match x {
                        Value::String(s) => Ok(s.clone()),
                        Value::Number(n) => Ok(n.to_string()),
                        _ => Err(wrong("an array of strings and numbers")),
                    })
                    .collect::<Result<_, _>>()?,
            };
            if p.positional {
                argv.extend(tokens);
            } else if p.kind == Kind::List {
                for t in tokens {
                    argv.push(p.option());
                    argv.push(t);
                }
            } else {
                argv.push(p.option());
                argv.extend(tokens);
            }
        }
        self.check_argv(&argv)?;
        Ok(argv)
    }
}

/// Run a command from the command line, after checking the arguments against its declaration.
pub fn run_cli(c: &Command, argv: &[String]) -> Result<(), String> {
    c.check_argv(argv)?;
    (c.run)(argv)
}

/// The program that runs a tool's command: `PT_EXE` when set (tests), else this program.
fn exe() -> Result<PathBuf, String> {
    std::env::var_os("PT_EXE").map(PathBuf::from).or_else(|| std::env::current_exe().ok()).ok_or_else(|| "cannot find the pt program to run the command".to_string())
}

/// The last `n` characters of `s`.
fn tail(s: &str, n: usize) -> &str {
    match s.char_indices().rev().nth(n.saturating_sub(1)) {
        Some((i, _)) if s.chars().count() > n => &s[i..],
        _ => s,
    }
}

/// Run the command of tool `tool` with JSON `args`; `None` when no registered command has that tool name. The command runs in a
/// child `pt` process (low priority and a memory cap when heavy), so what it prints cannot corrupt the tool stream and a crash in
/// it cannot take the session down. The result is `{command, ok, exit_code, output, stderr?}`; a failing command is returned as an
/// error whose text is that same JSON, so the agent sees what the command printed.
pub fn call_tool(tool: &str, args: &Value) -> Option<Result<Value, String>> {
    let c = ALL.iter().copied().find(|c| c.tool == tool)?;
    Some(run_child(c, args))
}

fn run_child(c: &Command, args: &Value) -> Result<Value, String> {
    let argv = c.argv_from_json(args)?;
    let mut p = Proc::new(exe()?);
    p.arg(c.name).args(&argv);
    // A heavy command runs at low priority and dies with this process. It gets no memory cap of its own: the programs it starts
    // (Blender, Godot) are confined by `lowload` themselves, and a second cap around the cook could fail a command the command
    // line would have finished.
    let out = if c.heavy {
        p.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        crate::lowload::spawn_uncapped(&mut p).and_then(|child| child.wait_with_output())
    } else {
        p.output()
    }
    .map_err(|e| format!("could not start 'pt {}': {e}", c.name))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stdout = stdout.trim();
    let body = serde_json::from_str::<Value>(stdout).unwrap_or_else(|_| json!(tail(stdout, 12_000)));
    let mut r = json!({"command": format!("pt {} {}", c.name, argv.join(" ")).trim_end(), "ok": out.status.success(), "exit_code": out.status.code(), "output": body});
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        r["stderr"] = json!(tail(stderr.trim(), 4_000));
    }
    if out.status.success() {
        Ok(r)
    } else {
        Err(r.to_string())
    }
}

/// The schemas of all registered commands, for `tools/list`.
pub fn tool_defs() -> Vec<Value> {
    ALL.iter().map(|c| c.tool_def()).collect()
}

/// `pt help`: every registered command with its usage.
pub fn help_text() -> String {
    let mut s = String::from("Registered commands (each is also an agent tool; `pt commands --json` lists them for programs):\n");
    for c in ALL {
        s.push_str(&format!("\n  {}{}\n    {}\n", c.usage(), if c.heavy { "   [HEAVY]" } else { "" }, c.summary));
    }
    s
}

/// `pt commands --json`
pub fn list_json() -> Value {
    json!(ALL.iter().map(|c| json!({"command": c.name, "tool": c.tool, "heavy": c.heavy, "summary": c.summary, "usage": c.usage()})).collect::<Vec<_>>())
}

/// The generated command table (`docs/generated/commands.md`).
pub fn markdown() -> String {
    let mut s = String::from("# Registered commands\n\nGenerated by `pt status --write` from `crates/pt-cli/src/registry.rs`. Do not edit by hand; `pt docs-check` fails when it is stale.\n\nEvery command below is on the command line and, under the tool name, in the agent's tool list. A heavy command takes the machine (GPU or several GB of memory) and runs at low priority under a memory cap; it needs no permission, one at a time.\n\n| Command | Tool | Heavy | What it does |\n|---|---|---|---|\n");
    for c in ALL {
        s.push_str(&format!("| `{}` | `{}` | {} | {} |\n", c.name, c.tool, if c.heavy { "yes" } else { "" }, c.summary));
    }
    s.push_str("\n## Usage\n\n");
    for c in ALL {
        s.push_str(&format!("### {}\n\n`{}`\n\n", c.name, c.usage()));
        for p in c.params {
            s.push_str(&format!("* `{}`{}: {}\n", if p.positional { p.name.to_string() } else { p.option() }, if p.required { " (required)" } else { "" }, p.doc));
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn names_are_unique_and_never_shadow_a_hand_written_tool() {
        let hand = crate::mcp::hand_written_tool_names();
        let mut cli = std::collections::HashSet::new();
        let mut tools = std::collections::HashSet::new();
        for c in ALL {
            assert!(cli.insert(c.name), "command '{}' is registered twice", c.name);
            assert!(tools.insert(c.tool), "tool '{}' is registered twice", c.tool);
            assert!(!hand.contains(&c.tool.to_string()), "tool '{}' would shadow a hand-written tool", c.tool);
            assert!(!c.summary.is_empty() && c.params.iter().all(|p| !p.doc.is_empty()), "'{}' needs a summary and a doc line per parameter", c.name);
        }
    }

    #[test]
    fn a_mistyped_option_is_refused_with_what_is_accepted() {
        let c = find("ground-check").unwrap();
        let e = c.check_argv(&s(&["w.hcs", "w.json", "--raduis", "30"])).unwrap_err();
        assert!(e.contains("unknown option '--raduis'") && e.contains("--radius") && e.contains("--frames"), "{e}");
        assert!(c.check_argv(&s(&["w.hcs", "w.json", "--radius", "30", "--lookdev"])).is_ok());
        let e = c.check_argv(&s(&["w.hcs", "w.json", "--radius", "abc"])).unwrap_err();
        assert!(e.contains("needs a number") && e.contains("'abc'"), "{e}");
        let e = c.check_argv(&s(&["w.hcs", "w.json", "--radius"])).unwrap_err();
        assert!(e.contains("needs a value"), "{e}");
    }

    #[test]
    fn the_wrong_number_of_arguments_and_a_missing_required_option_are_refused() {
        let c = find("ground-check").unwrap();
        assert!(c.check_argv(&s(&["only.hcs"])).unwrap_err().contains("expected 2"));
        assert!(c.check_argv(&s(&["a", "b", "c"])).unwrap_err().contains("expected 2"));
        let sc = find("scorecard").unwrap();
        assert!(sc.check_argv(&s(&["w.hcs", "w.json"])).unwrap_err().contains("missing required option --out"));
        assert!(sc.check_argv(&s(&["w.hcs", "w.json", "--out", "o", "--foil", "a", "--foil", "b"])).is_ok(), "a list option may repeat");
    }

    #[test]
    fn tool_arguments_become_the_same_command_line() {
        let c = find("ground-check").unwrap();
        let argv = c.argv_from_json(&json!({"recipe": "w.hcs", "scene": "w.json", "radius": 30, "lookdev": true, "frames": "out/f"})).unwrap();
        assert_eq!(argv, s(&["w.hcs", "w.json", "--radius", "30", "--frames", "out/f", "--lookdev"]), "positionals first in order, then options in declaration order");
        let argv = c.argv_from_json(&json!({"recipe": "w.hcs", "scene": "w.json", "lookdev": false})).unwrap();
        assert_eq!(argv, s(&["w.hcs", "w.json"]), "a false flag is left out");
        let edit = find("edit").unwrap();
        assert_eq!(edit.argv_from_json(&json!({"file": "e.json", "scope": "rocks", "id": 77, "op": "move", "numbers": [1, 0, -2.5]})).unwrap(), s(&["e.json", "rocks", "77", "move", "1", "0", "-2.5"]));
        assert_eq!(edit.argv_from_json(&json!({"file": "e.json", "scope": "trees", "op": "add", "numbers": [3, 0, 4, 1.2], "to": true})).unwrap(), s(&["e.json", "trees", "add", "3", "0", "4", "1.2", "--to"]), "an absent optional positional is skipped");
        let sc = find("scorecard").unwrap();
        assert_eq!(sc.argv_from_json(&json!({"recipe": "r", "scene": "s", "out": "o", "foil": ["a", "b"]})).unwrap(), s(&["r", "s", "--out", "o", "--foil", "a", "--foil", "b"]));
    }

    #[test]
    fn a_mistyped_tool_argument_is_refused_with_what_is_accepted() {
        let c = find("ground-check").unwrap();
        let e = c.argv_from_json(&json!({"recipe": "w.hcs", "scene": "w.json", "raduis": 30})).unwrap_err();
        assert!(e.contains("unknown argument 'raduis'") && e.contains("radius") && e.contains("frames"), "{e}");
        let e = c.argv_from_json(&json!({"recipe": "w.hcs"})).unwrap_err();
        assert!(e.contains("missing required argument 'scene'"), "{e}");
        let e = c.argv_from_json(&json!({"recipe": "w.hcs", "scene": "w.json", "radius": "far"})).unwrap_err();
        assert!(e.contains("must be a number"), "{e}");
        let e = c.argv_from_json(&json!({"recipe": 5, "scene": "w.json"})).unwrap_err();
        assert!(e.contains("must be a string"), "{e}");
        assert!(c.argv_from_json(&json!([1, 2])).unwrap_err().contains("JSON object"));
    }

    #[test]
    fn the_schema_lists_every_parameter_and_marks_heavy_commands() {
        let d = find("scorecard").unwrap().tool_def();
        assert_eq!(d["name"], "scorecard");
        assert!(d["description"].as_str().unwrap().contains("HEAVY"));
        assert_eq!(d["inputSchema"]["required"], json!(["recipe", "scene", "out"]));
        assert_eq!(d["inputSchema"]["properties"]["samples"]["type"], "integer");
        assert!(find("review-tiles").unwrap().tool_def()["description"].as_str().unwrap().contains("Same as `pt review-tiles"));
        assert!(!find("review-tiles").unwrap().tool_def()["description"].as_str().unwrap().contains("HEAVY"));
    }

    #[test]
    fn tail_keeps_the_end_on_a_character_boundary() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("abc", 10), "abc");
        assert_eq!(tail("åäö-åäö", 4), "-åäö");
    }

    #[test]
    fn the_generated_table_has_a_row_and_a_usage_block_per_command() {
        let md = markdown();
        for c in ALL {
            assert!(md.contains(&format!("| `{}` | `{}` |", c.name, c.tool)) && md.contains(&format!("### {}\n", c.name)), "{}", c.name);
        }
    }
}
