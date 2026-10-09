# Project status and handoff (read this first)

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust. Master plan:
[PLAN.md](PLAN.md). Research behind the design decisions: `reports/Houdini clone architecture validation.md`.
Agent-facing usage guide: [AGENT_GUIDE.md](AGENT_GUIDE.md).

## Verified state (last full run)

* `cargo test --workspace --release`: **99 tests pass**, 0 failures. `cargo clippy --workspace --all-targets`: 0 errors.
* `pt recipes`: **24/24 recipes** pass their specs, static check and golden hashes (`recipes/golden.json`).
* `pt gym-baseline`: oracle 1.000, perturbation agent 0.744, null agent 0.401 over 29 tasks (`gym/baseline.json`).
* Not yet verified: determinism **across operating systems** (only across thread counts), anything on Linux/macOS.

## Environment (Windows, everything on D:)

* Rust 1.99 GNU toolchain + portable MinGW at `D:\Fekri\toolchain` (`RUSTUP_HOME`, `CARGO_HOME` set as user env vars).
  In a fresh shell prepend `D:\Fekri\toolchain\cargo\bin;D:\Fekri\toolchain\mingw\mingw64\bin` to PATH.
* No Python, no Node on this machine. Use bash/PowerShell/curl. `target/` and `out/` are generated.
* Tooling gotchas: in Bash tool commands, double backslashes get collapsed (write Rust files with the Write/Edit tools, not
  heredocs, when they contain `\\` or `\u{..}`); multi-line `sed` rewrites by line number are fragile (re-read first).

## Crates

| Crate | Role |
|---|---|
| `pt-core` | Geometry v2 (columnar COW attributes, instance tables, groups, volumes), deterministic math/RNG/noise, OBJ/glTF/USD/`ptgeo` I/O, validation. |
| `pt-vex` | VEX-style language: lexer, parser, type checker (line/col diagnostics with fixes), interpreter generic over autodiff channels, `fit` (gradient fitting). |
| `pt-sop` | ~73 node types: primitives, curves + deformers (bend twist taper sweep revolve resample drop_to_surface...), attributes, groups, topology ops, terrain, SDF, simulation adapters, fused point kernel, attribute-flow `effect()`. |
| `pt-sim` | Solvers: particles, PBD cloth, smoke (stable fluids), rigid bodies (rapier3d, enhanced-determinism). |
| `pt-graph` | Node DAG, content-addressed cook cache with early cutoff, provenance, expressions (`$F`, `ch("node/param")`), assets/`repeat`, static `check()`, `merge` (diff + 3-way). |
| `pt-hcs` | Agent language (Python syntax): functions, loops, comprehensions, `spec()`, `asset()`, `frame()`, `to_hcs()`. |
| `pt-opt` | CMA-ES optimizer, objectives (chamfer, size, silhouette, image, specs), variation engine. |
| `pt-render` | Software rasterizer (silhouettes, heatmaps), CPU path tracer (BVH), image metrics. |
| `pt-cli` | Binary `pt` + library: MCP server, web UI, gym, recipe runner, TOP farm, demos. |

## Commands

`pt mcp [--ui PORT]` (MCP over stdio; UI shares the same live session) | `pt ui [PORT]` | `pt recipes [--update]` |
`pt shot <recipe> <out.png> [spp] [az]` (path-traced still) | `pt bench [a]` | `pt golden` | `pt optimize-demo` |
`pt recover-demo` | `pt vary-bench` | `pt gym` (JSON-lines env) | `pt gym-baseline` | `pt top <farm.json>` | `pt demo` | `pt render`.
Register with Claude Code: `claude mcp add proteus -- D:\Fekri\Houdini-clone\target\release\pt.exe mcp --ui 7700`.

## MCP tools (agent surface)

Discover: `list_node_types search_nodes describe_node get_guide list_recipes get_recipe`. Author: `run_hcs check_hcs graph_to_hcs
batch create_node set_param connect disconnect delete_node load_graph make_asset list_assets set_frame`. Inspect: `cook validate
check_graph explain get_graph`. See: `render_preview render_final critique`. Search: `optimize vary fit_wrangle`. Versioning:
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
* Sparse (VDB-like) volumes -> dense volumes only so far; own tiled hash grid planned (patent-safe, see PLAN D8).
* 80+ node types -> 73 now; remaining ops (polybevel, convex hull, lattice, atan2 in VEX) are small and follow `crates/pt-sop/src/deform.rs`.
* Cross-OS hash check -> CI workflow written (`.github/workflows/ci.yml`) but never run; needs a GitHub push.
* Legal/naming: original names only; never run or study Houdini (EULA, see PLAN D1). Final product name still to be decided.

## Suggested next steps

1. Push the repo to GitHub and watch the first CI run (`.github/workflows/ci.yml` is written: 3 OSes, tests, recipes, cross-OS golden compare, unverified until it runs).
2. DONE: recipe promotion (`propose_recipe` MCP tool -> `recipes/proposed/`, `pt recipes-accept [name]` promotes and updates golden). Next: a gym task that rewards good proposals.
3. DONE: recipes 18-24 (bolt, canyon, forest clearing, city block, lathe vase, spring+cable, hillside village) cover T1-T3; 11 new nodes in `deform.rs`. Next: a real VLM-judged critique experiment, gym tasks that are not recipes, sparse volumes, COP-lite.
4. Compositing (COP-lite), more simulation (pyro/FLIP), sparse volumes, Alembic/FBX interop.
5. Update PLAN.md phases as items land; keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`).
