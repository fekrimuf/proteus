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
    for must in ["ground_check", "review_tiles", "scorecard", "world", "edit", "render_world", "docs_check", "status", "recipes", "survey", "walk"] {
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

// ---- the survey and the walk-through: the game engine as the eye that finds flaws ----

#[test]
fn a_survey_plan_puts_every_kind_of_floating_object_in_front_of_the_eye() {
    // The class: an object the ground check flags that nobody looks at because no camera happens to face it. The plan must give
    // it a view of its own, and that view's middle pixel must be the object itself (asked of `pt pick`, not assumed).
    setup();
    let dir = scratch("survey_plan");
    let lifted = "wrangle(drop_to_surface(pts, terrain, footprint=0.5), code=\"@P.y = @P.y + 2.5;\")";
    let (recipe, scene) = write_world(&dir, &rocks_on_a_hill(lifted, 3), SCENE);
    let out = dir.join("survey");
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "survey", json!({"recipe": recipe, "scene": scene, "out": out.to_string_lossy(), "plan_only": true}));
    assert!(!err, "{v}");
    let report = &v["output"];
    assert!(report["flagged_in_area"]["float"].as_u64().unwrap_or(0) >= 10, "{}", report["flagged_in_area"]);
    // one prototype with one kind of defect is one class: three examples, two views each
    assert_eq!(report["views"]["flaw"], 6, "{}", report["views"]);
    assert_eq!(report["views"]["walk"], 24);
    let shows = report["closeups"][0]["shows"].as_str().unwrap();
    assert!(shows.starts_with("float ") && shows.contains("rock_test") && shows.contains("sight clear"), "{shows}");
    // the same view, asked of pick: the middle of the picture is a floating rock
    let views = out.join("survey.scene.json");
    assert!(views.exists() && out.join("plan.json").exists());
    let picked = Command::new(env!("CARGO_BIN_EXE_pt")).args(["pick", &recipe, &views.to_string_lossy(), "flaw_01a", "640", "360", "--size", "1280x720"]).output().unwrap();
    let text = String::from_utf8_lossy(&picked.stdout);
    assert!(picked.status.success() && text.contains("rock_test") && text.contains("ground-check flags: float"), "{text}");

    // the same rocks set down by the placement rule: nothing to look at closely
    let dir = scratch("survey_plan_seated");
    let (recipe, scene) = write_world(&dir, &rocks_on_a_hill("drop_to_surface(pts, terrain, footprint=0.5)", 3), SCENE);
    let (err, v) = tool(&mut s, "survey", json!({"recipe": recipe, "scene": scene, "out": dir.join("survey").to_string_lossy(), "plan_only": true}));
    assert!(!err, "{v}");
    assert_eq!(v["output"]["views"]["flaw"], 0, "{}", v["output"]["closeups"]);
    assert!(v["output"]["flagged_in_area"]["float"].is_null());
}

#[test]
fn a_survey_refuses_an_unknown_pass_and_a_world_without_ground() {
    setup();
    let dir = scratch("survey_refuse");
    let (recipe, scene) = write_world(&dir, &rocks_on_a_hill("pts", 1), SCENE);
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "survey", json!({"recipe": recipe, "scene": scene, "out": dir.join("s").to_string_lossy(), "passes": "lit,heat", "plan_only": true}));
    assert!(err && v["stderr"].as_str().unwrap().contains("unknown pass 'heat'") && v["stderr"].as_str().unwrap().contains("lit, flags, kinds"), "{v}");
    let (recipe, scene) = write_world(&dir, "out(box(size=[1, 1, 1]))\n", SCENE);
    let (err, v) = tool(&mut s, "survey", json!({"recipe": recipe, "scene": scene, "out": dir.join("s").to_string_lossy(), "plan_only": true}));
    assert!(err && v["stderr"].as_str().unwrap().contains("no terrain material"), "{v}");
}

/// A green ground (its colour is a point colour), a rock hanging 1.5 m in the air over the middle, and a rock on the ground
/// (the rock is 1.2 m tall with its underside at its origin).
const TWO_ROCKS: &str = r#"terrain = set_material(normals(wrangle(grid(size=[60, 60], rows=20, cols=20), code="@Cd = vec3(0.05, 0.6, 0.05);")), name="terrain_test")
spot = curve(points=[[0.0, 1.5, 0.0], [6.0, 0.0, 0.0]])
rock = set_material(fit_size(sphere(), size=[1.6, 1.2, 1.6], uniform=0), name="rock_test")
rocks = copy_to_points(rock, spot)
world = merge(terrain, rocks)
out(world)
"#;
const TWO_ROCKS_SCENE: &str = r#"{"cameras": [{"name": "front", "eye": {"at": [0, -12], "height": 1.7, "yaw": 90}, "lens": 28}]}"#;

fn pixel(img: &pt_render::Image, p: [f32; 2]) -> [u8; 3] {
    let o = (p[1] as usize * img.w + p[0] as usize) * 3;
    [img.rgb[o], img.rgb[o + 1], img.rgb[o + 2]]
}

#[test]
fn the_engine_shows_the_flagged_rock_in_red_exactly_where_the_camera_maths_puts_it() {
    // Three classes in one engine launch (skipped where the engine is not installed):
    // * the engine's camera and the program's camera (pick, the ground check, the boxes drawn on pictures) must be the same
    //   camera, or every pixel names the wrong object;
    // * a flagged object must show in its colour in the `flags` pass and only there;
    // * a point colour must reach the screen (the engine's importer left it off: every far tree's shell and every unbaked
    //   ground drew white).
    setup();
    if !pt_cli::engine::exe().exists() {
        eprintln!("skipped: the game engine is not installed (PT_GODOT)");
        return;
    }
    let dir = scratch("survey_engine");
    let (recipe, scene) = write_world(&dir, TWO_ROCKS, TWO_ROCKS_SCENE);
    let out = dir.join("survey");
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "survey", json!({"recipe": recipe, "scene": scene, "out": out.to_string_lossy(), "stations": 1, "closeups": 2, "size": "640x360"}));
    assert!(!err, "{v}");
    let report = &v["output"];
    assert_eq!(report["flagged_in_area"]["float"], 1, "{}", report["flagged_in_area"]);
    assert_eq!(report["engine"]["pictures"], report["views"]["pictures"], "every planned picture was taken: {}", report["engine"]);
    assert_eq!(report["engine"]["objects_painted_as_flagged"].as_u64().map(|n| n >= 1), Some(true), "{}", report["engine"]);
    for sheet in report["sheets"].as_array().unwrap() {
        assert!(Path::new(sheet["sheet"].as_str().unwrap()).exists(), "{sheet}");
    }
    assert!(report["sheets"].as_array().unwrap().iter().any(|s| s["sheet"].as_str().unwrap().contains("sheet_flaws_1")), "the close-ups have a sheet");
    let load = |name: &str| pt_render::decode_png(&std::fs::read(out.join(name)).unwrap()).unwrap();
    let (lit, flags) = (load("lit_cam_front.png"), load("flags_cam_front.png"));
    assert_eq!((lit.w, lit.h), (640, 360));
    // the scene's camera, as the program computes it
    let cam = pt_cli::survey::View { name: "front".into(), group: "camera", why: String::new(), eye: [0.0, 1.7, -12.0], yaw: 90.0, pitch: 0.0, lens: 28.0, target: None };
    let floating = cam.project([0.0, 2.1, 0.0], 640, 360).unwrap();
    let seated = cam.project([6.0, 0.6, 0.0], 640, 360).unwrap();
    let ground = cam.project([-3.0, 0.0, -4.0], 640, 360).unwrap();
    let red = |c: [u8; 3]| c[0] > 200 && c[1] < 90 && c[2] < 90;
    assert!(red(pixel(&flags, floating)), "the floating rock is not red at {floating:?} in the flags pass: {:?}", pixel(&flags, floating));
    assert!(!red(pixel(&lit, floating)), "the lit pass shows the world as it is: {:?}", pixel(&lit, floating));
    assert!(!red(pixel(&flags, seated)), "the rock on the ground is not flagged: {:?}", pixel(&flags, seated));
    let g = pixel(&lit, ground);
    assert!(g[1] as i32 > g[0] as i32 + 25 && g[1] as i32 > g[2] as i32 + 25, "the ground's point colour (green) did not reach the screen: {g:?}");
    // the close-up has the rock (a grey stand-in material) in the middle, not the green ground or the sky
    let c = pixel(&load("lit_flaw_01a.png"), [320.0, 180.0]);
    assert!((c[0] as i32 - c[1] as i32).abs() < 25 && (c[2] as i32 - c[1] as i32).abs() < 40, "the middle of the close-up is not the rock: {c:?}");
    assert!(out.join("boxed_flaw_01a.png").exists());
}

#[test]
fn a_walk_through_opens_notes_what_is_pointed_at_and_names_it() {
    // The owner's way into a world (skipped where the engine is not installed). `check` plays the owner: it points at the middle
    // of the first camera's view, where the floating rock hangs, and closes the window. The note must name that rock: the ray
    // the engine wrote down and the world the program cooked agree.
    setup();
    if !pt_cli::engine::exe().exists() {
        eprintln!("skipped: the game engine is not installed (PT_GODOT)");
        return;
    }
    let dir = scratch("walk_engine");
    let (recipe, scene) = write_world(&dir, TWO_ROCKS, TWO_ROCKS_SCENE);
    let out = dir.join("walk");
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "walk", json!({"recipe": recipe, "scene": scene, "out": out.to_string_lossy(), "size": "640x360", "check": true}));
    assert!(!err, "{v}");
    let notes = v["output"]["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1, "{}", v["output"]);
    assert_eq!(notes[0]["asked"], "this looks wrong");
    assert_eq!(notes[0]["result"]["kind"], "instance", "{}", notes[0]);
    assert_eq!(notes[0]["result"]["instance"], 0, "the rock in the air, not the one on the ground");
    assert_eq!(notes[0]["result"]["ground_check"][0]["kind"], "float");
    assert!(notes[0]["summary"].as_str().unwrap().contains("rock_test"), "{}", notes[0]["summary"]);
    assert!(Path::new(notes[0]["shot"].as_str().unwrap()).exists(), "the picture taken at the click");
    assert!(out.join("notes.json").exists());
}

/// A two-room cottage built through the tools, and its rooms and openings read back: the way a later stage (furnishing, a camera at a
/// door) learns where the doors and windows are. A plan with a room nobody can walk to is refused with the reason.
#[test]
fn a_cottage_built_from_a_plan_gives_back_its_rooms_and_openings() {
    setup();
    let plan = |inner: &str| {
        format!(
            r#"house = building(width=6.2, depth=4.6, height=2.5, cuts_z=[0.55], door=[0, 0.32, 1.0, 1.95], inner_doors={inner}, windows=[[0, 0.74, 0.8, 0.7, 1.0], [3, 0.22, 0.8, 0.8, 1.0], [2, 0.5, 0.8, 0.8, 1.0]])
doors = building(width=6.2, depth=4.6, height=2.5, cuts_z=[0.55], door=[0, 0.32, 1.0, 1.95], inner_doors={inner}, windows=[[0, 0.74, 0.8, 0.7, 1.0], [3, 0.22, 0.8, 0.8, 1.0], [2, 0.5, 0.8, 0.8, 1.0]], mode="openings")
out(house)
out(doors)
"#
        )
    };
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "run_hcs", json!({"code": plan("[[0, 1, 0.5, 0.9, 2.0]]")}));
    assert!(!err && v["ok"] == true, "{v}");
    let (shell, lines) = (v["outputs"][0]["node"].as_str().unwrap().to_string(), v["outputs"][1]["node"].as_str().unwrap().to_string());
    let (err, st) = tool(&mut s, "cook", json!({"node": shell}));
    assert!(!err, "{st}");
    let info = &st["detail"]["building"];
    let rooms = info["rooms"].as_array().unwrap();
    assert_eq!(rooms.len(), 2, "{info}");
    let (front, back) = (&rooms[0], &rooms[1]);
    assert!(front["center"][2].as_f64().unwrap() > back["center"][2].as_f64().unwrap(), "room 0 is the front room (+Z)");
    assert!(front["size"][0].as_f64().unwrap() > 5.0, "{front}");
    let opens = info["openings"].as_array().unwrap();
    assert_eq!(opens.iter().filter(|o| o["kind"] == "window").count(), 3);
    let entrance = &opens[0];
    assert_eq!((entrance["kind"].as_str(), entrance["room_from"].as_i64(), entrance["room_to"].as_i64()), (Some("entrance"), Some(0), Some(-1)));
    assert!(entrance["dir"][2].as_f64().unwrap() > 0.99, "the front door faces +Z: {entrance}");
    let inner = opens.iter().find(|o| o["kind"] == "inner_door").unwrap();
    assert!(inner["width"].as_f64().unwrap() >= 0.8 && inner["height"].as_f64().unwrap() >= 1.9, "{inner}");
    assert_eq!(st["primitive_attributes"]["room"]["max"][0].as_f64(), Some(1.0), "{}", st["primitive_attributes"]);
    assert!(st["primitive_groups"]["mat:hut_roof"].as_u64().unwrap() > 0 && st["primitive_groups"]["mat:hut_door"].as_u64().unwrap() > 0, "{}", st["primitive_groups"]);
    let (err, ln) = tool(&mut s, "cook", json!({"node": lines}));
    assert!(!err, "{ln}");
    assert_eq!(ln["primitives"], 5, "an entrance, an inner door and three windows: {ln}");
    // a plan with a room that has no door is refused, with the reason, and gives no building
    let (err, e) = tool(&mut s, "run_hcs", json!({"code": plan("[]")}));
    assert!(!err && e["ok"] == false, "{e}");
    let why = e["outputs"][0]["error"].as_str().unwrap();
    assert!(why.contains("room 1 cannot be reached") && why.contains("inner door"), "{why}");
}

/// The agent can see whether the machine is taken before it starts a render, and by whom.
#[test]
fn the_agent_sees_who_holds_the_machine() {
    setup();
    let dir = scratch("heavy_status");
    std::env::set_var("PT_HEAVY_LOCK_DIR", dir.join("lock"));
    let mut s = Session::new();
    let (err, v) = tool(&mut s, "heavy_status", json!({}));
    assert!(!err && v["output"]["held"] == false, "{v}");
    let slot = pt_cli::lowload::acquire("a stand-in render").unwrap();
    let (err, v) = tool(&mut s, "heavy_status", json!({}));
    assert!(!err && v["output"]["held"] == true, "{v}");
    assert!(v["output"]["holder"].as_str().unwrap().contains("a stand-in render"), "{v}");
    drop(slot);
    let (_, v) = tool(&mut s, "heavy_status", json!({}));
    assert_eq!(v["output"]["held"], false, "{v}");
}

/// The clean-title certificate through the agent tools: a world whose assets are in the ledger gets its report; one with an asset
/// the ledger lacks is an error that names the asset, and no report is written.
#[test]
fn certificate_tool_writes_the_report_and_refuses_an_unledgered_asset() {
    setup();
    let dir = scratch("certificate");
    let recipe = dir.join("world.hcs");
    std::fs::write(&recipe, "rock = import_asset(path=\"polyhaven/models/rock_09/rock_09_1k.gltf\")\nout(rock)\n").unwrap();
    let header = "id,source,url,license,license_url,sha256,fetched_at,attribution\n";
    // the model's own folder: the .gltf, its .bin and textures (a real rock_09 on disk names exactly these)
    let row: String = ["rock_09_1k.gltf", "rock_09.bin", "textures/rock_09_arm_1k.jpg", "textures/rock_09_diff_1k.jpg", "textures/rock_09_nor_gl_1k.jpg"]
        .iter()
        .map(|f| format!("polyhaven/models/rock_09/{f},polyhaven,https://example.org/rock,CC0-1.0,https://creativecommons.org/publicdomain/zero/1.0/,deadbeef,2026-10-10T00:00:00Z,\n"))
        .collect();
    let (good, bad) = (dir.join("good.csv"), dir.join("bad.csv"));
    std::fs::write(&good, format!("{header}{row}")).unwrap();
    std::fs::write(&bad, header).unwrap();
    let mut s = Session::new();
    let listed = handle(&mut s, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).unwrap();
    assert!(listed["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "certificate"), "certificate is not an agent tool");

    let out = dir.join("cert.md");
    let (err, v) = tool(&mut s, "certificate", json!({"recipe": recipe, "out": out, "ledger": good}));
    assert!(!err, "{v}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("rock_09_1k.gltf") && text.contains("deadbeef") && text.contains("CC0-1.0"), "{text}");

    let out2 = dir.join("cert2.md");
    let (err, v) = tool(&mut s, "certificate", json!({"recipe": recipe, "out": out2, "ledger": bad}));
    assert!(err, "a missing asset must fail");
    assert!(v.to_string().contains("polyhaven/models/rock_09/rock_09_1k.gltf") && v.to_string().contains("\"exit_code\":1"), "{v}");
    assert!(!out2.exists(), "a failed certificate left a file");
}

/// The certificate with `--scene`: the scene's materials are followed to their Poly Haven sets, every file of a set is listed, and a
/// set the ledger lacks is an error that names it.
#[test]
fn certificate_tool_follows_the_scenes_materials_to_their_sets() {
    setup();
    let dir = scratch("certificate_scene");
    let recipe = dir.join("world.hcs");
    std::fs::write(&recipe, "out(grid())\n").unwrap();
    let scene = dir.join("world.scene.json");
    std::fs::write(&scene, r#"{"materials": {"ground_flat": {"use": "forest_floor"}}}"#).unwrap();
    let header = "id,source,url,license,license_url,sha256,fetched_at,attribution\n";
    let row = |id: &str| format!("{id},polyhaven,https://example.org/m,CC0-1.0,https://creativecommons.org/publicdomain/zero/1.0/,cafe01,2026-10-10T00:00:00Z,\n");
    let (good, bad) = (dir.join("good.csv"), dir.join("bad.csv"));
    std::fs::write(&good, format!("{header}{}{}", row("polyhaven/materials/forrest_ground_01/forrest_ground_01_diff_1k.jpg"), row("polyhaven/materials/forrest_ground_01/forrest_ground_01_nor_gl_1k.jpg"))).unwrap();
    std::fs::write(&bad, header).unwrap();
    let mut s = Session::new();

    let out = dir.join("cert.md");
    let (err, v) = tool(&mut s, "certificate", json!({"recipe": recipe, "out": out, "scene": scene, "ledger": good}));
    assert!(!err, "{v}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("forrest_ground_01_diff_1k.jpg") && text.contains("forrest_ground_01_nor_gl_1k.jpg"), "{text}");

    let out2 = dir.join("cert2.md");
    let (err, v) = tool(&mut s, "certificate", json!({"recipe": recipe, "out": out2, "scene": scene, "ledger": bad}));
    assert!(err && v.to_string().contains("forrest_ground_01"), "a missing material set must fail and be named: {v}");
    assert!(!out2.exists(), "a failed certificate left a file");

    let out3 = dir.join("cert3.md");
    let (err, v) = tool(&mut s, "certificate", json!({"recipe": recipe, "out": out3, "ledger": bad}));
    assert!(!err, "{v}");
    assert!(std::fs::read_to_string(&out3).unwrap().contains("Materials were not certified"));
}
