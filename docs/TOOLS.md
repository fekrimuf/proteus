# Tools reference (crates, commands, MCP tools)

Changes when a command or crate is added. Counts are not quoted here; see [generated/state.md](generated/state.md). Usage guides: [AGENT_GUIDE.md](AGENT_GUIDE.md), [WORLD_GUIDE.md](WORLD_GUIDE.md).

## Crates

| Crate | Role |
|---|---|
| `pt-core` | Geometry v2 (columnar COW attributes, instance tables, groups, volumes), deterministic math/RNG/noise, OBJ/glTF/USD/`ptgeo` I/O, validation, `spatial` (point tree and triangle tree: nearest point, points within a radius, closest surface point, rays). |
| `pt-vex` | VEX-style language: lexer, parser, type checker (line/col diagnostics with fixes), interpreter generic over autodiff channels, `fit` (gradient fitting), `query` (the `near_*`, `surf_*` and `ray_*` questions a wrangle asks its second input; AGENT_GUIDE "Asking the world questions"). |
| `pt-sop` | ~85 node types (incl. `set_material`, `blend_materials`, `import_asset`, `copy_variants`, `dem`, `hf_flow` with river channels, `hf_fit`, `hf_hole`, `hf_refine`, `lod_hint`): primitives, curves + deformers (bend twist taper sweep revolve resample drop_to_surface...), sparse SDF ops (sparse_remesh, sparse_boolean), attributes, groups, topology ops, terrain and nested heightfield levels, hydrology, SDF, simulation adapters, fused point kernel, attribute-flow `effect()`. |
| `pt-sim` | Solvers: particles, PBD cloth, smoke (stable fluids), rigid bodies (rapier3d, enhanced-determinism). |
| `pt-graph` | Node DAG, content-addressed cook cache with early cutoff, provenance, expressions (`$F`, `ch("node/param")`), assets/`repeat`, static `check()`, `merge` (diff + 3-way). |
| `pt-hcs` | Agent language (Python syntax): functions, loops, comprehensions, `spec()`, `asset()`, `frame()`, `to_hcs()`. |
| `pt-opt` | CMA-ES optimizer, objectives (chamfer, size, silhouette, image, specs), variation engine. |
| `pt-render` | Software rasterizer (silhouettes, heatmaps), CPU path tracer (BVH), image metrics. |
| `pt-cli` | Binary `pt` + library: MCP server, web UI, gym, recipe runner, TOP farm, demos. |

## Commands

Commands that work on files (a recipe and a scene, a frame, the documents) are declared once in `crates/pt-cli/src/registry.rs`, beside their code. That one list is the command line (`pt help`, `pt commands --json`), the agent's tool list (each command is also a tool, run as a child process so a crash or a long print cannot reach the live session) and the generated table [generated/commands.md](generated/commands.md). Mistyped options and arguments are refused with what is accepted. New commands go in the registry; the commands below that are not in it yet are listed by `pt help`.

`pt mcp [--ui PORT]` (MCP over stdio; UI shares the same live session) | `pt ui [PORT]` | `pt recipes [--update]` |
`pt shot <recipe> <out.png> [spp] [az]` (path-traced still) | `pt render-scene <recipe|glb> <scene.json> <out.png> [--quick] [--only CAMERA,CAMERA]` (Blender Cycles photographic still, materials bound by name; the imported scene is cached, so a second render of the same world starts in seconds) | `pt dem-manifest <manifest> --id ID --lat .. --lon .. --zoom Z --size METRES` (tile list of an elevation window, then `pt corpus fetch`) | `pt scorecard <recipe> <scene.json> --out DIR [--quick] [--no-render] [--no-fps] [--no-lod] [--bake] [--judge] [--brief T] [--foil T]... [--baseline OLD.json] [--notes TEXT]` (one build measured by Blender, Godot, the glTF validator and the vision judge; writes world.glb for the render and world.engine.glb for the engine; the brief comes from the scene file's `brief`) | `pt judge status|frames|parts|compare|rate|which|critique|audit` (vision judges, see AGENT_GUIDE) | `pt terrain <recipe> [--out DIR] [--px N] [--find-valley RADIUS_M] [--reaches N --reach-spacing M]` (terrain statistics, a shaded top-down map, a search for valley floors, and, for an `hf_flow` grid with channels, a water map and the channel's centre line with heights) | `pt bench [a]` | `pt golden` | `pt optimize-demo` |
`pt recover-demo` | `pt vary-bench` | `pt gym` (JSON-lines env) | `pt gym-baseline` | `pt top <farm.json>` | `pt demo` | `pt render`.
The agent tool server is registered by the checked-in `.mcp.json` (build once with `cargo build --release -p pt-cli`; it starts `target/release/pt.exe mcp --ui 7700`, and the page at `http://127.0.0.1:7700` shows the live scene).

## MCP tools (agent surface)

Discover: `list_node_types search_nodes describe_node get_guide list_recipes get_recipe`. Author: `run_hcs check_hcs graph_to_hcs
batch create_node set_param connect disconnect delete_node load_graph make_asset list_assets set_frame`. Inspect: `cook validate
check_graph explain get_graph`. See: `render_preview render_final render_scene critique`. Search: `optimize vary fit_wrangle`. Versioning:
`checkpoint restore undo redo diff merge`. Deliver: `export` (obj, gltf, usda, ptgeo).

## Pro-quality tools (built 2026-10-10)

| Command | What it does |
|---|---|
| `pt corpus fetch/verify [manifest]` | licence ledger `corpus/LEDGER.csv` (sha256 per file, CC0 only); downloads with the system `curl`, zip members read in memory |
| `pt parts <ref>`, `pt describe <ref> [--brief]` | size, parts, and text cross-sections of a `.glb/.gltf/.obj` |
| `pt match <ref> <recipe or geometry> [--report DIR]` | F-score, Chamfer, silhouette IoU (10 views), normal consistency, validity, knob count, tier, pair picture |
| `pt refine <ref> <recipe.hcs>` | CMA-ES over the numbers in a recipe's source (decimals only; `--tune-integers` for machine-written ones) |
| `pt match-bench <manifest> --run NAME [--recipes DIR --only-recipes --resume ...]` | batch runner: results `corpus/results/NAME.json`, tuned recipes `corpus/recipes/tuned/NAME/`, pictures `D:\Fekri\corpus\out\NAME\pairs\` |
| `pt audit <manifest> <results.json>...` | second reading: largest uncovered patch per recipe |
| `pt report --baseline A.json --llm B.json --notes N.md --eye E.json --probe DIR --out DIR [--fragment]` | the checkpoint page |
| `pt export <recipe> <out.obj/glb/gltf/usda>` | cook and export (geometry, material names, instances; glTF keeps `Cd` as COLOR_0) |
| `pt render-scene <recipe\|glb> <scene.json> <out.png> [--quick] [--samples N] [--keep DIR]` | cook, check the scene file, render with Blender (`corpus/tools/render_scene.py`); prints the report (frames, triangles, material coverage, unresolved and unknown materials, warnings). Library: `corpus/materials/library.json`; fetch sets: `pt corpus fetch corpus/materials_manifest.json` (generated by `corpus/tools/make_materials_manifest.ps1`). Example: `scenes/forest_clearing.*` |

Code: `pt-core/src/gltf.rs` (GLB reader and writer), `pt-opt/src/{matching,solve,recipe}.rs` (metrics with an exact closest-point BVH, the primitive solver, source-level knob counting and refinement), `pt-cli/src/{corpus,matchcmd,report}.rs`. Blender 4.5.14 (portable, official zip, checksum verified) is at `D:\Fekri\tools\blender\blender-4.5.14-windows-x64\`; the probe script is `corpus/tools/probe_render.py`. Raw corpus: `D:\Fekri\corpus\` (`PT_CORPUS` overrides). Agent workflow and conventions: AGENT_GUIDE, "Matching a reference model".

## Documentation harness

| Command | What it does |
|---|---|
| `pt status [--write]` | prints, or with `--write` stores in `docs/generated/state.md`, every number the docs may quote (node types, test functions, recipes, gym, ledger, scorecard runs) |
| `pt gate [--quick] [--only STEPS]` (alias `cargo gate`) | the checks every commit must pass, in order: `build`, `lint` (clippy on all production code: the platform-independent maths rule of `clippy.toml` and the ban on `unwrap`, `expect`, `panic!` and friends set in the root `Cargo.toml`), `docs` (below) and `tests`; stops at the first failure and names the step; CI runs the same lint step |
| `pt docs-check` | fails on a stale `state.md`, a broken link, a file over its size budget, a banned heading, a mangled Windows path, a STATUS older than the last commit, or a document missing from the index; runs in CI |

## Placement and frame review

| Command | What it does |
|---|---|
| `pt ground-check <recipe.hcs> <scene.json> [--radius M] [--frames DIR --annotate DIR]` | cooks the world on the CPU (no render, no GPU) and lists, per scene camera, objects that float, hang over a slope or bank, are mostly under water (flat lids) or buried; with `--frames` it writes `ground_<camera>.png` with each one boxed (red float, orange hanging, yellow shallow, magenta buried). Fails when anything floats or hangs inside a view. It measures the real shape, not the bounding box (`pt_core::seat`, the measure the `seat` node places by): the base of a tree or a grass tile in the air is hanging, a log standing on a stub over ground or water is floating; `soles` lists each prototype's base width and where its body starts. Two rows of `pt scorecard` use the same rules |
| `pt review-tiles <frame.png> <out_dir>` | cuts a frame into six tiles, each enlarged 2x, so a frame is reviewed at full resolution tile by tile |
| `pt gen-texture needles OUT.png [--seed N] [--size PX]` | draws a conifer needle spray (RGBA, transparent background) with the program itself, for the cards of the node `tree_conifer`; the library material `conifer_needles` (kind `foliage`) points at `corpus/generated/needles_a.png` |
| `pt world <manifest> info \| scene \| render OUT.png \| check \| pick CAMERA X Y [--variant NAME] [flags]` | a world is a layer stack named by `worlds/<name>.world.json` (recipe, scene, variants of style + knobs + owner layer); the variant is expanded into a scene file under `out/world/` and the usual command runs on it. Recipes read the style's knobs with `knob(name, default)` (AGENT_GUIDE "One world, many looks") |
| `pt edit <edits.json> <scope> <pid> remove \| move DX DY DZ [--to] \| scale F` and `pt edit <edits.json> <scope> add X Y Z [PSCALE]` | records a decision about one placed object in the world's edits file (scope = the kind of object), keyed by the stable `pid` that `pt pick` reports; the recipe replays it with `apply_edits(points, ground, edits=edits_of(file, scope))`; a later decision about the same object and change replaces the earlier one (AGENT_GUIDE "Placement that keeps its identity") |
| `pt render-scene <recipe> <scene.json> <out.png> --only CAMERA[,CAMERA] --lookdev [--cull M] [--quick] [--fresh]` | look development: `--lookdev` is the cheap tier (recipes read it with `lookdev(full, light)`: lighter scans, thinner grass tiles) and, with `--only`, crops the world to what those cameras see (instances, far stand-ins and terrain cells outside the view; radius `--cull`, default 60 m). The cooked world is cached next to its model file (`out/render/<stem>.glb` and `.world.json`, keyed by recipe, program bytes, tier and crop): a render that changes only the scene file, a style or the render script skips the cook, and the import cache skips Blender's import; `--fresh` forces a cook |
| `pt pick <recipe.hcs> <scene.json> <camera> <x> <y> [--radius PX] [--frame F.png] [--size WxH] [--annotate OUT.png]` | what is at a pixel of a frame and what made it: casts the camera ray into the cooked world (CPU only) and names the prototype, instance, creating node and parameters, height above the ground and ground-check verdict; `--radius` counts the classes of object in a region. Also the MCP tool `pick`. Code: `pt-cli/src/pick.rs`, `pt_core::spatial` |
