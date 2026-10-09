//! Local web UI. It is a thin client over the same `Session` the MCP server uses, so a human
//! and an agent edit ONE live scene: the page polls the session revision and redraws when the
//! agent changes anything, and every edit made in the page is visible to the agent.
//! Loopback only; there is no remote access.

use crate::mcp::{Out, Session};
use pt_render::{Mode, Opts};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const INDEX: &str = include_str!("../ui/index.html");

fn query(url: &str) -> (String, std::collections::HashMap<String, String>) {
    let (path, q) = url.split_once('?').unwrap_or((url, ""));
    let mut m = std::collections::HashMap::new();
    for kv in q.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        m.insert(k.to_string(), url_decode(v));
    }
    (path.to_string(), m)
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn call_json(s: &mut Session, name: &str, args: &Value) -> Value {
    match s.call(name, args) {
        Ok(Out::Json(v)) => json!({"ok": true, "result": v}),
        Ok(Out::Image { png, meta }) => json!({"ok": true, "image_base64": pt_core::io::base64_encode(&png), "meta": meta}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

fn state_json(s: &mut Session) -> Value {
    let snap = s.graph.snapshot();
    let rev = blake3::hash(snap.as_bytes()).to_hex().to_string();
    let diags = s.graph.check().diags;
    json!({
        "rev": &rev[..16],
        "frame": s.graph.frame,
        "nodes": s.graph.nodes,
        "assets": s.graph.assets.keys().collect::<Vec<_>>(),
        "diagnostics": diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
    })
}

fn geo_json(s: &mut Session, node: &str, n: usize) -> Value {
    let g = match s.graph.cook(node) {
        Ok(g) => g,
        Err(e) => return json!({"error": e}),
    };
    let mut cols: Vec<String> = vec!["#".into(), "P.x".into(), "P.y".into(), "P.z".into()];
    let mut getters: Vec<(String, usize)> = vec![];
    for (name, c) in g.point.iter() {
        if name == "P" || name.starts_with("__") {
            continue;
        }
        match c {
            pt_core::Column::F32(_) => {
                cols.push(name.clone());
                getters.push((name.clone(), 0));
            }
            pt_core::Column::V3(_) => {
                for (k, ax) in ["x", "y", "z"].iter().enumerate() {
                    cols.push(format!("{name}.{ax}"));
                    getters.push((name.clone(), k + 1));
                }
            }
            _ => {}
        }
    }
    let p = g.p();
    let rows: Vec<Vec<Value>> = (0..p.len().min(n))
        .map(|i| {
            let mut r = vec![json!(i), json!(p[i][0]), json!(p[i][1]), json!(p[i][2])];
            for (name, comp) in &getters {
                r.push(match (g.point.get(name), comp) {
                    (Some(pt_core::Column::F32(v)), 0) => json!(v[i]),
                    (Some(pt_core::Column::V3(v)), k) => json!(v[i][k - 1]),
                    _ => Value::Null,
                });
            }
            r
        })
        .collect();
    json!({"points": p.len(), "instances": g.n_instances(), "primitives": g.n_prims(), "columns": cols, "rows": rows})
}

fn render_png(s: &mut Session, q: &std::collections::HashMap<String, String>) -> Result<Vec<u8>, String> {
    let node = q.get("node").ok_or("missing node")?;
    let geo = s.graph.cook(node)?;
    let num = |k: &str, d: f32| q.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    let (w, h) = (num("w", 720.0).clamp(64.0, 1920.0) as usize, num("h", 520.0).clamp(64.0, 1440.0) as usize);
    let mode = match q.get("mode").map(String::as_str).unwrap_or("shaded") {
        "shaded" => Mode::Shaded,
        "wire" => Mode::Wire,
        other => Mode::Attr(other.to_string()),
    };
    let (az, el) = (num("az", 35.0), num("el", 28.0));
    let img = if q.get("pt").map(String::as_str) == Some("1") {
        let o = pt_render::pathtrace::PtOpts { width: w, height: h, spp: num("spp", 24.0).clamp(1.0, 128.0) as usize, azimuth_deg: az, elevation_deg: el.min(80.0), mode, ..Default::default() };
        pt_render::pathtrace::pathtrace(&geo, &o).0
    } else {
        pt_render::render(&geo, &Opts { width: w, height: h, mode, azimuth_deg: az, elevation_deg: el, ..Default::default() }).0
    };
    pt_render::encode_png(&img)
}

pub fn serve_ui(session: Arc<Mutex<Session>>, addr: &str) -> Result<std::thread::JoinHandle<()>, String> {
    if !addr.starts_with("127.0.0.1") && !addr.starts_with("localhost") {
        return Err("the UI binds to loopback only (use 127.0.0.1:PORT)".into());
    }
    let server = tiny_http::Server::http(addr).map_err(|e| format!("cannot bind {addr}: {e}"))?;
    Ok(std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            let method = req.method().clone();
            let (path, q) = query(&url);
            let json_resp = |v: Value| tiny_http::Response::from_string(v.to_string()).with_header("Content-Type: application/json".parse::<tiny_http::Header>().unwrap());
            let resp: tiny_http::ResponseBox = match (method.as_str(), path.as_str()) {
                ("GET", "/") | ("GET", "/index.html") => tiny_http::Response::from_string(INDEX).with_header("Content-Type: text/html; charset=utf-8".parse::<tiny_http::Header>().unwrap()).boxed(),
                ("GET", "/api/state") => json_resp(state_json(&mut session.lock().unwrap())).boxed(),
                ("GET", "/api/schemas") => json_resp(json!(pt_graph::all_schemas().iter().map(|s| s.to_json()).collect::<Vec<_>>())).boxed(),
                ("GET", "/api/geo") => {
                    let n = q.get("n").and_then(|v| v.parse().ok()).unwrap_or(100usize).min(2000);
                    json_resp(geo_json(&mut session.lock().unwrap(), q.get("node").map(String::as_str).unwrap_or(""), n)).boxed()
                }
                ("GET", "/api/render") => match render_png(&mut session.lock().unwrap(), &q) {
                    Ok(png) => tiny_http::Response::from_data(png).with_header("Content-Type: image/png".parse::<tiny_http::Header>().unwrap()).boxed(),
                    Err(e) => json_resp(json!({"error": e})).with_status_code(400).boxed(),
                },
                ("POST", "/api/call") => {
                    let mut body = String::new();
                    let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                    match serde_json::from_str::<Value>(&body) {
                        Ok(v) => {
                            let name = v["name"].as_str().unwrap_or("").to_string();
                            json_resp(call_json(&mut session.lock().unwrap(), &name, &v["arguments"])).boxed()
                        }
                        Err(e) => json_resp(json!({"ok": false, "error": format!("bad JSON: {e}")})).with_status_code(400).boxed(),
                    }
                }
                _ => tiny_http::Response::from_string("not found").with_status_code(404).boxed(),
            };
            let _ = req.respond(resp);
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_decoding_and_queries() {
        assert_eq!(url_decode("a%20b+c%2Fd"), "a b c/d");
        let (p, q) = query("/api/render?node=hills&az=45&mode=Cd");
        assert_eq!(p, "/api/render");
        assert_eq!(q["node"], "hills");
        assert_eq!(q["az"], "45");
    }

    #[test]
    fn ui_and_agent_share_one_session() {
        let session = Arc::new(Mutex::new(Session::new()));
        {
            let mut s = session.lock().unwrap();
            let r = call_json(&mut s, "run_hcs", &json!({"code": "g = grid(size=[4,4])\nout(g)\n"}));
            assert_eq!(r["ok"], true);
        }
        let st1 = state_json(&mut session.lock().unwrap());
        // The "agent" changes something; the UI sees a new revision.
        call_json(&mut session.lock().unwrap(), "set_param", &json!({"node": "g", "name": "rows", "value": 3}));
        let st2 = state_json(&mut session.lock().unwrap());
        assert_ne!(st1["rev"], st2["rev"]);
        assert_eq!(st2["nodes"]["g"]["params"]["rows"], json!(3));
        let geo = geo_json(&mut session.lock().unwrap(), "g", 5);
        assert_eq!(geo["rows"].as_array().unwrap().len(), 5);
        let mut q = std::collections::HashMap::new();
        q.insert("node".to_string(), "g".to_string());
        assert!(render_png(&mut session.lock().unwrap(), &q).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
    }
}
