use pt_cli::{corpus, demos, gym, matchcmd, mcp, recipes, report, top};

use pt_graph::{hex, Graph};
use serde_json::json;
use std::time::Instant;

/// Terrain with scattered pillars: grid -> transform -> noise -> scatter -> copy_to_points.
pub fn demo_graph(grid_res: i64, count: i64) -> Result<Graph, String> {
    let mut g = Graph::new();
    g.create("grid", Some("terrain"))?;
    g.set_param("terrain", "size", json!([20.0, 20.0]))?;
    g.set_param("terrain", "rows", json!(grid_res))?;
    g.set_param("terrain", "cols", json!(grid_res))?;
    g.create("noise", Some("hills"))?;
    g.set_param("hills", "amp", json!(2.0))?;
    g.set_param("hills", "freq", json!(0.15))?;
    g.connect("terrain", "hills", 0)?;
    g.create("color_by_height", Some("tint"))?;
    g.connect("hills", "tint", 0)?;
    g.create("scatter", Some("seeds"))?;
    g.set_param("seeds", "count", json!(count))?;
    g.set_param("seeds", "pscale_min", json!(0.3))?;
    g.set_param("seeds", "pscale_max", json!(1.0))?;
    g.connect("hills", "seeds", 0)?;
    g.create("box", Some("pillar"))?;
    g.set_param("pillar", "size", json!([0.3, 1.5, 0.3]))?;
    g.set_param("pillar", "center", json!([0.0, 0.75, 0.0]))?;
    g.create("copy_to_points", Some("pillars"))?;
    g.set_param("pillars", "rot_y_random", json!(360.0))?;
    g.set_param("pillars", "orient_to_normal", json!(1))?;
    g.connect("pillar", "pillars", 0)?;
    g.connect("seeds", "pillars", 1)?;
    g.create("merge", Some("result"))?;
    g.connect("tint", "result", 0)?;
    g.connect("pillars", "result", 1)?;
    Ok(g)
}

fn demo(out_dir: &str) -> Result<(), String> {
    let mut g = demo_graph(80, 200)?;
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let geo = g.cook("result")?;
    std::fs::write(format!("{out_dir}/demo.gltf"), pt_core::io::to_gltf(&geo)).map_err(|e| e.to_string())?;
    std::fs::write(format!("{out_dir}/demo.obj"), pt_core::io::to_obj(&geo)).map_err(|e| e.to_string())?;
    std::fs::write(format!("{out_dir}/demo.graph.json"), g.to_json()).map_err(|e| e.to_string())?;
    let o = pt_render::Opts { width: 1024, height: 768, ..Default::default() };
    for (name, mode) in [("shaded", pt_render::Mode::Shaded), ("height", pt_render::Mode::Attr("Cd".into()))] {
        let (img, _) = pt_render::render(&geo, &pt_render::Opts { mode, ..o.clone() });
        std::fs::write(format!("{out_dir}/demo_{name}.png"), pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
    }
    let (sheet, _) = pt_render::contact_sheet(&geo, &o);
    std::fs::write(format!("{out_dir}/demo_sheet.png"), pt_render::encode_png(&sheet)?).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&geo.stats()).unwrap());
    println!("wrote {out_dir}/demo.{{gltf,obj,graph.json}} and demo_{{shaded,height,sheet}}.png");
    Ok(())
}

/// Phase 1 gate (a)/(b)/(c)/(d) measurements.
fn bench(only_a: bool) -> Result<(), String> {
    let t = Instant::now();
    let mut g = demo_graph(200, 5_000_000)?;
    let e = g.cook_entry("result")?;
    let cook_s = t.elapsed().as_secs_f64();
    let geo = e.geo.clone();
    let t = Instant::now();
    let (img, info) = pt_render::render(&geo, &pt_render::Opts::default());
    let render_s = t.elapsed().as_secs_f64();
    println!("== Gate (a): 5,000,000 instances + noise terrain ==");
    println!("cook:    {cook_s:.3} s   ({} instances, {} points)", geo.n_instances(), geo.n_points());
    println!("preview: {render_s:.3} s   ({}x{}, drew {} of {} instances, {} tris)", img.w, img.h, info.instances_drawn, info.instances_total, info.triangles_drawn);
    println!("total:   {:.3} s", cook_s + render_s);
    println!("content hash: {}", hex(&e.hash));
    println!("peak memory: {:.0} MB", peak_mem_mb());

    if only_a {
        return Ok(());
    }
    println!("== Gate (b): incremental recook ==");
    let before = g.stats.computed;
    g.set_param("pillar", "size", json!([0.4, 2.0, 0.4]))?;
    let t = Instant::now();
    g.cook("result")?;
    println!("changed 1 param -> recomputed {} node(s) in {:.3} s (graph has {} nodes)", g.stats.computed - before, t.elapsed().as_secs_f64(), g.nodes.len());

    println!("== Fusion + memory baseline (20-node pointwise chain on 10M points) ==");
    for fusion in [false, true] {
        let mut c = Graph::new();
        c.no_fusion = !fusion;
        c.create("grid", Some("g"))?;
        c.set_param("g", "rows", json!(3162))?;
        c.set_param("g", "cols", json!(3162))?;
        let mut prev = "g".to_string();
        for i in 0..20 {
            let (kind, name) = if i % 2 == 0 { ("transform", format!("t{i}")) } else { ("noise", format!("n{i}")) };
            c.create(kind, Some(&name))?;
            if kind == "transform" {
                c.set_param(&name, "t", json!([0.01, 0.0, 0.0]))?;
            } else {
                c.set_param(&name, "amp", json!(0.01))?;
            }
            c.connect(&prev, &name, 0)?;
            prev = name;
        }
        let t = Instant::now();
        let e = c.cook_entry(&prev)?;
        println!("fusion={fusion:5}: {:.3} s, {} points, hash {}..., cache entries {}, passes fused {}", t.elapsed().as_secs_f64(), e.geo.n_points(), &hex(&e.hash)[..12], c.cache_len(), c.stats.fused_passes);
    }
    Ok(())
}

/// Prints manifest hashes of golden scenes; compare across OSes/CPUs (Phase 1 gate (c)).
fn golden() -> Result<(), String> {
    for threads in [1usize, 4] {
        let pool = rayon_pool(threads);
        let line = pool.install(|| -> Result<String, String> {
            let mut g = demo_graph(120, 20_000)?;
            let a = g.cook_entry("result")?.hash;
            let b = g.cook_entry("seeds")?.hash;
            let c = g.cook_entry("tint")?.hash;
            Ok(format!("result={} seeds={} tint={}", &hex(&a)[..32], &hex(&b)[..32], &hex(&c)[..32]))
        })?;
        println!("threads={threads}: {line}");
    }
    Ok(())
}

fn rayon_pool(n: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new().num_threads(n).build().unwrap()
}

fn render_cmd(args: &[String]) -> Result<(), String> {
    let [graph_file, node, out] = args else { return Err("usage: pt render <graph.json> <node> <out.png>".into()) };
    let mut g = Graph::from_json(&std::fs::read_to_string(graph_file).map_err(|e| e.to_string())?)?;
    let geo = g.cook(node)?;
    let (sheet, _) = pt_render::contact_sheet(&geo, &pt_render::Opts { width: 1280, height: 960, ..Default::default() });
    std::fs::write(out, pt_render::encode_png(&sheet)?).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let r = match args.get(1).map(String::as_str) {
        Some("mcp") => {
            // `pt mcp [--ui PORT]`: the UI shares the agent's live session.
            let ui = args.iter().position(|a| a == "--ui").map(|i| format!("127.0.0.1:{}", args.get(i + 1).map_or("7700", String::as_str)));
            mcp::serve_with_ui(ui);
            Ok(())
        }
        Some("ui") => mcp::serve_ui_only(&format!("127.0.0.1:{}", args.get(2).map_or("7700", String::as_str))),
        Some("demo") => demo(args.get(2).map_or("out", String::as_str)),
        Some("bench") => bench(args.get(2).map(String::as_str) == Some("a")),
        Some("golden") => golden(),
        Some("trace") => trace_cmd(&args[2..]),
        Some("sparse-bench") => sparse_bench_cmd(),
        Some("critique-bench") => critique_bench_cmd(&args[2..]),
        Some("render") => render_cmd(&args[2..]),
        Some("corpus") => corpus_cmd(&args[2..]),
        Some("match") => matchcmd::match_cmd(&args[2..]),
        Some("match-bench") => matchcmd::bench_cmd(&args[2..]),
        Some("export") => matchcmd::export_cmd(&args[2..]),
        Some("parts") => matchcmd::parts_cmd(&args[2..]),
        Some("audit") => matchcmd::audit_cmd(&args[2..]),
        Some("report") => report::report_cmd(&args[2..]),
        Some("describe") => matchcmd::describe_cmd(&args[2..]),
        Some("refine") => matchcmd::refine_cmd(&args[2..]),
        Some("recipes") => recipes_cmd(&args[2..]),
        Some("recipes-accept") => recipes_accept_cmd(&args[2..]),
        Some("shot") => shot_cmd(&args[2..]),
        Some("render-scene") => pt_cli::blender::render_scene_cmd(&args[2..]),
        Some("scorecard") => pt_cli::scorecard::scorecard_cmd(&args[2..]),
        Some("gym") => {
            gym::serve();
            Ok(())
        }
        Some("gym-baseline") => gym::baseline(),
        Some("optimize-demo") => demos::optimize_demo(),
        Some("vary-bench") => demos::vary_bench(),
        Some("recover-demo") => demos::recover_demo(),
        Some("top") => top::run_file(args.get(2).map_or("farm.json", String::as_str)),
        _ => {
            eprintln!("usage: pt mcp                      run the MCP server on stdio\n       pt demo [dir]               build the demo graph, export + render it\n       pt bench                    Phase 1 gate measurements\n       pt golden                   print cook hashes (compare across machines)\n       pt render <graph> <node> <png>   contact sheet of a node");
            Ok(())
        }
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Peak working set in MB (Windows); 0 elsewhere.
#[cfg(windows)]
fn peak_mem_mb() -> f64 {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        rest: [usize; 6],
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(h: isize, c: *mut Counters, cb: u32) -> i32;
    }
    unsafe {
        let mut c: Counters = std::mem::zeroed();
        c.cb = std::mem::size_of::<Counters>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            c.peak_working_set as f64 / 1_048_576.0
        } else {
            0.0
        }
    }
}
#[cfg(not(windows))]
fn peak_mem_mb() -> f64 {
    0.0
}

fn recipes_cmd(args: &[String]) -> Result<(), String> {
    let update = args.iter().any(|a| a == "--update");
    let results = recipes::run_all(update, Some(std::path::Path::new("out/recipes")))?;
    let mut failed = 0;
    println!("{:<26} {:>7} {:>9} {:>7} {:>9}  {}", "recipe", "specs", "points", "inst", "ms", "hash");
    for r in &results {
        println!("{:<26} {:>3}/{:<3} {:>9} {:>7} {:>9.1}  {}", r.name, r.specs_ok, r.specs_total, r.points, r.instances, r.ms, if r.hash.is_empty() { "-" } else { &r.hash[..16] });
        for f in &r.failures {
            println!("    FAIL {f}");
        }
        if !r.failures.is_empty() {
            failed += 1;
        }
    }
    println!("{} recipes, {} failed{}", results.len(), failed, if update { " (golden updated)" } else { "" });
    if failed > 0 { Err(format!("{failed} recipe(s) failed")) } else { Ok(()) }
}

/// `pt shot <recipe-name-or-file> <out.png> [spp] [azimuth]`: path-traced still of a recipe.
fn shot_cmd(args: &[String]) -> Result<(), String> {
    let [name, out, rest @ ..] = args else { return Err("usage: pt shot <recipe> <out.png> [spp] [azimuth]".into()) };
    let path = if std::path::Path::new(name).exists() {
        std::path::PathBuf::from(name)
    } else {
        recipes::list().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(name.as_str()))).ok_or_else(|| format!("no recipe matching '{name}'"))?
    };
    let code = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut g = Graph::new();
    let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let geo = g.cook(rep.outputs.first().ok_or("recipe has no out()")?)?;
    let spp = rest.first().and_then(|s| s.parse().ok()).unwrap_or(48);
    let az = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(35.0);
    let t = Instant::now();
    let (img, st) = pt_render::pathtrace::pathtrace(&geo, &pt_render::pathtrace::PtOpts { width: 960, height: 640, spp, azimuth_deg: az, ..Default::default() });
    std::fs::write(out, pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
    println!("{} triangles, {:.1} M rays, {:.2} s -> {out}", st.triangles, st.rays as f64 / 1e6, t.elapsed().as_secs_f64());
    Ok(())
}

/// `pt recipes-accept [name]`: with a name, promote that proposal into recipes/; without, list proposals.
fn recipes_accept_cmd(args: &[String]) -> Result<(), String> {
    match args.first() {
        Some(n) => {
            let dst = recipes::accept(n)?;
            println!("accepted -> {} (golden updated; rebuild to compile it into the MCP recipe list)", dst.display());
        }
        None => {
            let p = recipes::list_proposed();
            if p.is_empty() {
                println!("no proposals in {}", recipes::proposed_dir().display());
            }
            for f in p {
                println!("{}", f.file_stem().unwrap().to_string_lossy());
            }
        }
    }
    Ok(())
}

/// `pt trace <recipe>`: cook a recipe and print the content hash of every node. Diff the output
/// between operating systems to find the first node whose result is not bit-identical.
fn trace_cmd(args: &[String]) -> Result<(), String> {
    let [name] = args else { return Err("usage: pt trace <recipe-name-or-file>".into()) };
    let path = if std::path::Path::new(name).exists() {
        std::path::PathBuf::from(name)
    } else {
        recipes::list().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(name.as_str()))).ok_or_else(|| format!("no recipe matching '{name}'"))?
    };
    let code = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut g = Graph::new();
    pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let ids: Vec<String> = g.nodes.keys().cloned().collect();
    for id in ids {
        match g.cook_entry(&id) {
            Ok(e) => println!("{id:<24} {} points={} prims={} inst={}", &hex(&e.hash)[..32], e.geo.n_points(), e.geo.n_prims(), e.geo.n_instances()),
            Err(err) => println!("{id:<24} ERROR {err}"),
        }
    }
    Ok(())
}

/// `pt critique-bench prepare [dir]` | `pt critique-bench score <judgments.json> [dir]` (see critique_bench.rs).
fn critique_bench_cmd(args: &[String]) -> Result<(), String> {
    let dir = |i: usize| std::path::PathBuf::from(args.get(i).map(String::as_str).unwrap_or("out/critique"));
    match args.first().map(String::as_str) {
        Some("prepare") => pt_cli::critique_bench::prepare(&dir(1)),
        Some("score") => pt_cli::critique_bench::score(std::path::Path::new(args.get(1).ok_or("usage: pt critique-bench score <judgments.json> [dir]")?), &dir(2)),
        _ => Err("usage: pt critique-bench prepare [dir] | score <judgments.json> [dir]".into()),
    }
}

/// `pt sparse-bench`: the rock recipe through `sparse_remesh` at shrinking voxel sizes, next to what the dense
/// `sdf_from_mesh` volume would need (it refuses above 48M voxels).
fn sparse_bench_cmd() -> Result<(), String> {
    let code = std::fs::read_to_string(recipes::list().into_iter().find(|p| p.to_string_lossy().contains("03_rock")).ok_or("no rock recipe")?).map_err(|e| e.to_string())?;
    let mut g = Graph::new();
    let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let rock = g.cook(rep.outputs.first().ok_or("no output")?)?;
    let (lo, hi) = rock.base_bounds().ok_or("empty rock")?;
    println!("rock: {} points, {:.2} x {:.2} x {:.2} m", rock.n_points(), hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
    println!("{:>8} {:>14} {:>10} | {:>11} {:>10} {:>8} {:>9} {:>9} {:>8}", "voxel", "dense voxels", "dense MB", "dense tiles", "const", "MB", "build ms", "mesh ms", "closed");
    for voxel in [0.04f32, 0.02, 0.01, 0.005, 0.0025] {
        let dims = [0, 1, 2].map(|a| ((hi[a] - lo[a] + 0.6) / voxel).ceil() as u128 + 1);
        let dense_n = dims[0] * dims[1] * dims[2];
        let t = Instant::now();
        let vol = pt_sop::sparse::build_sdf(&rock, voxel, 3, "bench")?;
        let build_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let mesh = pt_sop::sparse::to_mesh(&vol, 0.0, "auto", "bench")?;
        let mesh_ms = t.elapsed().as_secs_f64() * 1000.0;
        let closed = pt_core::validate::validate(&mesh).iter().all(|i| i.severity != "error");
        let dense_note = if dense_n > 48_000_000 { " (dense refuses)" } else { "" };
        println!(
            "{:>8} {:>14} {:>10.0} | {:>11} {:>10} {:>8.1} {:>9.0} {:>9.0} {:>8}{}",
            voxel, dense_n, dense_n as f64 * 4.0 / 1.0e6, vol.n_dense(), vol.n_const(), vol.memory_bytes() as f64 / 1.0e6, build_ms, mesh_ms, closed, dense_note
        );
    }
    Ok(())
}

/// `pt corpus fetch [manifest] [--set trial|probe]` | `pt corpus verify [manifest]`: the licence ledger (see corpus.rs).
fn corpus_cmd(args: &[String]) -> Result<(), String> {
    let manifest = std::path::PathBuf::from(args.get(1).filter(|a| !a.starts_with("--")).map_or("corpus/manifest.json", String::as_str));
    let ledger = std::path::Path::new("corpus/LEDGER.csv");
    match args.first().map(String::as_str) {
        Some("fetch") => {
            let set = args.iter().position(|a| a == "--set").and_then(|i| args.get(i + 1)).map(String::as_str);
            corpus::fetch(&manifest, ledger, set)
        }
        Some("verify") => {
            let r = corpus::verify(&manifest, ledger)?;
            for p in &r.problems {
                println!("PROBLEM {p}");
            }
            println!("ledger: {} rows, {} problem(s)", r.rows, r.problems.len());
            if r.problems.is_empty() { Ok(()) } else { Err("corpus verification failed".into()) }
        }
        _ => Err("usage: pt corpus fetch [manifest.json] [--set trial|probe] | pt corpus verify [manifest.json]".into()),
    }
}
