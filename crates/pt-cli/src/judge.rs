//! Vision judges (PRO_QUALITY_PLAN B15 row 6 and the B16 judge table): an autonomous, free, local judge that looks at
//! renders and answers structured questions, so no human or author-model eye is needed inside a gate.
//!
//! The judge is a vision-language model run by `llama-server` (llama.cpp, MIT) on the local GPU: Qwen3-VL-8B (Apache-2.0),
//! 4-bit, about 6 GB of video memory. Every question is asked with a JSON schema, so the answer is always parseable, at
//! temperature 0. Three things make a small model usable as a measuring instrument:
//!   * **Position swap.** Each pairwise question is asked in both orders. A verdict that flips with the order is a position
//!     artefact and counts as a tie; one that holds counts in full. The disagreement rate is reported as the judge's own
//!     reliability.
//!   * **Calibration before use** (`pt judge critique`, `pt judge audit`, `pt judge selftest`): the judge is scored against
//!     answers that are known by construction (task specs, flagged defects, two briefs), and only then enters a gate.
//!   * **A call cache** keyed by the exact request, so a report re-run is instant and bit-for-bit repeatable.
//!
//! The server is started on the first cache miss and stopped when the `Judge` is dropped. Nothing here spends money.

use pt_render::Image;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub const DEFAULT_SERVER: &str = r"D:\Fekri\tools\llama\bin\llama-server.exe";
pub const DEFAULT_MODEL: &str = r"D:\Fekri\tools\models\qwen3-vl-8b\model-Q4_K_M.gguf";
pub const DEFAULT_MMPROJ: &str = r"D:\Fekri\tools\models\qwen3-vl-8b\mmproj-Q8_0.gguf";
pub const DEFAULT_PORT: u16 = 8089;
/// Name written into every result, so a number is always tied to the judge that produced it.
pub const JUDGE_NAME: &str = "qwen3-vl-8b-q4";
/// Longest image side sent to the model: keeps one call near a second without losing the shapes a judge needs.
const MAX_SIDE: usize = 1024;
/// Context window of the server. At 12288 the 8 GB card was full (7.7 GB); a call holds two images of about 500 tokens each.
const SERVER_CONTEXT: &str = "8192";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Health {
    Ready,
    Loading,
    Down,
}

pub fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Area-average downscale (so thin lines survive, unlike nearest-neighbour). Returns the input when it is already small.
pub fn box_down(img: &Image, max_side: usize) -> Image {
    let big = img.w.max(img.h);
    if big <= max_side {
        return img.clone();
    }
    let (w, h) = ((img.w * max_side / big).max(1), (img.h * max_side / big).max(1));
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let (y0, y1) = (y * img.h / h, ((y + 1) * img.h / h).max(y * img.h / h + 1).min(img.h));
        for x in 0..w {
            let (x0, x1) = (x * img.w / w, ((x + 1) * img.w / w).max(x * img.w / w + 1).min(img.w));
            let mut acc = [0u32; 3];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = (yy * img.w + xx) * 3;
                    for c in 0..3 {
                        acc[c] += img.rgb[i + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            rgb.extend(acc.iter().map(|a| (a / n) as u8));
        }
    }
    Image { w, h, rgb }
}

/// Copy a rectangle out of an image.
pub fn crop(img: &Image, x0: usize, y0: usize, w: usize, h: usize) -> Image {
    let (w, h) = (w.min(img.w.saturating_sub(x0)), h.min(img.h.saturating_sub(y0)));
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in y0..y0 + h {
        rgb.extend_from_slice(&img.rgb[(y * img.w + x0) * 3..(y * img.w + x0 + w) * 3]);
    }
    Image { w, h, rgb }
}

/// Split a `pt match` pair picture (original on the left, our rebuild on the right, two camera angles stacked, 4 px gap)
/// into the two columns.
pub fn split_pair(img: &Image) -> (Image, Image) {
    let w = (img.w.saturating_sub(4)) / 2;
    (crop(img, 0, 0, w, img.h), crop(img, w + 4, 0, w, img.h))
}

/// What one pairwise question came back with, after both orders were asked.
#[derive(Clone, Debug, PartialEq)]
pub struct PairVerdict {
    /// In [-1, 1]: positive favours the FIRST image given. +-1 means both orders agreed; +-0.5 means one order said "same".
    pub margin: f64,
    /// The two orders named opposite images: the judge's answer followed the position, not the content.
    pub contradictory: bool,
    pub reasons: [String; 2],
}

/// Fold the answers from the two orders. `first` is the answer when the images were shown as (a, b), `second` when shown as
/// (b, a); both are +1 (the image shown first wins), -1 (the image shown second wins) or 0 (same).
pub fn combine_orders(first: i32, second: i32) -> (f64, bool) {
    // In the swapped order "image shown first" is b, so a's vote is the negation.
    let sum = first - second;
    let contradictory = first != 0 && second != 0 && first == second;
    (sum as f64 / 2.0, contradictory)
}

/// Score every candidate of a set by its share of pairwise wins (ties and contradictions count half). `pairs` holds
/// `(i, j, margin)` with a positive margin favouring `i`.
pub fn borda(n: usize, pairs: &[(usize, usize, f64)]) -> Vec<f64> {
    let mut s = vec![0.0; n];
    for &(i, j, m) in pairs {
        s[i] += 0.5 + m / 2.0;
        s[j] += 0.5 - m / 2.0;
    }
    s
}

pub fn pair_prompt(brief: &str) -> String {
    format!(
        "You are a strict judge of 3D scenes. The brief was:\n\"{brief}\"\n\nYou are shown two renders of two candidate scenes, \
made from the same camera. The first image is candidate 1 and the second image is candidate 2. Decide which candidate better \
satisfies the brief: all requested parts present, the right shapes, counts, proportions and layout. Ignore colour and lighting \
unless the brief mentions them. First state, in one sentence, the most important visible difference. Then answer \"1\", \"2\", or \
\"same\" if you cannot tell them apart."
    )
}

pub fn rate_prompt(brief: &str) -> String {
    format!(
        "You are a strict judge. The brief was:\n\"{brief}\"\n\nLook at the image. First say in one sentence what you actually see. \
Then score from 0 to 10 how well the image shows what the brief asks for: 0 = nothing like it, 5 = partly, 10 = exactly what was \
asked. Be strict: 10 is rare, and a picture of something else scores 0 to 2."
    )
}

pub fn which_prompt(a: &str, b: &str) -> String {
    format!(
        "Look at the image. Two descriptions follow.\nDescription 1: \"{a}\"\nDescription 2: \"{b}\"\n\nFirst say in one sentence what you \
actually see. Then answer which description fits the image better: \"1\" or \"2\" (answer \"neither\" only if both are clearly wrong)."
    )
}

/// 5x7 glyphs for the labels stamped into composite pictures (A to D for candidates, 1 and 2 for sides, L and R for
/// original and copy). A small model reads a letter in the picture far more reliably than "the second image".
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        _ => [0; 7],
    }
}

/// Stamp one label character, black on a white box, with its top-left corner at (x, y).
pub fn stamp(img: &mut Image, c: char, x: usize, y: usize, scale: usize) {
    let (bw, bh) = (7 * scale, 9 * scale);
    for yy in 0..bh {
        for xx in 0..bw {
            let (px, py) = (x + xx, y + yy);
            if px >= img.w || py >= img.h {
                continue;
            }
            let (gx, gy) = (xx / scale, yy / scale);
            let on = gx >= 1 && gx <= 5 && gy >= 1 && gy <= 7 && glyph(c)[gy - 1] >> (5 - gx) & 1 == 1;
            let v = if on { 0 } else { 255 };
            let i = (py * img.w + px) * 3;
            img.rgb[i..i + 3].copy_from_slice(&[v, v, v]);
        }
    }
}

/// Equal-size images in a grid with a label stamped into the corner of each cell.
pub fn labelled_grid(imgs: &[Image], labels: &[char], cols: usize) -> Image {
    const GAP: usize = 4;
    let mut out = pt_render::grid(imgs, cols, GAP);
    let (w, h) = imgs.first().map_or((1, 1), |i| (i.w, i.h));
    let scale = (h / 36).max(2);
    for (i, &c) in labels.iter().enumerate().take(imgs.len()) {
        stamp(&mut out, c, (i % cols) * (w + GAP) + 6, (i / cols) * (h + GAP) + 6, scale);
    }
    out
}

pub fn sheet_prompt(brief: &str, labels: &[char]) -> String {
    let names = labels.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
    format!(
        "You are a strict judge of 3D scenes. The brief was:\n\"{brief}\"\n\nThe image shows {} candidate scenes, labelled {names} in the \
top-left corner of each, all rendered from the same camera. Score each candidate from 0 to 10 for how fully it satisfies the brief: all \
requested parts present, the right shapes, counts, proportions and layout. Ignore colour and lighting unless the brief mentions them. \
Use the whole range, and give different scores to candidates that differ. Answer with the score of every label.",
        labels.len()
    )
}

pub fn side_by_side_prompt(brief: &str) -> String {
    format!(
        "You are a strict judge of 3D scenes. The brief was:\n\"{brief}\"\n\nThe image shows two candidate scenes side by side, rendered \
from the same camera: the one labelled 1 on the left and the one labelled 2 on the right. Decide which candidate better satisfies the \
brief: all requested parts present, the right shapes, counts, proportions and layout. Ignore colour and lighting unless the brief \
mentions them. First state, in one sentence, the most important visible difference. Then answer \"1\", \"2\", or \"same\"."
    )
}

pub fn structure_prompt() -> String {
    "The image shows one 3D object photographed twice from two angles (top row and bottom row). The column labelled L on the left is one \
object and the column labelled R on the right is another. For EACH of the two columns, count what you can see and answer with closed \
values only: rungs_rings_shelves_or_slats = how many separate repeated horizontal bars, bands, rings, shelves, slats, planks or steps \
(0 if none); legs_posts_or_columns = how many; arms_branches_or_handles = how many; openings_or_holes = how many windows, doors, \
holes or hollow basins; top = the shape of the top (flat, rim_or_lip, battlements, roof, dome_or_cap, or open_hollow). Count each column \
on its own; do not assume they match."
        .into()
}

fn schema_sheet(labels: &[char]) -> Value {
    let props: serde_json::Map<String, Value> = labels.iter().map(|c| (c.to_string(), json!({"type": "integer", "minimum": 0, "maximum": 10}))).collect();
    json!({"type": "object", "properties": props, "required": labels.iter().map(|c| c.to_string()).collect::<Vec<_>>(), "additionalProperties": false})
}

fn schema_structure() -> Value {
    let side = json!({"type": "object", "properties": {
        "rungs_rings_shelves_or_slats": {"type": "integer", "minimum": 0, "maximum": 20},
        "legs_posts_or_columns": {"type": "integer", "minimum": 0, "maximum": 20},
        "arms_branches_or_handles": {"type": "integer", "minimum": 0, "maximum": 20},
        "openings_or_holes": {"type": "integer", "minimum": 0, "maximum": 20},
        "top": {"type": "string", "enum": ["flat", "rim_or_lip", "battlements", "roof", "dome_or_cap", "open_hollow"]}},
        "required": ["rungs_rings_shelves_or_slats", "legs_posts_or_columns", "arms_branches_or_handles", "openings_or_holes", "top"], "additionalProperties": false});
    json!({"type": "object", "properties": {"L": side.clone(), "R": side}, "required": ["L", "R"], "additionalProperties": false})
}

/// Structural differences between two counted descriptions of an object (the judge only counts; this decides). A count differs when
/// it is off by two or more, or when one side has none and the other has some; the top differs when the named shape differs.
pub fn structure_diff(l: &Value, r: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for k in ["rungs_rings_shelves_or_slats", "legs_posts_or_columns", "arms_branches_or_handles", "openings_or_holes"] {
        let (a, b) = (l.get(k).and_then(Value::as_i64).unwrap_or(0), r.get(k).and_then(Value::as_i64).unwrap_or(0));
        if (a - b).abs() >= 2 || (a != b && a.min(b) == 0) {
            out.push(format!("{}: original {a}, copy {b}", k.replace('_', " ")));
        }
    }
    let (a, b) = (l.get("top").and_then(Value::as_str).unwrap_or(""), r.get("top").and_then(Value::as_str).unwrap_or(""));
    if a != b {
        out.push(format!("top: original {a}, copy {b}"));
    }
    out
}

fn schema_pair() -> Value {
    json!({"type": "object", "properties": {"difference": {"type": "string", "maxLength": 240}, "better": {"type": "string", "enum": ["1", "2", "same"]}}, "required": ["difference", "better"], "additionalProperties": false})
}
fn schema_rate() -> Value {
    json!({"type": "object", "properties": {"seen": {"type": "string", "maxLength": 240}, "score": {"type": "integer", "minimum": 0, "maximum": 10}}, "required": ["seen", "score"], "additionalProperties": false})
}
fn schema_which() -> Value {
    json!({"type": "object", "properties": {"seen": {"type": "string", "maxLength": 240}, "fits": {"type": "string", "enum": ["1", "2", "neither"]}}, "required": ["seen", "fits"], "additionalProperties": false})
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// A connection to the local judge model. The server starts on the first request that is not in the cache.
pub struct Judge {
    port: u16,
    may_spawn: bool,
    child: RefCell<Option<Child>>,
    cache: PathBuf,
    pub calls: Cell<u64>,
    pub cache_hits: Cell<u64>,
    seq: Cell<u64>,
    ready: Cell<bool>,
}

impl Judge {
    /// The default judge: local server on `PT_JUDGE_PORT` (8089), started when needed, cache in `out/judge_cache`.
    pub fn local() -> Judge {
        let port = std::env::var("PT_JUDGE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
        Judge::new(port, true, crate::blender::repo_root().join("out").join("judge_cache"))
    }

    pub fn new(port: u16, may_spawn: bool, cache: PathBuf) -> Judge {
        Judge { port, may_spawn, child: RefCell::new(None), cache, calls: Cell::new(0), cache_hits: Cell::new(0), seq: Cell::new(0), ready: Cell::new(false) }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn health(&self) -> Health {
        let body = Command::new("curl").args(["-s", "-m", "3"]).arg(self.url("/health")).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        if body.contains("\"ok\"") {
            Health::Ready
        } else if body.contains("Loading model") {
            Health::Loading
        } else {
            Health::Down
        }
    }

    /// Wait for the model to finish loading (a server that is still loading must not get a second one started on its port).
    fn wait_ready(&self, log_path: &Path) -> Result<(), String> {
        for _ in 0..300 {
            if let Some(c) = self.child.borrow_mut().as_mut() {
                if let Ok(Some(st)) = c.try_wait() {
                    let tail: Vec<String> = std::fs::read_to_string(log_path).unwrap_or_default().lines().rev().take(6).map(String::from).collect();
                    return Err(format!("the judge server stopped ({st}); last log lines: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" | ")));
                }
            }
            if self.health() == Health::Ready {
                self.ready.set(true);
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        Err("the judge server did not become ready in 5 minutes".into())
    }

    /// Make sure a server answers: reuse one that is running (waiting if it is still loading), otherwise start one.
    pub fn ensure_server(&self) -> Result<(), String> {
        if self.ready.get() {
            return Ok(());
        }
        match self.health() {
            Health::Ready => {
                self.ready.set(true);
                return Ok(());
            }
            Health::Loading => return self.wait_ready(&self.cache.join("server.log")),
            Health::Down => {}
        }
        if !self.may_spawn {
            return Err(format!("no judge server on port {}", self.port));
        }
        let (exe, model, mmproj) = (env_or("PT_JUDGE_SERVER", DEFAULT_SERVER), env_or("PT_JUDGE_MODEL", DEFAULT_MODEL), env_or("PT_JUDGE_MMPROJ", DEFAULT_MMPROJ));
        for (what, p) in [("server", &exe), ("model", &model), ("vision projector", &mmproj)] {
            if !Path::new(p).exists() {
                return Err(format!("judge {what} not found at {p} (see docs/AGENT_GUIDE.md, 'Vision judges')"));
            }
        }
        std::fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
        let log_path = self.cache.join("server.log");
        let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let mut server = Command::new(&exe);
        server
            .args(["-m", &model, "--mmproj", &mmproj, "-ngl", "99", "-c", SERVER_CONTEXT, "-np", "1", "--host", "127.0.0.1", "--port", &self.port.to_string(), "--temp", "0", "--seed", "1"])
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log));
        // low priority, and killed when pt exits, so no judge server is left holding the GPU
        let child = crate::lowload::spawn_uncapped(&mut server).map_err(|e| format!("could not start the judge server: {e}"))?;
        *self.child.borrow_mut() = Some(child);
        self.wait_ready(&log_path)
    }

    /// One structured question about one or more PNG images. Answers are cached by the exact request.
    pub fn ask(&self, images: &[&[u8]], prompt: &str, schema: &Value, max_tokens: u32) -> Result<Value, String> {
        let mut content: Vec<Value> = Vec::new();
        for png in images {
            let img = pt_render::decode_png(png)?;
            let small = pt_render::encode_png(&box_down(&img, MAX_SIDE))?;
            content.push(json!({"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64(&small))}}));
        }
        content.push(json!({"type": "text", "text": prompt}));
        let body = json!({
            "model": JUDGE_NAME,
            "messages": [{"role": "user", "content": content}],
            "temperature": 0, "seed": 1, "max_tokens": max_tokens,
            "response_format": {"type": "json_schema", "json_schema": {"name": "answer", "strict": true, "schema": schema}},
        });
        let text = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let key = blake3::hash(text.as_bytes()).to_hex().to_string();
        let cached = self.cache.join(format!("{key}.json"));
        if let Some(v) = std::fs::read_to_string(&cached).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            self.cache_hits.set(self.cache_hits.get() + 1);
            return Ok(v);
        }
        self.ensure_server()?;
        std::fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
        self.seq.set(self.seq.get() + 1);
        let tmp = self.cache.join(format!("req_{}_{}.json", std::process::id(), self.seq.get()));
        std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
        let out = Command::new("curl")
            .args(["-s", "-m", "600", "-H", "Content-Type: application/json", "--data-binary"])
            .arg(format!("@{}", tmp.display()))
            .arg(self.url("/v1/chat/completions"))
            .output();
        let _ = std::fs::remove_file(&tmp);
        let out = out.map_err(|e| format!("could not run curl: {e}"))?;
        self.calls.set(self.calls.get() + 1);
        let resp: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("judge reply is not JSON ({e}): {}", String::from_utf8_lossy(&out.stdout).chars().take(200).collect::<String>()))?;
        if let Some(err) = resp.get("error") {
            return Err(format!("judge error: {err}"));
        }
        let msg = resp.pointer("/choices/0/message/content").and_then(Value::as_str).ok_or("judge reply has no content")?;
        let answer: Value = serde_json::from_str(msg).map_err(|e| format!("judge answer is not JSON ({e}): {msg}"))?;
        let _ = std::fs::write(&cached, serde_json::to_string(&answer).unwrap_or_default());
        Ok(answer)
    }

    fn vote(answer: &Value, key: &str) -> i32 {
        match answer.get(key).and_then(Value::as_str) {
            Some("1") => 1,
            Some("2") => -1,
            _ => 0,
        }
    }

    /// Which of two renders satisfies the brief better, asked in both orders. Positive margin favours `a`.
    pub fn compare(&self, brief: &str, a: &[u8], b: &[u8]) -> Result<PairVerdict, String> {
        let p = pair_prompt(brief);
        let r1 = self.ask(&[a, b], &p, &schema_pair(), 160)?;
        let r2 = self.ask(&[b, a], &p, &schema_pair(), 160)?;
        let (margin, contradictory) = combine_orders(Self::vote(&r1, "better"), Self::vote(&r2, "better"));
        let text = |r: &Value| r.get("difference").and_then(Value::as_str).unwrap_or("").to_string();
        Ok(PairVerdict { margin, contradictory, reasons: [text(&r1), text(&r2)] })
    }

    /// Score one image against a brief, 0 to 10, with what the judge says it sees.
    pub fn rate(&self, brief: &str, img: &[u8]) -> Result<(f64, String), String> {
        let r = self.ask(&[img], &rate_prompt(brief), &schema_rate(), 160)?;
        Ok((r.get("score").and_then(Value::as_f64).unwrap_or(0.0), r.get("seen").and_then(Value::as_str).unwrap_or("").to_string()))
    }

    /// Which of two descriptions the image fits better, asked in both orders. Positive margin favours `a`.
    pub fn which_brief(&self, img: &[u8], a: &str, b: &str) -> Result<PairVerdict, String> {
        let r1 = self.ask(&[img], &which_prompt(a, b), &schema_which(), 160)?;
        let r2 = self.ask(&[img], &which_prompt(b, a), &schema_which(), 160)?;
        let (margin, contradictory) = combine_orders(Self::vote(&r1, "fits"), Self::vote(&r2, "fits"));
        let text = |r: &Value| r.get("seen").and_then(Value::as_str).unwrap_or("").to_string();
        Ok(PairVerdict { margin, contradictory, reasons: [text(&r1), text(&r2)] })
    }

    /// Score a set of candidates drawn into one labelled sheet (0 to 10 each). Asked twice, with the candidates in reverse order on
    /// the second sheet, and averaged, so a favourite position cannot decide the ranking.
    pub fn rank_sheet(&self, brief: &str, imgs: &[Image]) -> Result<Vec<f64>, String> {
        let labels: Vec<char> = "ABCD".chars().take(imgs.len()).collect();
        let n = imgs.len();
        let mut total = vec![0.0; n];
        for reversed in [false, true] {
            let order: Vec<usize> = if reversed { (0..n).rev().collect() } else { (0..n).collect() };
            let shown: Vec<Image> = order.iter().map(|&i| imgs[i].clone()).collect();
            let png = pt_render::encode_png(&labelled_grid(&shown, &labels, 2))?;
            let r = self.ask(&[&png], &sheet_prompt(brief, &labels), &schema_sheet(&labels), 80)?;
            for (slot, &cand) in order.iter().enumerate() {
                total[cand] += r.get(labels[slot].to_string()).and_then(Value::as_f64).unwrap_or(0.0) / 2.0;
            }
        }
        Ok(total)
    }

    /// Which of two renders satisfies the brief better, shown side by side in ONE picture with stamped labels, asked in both
    /// orders. Positive margin favours `a`.
    pub fn compare_side_by_side(&self, brief: &str, a: &Image, b: &Image) -> Result<PairVerdict, String> {
        let p = side_by_side_prompt(brief);
        let mut answers = Vec::new();
        for (x, y) in [(a, b), (b, a)] {
            let png = pt_render::encode_png(&labelled_grid(&[x.clone(), y.clone()], &['1', '2'], 2))?;
            answers.push(self.ask(&[&png], &p, &schema_pair(), 160)?);
        }
        let (margin, contradictory) = combine_orders(Self::vote(&answers[0], "better"), Self::vote(&answers[1], "better"));
        let text = |r: &Value| r.get("difference").and_then(Value::as_str).unwrap_or("").to_string();
        Ok(PairVerdict { margin, contradictory, reasons: [text(&answers[0]), text(&answers[1])] })
    }

    /// Count the structure of the original (left column) and the copy (right column) of a `pt match` pair picture, and name the
    /// differences. The judge only counts; `structure_diff` decides, so a leading question cannot invent a defect.
    pub fn structure(&self, pair: &Image) -> Result<(Vec<String>, Value), String> {
        let (l, r) = split_pair(pair);
        let png = pt_render::encode_png(&labelled_grid(&[l, r], &['L', 'R'], 2))?;
        let a = self.ask(&[&png], &structure_prompt(), &schema_structure(), 300)?;
        Ok((structure_diff(&a["L"], &a["R"]), a))
    }
}

impl Drop for Judge {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.borrow_mut().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, v: [u8; 3]) -> Image {
        Image { w, h, rgb: (0..w * h).flat_map(|_| v).collect() }
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(b64(i.as_bytes()), o);
        }
    }

    #[test]
    fn both_orders_agreeing_is_full_weight_and_a_flip_is_a_contradiction() {
        // a wins both times: shown first +1, then shown second -1
        assert_eq!(combine_orders(1, -1), (1.0, false));
        assert_eq!(combine_orders(-1, 1), (-1.0, false));
        // always answers "1" whatever the order: position bias, a tie that is flagged
        assert_eq!(combine_orders(1, 1), (0.0, true));
        assert_eq!(combine_orders(-1, -1), (0.0, true));
        // one order says same: half weight, not contradictory
        assert_eq!(combine_orders(1, 0), (0.5, false));
        assert_eq!(combine_orders(0, 0), (0.0, false));
    }

    #[test]
    fn borda_gives_the_clear_winner_the_top_score() {
        let s = borda(3, &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 0.5)]);
        assert_eq!(s, vec![2.0, 0.75, 0.25]);
        assert!((s.iter().sum::<f64>() - 3.0).abs() < 1e-12, "every pair hands out exactly one point");
    }

    #[test]
    fn box_down_averages_and_keeps_aspect() {
        let mut img = solid(8, 4, [0, 0, 0]);
        for y in 0..4 {
            for x in 0..4 {
                img.rgb[(y * 8 + x) * 3..(y * 8 + x) * 3 + 3].copy_from_slice(&[200, 100, 50]);
            }
        }
        let d = box_down(&img, 4);
        assert_eq!((d.w, d.h), (4, 2));
        assert_eq!(&d.rgb[0..3], &[200, 100, 50]);
        assert_eq!(&d.rgb[(3) * 3..(3) * 3 + 3], &[0, 0, 0]);
        assert_eq!(box_down(&img, 64).w, 8, "small images pass through");
    }

    #[test]
    fn a_pair_picture_splits_into_original_and_rebuild() {
        let a = solid(10, 6, [10, 10, 10]);
        let b = solid(10, 6, [250, 250, 250]);
        let sheet = pt_render::grid(&[a.clone(), b.clone(), a.clone(), b.clone()], 2, 4);
        let (l, r) = split_pair(&sheet);
        assert_eq!((l.w, l.h, r.w, r.h), (10, 16, 10, 16));
        assert!(l.rgb.chunks(3).all(|p| p[0] == 10 || p[0] == 255) && r.rgb.chunks(3).all(|p| p[0] == 250 || p[0] == 255));
    }

    #[test]
    fn labels_are_stamped_into_each_cell_of_the_grid() {
        let cell = |v: u8| solid(64, 64, [v, v, v]);
        let g = labelled_grid(&[cell(100), cell(100)], &['A', 'B'], 2);
        assert_eq!((g.w, g.h), (132, 64));
        let black_in = |x0: usize, x1: usize| (0..64).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| g.rgb[(y * g.w + x) * 3] == 0).count();
        assert!(black_in(0, 40) > 10 && black_in(68, 110) > 10, "a glyph in each cell");
        assert_eq!(black_in(46, 64), 0, "nothing else is drawn");
        // different letters leave different marks
        let (mut a, mut b) = (cell(100), cell(100));
        stamp(&mut a, 'A', 0, 0, 2);
        stamp(&mut b, 'B', 0, 0, 2);
        assert_ne!(a.rgb, b.rgb);
    }

    #[test]
    fn structure_diff_flags_counts_and_tops_but_not_noise() {
        let side = |rungs: i64, top: &str| json!({"rungs_rings_shelves_or_slats": rungs, "legs_posts_or_columns": 2, "arms_branches_or_handles": 0, "openings_or_holes": 0, "top": top});
        assert!(structure_diff(&side(8, "flat"), &side(8, "flat")).is_empty());
        assert!(structure_diff(&side(8, "flat"), &side(7, "flat")).is_empty(), "off by one is counting noise");
        assert_eq!(structure_diff(&side(8, "flat"), &side(4, "flat")).len(), 1, "eight rungs against four");
        assert_eq!(structure_diff(&side(3, "flat"), &side(0, "flat")).len(), 1, "rings against none");
        assert_eq!(structure_diff(&side(0, "battlements"), &side(0, "flat")).len(), 1, "battlements against a plain top");
    }

    #[test]
    fn prompts_carry_the_brief_and_the_schemas_are_closed() {
        assert!(sheet_prompt("a fence", &['A', 'B', 'C']).contains("labelled A, B, C"));
        assert_eq!(schema_sheet(&['A', 'B'])["required"], json!(["A", "B"]));
        assert!(side_by_side_prompt("a fence").contains("a fence") && structure_prompt().contains("labelled L"));
        assert!(pair_prompt("a fence with 8 posts").contains("a fence with 8 posts"));
        assert!(which_prompt("alpha", "beta").contains("Description 2: \"beta\""));
        for s in [schema_pair(), schema_rate(), schema_which(), schema_structure()] {
            assert_eq!(s["additionalProperties"], json!(false));
        }
    }

    /// The whole request path (image preparation, cache, curl, parsing, order swap) against a stand-in server that
    /// always answers "the first image wins": the judge must call it twice, mark the pair contradictory, and then
    /// answer from the cache without a third call.
    #[test]
    fn requests_are_swapped_parsed_and_cached() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h2 = hits.clone();
        let t = std::thread::spawn(move || {
            while let Ok(Some(mut req)) = server.recv_timeout(std::time::Duration::from_secs(20)) {
                if req.url() == "/health" {
                    let _ = req.respond(tiny_http::Response::from_string("{\"status\":\"ok\"}"));
                    continue;
                }
                let mut body = String::new();
                req.as_reader().read_to_string(&mut body).unwrap();
                let v: Value = serde_json::from_str(&body).unwrap();
                let content = v.pointer("/messages/0/content").and_then(Value::as_array).unwrap();
                assert_eq!(content.len(), 3, "two images then the prompt");
                assert_eq!(content[0]["type"], json!("image_url"));
                assert_eq!(content[2]["type"], json!("text"));
                assert_eq!(v["temperature"], json!(0));
                h2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let reply = json!({"choices": [{"message": {"content": "{\"difference\": \"x\", \"better\": \"1\"}"}}]});
                let _ = req.respond(tiny_http::Response::from_string(reply.to_string()));
                if h2.load(std::sync::atomic::Ordering::SeqCst) >= 2 {
                    break;
                }
            }
        });
        let dir = std::env::temp_dir().join(format!("pt_judge_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let j = Judge::new(port, false, dir.clone());
        let a = pt_render::encode_png(&solid(16, 16, [200, 30, 30])).unwrap();
        let b = pt_render::encode_png(&solid(16, 16, [30, 30, 200])).unwrap();
        let v = j.compare("brief", &a, &b);
        t.join().unwrap();
        let v = v.expect("request path works");
        assert!(v.contradictory && v.margin == 0.0, "a judge that always says '1' is position-biased: {v:?}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(j.calls.get(), 2);
        // second time: both answers come from the cache, the server is gone
        let again = j.compare("brief", &a, &b).unwrap();
        assert_eq!(again, v);
        assert_eq!(j.cache_hits.get(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
