# Project status and handoff (read this first)

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust. Master plan:
[PLAN.md](PLAN.md). Research behind the design decisions: `reports/Houdini clone architecture validation.md`.
Agent-facing usage guide: [AGENT_GUIDE.md](AGENT_GUIDE.md).

## Verified state (last full run)

* `cargo test --workspace --release`: **169 tests pass**, 0 failures (33 added by the pro-quality Phase 0 tools, 8 by Level 2, 15 by the steel thread and engine delivery). `cargo clippy --workspace --all-targets`: 0 errors.
* `pt recipes`: **25/25 recipes** pass their specs, static check and golden hashes (`recipes/golden.json`).
* `pt gym-baseline`: oracle 1.000, perturbation agent 0.730, null agent 0.438 over 41 tasks (24 build, 6 repair, 5 edit, + 6 T1; `pt gym` kinds: build | repair | edit) (`gym/baseline.json`).
* Cross-OS determinism: Windows (MSVC and GNU) matches Linux and macOS on all recipes once the physics math stack is patched (`vendor/`, see vendor/README.md). Repo: https://github.com/fekrimuf/proteus (private), CI in `.github/workflows/ci.yml`.

## Environment (Windows, everything on D:)

* Rust 1.99 GNU toolchain + portable MinGW at `D:\Fekri\toolchain` (`RUSTUP_HOME`, `CARGO_HOME` set as user env vars).
  In a fresh shell prepend `D:\Fekri\toolchain\cargo\bin;D:\Fekri\toolchain\mingw\mingw64\bin` to PATH.
* No Python, no Node on this machine. Use bash/PowerShell/curl. `target/` and `out/` are generated.
* Referees for `pt scorecard` (all on D:, none installed system-wide): Blender 4.5.14 at `D:\Fekri\tools\blender\blender-4.5.14-windows-x64\` (`PT_BLENDER`), Godot 4.7.2 (MIT, sha512 checked) at `D:\Fekri\tools\godot\` (`PT_GODOT`, use the `_console.exe`), Khronos glTF-Validator 2.0.0-dev.3.10 at `D:\Fekri\tools\gltf_validator\` (`PT_GLTF_VALIDATOR`; no published checksum, taken from the official release). GPU: RTX 2070 SUPER (OptiX works in Blender, Vulkan in Godot).
* Recipe-language gotchas learned: no `**kwargs`, `asset` is a reserved built-in name, node parameters named `class` are fine as kwargs. When patching Rust or recipes from the shell never use perl `s|..\|..|` (an escaped delimiter turns into an alternation and silently inserts text at the top of the file: it happened three times); use the Edit tool or index/substr scripts, and write multi-line code with the Write tool, not heredocs with quotes.
* GDScript/Godot gotchas learned: a Packed array stored in a Dictionary is copied by a cast (use a plain Array for grids); comments are `#` not `//`; `await physics_frame` several times before using the navigation map, and `NavigationServer3D.map_get_closest_point` returned nothing for a baked mesh here, so reach is tested geometrically; Godot's `get_rendering_info` primitives count includes shadow passes; vsync must be switched off to measure frame rate; a regex with backslashes is mangled by the Bash tool (use string functions).
* Tooling gotchas: in Bash tool commands, double backslashes get collapsed (write Rust files with the Write/Edit tools, not
  heredocs, when they contain `\\` or `\u{..}`); multi-line `sed` rewrites by line number are fragile (re-read first).

## Crates

| Crate | Role |
|---|---|
| `pt-core` | Geometry v2 (columnar COW attributes, instance tables, groups, volumes), deterministic math/RNG/noise, OBJ/glTF/USD/`ptgeo` I/O, validation. |
| `pt-vex` | VEX-style language: lexer, parser, type checker (line/col diagnostics with fixes), interpreter generic over autodiff channels, `fit` (gradient fitting). |
| `pt-sop` | ~79 node types (incl. `set_material`, `blend_materials`, `import_asset`, `copy_variants`): primitives, curves + deformers (bend twist taper sweep revolve resample drop_to_surface...), sparse SDF ops (sparse_remesh, sparse_boolean), attributes, groups, topology ops, terrain, SDF, simulation adapters, fused point kernel, attribute-flow `effect()`. |
| `pt-sim` | Solvers: particles, PBD cloth, smoke (stable fluids), rigid bodies (rapier3d, enhanced-determinism). |
| `pt-graph` | Node DAG, content-addressed cook cache with early cutoff, provenance, expressions (`$F`, `ch("node/param")`), assets/`repeat`, static `check()`, `merge` (diff + 3-way). |
| `pt-hcs` | Agent language (Python syntax): functions, loops, comprehensions, `spec()`, `asset()`, `frame()`, `to_hcs()`. |
| `pt-opt` | CMA-ES optimizer, objectives (chamfer, size, silhouette, image, specs), variation engine. |
| `pt-render` | Software rasterizer (silhouettes, heatmaps), CPU path tracer (BVH), image metrics. |
| `pt-cli` | Binary `pt` + library: MCP server, web UI, gym, recipe runner, TOP farm, demos. |

## Commands

`pt mcp [--ui PORT]` (MCP over stdio; UI shares the same live session) | `pt ui [PORT]` | `pt recipes [--update]` |
`pt shot <recipe> <out.png> [spp] [az]` (path-traced still) | `pt render-scene <recipe|glb> <scene.json> <out.png> [--quick]` (Blender Cycles photographic still, materials bound by name) | `pt scorecard <recipe> <scene.json> --out DIR [--quick] [--no-render] [--no-fps] [--no-lod] [--no-bake] [--baseline OLD.json] [--notes TEXT]` (one build measured by Blender, Godot and the glTF validator; writes world.glb for the render and world.engine.glb for the engine) | `pt bench [a]` | `pt golden` | `pt optimize-demo` |
`pt recover-demo` | `pt vary-bench` | `pt gym` (JSON-lines env) | `pt gym-baseline` | `pt top <farm.json>` | `pt demo` | `pt render`.
Register with Claude Code: `claude mcp add proteus -- D:\Fekri\Houdini-clone\target\release\pt.exe mcp --ui 7700`.

## MCP tools (agent surface)

Discover: `list_node_types search_nodes describe_node get_guide list_recipes get_recipe`. Author: `run_hcs check_hcs graph_to_hcs
batch create_node set_param connect disconnect delete_node load_graph make_asset list_assets set_frame`. Inspect: `cook validate
check_graph explain get_graph`. See: `render_preview render_final render_scene critique`. Search: `optimize vary fit_wrangle`. Versioning:
`checkpoint restore undo redo diff merge`. Deliver: `export` (obj, gltf, usda, ptgeo).

## Measured gates

| Gate | Result |
|---|---|
| P1: 5M instances + terrain cook+preview | 0.89 s, 856 MB peak (`pt bench a`) |
| P1: incremental recook | 1 param change recomputes only the downstream cone |
| P1/D2: determinism | bit-identical across thread counts; cross-OS **not yet tested** |
| P2: autodiff | forward-mode gradients match finite differences on 8 programs |
| P3: fit rock to held-out noisy scans | 98-101% of achievable gain, 1500 evals, 5 restarts (`pt optimize-demo`) |
| P3: 64-variant exploration | 17.6x faster with the shared cache (`pt vary-bench`) |
| P3: mesher robustness | surface nets alone non-manifold/open in 14/24 fuzz cases; `auto` fallback fixed 24/24 |
| P6: recover params from silhouettes only | IoU 0.998 / 0.992 / 0.988, coarse-to-fine (`pt recover-demo`) |
| P4: gym | oracle 1.00 / perturbed 0.74 / null 0.40 |

## Not built (with the route for each)

* Native GUI window -> the web UI is the GUI; an egui shell can reuse the same API. Native wgpu viewport not started.
* Fire/pyro, FLIP water, vellum hair/grains, neural surrogates, differentiable simulation -> wrap/port open solvers; interop-first.
* Alembic, FBX, MaterialX, Blender/Unreal live links -> USD/glTF today; others behind cargo features.
* COP (compositing), CHOP, KineFX rigging, crowds, Python bindings, WASM plugins, browser build, CRDT multiplayer -> next; the
  JSON-RPC (MCP) and gym protocols are already language-agnostic.
* Sparse volumes: DONE as `pt_core::SparseVolume` (8^3 tiles, const interior tiles, deterministic) plus `sparse_remesh`/`sparse_boolean` (mesh in, mesh out; `pt sparse-bench`: rock at 2.5 mm voxels = 166 MB vs 4.3 GB dense). Not yet: sparse volumes as a first-class `Geometry` attribute, sparse smoothing/displace/advection (smoke is still dense).
* 80+ node types -> 79 now; remaining ops (polybevel, convex hull, lattice) are small and follow `crates/pt-sop/src/deform.rs`.
* Legal/naming: original names only; never run or study Houdini (EULA, see PLAN D1). Final product name still to be decided.

## Done recently (all verified by tests/CI)

* CI on Windows/Linux/macOS is green incl. the cross-OS hash compare; physics math stack vendored+patched (`vendor/`).
* Recipe promotion (`propose_recipe`, `pt recipes-accept`); recipes 18-25; 14 new nodes (deformers, curves, placement, sparse SDF).
* VEX: `atan2 atan asin acos mod radians degrees` with autodiff.
* Gym: repair tasks (inject a defect into a verified recipe) and edit tasks (change requirements of a given program); the
  starter program is part of the observation; spec credit halves while the static flow check fails.
* Critique experiment (`pt critique-bench`, result in `gym/critique_experiment.json`, judgments in `gym/critique_judgments_claude.json`):
  10 tasks x 4 candidates, same-camera 2x2 sheets, blind judge = Claude reading the sheets. Agreement with spec ground truth:
  vision judge Spearman 0.84 / 92% pairwise / best-pick 90%; SSIM to oracle 0.54 / 75% / 100%; generic `critique` rubric 0.10 / 52%
  (chance). Caveats: n=10 tasks, one judge (the author model, not an external VLM), perturbation defects only, oracle-framed camera.
  Consequence: `render_compare` tool added (shared camera, true sizes), `critique` documented as hygiene-only.

## Current priority

**Read [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) first.** North star (owner's goal): an AUTONOMOUS system that makes a FINISHED AAA world; staged by autonomy rungs A0-A4 and an AAA Readiness Scorecard. Plan covers five levels (assets, appearance, systems, composition, delivery; assets alone do NOT give realistic worlds). Track A: rebuild public-domain professional
assets as recipes, score by measurement, mine reusable building blocks, then train a proposer. Track B: realism probe, then a "photoreal valley" from real elevation data + CC0 scans/materials rendered in Blender. Tomorrow starts at Phase 0 + the Realism Probe.

**Approach review 2026-10-10 (PRO_QUALITY_PLAN B15) supersedes the order below:** steel thread first (brief to scorecard end to end), Godot as the free engine referee, Level 1 reframed as learning generators, worlds built from CC0 assets directly, autonomous judges in every gate, scorecard as the headline KPI, training at $0 on the local GPU.

**State after 2026-10-10:** Phase 0 tools, the Realism Probe and Checkpoint 1 are done and green: the stack solved 24/50 (48%) by the plan's score and 18/50 after a visual check; hold-out 5/10 and 4/10 (PRO_QUALITY_PLAN B14, `corpus/results/checkpoint1/report.html`). **Level 2 steps 1 and 2 are done** (B14, `corpus/results/level2/README.md`): `set_material` names, material-carrying glTF/USD/OBJ, 23 CC0 PBR sets in the ledger, `pt render-scene` and MCP `render_scene` (Blender Cycles, sky/sun/haze/cameras from a scene file, calibrated exposure, repeat renders pixel-identical on CPU). **Steel thread is done and delivered to an engine** (B14, `corpus/results/steelthread/v0` and `v1`, private artifact "Readiness Scorecard"): `import_asset`, `copy_variants`, `blend_materials`, styles, `scenes/valley.hcs`, Godot 4.7.2 as engine referee, the glTF validator, `pt scorecard`, then the engine delivery path (`pt_core::lod` LOD chains with a per-level triangle cap, per-instance LOD nodes, Blender bake of layered terrain to textures, tangents). **Scorecard 11 of 16 (baseline) to 17 of 17 (v1):** 2.1 fps to 93-173 fps, 1.42 billion to 18.5 million triangles in frame, LOD chains 0 of 12 to 24 of 24, validator 19 warnings to 0, terrain textured in the engine, 30 prototypes. Two metrics were corrected, not loosened (navigation reach on the intended walkable ground; repetition per 50 m square for large prototypes). Visual is presence checks only, and the pictures show flat-plaster mountains, thin ground cover and few tree species. **Next, in order (B15):** (1) vision judges so Visual is a measured family (pairwise judge agreed 0.84 in the critique experiment; per-part recall; style distinctness), (2) real elevation data for the valley (W1: USGS 3DEP tile, ledger row, terrain statistics) replacing the analytic terrain, (3) a tree and ground-cover generator family (repetition by generators, CC0 share falls), (4) the four queued recipe repairs (ladder, Shelf_01, bucket, vase), (5) Phase 1 mining on 150 more assets and a fresh 50 for a clean test.

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

Gotchas: `cargo test` and `cargo build` cannot replace `pt.exe` while a bench is running (wait, or use another target dir). `merge` takes at most 4 inputs. In hcs write tunable numbers with a decimal point and structure (counts, quarter turns) as whole numbers. Never name a bench run like the folder that holds hand-written sources (the first version overwrote them; output now goes to `corpus/recipes/tuned/`). A full 50-asset baseline run takes about 8 minutes. Quaternius is excluded (licence changed, see `corpus/SOURCES.md`).

## Suggested next steps

1. Repeat the critique experiment with an external VLM and with a second judge to check the 0.84 isn't judge-specific; add defects
   that perturbation can't produce (missing parts, wrong topology). Feed `render_compare` into the gym as an observation option.
2. Sparse volumes as a first-class `Geometry` attribute (smooth/displace/offset on tiles, cache serialization), then sparse smoke.
3. More gym tasks: multi-step "conversation" edits, tasks scored by reference similarity, and a task that rewards good recipe proposals.
4. Compositing (COP-lite), pyro/FLIP, Alembic/FBX interop, polybevel/convex hull/lattice nodes.
5. Keep PLAN.md phases, `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).
