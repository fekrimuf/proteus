//! The agent's seat: tools called the way an MCP client calls them (`tools/call` through `mcp::handle`), against the real `pt`
//! program, asserting on what was written and measured, not on "the call returned".
//!
//! Three kinds of test live here:
//! * the one command list: the command line and the tool list agree, a mistyped option or argument is refused with what is
//!   accepted, a failing command comes back with what it printed;
//! * the classes of defect the owner found by eye in a frame (objects floating, hanging, drowned in water): each is a permanent
//!   check on the placement rule itself, over several seeds, not a fix for one stone;
//! * a crash in one tool leaves the scene and the session alive.

use pt_cli::mcp::{handle, Session};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn setup() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("PT_EXE", env!("CARGO_BIN_EXE_pt")));
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pt_agent_tools_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A `tools/call` as a client sends it: (is_error, the first text block parsed as JSON when it is JSON, else as a string).
fn tool(s: &mut Session, name: &str, args: Value) -> (bool, Value) {
    let reply = handle(s, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})).expect("a reply");
    let r = &reply["result"];
    let text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
    (r["isError"] == true, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// A world with a hill, rock instances placed on it by `place` (an hcs expression over `pts`, the scattered points, and `terrain`).
fn rocks_on_a_hill(place: &str, seed: u32) -> String {
    format!(
        r#"terrain = set_material(wrangle(grid(size=[40, 40], rows=80, cols=80), code="@P.y = 4.0 * exp(-(@P.x * @P.x + @P.z * @P.z) / 40.0) + 0.6 * sin(@P.x * 1.3) * cos(@P.z * 1.1);"), name="terrain_test")
terrain = normals(terrain)
pts = scatter_poisson(grid(size=[30, 30]), count=70, min_dist=1.8, seed={seed}, pscale_min=1.0, pscale_max=1.8)
placed = {place}
rock = set_material(fit_size(sphere(), size=[1.4, 0.8, 1.2], uniform=0), name="rock_test")
rocks = copy_to_points(rock, placed, rot_y_random=360.0, seed=2)
world = merge(terrain, rocks)
spec(world, "instances", min=20)
out(world)
"#
    )
}

const SCENE: &str = r#"{"cameras": [{"name": "front", "eye": {"at": [0, -20], "height": 1.7, "yaw": 90}, "lens": 28}]}"#;

fn write_world(dir: &Path, recipe: &str, scene: &str) -> (String, String) {
    let (r, s) = (dir.join("world.hcs"), dir.join("scene.json"));
    std::fs::write(&r, recipe).unwrap();
    std::fs::write(&s, scene).unwrap();
    (r.to_string_lossy().into_owned(), s.to_string_lossy().into_owned())
}

#[test]
fn the_command_line_and_the_tool_list_come_from_the_same_declaration() {
    setup();
    let out = Command::new(env!("CARGO_BIN_EXE_pt")).args(["commands"]).output().unwrap();
    assert!(out.status.success());
    let listed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut s = Session::new();
    let reply = handle(&mut s, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).unwrap();
    let tools: Vec<&str> = reply["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(listed.as_array().unwrap().len() >= 9, "the registry lists its commands");
    for c in listed.as_array().unwrap() {
        let name = c["tool"].as_str().unwrap();
        assert!(tools.contains(&name), "registered command '{}' is missing from the agent's tool list as '{name}'", c["command"]);
    }
    for must in ["ground_check", "review_tiles", "scorecard", "world", "edit", "render_world", "docs_check", "status", "recipes"] {
        assert!(tools.contains(&must), "the agent cannot see '{must}'");
    }
    let mut seen = std::collections::HashSet::new();
    assert!(tools.iter().all(|t| seen.insert(*t)), "a tool is listed twice: {tools:?}");
}

#[test]
fn the_command_line_refuses_a_mistyped_option_and_names_the_accepted_ones() {
    let out = Command::new(env!("CARGO_BIN_EXE_pt")).args(["ground-check", "w.hcs", "s.json", "--raduis", "30"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown option '--raduis'") && err.contains("--radius") && err.contains("--frames"), "{err}");
    let out = Command::new(env!("CARGO_BIN_EXE_pt")).args(["review-tiles", "only_one_argument.png"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: pt review-tiles <frame> <out_dir>"));
}

#[test]
fn a_mistyped_tool_argument_is_refused_before_anything_changes() {
    setup();
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "create_node", json!({"kind": "box", "nmae": "b"}));
    assert!(err, "{v}");
    let text = v.as_str().unwrap();
    assert!(text.contains("unknown argument 'nmae'") && text.contains("accepted: kind, name"), "{text}");
    let (err, v) = tool(&mut s, "describe_node", json!({"node": "b"}));
    assert!(err, "the refused call created nothing: {v}");
    let (err, v) = tool(&mut s, "set_param", json!({"node": "a", "name": "rows"}));
    assert!(err && v.as_str().unwrap().contains("missing required argument 'value'"), "{v}");
    let (err, v) = tool(&mut s, "cook", json!({"node": 3}));
    assert!(err && v.as_str().unwrap().contains("must be of type string"), "{v}");
    let (err, v) = tool(&mut s, "undo", json!({"steps": 2}));
    assert!(err && v.as_str().unwrap().contains("accepted: none"), "{v}");
    let (err, v) = tool(&mut s, "ground_check", json!({"recipe": "w.hcs", "scene": "s.json", "raduis": 30}));
    assert!(err && v.as_str().unwrap().contains("unknown argument 'raduis'") && v.as_str().unwrap().contains("radius"), "{v}");
}

#[test]
fn review_tiles_through_the_agent_tools_writes_six_enlarged_tiles() {
    setup();
    let dir = scratch("tiles");
    let img = pt_render::Image { w: 300, h: 200, rgb: (0..300 * 200 * 3).map(|i| (i % 251) as u8).collect() };
    let frame = dir.join("frame.png");
    std::fs::write(&frame, pt_render::encode_png(&img).unwrap()).unwrap();
    let out_dir = dir.join("tiles");
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "review_tiles", json!({"frame": frame.to_string_lossy(), "out_dir": out_dir.to_string_lossy()}));
    assert!(!err, "{v}");
    assert_eq!(v["ok"], true);
    assert!(v["command"].as_str().unwrap().starts_with("pt review-tiles "), "{v}");
    let mut names: Vec<String> = std::fs::read_dir(&out_dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["tile_r1c1.png", "tile_r1c2.png", "tile_r1c3.png", "tile_r2c1.png", "tile_r2c2.png", "tile_r2c3.png"]);
    let tile = pt_render::decode_png(&std::fs::read(out_dir.join("tile_r1c1.png")).unwrap()).unwrap();
    assert_eq!((tile.w, tile.h), (100 * 2, 100 * 2), "each of the 3 x 2 tiles is enlarged twice");
}

#[test]
fn a_failing_command_comes_back_as_an_error_with_what_it_printed() {
    setup();
    let dir = scratch("fail");
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "review_tiles", json!({"frame": dir.join("missing.png").to_string_lossy(), "out_dir": dir.join("t").to_string_lossy()}));
    assert!(err);
    let r = &v; // the error text is the command's result as JSON, already parsed
    assert_eq!((r["ok"].clone(), r["exit_code"].clone()), (json!(false), json!(1)));
    assert!(r["stderr"].as_str().unwrap().contains("missing.png"), "the agent sees why: {r}");
}

#[test]
fn rule_placed_objects_never_float_or_hang_whatever_the_seed() {
    setup();
    let mut s = Session::new();
    for seed in 1..=4 {
        let dir = scratch(&format!("settle{seed}"));
        let (recipe, scene) = write_world(&dir, &rocks_on_a_hill("drop_to_surface(pts, terrain, footprint=0.5)", seed), SCENE);
        let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
        assert!(!err, "seed {seed}: objects dropped with a footprint must not float or hang inside the view: {v}");
        let report = &v["output"];
        assert_eq!(report["fail"], false, "seed {seed}: {report}");
        assert!(report["instances"].as_u64().unwrap() >= 20, "the world has rocks to check: {report}");
        assert!(report["findings"]["float"].is_null() && report["findings"]["overhang"].is_null(), "seed {seed}: {}", report["findings"]);
    }
}

#[test]
fn the_same_rocks_without_the_footprint_rule_hang_over_the_slope() {
    // The sensitivity of the check above: take the rule away and the same world is caught. Without this the "never hang" test
    // could pass on a hill too gentle to tell the rule from no rule.
    setup();
    let mut s = Session::new();
    for seed in 1..=4 {
        let dir = scratch(&format!("nofoot{seed}"));
        let (recipe, scene) = write_world(&dir, &rocks_on_a_hill("drop_to_surface(pts, terrain)", seed), SCENE);
        let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
        assert!(err, "seed {seed}: rocks dropped by their centre alone hang over the slope and must fail the check");
        assert!(v["output"]["findings"]["overhang"].as_u64().unwrap_or(0) >= 5, "seed {seed}: {}", v["output"]["findings"]);
    }
}

#[test]
fn rocks_left_in_the_air_are_caught_as_floating() {
    setup();
    let dir = scratch("float");
    let lifted = "wrangle(drop_to_surface(pts, terrain, footprint=0.5), code=\"@P.y = @P.y + 2.5;\")";
    let (recipe, scene) = write_world(&dir, &rocks_on_a_hill(lifted, 3), SCENE);
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
    assert!(err, "floating rocks inside a view must fail the check");
    let report = &v["output"]; // a failing command comes back as an error whose text is its result as JSON
    assert_eq!(report["fail"], true);
    assert!(report["findings"]["float"].as_u64().unwrap_or(0) >= 10, "{}", report["findings"]);
    assert!(report["cameras"][0]["counts"]["float"].as_u64().unwrap_or(0) >= 5, "seen from the camera: {}", report["cameras"]);
}

#[test]
fn stones_sitting_on_a_submerged_bed_are_caught_as_flat_lids() {
    setup();
    let dir = scratch("drown");
    let recipe = r#"bed = set_material(normals(wrangle(grid(size=[40, 40], rows=80, cols=80), code="@P.y = -0.9 * exp(-(@P.x * @P.x) / 30.0);")), name="terrain_test")
water = set_material(normals(wrangle(grid(size=[40, 40], rows=2, cols=2), code="@P.y = 0.0;")), name="stream_water")
pts = scatter_poisson(grid(size=[2, 30]), count=40, min_dist=1.2, seed=4, pscale_min=0.9, pscale_max=1.1)
settled = drop_to_surface(pts, bed, footprint=0.4)
stone = set_material(fit_size(sphere(), size=[0.6, 0.4, 0.5], uniform=0), name="rock_test")
stones = copy_to_points(stone, settled, rot_y_random=360.0, seed=2)
world = merge(bed, water, stones)
spec(world, "instances", min=10)
out(world)
"#;
    let (recipe, scene) = write_world(&dir, recipe, SCENE);
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
    assert!(!err, "drowned stones do not fail the command, they are counted: {v}");
    let report = &v["output"];
    assert!(report["findings"]["shallow"].as_u64().unwrap_or(0) >= 10, "stones under 0.9 m of water show only a flat lid or nothing: {}", report["findings"]);
    assert!(report["cameras"][0]["by_prototype"]["shallow rock_test"].as_u64().unwrap_or(0) >= 10, "the finding names the prototype: {}", report["cameras"]);
}

/// Trees on the same hill: a root plate 1.6 m wide, a trunk and a crown far above. `stand` is an hcs expression over `trees`
/// (the instances, each dropped onto the ground by its centre) and `terrain`.
fn trees_on_a_hill(stand: &str, seed: u32) -> String {
    format!(
        r#"terrain = set_material(wrangle(grid(size=[40, 40], rows=80, cols=80), code="@P.y = 4.0 * exp(-(@P.x * @P.x + @P.z * @P.z) / 40.0) + 0.6 * sin(@P.x * 1.3) * cos(@P.z * 1.1);"), name="terrain_test")
terrain = normals(terrain)
pts = scatter_poisson(grid(size=[30, 30]), count=70, min_dist=1.8, seed={seed}, pscale_min=0.8, pscale_max=1.4)
tree = set_material(merge(box(size=[1.6, 0.1, 1.6], center=[0.0, 0.05, 0.0]), box(size=[0.4, 14.0, 0.4], center=[0.0, 7.1, 0.0]), box(size=[6.0, 6.0, 6.0], center=[0.0, 17.1, 0.0])), name="tree_test")
trees = copy_to_points(tree, drop_to_surface(pts, terrain), rot_y_random=360.0, seed=2)
world = merge(terrain, {stand})
spec(world, "instances", min=20)
out(world)
"#
    )
}

#[test]
fn a_tree_dropped_by_its_centre_has_its_root_plate_in_the_air_and_the_check_sees_it() {
    // The owner's east frame: firs on the right bank stood with the rim of their base over the slope, and the check passed
    // them, because it measured a 20 m bounding box (a third of that height had to be air before it spoke).
    setup();
    let mut s = Session::new();
    for seed in 1..=4 {
        let dir = scratch(&format!("treeair{seed}"));
        let (recipe, scene) = write_world(&dir, &trees_on_a_hill("trees", seed), SCENE);
        let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
        assert!(err, "seed {seed}: root plates over a slope must fail the check: {v}");
        assert!(v["output"]["findings"]["overhang"].as_u64().unwrap_or(0) >= 10, "seed {seed}: {}", v["output"]["findings"]);
        assert!(v["output"]["cameras"][0]["by_prototype"]["overhang tree_test"].as_u64().unwrap_or(0) >= 3, "the finding names the tree: {}", v["output"]["cameras"]);
    }
}

#[test]
fn seated_trees_stand_in_the_ground_whatever_the_seed() {
    setup();
    let mut s = Session::new();
    for seed in 1..=4 {
        let dir = scratch(&format!("treeseat{seed}"));
        let (recipe, scene) = write_world(&dir, &trees_on_a_hill("seat(trees, terrain)", seed), SCENE);
        let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
        assert!(!err, "seed {seed}: seated trees must pass: {v}");
        let report = &v["output"];
        assert!(report["instances"].as_u64().unwrap() >= 20, "the trees are still there: {report}");
        assert!(report["findings"]["float"].is_null() && report["findings"]["overhang"].is_null(), "seed {seed}: {}", report["findings"]);
    }
}

/// Logs along a creek: each log is a 4 m trunk that stands half a metre up on one thin stub, the way a scanned dead trunk
/// stands on a broken branch. The bed is 0.9 m under the water in the middle and dry at the sides.
fn logs_along_a_creek(lie: &str, seed: u32) -> String {
    format!(
        r#"bed = set_material(normals(wrangle(grid(size=[40, 40], rows=80, cols=80), code="@P.y = 0.3 - 1.2 * exp(-(@P.x * @P.x) / 30.0);")), name="terrain_test")
water = set_material(normals(wrangle(grid(size=[40, 40], rows=2, cols=2), code="@P.y = 0.0;")), name="stream_water")
pts = scatter_poisson(grid(size=[24, 30]), count=40, min_dist=4.5, seed={seed}, pscale_min=0.9, pscale_max=1.1)
log = set_material(merge(box(size=[0.08, 0.5, 0.08], center=[0.0, 0.25, 0.0]), box(size=[4.0, 0.4, 0.4], center=[0.0, 0.7, 0.0])), name="log_test")
logs = copy_to_points(log, drop_to_surface(pts, bed), rot_y_random=360.0, seed=2)
world = merge(bed, water, {lie})
spec(world, "instances", min=10)
out(world)
"#
    )
}

#[test]
fn a_log_standing_on_a_stub_is_caught_as_floating_on_land_and_over_water() {
    // The owner's east frame: a dead trunk in the creek hung over the water line. Its lowest twig touched, so the box touched.
    setup();
    let mut s = Session::new();
    for seed in 1..=3 {
        let dir = scratch(&format!("logair{seed}"));
        let (recipe, scene) = write_world(&dir, &logs_along_a_creek("logs", seed), SCENE);
        let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
        assert!(err, "seed {seed}: logs standing on a stub must fail the check: {v}");
        assert!(v["output"]["findings"]["float"].as_u64().unwrap_or(0) >= 5, "seed {seed}: {}", v["output"]["findings"]);
    }
}

#[test]
fn seated_logs_lie_on_the_ground_and_in_the_water_line() {
    setup();
    let mut s = Session::new();
    for seed in 1..=3 {
        let dir = scratch(&format!("logseat{seed}"));
        let (recipe, scene) = write_world(&dir, &logs_along_a_creek("seat(logs, bed, water, draft=0.55)", seed), SCENE);
        let (err, v) = tool(&mut s, "ground_check", json!({"recipe": recipe, "scene": scene}));
        assert!(!err, "seed {seed}: seated logs must pass: {v}");
        let report = &v["output"];
        assert!(report["instances"].as_u64().unwrap() >= 10, "the logs are still there: {report}");
        assert!(report["findings"]["float"].is_null() && report["findings"]["overhang"].is_null(), "seed {seed}: {}", report["findings"]);
    }
}

#[test]
fn hostile_input_gets_answers_and_the_session_keeps_working() {
    // A burst of hostile input through the public entry point: malformed requests, wrong shapes, unknown tools. (A panic inside a
    // tool is exercised in the library: mcp.rs, `a_panic_in_a_tool_becomes_an_error_and_the_scene_is_put_back`.)
    setup();
    let mut s = Session::new();
    for req in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": 5, "arguments": "x"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "no_such_tool", "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "cook", "arguments": {"node": "nothing"}}}),
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "run_hcs", "arguments": {"code": "out(((("}}}),
        json!({"jsonrpc": "2.0", "id": 6, "method": "no/such/method"}),
    ] {
        let reply = handle(&mut s, &req).expect("every request with an id gets an answer");
        assert!(reply.get("result").is_some() || reply.get("error").is_some(), "{reply}");
    }
    let (err, _) = tool(&mut s, "create_node", json!({"kind": "box", "name": "still_alive"}));
    assert!(!err, "the session still works after hostile input");
}
