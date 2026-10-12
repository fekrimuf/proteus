# Houdini Clone — Master Plan (v2)

## 1. Thesis

Houdini's genius is **procedural, non-destructive, attribute-driven dataflow**. Its mature features (packed prims, paged attributes, PDG, USD layers) are bolted onto a decades-old core; what it still lacks is **uniform** sharing and instancing by default, content-addressed cross-machine caching, static checking of networks, differentiability, and an interface designed for machines. A human must also pick the right nodes out of thousands.

We keep the genius and replace the architecture. The result is not "Houdini, but free":

> **A procedural-content *compiler* with a verifiable, differentiable, content-addressed runtime, whose primary users are agents and whose output is professional quality by construction.**

Three outcomes define success:

1. **Agent-native.** An agent connects with one command, writes scenes as short typed programs, gets static feedback before cooking, sees renders, and can search, optimize and verify, with deterministic, replayable results.
2. **Pro-grade.** Agents reproduce what top Houdini artists produce (terrain, destruction, FX, buildings, vegetation, lookdev) because the platform encodes their techniques as tested, composable, measurable assets.
3. **Better than Houdini on axes Houdini cannot reach:** inverse design (differentiable), lazy/infinite worlds, shared global caches, provenance ("why does this vertex exist?"), branching scenes, and interactive neural-surrogate simulation.

## 2. Five foundational bets (the architecture)

Every other feature sits on these. They are decided now because retrofitting any one of them is a rewrite.

### Bet 1 — Graph → IR → fused kernels (a compiler, not an interpreter)
- Nodes **lower** to a typed dataflow **IR** over columnar attributes. A scene is a program; cooking is compilation plus execution.
- **Fusion:** chains of pointwise work (transform, noise, wrangles, attribute math, remap, color) collapse into one pass over memory. Fusion removes intermediate passes and buffers; the gain is a hypothesis that the Phase 1 gate must measure.
- **Backends:** CPU (SIMD + rayon; Cranelift JIT for user VEX), GPU (WGSL generated from the same IR, via wgpu), and an interpreter reference backend used as the correctness oracle.
- **Autodiff is an IR transform** (source-to-source, forward and reverse), so *any* VEX snippet or fused chain is differentiable without per-node hand-written gradients. Nodes opt out by declaring `Differentiability::{Smooth, Piecewise, None}`.
- **Incremental by construction:** each op declares its dependency granularity (whole-geo, per-point, per-prim, per-tile). Changing one parameter recomputes only the affected region.

### Bet 2 — Geometry v2: columnar, copy-on-write, instance-lazy, multi-representation
- **Columnar attribute store** (Arrow-like): typed columns (f32/f64/i32/u8, vec2–4, mat3/4, quat, interned string, arrays, dict) in domains Point / Vertex / Primitive / Detail / Edge. Columns are `Arc`'d chunks: a node that edits `P` shares every other column with its input. Target: ≥2× lower peak memory than a naive copy-per-node baseline on wide geometry (Blender reports ≥25% in simple cases; we must measure ours). All sharing goes through one kernel API, not per-node logic, to avoid Blender-style sharing bugs.
- **Lazy instancing:** `copy_to_points` and scatter produce an **instance table** (prototype ref + per-instance transform/attrs), not N copies. 100M instances cost the memory of 100M transforms. Realize on demand (`unpack`/export). Houdini's packed prims are the closest analogue but restrict per-instance attributes; ours is the default and carries arbitrary per-instance attributes.
- **One container, many representations:** `Geo` holds any mix of **meshes/points, curves, sparse volumes (VDB-like tiles), implicit fields (SDF expression trees), Gaussian splats, instance tables, and heightfields**, with first-class conversions between them (mesh↔SDF↔volume↔splat). Boolean-by-SDF, remesh-by-SDF and splat capture/import (3DGS) are native.
- **Groups** as bitsets/ranges, **topology** as lazily built half-edge adjacency, all cached by content hash.
- **Provenance column (optional, on by default in dev):** every point/prim carries `(node, op, generation)`. `explain(vertex)` returns the chain of nodes and parameters that produced it. This enables debugging for agents, direct manipulation in the UI, and impact analysis.
- **Semantics & units:** geometry and fields carry units (meters by default) and optional semantic tags (`wall`, `door`, `roof`, `ground`). Scale errors, the commonest agent failure, become static errors.

### Bet 3 — Content-addressed, deterministic, pure-function runtime
- Every cook is a pure function: `key = H(op id + version, params, input hashes, seeds, backend-class)`. Results live in a **content-addressed store (CAS)**.
- **Deterministic by contract:** fixed reduction orders, no fast-math in the reference path, seeded counter-based RNG, sorted parallel merges. CI re-cooks and compares hashes.
- **Consequences, all free:** persistent disk cache; **shared remote cache** across machines and agents (like Bazel remote cache): the second agent to cook the same terrain gets it instantly; trivially distributed cooking on a farm; reproducible bug reports ("here's the hash"); perfect regression tests; branch/diff/merge of scenes is a merge of hashes.
- **Demand-driven, region/LOD-aware cooks:** ask for "the tile at (x,z) at LOD 2" or "what the camera sees". This makes infinite terrain and 100M-object cities feasible and makes previews fast. Houdini cooks whole networks.

### Bet 4 — Agents get a language, a type system, a search engine, and a gym
- **`hcs` — a text language for graphs.** Typed, Rust-flavored, round-trips with the graph. Agents write 30 lines instead of issuing 200 tool calls; humans can read and diff it; recipes are `hcs` modules. (Tools via MCP remain for fine-grained edits.)
  ```
  let terrain = grid(size=[2km, 2km], res=1m) |> fbm(freq=0.002, oct=6) |> erode(iters=40);
  let trees   = scatter(terrain, density=0.02/m2, mask=slope < 25deg) |> instance(oak, jitter=0.3);
  out merge(terrain, trees);
  ```
- **Static analysis before cooking:** an **attribute-flow type system** knows at each node which attributes, groups, domains and units exist. `error: node 'copy1' reads @pscale but 'scatter1' (input 1) does not write it; add set_pscale or pass scale=` fires without cooking anything. Typed holes (`?`) let an agent leave a step open and ask `suggest(hole)`.
- **Specs as first-class:** `assert bbox.height in 1.8m..2.2m`, `assert manifold`, `assert no_self_intersection`, `assert min_clearance(path, 0.9m)`. Specs run on every cook and become the agent's definition of done.
- **Search over edits:** the platform exposes `fork/eval/score/merge` primitives for beam search and MCTS over graph edits, with parallel cooks sharing the CAS cache, so exploring 64 variants is cheap.
- **`HoudiniGym`:** the whole system as a Gymnasium-style environment: observation = (graph, stats, renders, spec results), action = edits, reward = benchmark rubric/metric. It lets us evaluate any model's modeling skill, train or fine-tune agents with RL on procedural modeling, and track platform usability as a number.
- **Observability for agents:** cook traces, per-node timings and memory, flame graphs, provenance queries, attribute histograms, and diff-of-cooks ("what changed in the output when I changed this param?").

### Bet 5 — Differentiable and learnable end to end
- **Inverse design:** `optimize(graph, free_params, objective, budget)`. Objectives: image/silhouette match, mesh/SDF fit (Chamfer, SDF L2), volume/mass/height profiles, physical constraints (stability, slope, clearance), and *spec satisfaction* as a soft loss.
- **Hybrid solver:** gradients for continuous parameters (autodiff via IR), CMA-ES / Bayesian optimization / beam search for discrete choices and topology, all on the deterministic cached runtime so thousands of evaluations are cheap and reproducible.
- **Differentiable rendering:** soft rasterizer first (GPU-free, fast), differentiable path tracing later.
- **Differentiable simulation where feasible:** cloth/rigid/particles first (design parameters through the solver, e.g. "make this cloth drape land here"); fluids via adjoint or surrogate.
- **Neural surrogates:** small networks trained from solver caches (smoke, erosion, cloth statistics, FLIP) give interactive previews in seconds. Two quality tiers (`preview`, `final`) share one graph; confidence estimates trigger fallback to the exact solver.
- **Learned priors:** a library embedding of recipes and geometry (shape/material/style embeddings) powers `find_recipes(brief)`, `similar_to(geo)` and parameter initialization for the optimizer.

## 3. Reaching top-professional quality (the system, not hope)

1. **Recipe library = encoded craft.** Tested, parametrized, versioned HDAs per domain: terrain (erosion, masks, scatter), buildings/cities (rules, modules, WFC), vegetation (L-systems, growth), destruction (fracture, constraints), pyro/FLIP/cloth/hair, crowds, procedural materials, lighting/camera rigs. Each recipe ships with: knobs (semantic, unit-bearing), specs, reference renders, a failure gallery, and a technique note (why a pro does it this way).
2. **Technique guides as data.** `get_guide("rocks")` returns structured advice: layering noise octaves, breaking symmetry, secondary detail, real-world scale references, wear patterns. Agents consume it as context, humans as docs.
3. **Presentation by default.** Standard lookdev, lighting and camera rigs, HDRIs, turntable and contact-sheet renders so output is always shown well, because quality is judged on the image.
4. **Critique loop.** `critique(render, rubric, reference?)` combines metric scoring (silhouette IoU, detail density, edge wear stats, color distribution, FID-like embeddings) with a rubric prompt for a VLM-based judge, scored per criterion.
5. **Benchmarks with a difficulty ladder** (see §7).
6. **Parity checks.** Compare our node outputs against published papers, third-party tutorials and open implementations (Blender, Infinigen, libigl). We never run Houdini to extract behavior (see §12, legal protocol).
7. **Expert distillation.** A pro records a technique once as a recipe + guide + specs; every agent applies it at scale. Contributions are reviewed and scored before promotion.

## 4. The agent surface

| Layer | What it gives |
|---|---|
| **Connect** | `hc mcp` (stdio), HTTP/WebSocket JSON-RPC, one-line registration for Claude Code and other MCP clients. Session = a live scene with history. |
| **Author** | `hcs` programs (primary) + fine-grained tools: `create_node`, `set_param`, `connect`, `batch` (atomic), `create_subnet`, `promote_param`, `save_hda`, `create_node_type` (WASM plugin from source). |
| **Discover** | `search_nodes(intent)`, `describe_node`, `find_recipes(brief)`, `similar_to(geo)`, `get_guide(topic)`, `examples(node)`. |
| **Check** | Static attribute-flow/unit analysis, `validate` (manifold, self-intersection, NaN, degenerate, scale), `assert` specs. |
| **Inspect** | `cook` stats with attribute min/max/mean/histograms, `spreadsheet(query)`, `explain(vertex)`, `diff_cooks(a, b)`, timings and memory. |
| **See** | `render_preview` (software, no GPU): shaded / wire / attribute-heatmap / section / turntable sheet; `compare_images`; progressive final renders. |
| **Search** | `fork`, `eval`, `score`, `merge`, `vary(n, diversity)`, `optimize(...)`, `suggest(hole)`. |
| **Versioning** | Content-addressed history, checkpoints, branches, three-way merge of graphs, shareable hashes. |
| **Scale** | Async cooks with progress events, cancel, distributed cooking through the shared CAS, background sims with frame streaming. |
| **Train/Eval** | `HoudiniGym` env, task suite, trajectory recording and replay. |

Every error is machine-actionable: it names the cause and the fix, and gives the valid options.

## 5. Houdini feature coverage (the floor)

Parity with Houdini's contexts, built on the bets above (not copied from its architecture):

| Houdini | Our implementation | Crate | Pri |
|---|---|---|---|
| SOP geometry | 200+ ops: primitives, attributes, groups, copy/instance, scatter, noise, boolean (exact + SDF), remesh, subdivide, extrude, bevel, sweep, resample, ray, transfer, for-each/solver loops, UV tools, curves, packed/instanced | hc-sop | P0 |
| VEX/VOP | Typed language, point/prim/detail/volume wrangles, VOP → IR; Cranelift + GPU backends; autodiff | hc-vex | P0 |
| Parameters | Expressions, `ch()`, `chramp()`, references, ramps, spare/promoted params, units | hc-graph | P0 |
| HDA | Subnets → assets, versioning, semantic knobs, specs, library | hc-graph | P0 |
| Volumes/VDB | Sparse tiled grids, SDF, advect, smooth, combine, meshing | hc-core/vol | P1 |
| Heightfields | Noise, erode, masks, scatter, layers | hc-terrain | P1 |
| Viewport/UI | wgpu viewport, node editor (egui), param pane, spreadsheet, handles; **same API as agents; shows agent cursor** | hc-app | P1 |
| DOP | POP, RBD (fracture + constraints), vellum (cloth/hair/soft), grains, FLIP, pyro on sparse grids; checkpointed, branchable, time-travel | hc-sim | P1–2 |
| LOP/USD | Stage, layers, variants, MaterialX, lights, cameras | hc-lop | P1 |
| Render (Karma class) | CPU path tracer → wgpu; PBR, volumes, area lights, HDRI, AOVs, denoise; splat rendering | hc-render | P1–2 |
| COP | CPU/GPU image network | hc-cop | P2 |
| TOP/PDG | Task graph over the CAS (distribution is native) | hc-top | P2 |
| CHOP | Channels/audio/motion | hc-chop | P3 |
| KineFX/APEX | Skeleton, rigging, retargeting | hc-rig | P3 |
| Crowds | Agents, states, navigation | hc-crowd | P3 |
| Scripting | Rust API, `hcs`, embedded Python, WASM plugins | hc-py/plug | P2 |
| Interop | glTF, OBJ, USD, Alembic, FBX, VDB, EXR; **live links** to Blender / Unreal / Unity | hc-io | P1–2 |

## 6. Roadmap with critical path

The critical path is **kernel → language/analysis → agent loop → breadth → quality**. Breadth ops are cheap once the kernel and node-definition macro exist, so we invest in the kernel first.

### Phase 0 — Seed ✅ (done)
Geometry, DAG, 9 SOPs, OBJ/glTF, MCP server. *Proved the loop; will be refactored onto Geometry v2.*

### Phase 1 — Kernel (the bets, in a thin vertical slice)
- **Geo v2:** columnar COW attributes, domains, groups, instance tables, provenance column; migrate existing ops.
- **IR + interpreter backend + fusion** for pointwise ops; node-definition macro (`#[sop]`: schema, docs, differentiability, granularity, tests from one declaration).
- **CAS runtime:** content-hash keys, deterministic RNG and reductions, disk cache, incremental invalidation by hash.
- **`render_preview`** (software rasterizer), `validate`, richer `cook` stats, batch/checkpoints.
- **Gate:** (a) scatter 5M instances + noise terrain cooks and previews in under 2 s with under 1 GB; (b) editing one param recooks only the downstream cone; (c) `ref-cpu` cooks of golden scenes produce identical manifest hashes across Windows x64, Linux x64, macOS arm64 and the WASM runtime at thread counts 1..N, while GPU/`fast` backends agree within documented per-op tolerance; (d) an agent builds a scene, sees the preview, fixes a flaw, exports.

### Phase 2 — Language, analysis, autodiff
- **`hcs` language** + parser/printer round-trip; attribute-flow/unit type checker; typed holes; spec assertions.
- **`hc-vex`:** language, wrangles, Cranelift backend, **IR autodiff** (forward and reverse) with finite-difference gradient checks for every smooth op.
- Expression engine, ramps, subnets/HDAs, scene history with branch/diff/merge.
- 80+ SOPs; first 10 recipes with knobs and specs.
- **Gate:** the "classic ten" tutorial set reproduced by an agent from a text brief via `hcs`; gradient checks green; static analysis catches 90% of seeded agent mistakes before cooking.

### Phase 3 — Fields, volumes, terrain, optimization
- Sparse volumes, SDF fields, mesh↔SDF↔volume↔splat conversions, SDF boolean/remesh.
- Heightfield suite + erosion; terrain recipes.
- **`optimize`** (gradient + CMA-ES hybrid), geometry losses, `vary`, `fork/eval/score/merge`.
- **Gate:** for a recipe with declared parameter ranges, reach a stated Chamfer/SDF-L2 threshold within a fixed cook budget on N held-out scans, using multi-start and an identifiability report; "alpine valley" benchmark at target; 64-variant exploration shares cache (≥10× faster than naive).

### Phase 4 — Seeing, UI, gym
- wgpu viewport + egui network editor + spreadsheet (a thin client of the agent API; human and agent co-edit one session live).
- Rendering: CPU path tracer v1, lookdev rigs, turntables, `critique` helpers.
- **`HoudiniGym`** + task suite + trajectory recording.
- **Gate:** a human builds the classic ten in the GUI while an agent assists; baseline agent score recorded on the gym.

### Phase 5 — Simulation
- POP, RBD, vellum, grains, then sparse pyro and FLIP (wgpu compute, CPU fallback). Checkpointed, branchable, deterministic. Cache/playback with `$F`.
- Differentiable subset (cloth/rigid/particles); first neural surrogates (smoke, erosion).
- **Gate:** shattering wall, flag in wind, smoke plume benchmarks; smoke surrogate meets per-metric thresholds on a held-out distribution with solver-in-the-loop fallback (fallback rate reported); speedup target set after measurement.

### Phase 6 — Render, USD, interop
- Production path tracer (wgpu), volumes, AOVs, denoise; differentiable renderer; LOP/USD; MaterialX; Alembic/FBX/VDB/EXR; live links.
- **Gate:** recover scene parameters from a synthetic render of our own graph (real photos are a research stretch needing learned initialization); Mitsuba `llvm_ad` used as an out-of-process gradient oracle; USD round-trips with other DCCs.

### Phase 7 — Production breadth + self-improvement
- COP, TOP over CAS, CHOP, KineFX, crowds, Python/WASM plugins, browser (WASM) build, multiplayer op-log sessions.
- Recipe promotion pipeline (score → review → library); learned retrieval and optimizer initialization.
- **Gate:** end-to-end shot (model, sim, light, render, comp) from a text brief with no human edits; measurable monotonic gain on the gym as recipes accrue.

## 7. Evaluation: how we know it is working

**Capability ladder** (each rung is a named task set with automated + rubric scoring):

| Tier | Example tasks |
|---|---|
| T1 Primitives | parametric fence, stairs, bolt, pipe network |
| T2 Assets | rock generator, tree, building facade, sci-fi panel kit |
| T3 Environments | alpine valley, canyon, forest clearing, city block |
| T4 FX | shattering wall, pouring water, smoke plume, flag, fire |
| T5 Shots | camera + lighting + sim + render from a brief |
| T6 Inverse | fit to photo/scan, match silhouette and mass, satisfy spec set |

**Metrics tracked per release:** geometric validity, spec pass rate, detail density, render similarity (SSIM/LPIPS-class), VLM-rubric score, agent success rate and edits-to-success on the gym, cook time/memory vs published Houdini numbers on equivalent graphs, cache hit rate, hash determinism rate.

**CI gates:** deterministic-hash tests, per-node golden tests, property tests (manifold, attribute sizes, bounds), gradient checks (autodiff vs finite differences), fuzzing of booleans/remesh, performance budgets (criterion), agent-eval regression suite.

## 8. How we build it (execution model)

- **Parallel agent workstreams** with crate ownership and contract tests: Kernel/IR, Geometry, SOP breadth, VEX/autodiff, Language/analysis, Render, Sim, UI, Recipes/Benchmarks, Gym/Evals. Each stream works against the node-definition macro and the golden-test harness, so breadth scales with agents.
- **Node-definition macro:** one declaration yields schema, docs, `hcs` binding, MCP exposure, differentiability metadata, granularity, golden-test scaffolding.
- **Recipes and benchmarks are code:** versioned in-repo with reference renders; a PR that lowers scores fails.
- **Docs generated from schemas**, so agent-facing docs never drift.
- **Clean-room:** see the legal protocol in §12. No Houdini code, assets, documentation text or trademarks; behavior comes from papers and open implementations only.

## 9. Risks and the route through each

| Risk | Route |
|---|---|
| Scale of Houdini's node set | Kernel + macro first; many nodes are thin over wrangles/IR; parallel agent workstreams; prioritize by the capability ladder. |
| Robust booleans/remesh | Exact/adaptive predicates for polygon paths plus an SDF path that is robust by construction; fuzz corpus; choose per input automatically. |
| Determinism across CPU/GPU | Reference CPU backend defines truth; GPU results tagged by backend-class in the cache key; tolerance-checked against the reference in CI. |
| Autodiff through topology changes | Differentiate through parameters and attributes; treat topology as discrete (search/relaxation: soft SDF blends, straight-through estimators). |
| Fluid/pyro performance | wgpu compute, sparse tiles, surrogate previews, cache-first workflow. |
| GPU-less environments (agents, CI) | Every pipeline stage has a CPU path; software preview renderer; CPU path tracer for finals. |
| Matching "pro" taste | Recipes + guides + critique loop + benchmarks + expert distillation, measured on the ladder. |
| Interop (USD/Alembic/FBX/VDB) | Open-source libraries behind feature flags; glTF/OBJ/native first; live-link plugins last. |
| `.hip`/`.hda` compatibility | Importer for the supported subset; native format remains source of truth. |
| LLM judge reliability | Combine hard metrics with VLM scoring; calibrate on human-rated sets; keep specs as the hard gate. |

## 10. Phase 1 start tasks (done)

All ten Phase 1 start tasks (Geometry v2, instance tables, provenance, content-addressed cooking, the IR skeleton with pointwise fusion, the node macro, the software rasterizer, validate/batch/checkpoints, the benchmark harness with CI determinism check, the `hcs` grammar and attribute-flow type system) are built and gated; dated detail is in [LOG.md](LOG.md). What is next is in [STATUS.md](STATUS.md).

## 11. Status and coverage

Current state lives in [STATUS.md](STATUS.md); every count and score is generated in [generated/state.md](generated/state.md), so this section quotes none.

| Phase | State |
|---|---|
| 0 Seed, 1 Kernel | Done. Gates measured (5M instances plus terrain, determinism across thread counts and across Windows, Linux and macOS in CI). |
| 2 Language, checking, autodiff | Done. Node library, recipes and gym are counted in the generated state. |
| 3 Fields, terrain, optimization | Done for dense volumes; sparse volumes exist as `SparseVolume` with `sparse_remesh` and `sparse_boolean`, and become a first-class attribute next. |
| 4 Seeing, UI, gym | Done: web UI, path tracer, critique metrics, vision judges (local, free), gym with build, repair and edit tasks. |
| 5 Simulation | Particles, cloth, smoke, fracture and rigid bodies done; pyro, FLIP, hair and surrogates follow. |
| 6 Render, USD | Path tracer, USD with PointInstancer and recover-from-images done; Blender Cycles for photographic stills; Godot as engine referee. |
| 7 Production breadth | TOP-style farm runner done; the rest is below. |

Coverage still to add, each with its route:

* Native GUI window: the web UI is the GUI; an egui shell reuses the same API; a native wgpu viewport follows.
* Fire/pyro, FLIP water, vellum hair/grains, neural surrogates, differentiable simulation: wrap or port open solvers, interop first.
* Alembic, FBX, MaterialX, Blender/Unreal live links: USD and glTF today; the others behind cargo features.
* COP (compositing), CHOP, KineFX rigging, crowds, Python bindings, WASM plugins, browser build, CRDT multiplayer: the JSON-RPC (MCP) and gym protocols are already language-agnostic, so they plug in as clients.
* Sparse volumes as a first-class `Geometry` attribute, sparse smoothing/displace/advection, then sparse smoke (rock at 2.5 mm voxels is 166 MB sparse against 4.3 GB dense, `pt sparse-bench`).
* Small node ops (polybevel, convex hull, lattice): follow `crates/pt-sop/src/deform.rs`.
* Legal and naming: original names only; Houdini is never run or studied (EULA, D1). Final product name is an owner decision.

Beyond-Houdini track: B1 differentiable (autodiff, gradient fit, CMA inverse design) done for procedurals; B3 scored recipes and gym baselines in place, promotion pipeline built (`propose_recipe`, `pt recipes-accept`); B4 incremental cooking done, neural surrogates next; B5 diff/merge and shared live session done, CRDT next.

## 12. Research-driven decisions (from `reports/Houdini clone architecture validation.md`)

Each finding from the research has a decision here. Nothing is left as an open worry.

### D1. Legal: build so the EULA never applies to us
SideFX's EULA bars using Houdini to build a competing product. Decision: **nobody on this project runs Houdini to learn behavior, and we do not need to.**
- **Spec sources allowed:** published papers, third-party tutorials, open-source implementations (Blender Geometry Nodes, Infinigen, libigl, OpenVDB), and our own benchmarks. Quality is defined by *our* capability ladder, not by matching Houdini output.
- **Roles:** a *spec-writer* role (agents or people) writes behavior specs from allowed sources into `docs/specs/`; an *implementer* role codes only from those specs. A provenance log per module records its sources.
- **Agent hygiene:** system prompts and retrieval corpora exclude SideFX documentation text. CI greps for SideFX doc phrases, icons and asset names.
- **Names:** the repo, crates, gym and file extensions use original names. Working codename **Proteus** (crates `pt-*`); the gym is `ProteusGym`; native files are `.pgraph`/`.passet`. Compatibility is stated in plain text with a non-affiliation disclaimer. Avoid Karma, Solaris, KineFX, VEX and HOUDINI in product terms. Rename `hc-*` → `pt-*` as Phase 1 task 0 (cheap now, expensive later).
- **Licenses:** Apache-2.0 for the project, DCO sign-off plus an `Assisted-by:` trailer, `cargo-deny` in CI. Counsel review of EULA/naming and a patent search happen before any public release.

### D2. Determinism: a three-class contract that we can actually keep
- **`ref-cpu` (truth):** bit-identical across the supported platform matrix. Requires: a pinned in-repo math crate (no std transcendentals in hashed paths, enforced by a lint), FMA contraction off, fixed-shape reduction trees over fixed-size blocks, total-order merges, Philox-style counter RNG keyed by stable element ids, NaN/-0 canonicalization at node boundaries, rayon only through deterministic wrappers.
- **`fast-cpu` (wide SIMD):** bitwise identical within its own class; tolerance-checked against `ref-cpu`.
- **`gpu`:** tolerance-checked against `ref-cpu`; float atomics replaced by sorted/segmented reductions where determinism matters.
- Cache keys include `backend_class` and `numeric_profile_version`, so classes never cross-contaminate. A CI hash matrix with a first-differing-chunk diff tool makes drift visible within one PR.

### D3. Cache: Salsa-style early cutoff + Bazel-style shared store
- Two layers: in-process incremental graph (early cutoff when a recomputed output hash equals the old one) and a persistent action cache → CAS.
- Output = Merkle manifest of fixed-size per-column chunks (BLAKE3). Node-level keys first; tile-level keys only for volumes and heightfields.
- Key hygiene: op version, canonicalized params, imported file hashes and WASM module hashes go in; thread count, clock and host paths never do.
- Trust model: per-principal write namespaces, authenticated transport, and a **sampling auditor** that re-cooks random hits on trusted workers and quarantines mismatching writers. Local-only by default.

### D4. IR rules learned from Warp/Slang/JAX/Enzyme
Columns are write-once per kernel (versioned); differentiable loops have static or declared bounds; gather adjoints are scatter-adds with a deterministic CPU fallback; custom-derivative hooks exist for noise, SDF queries, raycasts and sorts; autodiff runs after simplification and before fusion; slider parameters are runtime uniforms so edits never recompile; GPU type is f32; fusion regions stop at cardinality changes, neighbour gathers, global reductions and active-set changes. Backends staged: column-chunked SIMD interpreter with AD → fusion + background Cranelift JIT with IR-hash kernel cache → WGSL. Fallback if Cranelift SIMD is weak: precompiled SIMD math called from JIT glue (the OSL pattern). `std::autodiff` is not used.

### D5. Agent bet: make the *runtime* the product, make the *language* a measured choice
- MCP is table stakes (SideFX previewed one). Our edge is the checked, deterministic, searchable runtime behind it.
- **`hcs` is a typed, deterministic, Python-syntax subset** (Starlark-style): models already know the syntax, we add static attribute/unit checking, millisecond diagnostics with machine-applicable fixes, and a published grammar. This removes the cold-start risk of a novel syntax.
- The graph IR is the source of truth; frontends are interchangeable: `hcs`, embedded Python bindings, and flat tools. A **three-arm gym experiment** (`hcs` / Python / tools) on T1 tasks picks the default with data.
- The surface leads with `check_hcs` and `run_hcs`; everything else is reachable through `search_api(intent, detail)` with progressive disclosure (no 100-tool dump).
- Security: loopback-only and authenticated by default; sandboxed execution; no arbitrary code injection path.
- Quality feedback: hard specs (`grounded`, `connected`, `bounds`, `manifold`) gate; VLM critics shape. Critique uses order-swapped pairwise judging and question-decomposed rubrics. A relations-in, instances-out `place` node (solver-based, semantic) replaces LLM-written coordinates. Search exposes a generate-vs-verify budget split.
- RL: CAD-Coder-style verifiable rewards (Chamfer/spec pass) on cheap deterministic cooks; treated as a Phase 4+ experiment, not a promise.

### D6. Scope control for multi-representation geometry
Roll out representations in order, each behind a gate: (1) points + polygon meshes + instance tables, (2) curves, (3) heightfields and SDF fields (Fidget-style tapes with interval simplification, interpreter + JIT), (4) tiled sparse volumes, (5) splats. Conversions are added only when a recipe needs them. Closures (higher-order graph values, as in Blender 5.0) are designed alongside subnets/HDAs in Phase 2.

### D7. Booleans and meshing
Primary path: **SDF/volume-based** booleans and remesh (robust by construction). Secondary: wrap `boolmesh` (pure Rust, MPL-2.0 → isolated crate behind a feature) with `manifold` as a test oracle, behind a fuzz corpus. Selection is automatic by input quality.

### D8. Patents
Use our own tiled sparse grid (hash-addressed dense blocks) instead of cloning a VDB-style tree; use published, expired or unencumbered methods for fluids (stable fluids, FLIP variants from papers) and pattern synthesis; commission a patent search before public release.

### D9. Dependencies (adopt the periphery, own the kernel)
Adopt: `rmcp`, `glam`, `bytemuck`, `wide`, `blake3`, `cranelift-jit`, `wasmtime`, `salsa`, `chumsky` + `logos`, `egui` + `egui-snarl`, `openusd`, `gltf`, `exr`, `rapier3d` (`enhanced-determinism`). Isolate `wgpu`/`naga` behind a single facade crate with exact pins. Write own: columnar store, noise (must fuse), sparse volumes, viewport, deterministic math. All C++-binding crates sit behind cargo features; `cargo check` runs on both Windows targets in CI. All installs stay on D:.

### D10. Week-1 de-risking spike (before the kernel hardens)
Four experiments, each with a pass/fail line:
1. **Cranelift SIMD:** JIT a fused noise+transform kernel on 10M points; pass if ≥70% of hand-written Rust SIMD throughput, else adopt the precompiled-math-plus-glue pattern.
2. **Determinism:** cook a golden scene under `ref-cpu` on two OSes (Windows + WSL/Linux or CI) at 1 and N threads; pass if manifest hashes are identical.
3. **Memory/fusion baseline:** 20-node wrangle chain on 10M points, naive copy-per-node vs COW+fusion; record the real ratio to replace the targets above.
4. **Gym arms:** 10 T1 tasks via `hcs`-like Python-subset vs. flat tools with a baseline agent; record success rate and tokens to pick the default surface.
