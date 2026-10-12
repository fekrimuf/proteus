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

`pt mcp [--ui PORT]` (MCP over stdio; UI shares the same live session) | `pt ui [PORT]` | `pt recipes [--update]` |
`pt shot <recipe> <out.png> [spp] [az]` (path-traced still) | `pt render-scene <recipe|glb> <scene.json> <out.png> [--quick] [--only CAMERA,CAMERA]` (Blender Cycles photographic still, materials bound by name; the imported scene is cached, so a second render of the same world starts in seconds) | `pt dem-manifest <manifest> --id ID --lat .. --lon .. --zoom Z --size METRES` (tile list of an elevation window, then `pt corpus fetch`) | `pt scorecard <recipe> <scene.json> --out DIR [--quick] [--no-render] [--no-fps] [--no-lod] [--bake] [--judge] [--brief T] [--foil T]... [--baseline OLD.json] [--notes TEXT]` (one build measured by Blender, Godot, the glTF validator and the vision judge; writes world.glb for the render and world.engine.glb for the engine; the brief comes from the scene file's `brief`) | `pt judge status|frames|parts|compare|rate|which|critique|audit` (vision judges, see AGENT_GUIDE) | `pt terrain <recipe> [--out DIR] [--px N] [--find-valley RADIUS_M] [--reaches N --reach-spacing M]` (terrain statistics, a shaded top-down map, a search for valley floors, and, for an `hf_flow` grid with channels, a water map and the channel's centre line with heights) | `pt bench [a]` | `pt golden` | `pt optimize-demo` |
`pt recover-demo` | `pt vary-bench` | `pt gym` (JSON-lines env) | `pt gym-baseline` | `pt top <farm.json>` | `pt demo` | `pt render`.
Register with Claude Code: `claude mcp add proteus -- D:\Fekri\Houdini-clone\target\release\pt.exe mcp --ui 7700`.

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
| `pt docs-check` | fails on a stale `state.md`, a broken link, a file over its size budget, a banned heading, a mangled Windows path, a STATUS older than the last commit, or a document missing from the index; runs in CI |

## Placement and frame review

| Command | What it does |
|---|---|
| `pt ground-check <recipe.hcs> <scene.json> [--radius M] [--frames DIR --annotate DIR]` | cooks the world on the CPU (no render, no GPU) and lists, per scene camera, objects that float, hang over a slope or bank, are mostly under water (flat lids) or buried; with `--frames` it writes `ground_<camera>.png` with each one boxed (red float, orange hanging, yellow shallow, magenta buried). Fails when anything floats or hangs inside a view. Two rows of `pt scorecard` use the same rules |
| `pt review-tiles <frame.png> <out_dir>` | cuts a frame into six tiles, each enlarged 2x, so a frame is reviewed at full resolution tile by tile |
| `pt pick <recipe.hcs> <scene.json> <camera> <x> <y> [--radius PX] [--frame F.png] [--size WxH] [--annotate OUT.png]` | what is at a pixel of a frame and what made it: casts the camera ray into the cooked world (CPU only) and names the prototype, instance, creating node and parameters, height above the ground and ground-check verdict; `--radius` counts the classes of object in a region. Also the MCP tool `pick`. Code: `pt-cli/src/pick.rs`, `pt_core::spatial` |
