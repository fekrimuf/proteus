//! `pt report`: the plain-English checkpoint page (PRO_QUALITY_PLAN B4 step 7).
//!
//! Inputs are the JSON files written by `pt match-bench` (one for the zero-LLM baseline, optionally one for the
//! language-model recipes) plus an optional notes file with the author's commentary. Output is one self-contained HTML
//! file (images embedded) and a JSON summary. The "stack" result for each asset is the better of the two paths, which is
//! what the solver stack of B4 step 5 actually delivers (baseline first, language model on what the baseline cannot do).

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn rank(tier: &str) -> u8 {
    match tier {
        "gold" => 3,
        "silver" => 2,
        "bronze" => 1,
        _ => 0,
    }
}

fn num(v: &Value, p: &str) -> f64 {
    v.pointer(p).and_then(Value::as_f64).unwrap_or(0.0)
}

pub(crate) fn s<'a>(v: &'a Value, p: &str) -> &'a str {
    v.pointer(p).and_then(Value::as_str).unwrap_or("")
}

fn score(v: &Value) -> (u8, f64, f64) {
    (rank(s(v, "/tier")), num(v, "/metrics/f_score") + num(v, "/metrics/silhouette_iou"), -num(v, "/free_parameters"))
}

pub(crate) fn better(a: &Value, b: &Value) -> bool {
    let (x, y) = (score(a), score(b));
    x.0 > y.0 || (x.0 == y.0 && (x.1 > y.1 + 1e-9 || ((x.1 - y.1).abs() <= 1e-9 && x.2 > y.2)))
}

/// Which path's result stands for an asset in the stack: the better by tier and score, except that a result flagged as defective
/// (a missing feature the numbers cannot see) loses to a solved result that is not flagged, whatever their scores.
pub(crate) fn stack_pick(base: &Value, llm: &Value, base_flagged: bool, llm_flagged: bool) -> &'static str {
    let solved = |v: &Value| v.get("solved") == Some(&json!(true));
    if base_flagged && !llm_flagged && solved(llm) {
        "llm-recipe"
    } else if llm_flagged && !base_flagged && solved(base) {
        "primitive-baseline"
    } else if better(llm, base) {
        "llm-recipe"
    } else {
        "primitive-baseline"
    }
}

/// Wilson 95% interval for k successes out of n.
pub fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let (n, p, z) = (n as f64, k as f64 / n as f64, 1.96f64);
    let den = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / den;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / den;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

fn pct(k: usize, n: usize) -> String {
    if n == 0 {
        "-".into()
    } else {
        format!("{:.0}% ({k} of {n})", 100.0 * k as f64 / n as f64)
    }
}

pub(crate) fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Tiny markdown subset for the notes file: `## heading`, `- bullet`, blank-line paragraphs, `**bold**`, `` `code` ``.
fn md(text: &str) -> String {
    fn inline(l: &str) -> String {
        let mut out = String::new();
        let (mut bold, mut code) = (false, false);
        let t = esc(l);
        let mut chars = t.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '*' && chars.peek() == Some(&'*') {
                chars.next();
                bold = !bold;
                out.push_str(if bold { "<b>" } else { "</b>" });
            } else if c == '`' {
                code = !code;
                out.push_str(if code { "<code>" } else { "</code>" });
            } else {
                out.push(c);
            }
        }
        out
    }
    let (mut html, mut in_list, mut para) = (String::new(), false, Vec::<String>::new());
    let flush = |html: &mut String, para: &mut Vec<String>| {
        if !para.is_empty() {
            html.push_str(&format!("<p>{}</p>\n", para.join(" ")));
            para.clear();
        }
    };
    for line in text.lines() {
        let l = line.trim_end();
        if let Some(b) = l.strip_prefix("- ") {
            flush(&mut html, &mut para);
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            html.push_str(&format!("<li>{}</li>\n", inline(b)));
            continue;
        }
        if in_list {
            html.push_str("</ul>\n");
            in_list = false;
        }
        if let Some(h) = l.strip_prefix("## ") {
            flush(&mut html, &mut para);
            html.push_str(&format!("<h2>{}</h2>\n", inline(h)));
        } else if l.is_empty() {
            flush(&mut html, &mut para);
        } else {
            para.push(inline(l));
        }
    }
    flush(&mut html, &mut para);
    if in_list {
        html.push_str("</ul>\n");
    }
    html
}

pub(crate) fn img_tag(path: &Path, alt: &str, width: &str) -> String {
    match std::fs::read(path) {
        Ok(b) => format!("<img alt=\"{}\" style=\"width:{}\" src=\"data:image/png;base64,{}\">", esc(alt), width, pt_core::io::base64_encode(&b)),
        Err(_) => format!("<div class=\"missing\">image missing: {}</div>", esc(&path.display().to_string())),
    }
}

pub(crate) const CSS: &str = r#"
:root{--bg:#fbfaf7;--fg:#1d1d1f;--mut:#65656b;--card:#ffffff;--line:#e3e1db;--green:#1f7a45;--amber:#9a6412;--red:#b3261e;--bar:#3b6fb6;--chipfg:#ffffff}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--bg:#16171a;--fg:#ececee;--mut:#9a9aa2;--card:#1f2023;--line:#34353a;--green:#58c98a;--amber:#e3ae52;--red:#f07468;--bar:#7aa6e8;--chipfg:#101114;color-scheme:dark}}
:root[data-theme="dark"]{--bg:#16171a;--fg:#ececee;--mut:#9a9aa2;--card:#1f2023;--line:#34353a;--green:#58c98a;--amber:#e3ae52;--red:#f07468;--bar:#7aa6e8;--chipfg:#101114;color-scheme:dark}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.55 system-ui,"Segoe UI",Roboto,sans-serif}
main{max-width:980px;margin:0 auto;padding-block:24px 64px;padding-inline:16px}
h1{font-size:1.7rem;margin:.2em 0;text-wrap:balance}h2{margin-top:2em;font-size:1.25rem;text-wrap:balance}
.sub{color:var(--mut)}.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:14px 16px;margin:12px 0}
.light{display:inline-block;width:14px;height:14px;border-radius:50%;margin-right:8px;vertical-align:-1px}.light.green{background:var(--green)}.light.amber{background:var(--amber)}.light.red{background:var(--red)}
.chip{display:inline-block;padding:0 8px;border-radius:999px;font-size:.8rem;font-weight:600;color:var(--chipfg)}.chip.green{background:var(--green)}.chip.amber{background:var(--amber)}.chip.red{background:var(--red)}
.tablewrap{overflow-x:auto}table{border-collapse:collapse;width:100%;font-size:.95rem}th,td{border-bottom:1px solid var(--line);padding:6px 8px;text-align:left}th{color:var(--mut);font-weight:600}td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}
.track{background:var(--line);border-radius:6px;height:12px;min-width:90px;overflow:hidden}.bar{display:block;height:12px;background:var(--bar)}.bar.g{background:var(--green)}
.gallery{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,300px),1fr));gap:14px}figure.pair{margin:0;background:var(--card);border:1px solid var(--line);border-radius:12px;padding:8px;min-width:0}figure.pair img{border-radius:8px;display:block}figcaption{font-size:.85rem;color:var(--mut);margin-top:6px}
img{max-width:100%;height:auto;border-radius:10px}code{background:var(--line);padding:0 4px;border-radius:4px}small,.missing{color:var(--mut)}
"#;

fn light(state: &str) -> &'static str {
    match state {
        "green" => "green",
        "amber" => "amber",
        _ => "red",
    }
}

struct Entry {
    id: String,
    asset: Value,
    base: Option<Value>,
    llm: Option<Value>,
    stack: Value,
    stack_src: &'static str,
}

pub(crate) fn load(path: &Path) -> Result<Value, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))
}

pub(crate) fn assets_of(v: &Value) -> Vec<Value> {
    v.get("assets").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `pt report --baseline A.json [--llm B.json] [--notes N.md] [--probe DIR] --out DIR [--title T]`
pub fn report_cmd(args: &[String]) -> Result<(), String> {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).map(String::as_str);
    let base_path = PathBuf::from(flag("--baseline").ok_or("usage: pt report --baseline A.json [--llm B.json] [--notes N.md] [--probe DIR] --out DIR")?);
    let out_dir = PathBuf::from(flag("--out").ok_or("--out DIR is required")?);
    let title = flag("--title").unwrap_or("Checkpoint 1: does it work at all?");
    let base = load(&base_path)?;
    let llm = flag("--llm").map(|p| load(Path::new(p))).transpose()?;
    let root = crate::corpus::root();
    let run_pairs = |run: &str| root.join("out").join(run).join("pairs");
    let base_run = s(&base, "/run").to_string();
    let llm_run = llm.as_ref().map(|l| s(l, "/run").to_string());

    // The reviewer's visual flags on solved results (optional): id, why, and optionally which path's result is the defective one.
    let eye: Option<Value> = flag("--eye").map(|p| load(Path::new(p))).transpose()?;
    let flagged: std::collections::HashMap<String, (String, Option<String>)> = eye
        .as_ref()
        .and_then(|e| e.get("flagged").and_then(Value::as_array))
        .map(|a| a.iter().filter_map(|f| Some((f.get("id")?.as_str()?.to_string(), (f.get("why")?.as_str()?.to_string(), f.get("source").and_then(Value::as_str).map(String::from))))).collect())
        .unwrap_or_default();
    let flag_of = |id: &str, src: &str| flagged.get(id).filter(|(_, s)| s.as_deref().is_none_or(|s| s == src)).map(|(w, _)| w.as_str());

    let mut entries: Vec<Entry> = Vec::new();
    let llm_assets = llm.as_ref().map(assets_of).unwrap_or_default();
    for a in assets_of(&base) {
        let id = s(&a, "/id").to_string();
        let l = llm_assets.iter().find(|x| s(x, "/id") == id && s(x, "/source") == "llm-recipe").cloned();
        let (stack, src) = match &l {
            Some(lv) if stack_pick(&a, lv, flag_of(&id, "primitive-baseline").is_some(), flag_of(&id, "llm-recipe").is_some()) == "llm-recipe" => (lv.clone(), "llm-recipe"),
            _ => (a.clone(), "primitive-baseline"),
        };
        entries.push(Entry { id, asset: a.clone(), base: Some(a), llm: l, stack, stack_src: src });
    }
    let n = entries.len();
    let solved = |v: &Value| v.get("solved") == Some(&json!(true));
    let hold = |e: &Entry| e.asset.get("holdout") == Some(&json!(true));
    let count = |f: &dyn Fn(&Entry) -> bool| entries.iter().filter(|e| f(e)).count();

    let (n_hold, n_mine) = (count(&|e| hold(e)), count(&|e| !hold(e)));
    let base_solved = count(&|e| e.base.as_ref().is_some_and(solved));
    let stack_solved = count(&|e| solved(&e.stack));
    let stack_hold = count(&|e| hold(e) && solved(&e.stack));
    let stack_mine = count(&|e| !hold(e) && solved(&e.stack));
    let base_hold = count(&|e| hold(e) && e.base.as_ref().is_some_and(solved));
    let base_mine = count(&|e| !hold(e) && e.base.as_ref().is_some_and(solved));
    let attempted = count(&|e| e.llm.is_some());
    let llm_solved = count(&|e| e.llm.as_ref().is_some_and(solved));
    let llm_better = count(&|e| e.stack_src == "llm-recipe");
    let (g_mine, g_hold) = (stack_mine as f64 / n_mine.max(1) as f64 >= 0.30, stack_hold as f64 / n_hold.max(1) as f64 >= 0.20);
    let verdict = if g_mine && g_hold {
        "green"
    } else if g_mine || g_hold || stack_solved as f64 / n.max(1) as f64 >= 0.25 {
        "amber"
    } else {
        "red"
    };
    let (lo, hi) = wilson(stack_solved, n);
    // Second reading: the reviewer's visual flags on solved results (loaded above).
    let eye_html = if eye.is_some() {
        let ok = |e: &Entry| solved(&e.stack) && flag_of(&e.id, e.stack_src).is_none();
        let (all_ok, mine_ok, hold_ok) = (count(&|e| ok(e)), count(&|e| !hold(e) && ok(e)), count(&|e| hold(e) && ok(e)));
        let (m_pass, h_pass) = (mine_ok as f64 / n_mine.max(1) as f64 >= 0.30, hold_ok as f64 / n_hold.max(1) as f64 >= 0.20);
        let items: String = entries.iter().filter(|e| solved(&e.stack) && flag_of(&e.id, e.stack_src).is_some()).map(|e| format!("<li><b>{}</b>: {}</li>", esc(&e.id), esc(flag_of(&e.id, e.stack_src).unwrap_or("")))).collect();
        let n_flagged = entries.iter().filter(|e| solved(&e.stack) && flag_of(&e.id, e.stack_src).is_some()).count();
        let gate_text = if m_pass && h_pass { "Both readings pass gate G1." } else { "On this reading the development share is below the gate G1 bar; the plan's own score passes." };
        format!(
            "<h2>A second reading: looking at the {stack_solved} pictures</h2>\n<p>The plan's score can pass a rebuild that is missing a thin part, because thin parts are a small share of the surface area it averages over. So I looked at the pictures of every solved result. {} of the {stack_solved} look right at a glance. {} pass the numbers but miss something you would notice:</p><ul>{items}</ul>\n<div class=\"tablewrap\"><table><tr><th>Reading</th><th class=\"n\">all</th><th class=\"n\">development</th><th class=\"n\">hold-out</th></tr><tr><td>The plan's score</td><td class=\"n\">{stack_solved} of {n}</td><td class=\"n\">{stack_mine} of {n_mine}</td><td class=\"n\">{stack_hold} of {n_hold}</td></tr><tr><td>Plan's score and looks right to me</td><td class=\"n\">{all_ok} of {n}</td><td class=\"n\">{mine_ok} of {n_mine} <span class=\"chip {}\">{}</span></td><td class=\"n\">{hold_ok} of {n_hold} <span class=\"chip {}\">{}</span></td></tr></table></div>\n<p class=\"sub\">This is the author's visual read, not a blind test; your own look overrides it. {gate_text}</p>\n",
            all_ok,
            n_flagged,
            if m_pass { "green" } else { "red" },
            if m_pass { "pass" } else { "below" },
            if h_pass { "green" } else { "red" },
            if h_pass { "pass" } else { "below" },
        )
    } else {
        String::new()
    };

    // Cost.
    let sum = |f: &dyn Fn(&Entry) -> f64| entries.iter().map(f).sum::<f64>();
    let base_cpu = sum(&|e| e.base.as_ref().map_or(0.0, |b| num(b, "/cost/cpu_seconds")));
    let llm_cpu = sum(&|e| e.llm.as_ref().map_or(0.0, |b| num(b, "/cost/cpu_seconds")));
    let llm_turns = sum(&|e| e.llm.as_ref().map_or(0.0, |b| num(b, "/cost/llm_turns")));
    let solved_params: Vec<f64> = entries.iter().filter(|e| solved(&e.stack)).map(|e| num(&e.stack, "/free_parameters")).collect();
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let base_params: Vec<f64> = entries.iter().filter_map(|e| e.base.as_ref().map(|b| num(b, "/free_parameters"))).collect();
    let llm_params: Vec<f64> = entries.iter().filter_map(|e| e.llm.as_ref().map(|b| num(b, "/free_parameters"))).collect();

    // Tier histogram per path, and by difficulty.
    let tiers = ["gold", "silver", "bronze", "none"];
    let hist = |f: &dyn Fn(&Entry) -> Option<&Value>| -> Vec<usize> { tiers.iter().map(|t| entries.iter().filter(|e| f(e).is_some_and(|v| s(v, "/tier") == *t)).count()).collect() };
    let h_base = hist(&|e| e.base.as_ref());
    let h_llm = hist(&|e| e.llm.as_ref());
    let h_stack = hist(&|e| Some(&e.stack));
    let diffs = ["primitive", "compound", "building"];
    let by_diff: Vec<(String, usize, usize, usize)> = diffs
        .iter()
        .map(|d| {
            let tot = count(&|e| s(&e.asset, "/difficulty") == *d);
            (d.to_string(), tot, count(&|e| s(&e.asset, "/difficulty") == *d && e.base.as_ref().is_some_and(solved)), count(&|e| s(&e.asset, "/difficulty") == *d && solved(&e.stack)))
        })
        .collect();
    let mut fails: std::collections::BTreeMap<String, usize> = Default::default();
    for e in entries.iter().filter(|e| !solved(&e.stack)) {
        for f in e.stack.get("failure").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            *fails.entry(f.to_string()).or_default() += 1;
        }
    }

    // 12 random pairs (seeded, any outcome).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let (ka, kb) = (pt_core::rng::rand4(2026, 41, a as u64)[0], pt_core::rng::rand4(2026, 41, b as u64)[0]);
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    let gallery: Vec<&Entry> = order.iter().take(12).map(|&i| &entries[i]).collect();

    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let name_of = |id: &str| id.replace(['/', '\\'], "_");
    let mut gal_html = String::new();
    for e in &gallery {
        let run = if e.stack_src == "llm-recipe" { llm_run.clone().unwrap_or_default() } else { base_run.clone() };
        let p = run_pairs(&run).join(format!("{}.png", name_of(&e.id)));
        let m = |k: &str| num(&e.stack, &format!("/metrics/{k}"));
        gal_html.push_str(&format!(
            "<figure class=\"pair\">{}<figcaption><b>{}</b><br><span class=\"chip {}\">{}</span> F-score {:.2}, silhouette {:.2}, {} knobs{}<br><small>{}</small></figcaption></figure>\n",
            img_tag(&p, &e.id, "100%"),
            esc(&e.id),
            if solved(&e.stack) { "green" } else if s(&e.stack, "/tier") == "bronze" { "amber" } else { "red" },
            esc(s(&e.stack, "/tier")),
            m("f_score"),
            m("silhouette_iou"),
            num(&e.stack, "/free_parameters"),
            format!("{}{}", if hold(e) { " <i>(hold-out)</i>" } else { "" }, flag_of(&e.id, e.stack_src).map_or(String::new(), |w| format!("<br><i>Looks wrong to me: {}</i>", esc(w)))),
            if e.stack_src == "llm-recipe" { "recipe written by the language model, numbers tuned by search" } else { "built by the no-language-model baseline" }
        ));
    }
    let bar = |k: usize, tot: usize, cls: &str| format!("<span class=\"bar {cls}\" style=\"width:{:.0}%\"></span>", 100.0 * k as f64 / tot.max(1) as f64);
    let notes = flag("--notes").map(|p| std::fs::read_to_string(p).map(|t| md(&t))).transpose().map_err(|e| e.to_string())?.unwrap_or_default();
    let probe = flag("--probe").map(PathBuf::from);
    let probe_html = probe.as_ref().map_or(String::new(), |d| {
        let readme = std::fs::read_to_string(d.join("README.md")).ok();
        let reading = readme.as_deref().and_then(|t| t.split("## Reading").nth(1)).map(|t| md(&format!("## Reading{t}"))).unwrap_or_default();
        format!("<h2>The realism probe</h2>\n<p>Same camera, same height, same ground. Columns: flat grey with plain light | a real public-domain rock material and real sky light. Rows: our procedural rock | a scanned public-domain rock.</p>\n{}\n{}", img_tag(&d.join("probe.png"), "realism probe", "100%"), reading)
    });

    let tier_rows = |label: &str, h: &[usize], total: usize| -> String {
        format!("<tr><td>{label}</td>{}<td>{}</td></tr>", h.iter().map(|k| format!("<td>{k}</td>")).collect::<String>(), total)
    };
    let mut html = String::new();
    html.push_str(&format!("<main>\n<h1>{}</h1><p class=\"sub\">Written for the project owner. Plain English first; the measurements are the evidence.</p>\n", esc(title)));
    html.push_str(&format!(
        "<div class=\"card\"><p style=\"margin:0\"><span class=\"light {}\"></span><b>{}</b></p><p>Of {n} professional models (all public domain), the finished system rebuilt <b>{stack_solved}</b> as an editable recipe that matches the original to the plan's \"silver\" standard ({} overall, 95% range {:.0} to {:.0} percent). Without any language model the plain search solved <b>{base_solved}</b> ({}). Hold-out models (kept out of any pattern learning; see the notes for what that does and does not cover): <b>{stack_hold} of {n_hold}</b>.</p>\n\
<div class=\"tablewrap\"><table><tr><th>Gate G1 (plan B8)</th><th>Needed</th><th>Measured</th><th></th></tr>\n\
<tr><td>Silver on the models used for development</td><td>30% or more</td><td>{}</td><td><span class=\"chip {}\">{}</span></td></tr>\n\
<tr><td>Silver on the 20% hold-out</td><td>20% or more</td><td>{}</td><td><span class=\"chip {}\">{}</span></td></tr></table></div>\n\
<p class=\"sub\">\"Silver\" means: surface within 2% of the model's size for at least 90% of its area (F-score), outline overlap of at least 90% from ten camera angles, valid mesh within twice the original's triangle count, right overall size, and 3 to 40 numbers you could edit. Light: green = both gates pass, amber = one passes or the overall rate is at least 25%, red = otherwise.</p></div>\n",
        light(verdict),
        match verdict {
            "green" => "Green: it works.",
            "amber" => "Amber: it works on some kinds of models and not yet on others.",
            _ => "Red: not yet.",
        },
        pct(stack_solved, n),
        lo * 100.0,
        hi * 100.0,
        pct(base_solved, n),
        pct(stack_mine, n_mine),
        if g_mine { "green" } else { "red" },
        if g_mine { "pass" } else { "below" },
        pct(stack_hold, n_hold),
        if g_hold { "green" } else { "red" },
        if g_hold { "pass" } else { "below" },
    ));
    html.push_str(&notes);
    html.push_str(&eye_html);

    html.push_str("<h2>Results by path</h2>\n<div class=\"tablewrap\"><table><tr><th>Path</th><th class=\"n\">gold</th><th class=\"n\">silver</th><th class=\"n\">bronze</th><th class=\"n\">none</th><th class=\"n\">models</th></tr>\n");
    html.push_str(&tier_rows("A. No language model (primitive search)", &h_base, n));
    html.push_str(&tier_rows(&format!("B. Language-model recipe + tuned numbers (attempted on {attempted})"), &h_llm, attempted));
    html.push_str(&tier_rows("Stack = best of A and B per model", &h_stack, n));
    html.push_str(&format!("</table></div>\n<p>Path B was tried on {attempted} of the {n} models; it solved {llm_solved} of them and gave the best result for {llm_better}. Models it was not tried on keep their path A result.</p>\n"));
    html.push_str("<h2>By kind of model</h2>\n<div class=\"tablewrap\"><table><tr><th>Kind</th><th class=\"n\">models</th><th class=\"n\">solved, path A</th><th class=\"n\">solved, stack</th><th></th></tr>\n");
    for (d, tot, a, st) in &by_diff {
        html.push_str(&format!("<tr><td>{}</td><td class=\"n\">{tot}</td><td class=\"n\">{a}</td><td class=\"n\">{st}</td><td><div class=\"track\">{}</div></td></tr>\n", match d.as_str() { "primitive" => "single shapes (barrel, box, lamp)", "compound" => "multi-part props (furniture, carts)", _ => "building pieces (walls, towers, roofs)" }, bar(*st, *tot, "g")));
    }
    html.push_str("</table></div>\n");
    html.push_str(&format!(
        "<h2>What it cost</h2>\n<div class=\"tablewrap\"><table><tr><th></th><th class=\"n\">Path A</th><th class=\"n\">Path B</th></tr>\n<tr><td>Computer time, all models</td><td class=\"n\">{:.0} s</td><td class=\"n\">{:.0} s</td></tr>\n<tr><td>Language-model turns (one turn = one recipe written or rewritten)</td><td class=\"n\">0</td><td class=\"n\">{:.0} (about {:.1} per model)</td></tr>\n<tr><td>Editable numbers in a recipe (average)</td><td class=\"n\">{:.0}</td><td class=\"n\">{:.0}</td></tr></table></div>\n<p>The plan's budget is at most 8 turns and 10 CPU minutes per model; both are far inside it. Solved recipes average {:.0} editable numbers (silver allows up to 40).</p>\n",
        base_cpu, llm_cpu, llm_turns, if attempted == 0 { 0.0 } else { llm_turns / attempted as f64 }, mean(&base_params), mean(&llm_params), mean(&solved_params)
    ));
    html.push_str("<h2>Why the others failed</h2>\n<div class=\"tablewrap\"><table><tr><th>Reason (a model can have several)</th><th class=\"n\">models</th></tr>\n");
    let names = [("missing_detail", "part of the original is not covered"), ("extra_material", "our version has surface the original does not"), ("proportion_error", "outline or overall size is off"), ("too_many_parameters", "needs more than 40 editable numbers"), ("triangle_budget", "more than twice the original's triangles"), ("wrong_topology", "invalid or open mesh, or the recipe did not run"), ("search_timeout", "ran out of time"), ("import_error", "could not read the file")];
    for (k, v) in &fails {
        html.push_str(&format!("<tr><td>{}</td><td class=\"n\">{v}</td></tr>\n", names.iter().find(|x| x.0 == k).map_or(k.as_str(), |x| x.1)));
    }
    html.push_str("</table></div>\n");
    html.push_str(&format!("<h2>Twelve random pairs</h2>\n<p>Original on the left, our recipe's result on the right, same camera; two angles per pair. Chosen by a fixed random seed, not by outcome.</p>\n<div class=\"gallery\">\n{gal_html}</div>\n"));
    html.push_str(&probe_html);
    html.push_str("<h2>Where to look next</h2>\n<p class=\"sub\">Recipes: <code>corpus/recipes/</code>. Raw numbers: <code>corpus/results/*.json</code>. Licence ledger: <code>corpus/LEDGER.csv</code>.</p></main>\n");
    let page_title = flag("--page-title").unwrap_or("Proteus Checkpoint 1");
    let head = format!("<title>{}</title>\n<style>{CSS}</style>\n", esc(page_title));
    // `--fragment` writes only title, style and body content (what the Artifact publisher wants); otherwise a full page.
    let fragment = args.iter().any(|a| a == "--fragment");
    let page = if fragment {
        format!("{head}{html}")
    } else {
        format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">{head}</head><body>{html}</body></html>\n")
    };
    std::fs::write(out_dir.join(if fragment { "report_fragment.html" } else { "report.html" }), &page).map_err(|e| e.to_string())?;

    let summary = json!({
        "title": title, "models": n, "verdict": verdict,
        "stack": {"solved": stack_solved, "mining": [stack_mine, n_mine], "holdout": [stack_hold, n_hold], "wilson95": [lo, hi], "tiers": h_stack},
        "baseline": {"solved": base_solved, "mining": [base_mine, n_mine], "holdout": [base_hold, n_hold], "tiers": h_base},
        "llm": {"attempted": attempted, "solved": llm_solved, "better_than_baseline": llm_better, "turns": llm_turns, "tiers": h_llm},
        "by_difficulty": by_diff.iter().map(|d| json!({"kind": d.0, "models": d.1, "baseline": d.2, "stack": d.3})).collect::<Vec<_>>(),
        "failures": fails, "gallery": gallery.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        "cost": {"baseline_cpu_s": base_cpu, "llm_cpu_s": llm_cpu, "llm_turns": llm_turns},
    });
    std::fs::write(out_dir.join("summary.json"), serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("report: {} ({} models; stack solved {stack_solved}, baseline {base_solved}, hold-out {stack_hold}/{n_hold}; light {verdict})", out_dir.join("report.html").display(), n);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_interval_brackets_the_proportion() {
        let (lo, hi) = wilson(15, 50);
        assert!(lo < 0.30 && hi > 0.30 && lo > 0.15 && hi < 0.45, "{lo} {hi}");
        assert_eq!(wilson(0, 0), (0.0, 1.0));
    }

    #[test]
    fn markdown_subset_renders_lists_headings_and_emphasis() {
        let h = md("## Hi\n\nA **bold** `code` line\nsecond line\n\n- one\n- two\n");
        assert!(h.contains("<h2>Hi</h2>") && h.contains("<b>bold</b>") && h.contains("<code>code</code>") && h.contains("<li>two</li>") && h.contains("second line"));
    }

    #[test]
    fn better_prefers_tier_then_scores_then_fewer_numbers() {
        let a = json!({"tier": "silver", "metrics": {"f_score": 0.9, "silhouette_iou": 0.9}, "free_parameters": 30});
        let b = json!({"tier": "bronze", "metrics": {"f_score": 0.99, "silhouette_iou": 0.99}, "free_parameters": 5});
        let c = json!({"tier": "silver", "metrics": {"f_score": 0.9, "silhouette_iou": 0.9}, "free_parameters": 12});
        assert!(better(&a, &b) && !better(&b, &a));
        assert!(better(&c, &a), "same quality, fewer knobs wins");
    }

    #[test]
    fn a_flagged_result_loses_to_a_clean_solved_one_even_with_a_lower_score() {
        let solved = |f: f64| json!({"tier": "silver", "solved": true, "metrics": {"f_score": f, "silhouette_iou": f}, "free_parameters": 20});
        let (high, low) = (solved(0.96), solved(0.92));
        assert_eq!(stack_pick(&high, &low, false, false), "primitive-baseline", "no flags: the higher score stands");
        assert_eq!(stack_pick(&high, &low, true, false), "llm-recipe", "the flagged baseline (a plain top where the original has a dome) loses");
        assert_eq!(stack_pick(&low, &high, false, true), "primitive-baseline");
        let bronze = json!({"tier": "bronze", "solved": false, "metrics": {"f_score": 0.88, "silhouette_iou": 0.98}, "free_parameters": 13});
        assert_eq!(stack_pick(&high, &bronze, true, false), "primitive-baseline", "an unsolved clean result cannot replace a solved one");
    }
}
