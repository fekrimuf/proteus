# Rust Ecosystem Build-vs-Buy Matrix for a Procedural 3D/DCC Engine (as of 2026-10-09)

Method note: versions, dates and licenses come from the crates.io JSON API (`https://crates.io/api/v1/crates/<name>`), fetched 2026-10-09. The fetcher summarises with a small model and truncates pages over 100k characters, so "latest version" figures are read from the top of each listing. Items marked (unverified) are from my background knowledge, not fetched this session. The Windows column is mostly (unverified) because crates.io does not list targets; confirm with `cargo check --target x86_64-pc-windows-gnu`. Project context: D:/Fekri/Houdini-clone/docs/PLAN.md (columnar COW geometry, IR + fusion, Cranelift/WGSL backends, CAS cache, MCP agent surface, toolchain Rust 1.99 GNU + MinGW at D:\Fekri\toolchain).

## 1. Rendering / GPU / UI (wgpu, naga, winit, egui, node editors, vello, rend3, WGSL gaps)

### Takeaway
wgpu (30.0.1) and naga (30.0.1) are healthy, pure-Rust, and fast-moving (a major roughly every 3 months, with breaking changes each time). egui/eframe 0.36.2 is current and egui-snarl 0.12.0 is the maintained node editor. winit is still on 0.30.13 stable with 0.31 only in beta. rend3 is dead; vello is alive but pre-1.0. Adopt wgpu + egui + snarl behind thin wrapper crates and pin versions.

### Cited Findings
- wgpu 30.0.1, released 2026-08-22, MIT OR Apache-2.0; previous 29.0.4 (2026-07-02) and 30.0.0 (2026-07-01) — [crates.io wgpu](https://crates.io/api/v1/crates/wgpu)
- naga 30.0.1, released 2026-08-22, MIT OR Apache-2.0, versioned in lockstep with wgpu — [crates.io naga](https://crates.io/api/v1/crates/naga)
- wgpu v30.0.0 notes: ray tracing pipeline SPIR-V output, full mesh shader support on Metal including WGSL, bindless storage buffer arrays on Metal, `i16`/`u16` behind `SHADER_I16`, cooperative matrix multiply-add on MSL. Breaking changes: vertex buffer slots are `Option`, integer shader I/O no longer defaults to flat interpolation, `SurfaceTexture::present()` replaced by `Queue::present()`, `dispatch` renamed `dispatch_workgroups` — [wgpu CHANGELOG](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/CHANGELOG.md)
- wgpu v29.0.0 notes: WGSL input and WGSL/SPIR-V output for ray tracing pipelines, HLSL mesh shaders (DX12 mesh shaders complete), TLAS binding arrays, cooperative load/store, MSRV lowered to 1.87 with policy "stable minus three"; `get_current_texture` now returns a `CurrentSurfaceTexture` enum — [wgpu CHANGELOG](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/CHANGELOG.md)
- A 29.0.1 patch (2026-03-26) fixed a buffer-limit regression and notes validation is no longer fully WebGPU-compliant on native — [newreleases.io mirror](https://newreleases.io/project/github/gfx-rs/wgpu/release/v29.0.1)
- naga's validator exposes subgroup stage/operation configuration, so subgroups are at least partly supported — [naga docs mirror](https://docs.slint.dev/latest/docs/rust/slint/wgpu_29/wgpu/wgc/naga/)
- winit: stable 0.30.13 (2026-03-02); 0.31.0-beta.3 (2026-09-04); license Apache-2.0 — [crates.io winit](https://crates.io/api/v1/crates/winit)
- egui 0.36.2 (2026-09-08), MIT OR Apache-2.0; eframe 0.36.2 (2026-09-08); previous egui 0.36.0 (2026-08-05) and 0.36.1 (2026-08-07), so about 1 minor every 1 to 3 months — [crates.io egui](https://crates.io/api/v1/crates/egui), [crates.io eframe](https://crates.io/api/v1/crates/eframe)
- egui-snarl 0.12.0 (2026-08-26), MIT OR Apache-2.0, repo zakarumych/egui-snarl; 0.11.0 (2026-06-29), 0.10.0 (2026-06-18), so actively released — [crates.io egui-snarl](https://crates.io/api/v1/crates/egui-snarl)
- vello 0.11.0 (2026-10-02), Apache-2.0 OR MIT, pre-1.0 — [crates.io vello](https://crates.io/api/v1/crates/vello)
- rend3 latest 0.3.0 dated 2022-02-12, license MIT OR Apache-2.0 OR Zlib: abandoned — [crates.io rend3](https://crates.io/api/v1/crates/rend3)
- Chrome's WebGPU notes show WGSL language extensions still evolving (subgroups, swizzle_assignment) — [Chrome WebGPU 153-154](https://developer.chrome.com/blog/new-in-webgpu-153-154?hl=fr)

### Inferences
- wgpu breaks API every major (about quarterly), so isolate it in one `hc-gpu` crate and pin exact versions; upgrades cost a day, not a rewrite.
- For hc-cli/agents (headless, CI) wgpu is optional; GPU is a late-phase backend (PLAN Phase 4+), consistent with the CPU-first design.
- WGSL gaps relevant to a generated-from-IR compute backend (from background knowledge, unverified): no 64-bit floats or atomics on floats in core WGSL, no pointers-to-storage across function boundaries in older spec, no recursion, no dynamic-sized arrays on the stack, limited workgroup memory. The IR-to-WGSL generator should treat f64 as CPU-only and use fixed-point or int atomics for GPU reductions. Reductions are also non-deterministic on GPU, which matches the PLAN's "GPU results tagged by backend-class".
- Node editor: egui-snarl is the pragmatic buy; egui_node_graph is unmaintained (not re-verified this session). Because the PLAN wants the GUI to be a thin client of the agent API, the graph widget is replaceable; wrap it behind a small trait.
- Don't adopt rend3 (dead). For the viewport, write a small own wgpu renderer (shaded/wire/heatmap); vello is only useful for 2D overlays and is optional.

### Gaps
- Exact current status of subgroups, f16, and binding arrays in naga for v30 (changelog was truncated at 100k characters; only the v29/v30 headings were read).
- egui_node_graph maintenance status not re-checked.
- Windows-GNU compatibility of wgpu not tested this session. wgpu supports Vulkan and DX12 on Windows, and the GNU target is a known-working path for `ash`/`windows` crates (unverified), but DX12 shader compile via DXC/FXC loading is runtime-only, so no C++ build is needed.

## 2. Math / parallel (glam, nalgebra, rayon, wide, pulp, std::simd, bytemuck)

### Takeaway
glam 0.34.1, rayon 1.12.0, bytemuck 1.25.2, wide 1.7.1, pulp 0.22.3 are all current and permissively licensed. `std::simd` is still nightly-only, so use `wide` (or pulp for runtime dispatch) on stable. nalgebra only where decompositions/sparse needed.

### Cited Findings
- glam 0.34.1 (2026-10-06), MIT OR Apache-2.0; 0.34.0 (2026-10-04), 0.33.12 (2026-10-01) — very active — [crates.io glam](https://crates.io/api/v1/crates/glam)
- nalgebra 0.35.0 (2026-05-24), license now Apache-2.0 (0.32.x and earlier BSD-3-Clause); 0.34.2 (2026-03-28) — [crates.io nalgebra](https://crates.io/api/v1/crates/nalgebra)
- rayon 1.12.0 (2026-04-14), MIT OR Apache-2.0; prior 1.11.0 (2025-08-12), so low-churn — [crates.io rayon](https://crates.io/api/v1/crates/rayon)
- wide 1.7.1 (2026-09-14), Zlib OR Apache-2.0 OR MIT — [crates.io wide](https://crates.io/api/v1/crates/wide)
- pulp 0.22.3 (2026-06-20), MIT — [crates.io pulp](https://crates.io/api/v1/crates/pulp)
- bytemuck 1.25.2 (2026-07-19), Zlib OR Apache-2.0 OR MIT — [crates.io bytemuck](https://crates.io/api/v1/crates/bytemuck)
- std::simd (`portable_simd`, tracking issue rust-lang/rust#86656) is still nightly-only and described as incomplete in 2026; a 2026 survey favored third-party `fearless_simd` — [Rust SIMD ecosystem discussion](https://zeli.app/story/49844629), [tracking issue](https://github.com/rust-lang/rust/issues/86656)

### Inferences
- glam (f32 SIMD types, f64 variants `DVec3`) matches the PLAN's columnar f32/f64/vec2-4/mat/quat; adopt. Keep nalgebra out of the hot path.
- Determinism contract: avoid `fast-math`-like FMA contraction differences. glam SIMD vs scalar paths and `wide` may give different rounding on different CPUs (FMA availability). The reference path should use fixed scalar order; verify glam's `libm`/`scalar-math` feature for cross-machine determinism (feature names unverified).
- rayon: default work-stealing is non-deterministic in reduction order; use `par_chunks` with fixed chunk size and ordered fold/merge as PLAN says.

### Gaps
- Did not verify glam's `scalar-math`/`libm` features or `wide` FMA behavior for bit-reproducibility.
- `fearless_simd` version/license not fetched.

## 3. Geometry (parry/rapier, BVH/RT, meshopt, triangulation, booleans, VDB, noise)

### Takeaway
Rapier 0.36.0 / parry 0.31.1 are active and rapier has `enhanced-determinism` for cross-platform sim determinism. For booleans there is no single mature pure-Rust answer: `boolmesh` (MPL-2.0, active) and `manifold-rs` (C++ wrapper) are the candidates; the PLAN's own SDF-boolean path plus a custom exact-predicates polygon path is still the safe route. VDB: no usable binding, so write own sparse tile grid and use `vdb-rs` for read-only import.

### Cited Findings
- rapier3d 0.36.0 (2026-09-25), Apache-2.0; features include `enhanced-determinism`, `parallel`, `simd8`, `f32`, `serde-serialize`, `debug-render`, `profiler`, `fem`; no `simd-stable` feature in 0.36.0 (replaced by `simd8`) — [crates.io rapier3d](https://crates.io/api/v1/crates/rapier3d)
- parry3d 0.31.1 (2026-09-18, 0.31.0 yanked), Apache-2.0; 0.30.2-glamx0.2-b (2026-08-08) suggests migration to the `glamx` math backend — [crates.io parry3d](https://crates.io/api/v1/crates/parry3d)
- bvh 0.12.0 (2025-11-16), MIT — [crates.io bvh](https://crates.io/api/v1/crates/bvh)
- meshopt (Rust bindings to meshoptimizer) 0.6.2 (2025-10-09), MIT OR Apache-2.0 — [crates.io meshopt](https://crates.io/api/v1/crates/meshopt)
- spade 2.15.1 (2026-03-24), MIT OR Apache-2.0 — [crates.io spade](https://crates.io/api/v1/crates/spade)
- delaunator 1.1.0 (2026-05-12), ISC — [crates.io delaunator](https://crates.io/api/v1/crates/delaunator)
- geo 0.33.1 (2026-04-20), MIT OR Apache-2.0 — [crates.io geo](https://crates.io/api/v1/crates/geo)
- i_overlay 9.0.1 (2026-10-08), MIT OR Apache-2.0 (2D polygon booleans, very active) — [crates.io i_overlay](https://crates.io/api/v1/crates/i_overlay)
- csgrs: crates.io shows 0.20.1 (2025-07-24), MIT, "CSG on meshes using BSP trees"; the GitHub README (timschmidt/csgrs) describes a newer "Hypermesh" certified-boolean backend and cites 0.23.0, requires finite closed oriented inputs, ~10.6k commits — conflict between crates.io (0.20.1) and README (0.23.0); treat crates.io release as stale — [crates.io csgrs](https://crates.io/api/v1/crates/csgrs), [GitHub csgrs](https://github.com/timschmidt/csgrs)
- manifold-rs 0.7.0 (2026-02-24), MIT OR Apache-2.0, "Rust wrapper for manifold" (C++ dependency) — [crates.io manifold-rs](https://crates.io/api/v1/crates/manifold-rs)
- boolmesh 0.1.10 (2026-09-18), MPL-2.0, "Robust 3D mesh boolean library", created 2025-11-19 — [crates.io boolmesh](https://crates.io/api/v1/crates/boolmesh)
- truck-modeling 0.6.0 (2024-09-20), Apache-2.0 (B-rep kernel; slow release cadence) — [crates.io truck-modeling](https://crates.io/api/v1/crates/truck-modeling)
- embree4-rs 0.0.16 (2023-09-27), non-standard license, effectively stale; wraps C++ Embree 4 — [crates.io embree4-rs](https://crates.io/api/v1/crates/embree4-rs)
- vdb-rs 0.6.0 (2024-01-04), MIT, "OpenVDB manipulation library" (pure-Rust reader) — [crates.io vdb-rs](https://crates.io/api/v1/crates/vdb-rs)
- The only crates.io `openvdb` crate is a 0.1.0 placeholder; MSYS2 ships mingw-w64 OpenVDB 12.0.1 packages — [lib.rs openvdb](https://lib.rs/crates/openvdb), [MSYS2 openvdb](https://packages.msys2.org/base/mingw-w64-openvdb)
- noise 0.9.0 (2024-03-23), MIT OR Apache-2.0 (older cadence); meshopt/bvh also slow — [crates.io noise](https://crates.io/api/v1/crates/noise)

### Inferences
- Determinism: rapier's `enhanced-determinism` disables SIMD/parallel and uses deterministic math, matching the CAS contract, but it only guarantees same-binary-same-platform in practice; plan to hash-test across the Windows GNU and MSVC builds in CI (unverified claim about cross-target guarantees).
- Triangle ray queries: `parry3d` BVH or `bvh` crate is enough for the `ray`/`transfer` SOPs; embree is not worth the C++ build cost on MinGW (embree4-rs is dead).
- Booleans (PLAN risk table): recommendation is wrap `boolmesh` behind a trait as candidate pure-Rust exact path (license MPL-2.0 is file-level copyleft, fine for linking) and keep `manifold-rs` as an optional feature for comparison fuzzing; own SDF path stays primary. Not tested for robustness this session.
- Noise: write own counter-based noise (Perlin/simplex/Worley, deterministic, fusable into IR) rather than adopt `noise`, because IR fusion and GPU codegen need the source algorithm anyway.
- VDB: write own sparse tile grid (hc-core/vol); `vdb-rs` as optional read-only import; defer NanoVDB.

### Gaps
- Robustness test results for boolmesh/csgrs/manifold on degenerate inputs (needs a fuzz run).
- half-edge mesh crates (e.g. `plexus`, `hedron`) not searched; recommendation is to write own lazily built half-edge adjacency as PLAN states.
- simdnoise/fastnoise versions not fetched.

## 4. File formats (gltf, USD, Alembic, FBX, OBJ, EXR, VDB, MaterialX)

### Takeaway
Pure-Rust USD now exists and is credible: `openusd` (mxpv) 0.7.0 reads/writes usda/usdc/usdz with composition. glTF, EXR are mature. FBX is read-only and stale. Alembic and MaterialX have no verified Rust option.

### Cited Findings
- gltf 1.4.1 (2024-05-10), MIT OR Apache-2.0 (stable but low cadence) — [crates.io gltf](https://crates.io/api/v1/crates/gltf)
- exr 1.74.2 (2026-07-10), BSD-3-Clause — [crates.io exr](https://crates.io/api/v1/crates/exr)
- fbxcel 0.9.0 (2022-10-29), MIT OR Apache-2.0, stale — [crates.io fbxcel](https://crates.io/api/v1/crates/fbxcel)
- openusd (mxpv) 0.7.0 (2026-09-05), MIT (vendored schemas under Tomorrow Open Source Technology License 1.0), "Rust native USD library"; ~26k total downloads; repo has 1,046 commits, 132 stars, 2 open issues; supports usda, usdc, usdz, sublayers/references/payloads/variants/inherits/specializes, authoring with undo, schema families behind feature flags, tests against AOUSD Core Spec data, CI on Linux/macOS/Windows; APIs may change before 1.0 — [crates.io openusd](https://crates.io/api/v1/crates/openusd), [GitHub mxpv/openusd](https://github.com/mxpv/openusd)
- Other USD crates: `openusd-rs` (FloatyMonkey, WIP, reads flattened files), `usd-rs` (vfx-rs, C++ bindings at 0.0.x needing cmake and c++14), `pxr` (C++ wrapper, last release 0.2.0 April 2024, docs.rs build failed) — [docs.rs search results](https://docs.rs/pxr), [docs.rs usd 0.0.5](https://docs.rs/crate/usd/0.0.5)
- A third-party listing describes another `usd-rs` as a pure-Rust ground-up port (aggregator source, unverified) — [tomevault](https://tomevault.io/tome/ssoj13/usd-rs)

### Inferences
- openusd (pure Rust, no C++, builds on both Windows targets presumably) fits PLAN hc-lop/hc-io at Phase 6; adopt behind a feature flag with golden round-trip tests against Blender/usdview output.
- OBJ: write own (trivial, already in repo) or `tobj` (not fetched). glTF: adopt `gltf` for import, keep own exporter.
- FBX: import-only via fbxcel is stale; defer, or route through USD/glTF converters outside the engine.

### Gaps
- No Rust Alembic reader/writer found (not searched deeply); no MaterialX Rust binding verified (the `materialx` crates were not searched). Routes: Alembic via USD converter or own Ogawa reader; MaterialX via own parser of the `.mtlx` XML plus openusd's UsdShade.
- `openexr` (C++ binding) not checked; `exr` pure-Rust is enough for scanline/tiled/deep reading, deep support not verified.

## 5. ML / autodiff (burn, candle, cubecl, ndarray, std::autodiff)

### Takeaway
burn 0.22.0 and cubecl 0.11.0 both reached stable releases on 2026-10-06; candle-core 0.11.0 and ndarray 0.17.2 are current. `std::autodiff` (Enzyme) is nightly-only and experimental. Since the PLAN does autodiff as an IR transform, none of these is a dependency of the kernel; use burn/candle only for neural surrogates.

### Cited Findings
- burn 0.22.0 (2026-10-06), MIT OR Apache-2.0 (0.22.0-pre.4 on 2026-09-22) — [crates.io burn](https://crates.io/api/v1/crates/burn)
- cubecl 0.11.0 (2026-10-06), MIT OR Apache-2.0 — [crates.io cubecl](https://crates.io/api/v1/crates/cubecl)
- candle-core 0.11.0 (2026-06-26), MIT OR Apache-2.0 — [crates.io candle-core](https://crates.io/api/v1/crates/candle-core)
- ndarray 0.17.2 (2026-01-10), MIT OR Apache-2.0 — [crates.io ndarray](https://crates.io/api/v1/crates/ndarray)
- std::autodiff: available on nightly via a rustup `enzyme` component, with listed targets incl. Windows (llvm-mingw); requires `lto="fat"` and `-Zautodiff=Enable`; still labeled experimental — [rustc-dev-guide autodiff installation](https://rust.googlesource.com/rustc-dev-guide/+show/refs/heads/main/src/autodiff/installation.md)

### Inferences
- Project pins Rust 1.99 stable GNU; `std::autodiff` would force nightly. Keep it out of the critical path; the IR-level source-to-source autodiff (PLAN Bet 1) is also the only way to differentiate Cranelift/WGSL-generated kernels anyway. Possible use: an oracle for gradient checks in an optional nightly CI job.
- burn+cubecl: wgpu/CUDA backends in Rust; adopt burn for surrogate nets (Phase 5); dfdx not checked (appears superseded).

### Gaps
- dfdx status not fetched. burn's Windows-GNU compatibility not verified.

## 6. Compilation / parsing / storage (cranelift, wasmtime, inkwell, parsers, salsa, blake3, dashmap, arrow, CAS)

### Takeaway
Cranelift (0.136.2 stable, 0.137 rc) and wasmtime (49.0.2 stable; 50 rc) release monthly with breaking-version bumps, which is fine if pinned. For the DSL: chumsky + logos (or winnow) for `hcs`/VEX parser; salsa 0.28.5 for incremental analysis; blake3 1.8.7 for CAS keys.

### Cited Findings
- cranelift-jit 0.136.2 stable (0.137.0-rc.1 on 2026-10-05), Apache-2.0 WITH LLVM-exception — [crates.io cranelift-jit](https://crates.io/api/v1/crates/cranelift-jit)
- wasmtime 49.0.2 stable (50.0.0-rc.1 on 2026-10-05; 48.0.5 patch on 2026-10-02), Apache-2.0 WITH LLVM-exception — [crates.io wasmtime](https://crates.io/api/v1/crates/wasmtime)
- inkwell 0.10.0 (2026-08-06), Apache-2.0 (needs a system LLVM) — [crates.io inkwell](https://crates.io/api/v1/crates/inkwell)
- chumsky 0.13.0 (2026-05-06), MIT — [crates.io chumsky](https://crates.io/api/v1/crates/chumsky)
- logos 0.16.1 (2026-01-30), MIT OR Apache-2.0 — [crates.io logos](https://crates.io/api/v1/crates/logos)
- winnow 1.0.4 (2026-07-13), MIT — [crates.io winnow](https://crates.io/api/v1/crates/winnow)
- tree-sitter 0.27.1 (2026-10-08), MIT — [crates.io tree-sitter](https://crates.io/api/v1/crates/tree-sitter)
- salsa 0.28.5 (2026-09-24; 0.28.3 yanked), Apache-2.0 OR MIT — [crates.io salsa](https://crates.io/api/v1/crates/salsa)
- blake3 1.8.7 (2026-08-20), CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception — [crates.io blake3](https://crates.io/api/v1/crates/blake3)
- dashmap 6.2.1 (2026-05-17), MIT — [crates.io dashmap](https://crates.io/api/v1/crates/dashmap)
- arrow 60.0.0 (2026-09-15), Apache-2.0 — [crates.io arrow](https://crates.io/api/v1/crates/arrow)
- redb: the fetch returned conflicting values (max_stable 4.3.0 vs "newest 2.6.4 on 2026-10-02"); license MIT OR Apache-2.0 — [crates.io redb](https://crates.io/api/v1/crates/redb); re-check before pinning

### Inferences
- Cranelift JIT fits user-VEX on CPU (PLAN Bet 1), pure Rust, no LLVM install, builds on both Windows targets (wasmtime/cranelift are CI-tested on windows-gnu and msvc, unverified). Choose it over inkwell, which needs system LLVM and a matching MinGW build.
- arrow: don't adopt the whole crate for the attribute store; the PLAN's own `Arc`'d typed chunk columns are simpler; use Arrow only at the interchange edge (Parquet/IPC) if needed.
- CAS backend: start with plain content-addressed files + `blake3` and a small `redb` index; rocksdb needs a C++ build (bad on MinGW) and sled is unmaintained (neither fetched).
- DSL: `hcs` needs good error recovery for agent feedback; chumsky (recovery built in) over logos tokens, or hand-written recursive descent + Pratt if the grammar stays small. Salsa for incremental type checking is a strong match for the attribute-flow analyzer, but its API is churny (0.28.3 yanked), so pin.

### Gaps
- lalrpop, rocksdb, sled not fetched. wasmtime component-model plugin status not verified this session.

## 7. Agent / protocol (rmcp, axum, tokio, jsonrpsee, serde)

### Takeaway
The official Rust MCP SDK `rmcp` is at 3.5.1 (2026-10-05) targeting spec 2026-07-28 and supports stdio and streamable HTTP; adopt it rather than the hand-rolled MCP server in hc-cli.

### Cited Findings
- rmcp 3.5.1 (2026-10-05), Apache-2.0, repo modelcontextprotocol/rust-sdk, ~33.9M downloads; 3.5.0 on 2026-09-28, 3.4.1 on 2026-09-23 (rapid cadence) — [crates.io rmcp](https://crates.io/api/v1/crates/rmcp)
- SDK implements stable MCP 2026-07-28 spec, compatible with 2025-11-25 and earlier; stdio via `transport-io`, streamable HTTP server as a Tower service, client via reqwest, in-process `transport-worker`; legacy HTTP+SSE intentionally omitted; ~4k stars — [GitHub rust-sdk](https://github.com/modelcontextprotocol/rust-sdk)
- A secondary blog (unverified) claims the Rust SDK moved to Tier 1 conformance in August 2026 — [digitalapplied](https://www.digitalapplied.com/blog/mcp-sdk-conformance-tiers-what-tier-1-means)
- axum 0.8.9 (2026-04-14), MIT; tokio 1.53.2 (2026-10-03), MIT; jsonrpsee 0.26.1 (2026-09-30), MIT; — [crates.io axum](https://crates.io/api/v1/crates/axum), [crates.io tokio](https://crates.io/api/v1/crates/tokio), [crates.io jsonrpsee](https://crates.io/api/v1/crates/jsonrpsee)

### Inferences
- Adopt rmcp for the `hc mcp` stdio and HTTP surfaces; the PLAN's tool list maps to `#[tool]`-style handlers generated by the `#[sop]` macro. Use axum only if HTTP/WebSocket JSON-RPC beyond MCP is needed; jsonrpsee is optional.
- Version pin: rmcp's major jumped (0.x to 3.x within about a year), so watch breaking changes.

### Gaps
- Exact rmcp macro API names and MSRV not verified; serde version not fetched (ubiquitous, assume 1.x).

## 8. Scripting (pyo3, rhai, mlua, wasmtime component model)

### Takeaway
pyo3 0.29.3, rhai 1.26.1, mlua 0.12.2 are all current. For the PLAN's WASM plugin story use wasmtime with the component model; for Python embedding use pyo3 (with the Windows caveats below).

### Cited Findings
- pyo3 0.29.3 (2026-09-30), MIT OR Apache-2.0 — [crates.io pyo3](https://crates.io/api/v1/crates/pyo3)
- rhai 1.26.1 (2026-09-10), MIT OR Apache-2.0 — [crates.io rhai](https://crates.io/api/v1/crates/rhai)
- mlua 0.12.2 (2026-10-03), MIT — [crates.io mlua](https://crates.io/api/v1/crates/mlua)

### Inferences
- `hcs` is the primary language; a separate scripting language adds little. Python via pyo3 for pipeline/TD compatibility (Phase 7); on x86_64-pc-windows-gnu pyo3 needs a MinGW-compatible libpython (unverified), so build the Python module with maturin on MSVC or keep the Python bridge out-of-process (MCP/JSON-RPC) to sidestep.
- WASM plugins (`create_node_type` from source) via wasmtime (pure-Rust Cranelift, sandboxed, deterministic with NaN canonicalization options) fits the cache determinism goal.

### Gaps
- wasmtime component-model stability (WASI 0.3 / async) not verified.

## 9. Windows GNU (MinGW) pitfalls for C++-binding crates and the all-on-D: option

### Takeaway
Sticking to pure-Rust crates removes most MinGW risk. Of the C++-binding options, none has a verified MinGW recipe, so avoid them or use MSVC for those leaf crates.

### Cited Findings
- OpenVDB: no maintained Rust binding; mingw-w64 OpenVDB 12.0.1 packages exist in MSYS2 (last updated May 2025) — [MSYS2](https://packages.msys2.org/base/mingw-w64-openvdb)
- `usd-rs` (C++ bindings) needs g++/clang with c++14, cmake, python; `pxr` is early and its docs.rs build failed — [docs.rs usd](https://docs.rs/crate/usd/0.0.5), [docs.rs pxr](https://docs.rs/pxr)
- Enzyme-based `std::autodiff` lists Windows via llvm-mingw as supported — [rustc-dev-guide](https://rust.googlesource.com/rustc-dev-guide/+show/refs/heads/main/src/autodiff/installation.md)
- Reported analogues: OpenVR does not support MinGW; the Rust target must match the C++ compiler for `cxx`/`cmake` crates — [search result summary on openvr-sys](https://crates-io.ganymede-static.progval.net/crates/openvr/0.6.0)

### Inferences
- Rules to follow: (1) default to pure-Rust crates (openusd, boolmesh, vdb-rs, exr, gltf, cranelift, wasmtime). (2) For C/C++ crates (manifold-rs, meshopt's bundled C++, any embree) pin the compiler to the MinGW g++ at D:\Fekri\toolchain and set `CC`/`CXX`/`CMAKE_GENERATOR`; keep them behind cargo features so default `cargo build` never needs them. (3) Prefer prebuilt shared libs from MSYS2 (`mingw-w64-x86_64-*`) installed under D: and located via `pkg-config`/env vars (e.g. `OPENVDB_ROOT`, `TBB_ROOT`). (4) Isolate unavoidable MSVC-only dependencies in a separate helper executable (separate target dir, out-of-process over the CAS/IPC) so the main workspace stays GNU. (5) CI should compile both `x86_64-pc-windows-gnu` and `-msvc` with `cargo check`; hash-test cooks across both.
- All-on-D: path: set `CARGO_HOME`, `RUSTUP_HOME`, `CARGO_TARGET_DIR`, and MSYS2 root under D:\Fekri; keep `sccache`/target on D:. (General practice; no source needed.)

### Gaps
- No Windows-GNU build verification was run for any crate in this session. Recommended next action: a one-hour spike running `cargo check --target x86_64-pc-windows-gnu` on a scratch workspace depending on wgpu, egui, rmcp, cranelift-jit, wasmtime, openusd, boolmesh, rapier3d, blake3 (blake3's C/asm features need a working gcc on MinGW; pure Rust fallback exists via feature flag, unverified).

## Build-vs-Buy Matrix

Legend: Adopt / Wrap (behind own trait/crate) / Write own. Windows column: "pure" = pure Rust, expected to build on both GNU and MSVC (unverified); "C++" = native dependency, risky on GNU.

| Component | Crate | Version (date) | License | Maintenance | Windows | Fit / Rec |
|---|---|---|---|---|---|---|
| GPU API | wgpu | 30.0.1 (2026-08-22) | MIT/Apache-2.0 | very active, quarterly majors | pure | Wrap in hc-gpu |
| Shader IR | naga | 30.0.1 (2026-08-22) | MIT/Apache-2.0 | lockstep with wgpu | pure | Adopt for validation; generate WGSL from own IR |
| Window | winit | 0.30.13 stable / 0.31.0-beta.3 (2026-09-04) | Apache-2.0 | active, 0.31 beta | pure | Adopt via eframe |
| UI | egui / eframe | 0.36.2 (2026-09-08) | MIT/Apache-2.0 | very active | pure | Adopt |
| Node editor | egui-snarl | 0.12.0 (2026-08-26) | MIT/Apache-2.0 | active | pure | Wrap |
| Renderer | rend3 | 0.3.0 (2022) | MIT/Apache/Zlib | dead | - | Do not use; write own viewport |
| 2D vector | vello | 0.11.0 (2026-10-02) | Apache/MIT | active, pre-1.0 | pure | Optional overlays |
| Math | glam | 0.34.1 (2026-10-06) | MIT/Apache-2.0 | very active | pure | Adopt |
| Linear algebra | nalgebra | 0.35.0 (2026-05-24) | Apache-2.0 | active | pure | Adopt narrowly |
| Parallel | rayon | 1.12.0 (2026-04-14) | MIT/Apache-2.0 | stable | pure | Adopt; ordered merges |
| SIMD | wide / pulp | 1.7.1 / 0.22.3 | Zlib+MIT / MIT | active | pure | Adopt wide; std::simd still nightly |
| Casting | bytemuck | 1.25.2 (2026-07-19) | Zlib/Apache/MIT | active | pure | Adopt |
| Collision/BVH | parry3d | 0.31.1 (2026-09-18) | Apache-2.0 | active | pure | Adopt for ray/closest queries |
| Physics | rapier3d | 0.36.0 (2026-09-25) | Apache-2.0 | active, enhanced-determinism | pure | Adopt for rigid sim, determinism test |
| BVH | bvh | 0.12.0 (2025-11) | MIT | moderate | pure | Optional |
| Mesh opt | meshopt | 0.6.2 (2025-10) | MIT/Apache | moderate | C (bundled) | Wrap |
| 2D tri | spade / delaunator | 2.15.1 / 1.1.0 | MIT+Apache / ISC | active | pure | Adopt spade (constrained) |
| 2D geo/booleans | geo / i_overlay | 0.33.1 / 9.0.1 (2026-10-08) | MIT+Apache | active | pure | Adopt i_overlay |
| 3D booleans | boolmesh | 0.1.10 (2026-09-18) | MPL-2.0 | young, active | pure | Wrap + fuzz; SDF path own |
| 3D booleans | manifold-rs | 0.7.0 (2026-02) | MIT/Apache | moderate | C++ | Optional oracle |
| 3D booleans | csgrs | 0.20.1 crates.io (README: 0.23.0) | MIT | active (GitHub) | pure | Evaluate |
| B-rep | truck | 0.6.0 (2024) | Apache-2.0 | slow | pure | Skip |
| Ray tracing | embree4-rs | 0.0.16 (2023) | non-standard | dead | C++ | Skip |
| VDB | vdb-rs | 0.6.0 (2024) | MIT | stale | pure | Read-only; write own volumes |
| Noise | noise | 0.9.0 (2024) | MIT/Apache | slow | pure | Write own (IR-fusable) |
| glTF | gltf | 1.4.1 (2024) | MIT/Apache | stable | pure | Adopt |
| EXR | exr | 1.74.2 (2026-07) | BSD-3 | active | pure | Adopt |
| FBX | fbxcel | 0.9.0 (2022) | MIT/Apache | stale | pure | Defer |
| USD | openusd | 0.7.0 (2026-09-05) | MIT (+TOST schemas) | active, pre-1.0 | pure | Adopt (feature flag) |
| USD (C++) | usd-rs / pxr | 0.0.x / 0.2.0 | - | stalled | C++ | Skip |
| Alembic / MaterialX | - | none found | - | - | - | Gap: write own / via USD |
| ML | burn / cubecl | 0.22.0 / 0.11.0 (2026-10-06) | MIT/Apache | very active | pure | Adopt for surrogates |
| ML | candle-core / ndarray | 0.11.0 / 0.17.2 | MIT/Apache | active | pure | Optional |
| Autodiff | std::autodiff | nightly | - | experimental | llvm-mingw | Skip; own IR autodiff |
| JIT | cranelift-jit | 0.136.2 (rc 0.137) | Apache+LLVM | very active | pure | Adopt |
| Plugins | wasmtime | 49.0.2 (rc 50) | Apache+LLVM | very active | pure | Adopt |
| LLVM | inkwell | 0.10.0 | Apache-2.0 | active | needs LLVM | Skip |
| Parse | chumsky / logos / winnow / tree-sitter | 0.13.0 / 0.16.1 / 1.0.4 / 0.27.1 | MIT | active | pure (ts has C) | chumsky+logos |
| Incremental | salsa | 0.28.5 (2026-09-24) | Apache/MIT | active, churny | pure | Adopt, pin |
| Hash | blake3 | 1.8.7 (2026-08-20) | CC0/Apache | active | pure/C asm | Adopt |
| Concurrent map | dashmap | 6.2.1 | MIT | stable | pure | Adopt |
| Columnar | arrow | 60.0.0 (2026-09-15) | Apache-2.0 | very active | pure | Edge only; write own store |
| KV | redb | see conflict above | MIT/Apache | active | pure | Adopt for CAS index |
| MCP | rmcp | 3.5.1 (2026-10-05) | Apache-2.0 | very active | pure | Adopt |
| Async/web | tokio / axum / jsonrpsee | 1.53.2 / 0.8.9 / 0.26.1 | MIT | active | pure | Adopt as needed |
| Python | pyo3 | 0.29.3 (2026-09-30) | MIT/Apache | active | link caveats | Wrap, out-of-process first |
| Scripting | rhai / mlua | 1.26.1 / 0.12.2 | MIT | active | pure / C | Skip; use hcs |

## Top 10 Recommendations for D:/Fekri/Houdini-clone

1. Adopt `rmcp` 3.5.x now for `hc mcp` (stdio + streamable HTTP); replace the hand-rolled server in hc-cli, mapping the future `#[sop]` macro output to rmcp tools.
2. Write own columnar COW attribute store (hc-core) on `glam` + `bytemuck` + `Arc` chunks; do not adopt `arrow` for the hot path. Use `blake3` for CAS keys, `dashmap` for in-memory caches, `redb` for the persistent index (re-verify redb version).
3. Use `wide` (stable) for CPU SIMD; do not wait for `std::simd` (still nightly) and do not depend on `std::autodiff` (nightly, experimental). Implement autodiff as an IR transform per PLAN and gradient-check against finite differences.
4. Adopt `cranelift-jit` (pinned to 0.136.x) for VEX CPU JIT and `wasmtime` 49.x for sandboxed node plugins; skip `inkwell` (system LLVM pain on MinGW).
5. Parse `hcs` with `logos` + `chumsky` (error recovery for agent-facing diagnostics); use `salsa` (pin 0.28.x) for the incremental attribute-flow analyzer.
6. Isolate `wgpu`/`naga` 30.x in one `hc-gpu` crate with exact pinning; generate WGSL from own IR, treat f64 as CPU-only, tag cache keys by backend class. Postpone until Phase 4.
7. UI: `eframe`/`egui` 0.36 with `egui-snarl` 0.12 behind a wrapper trait; skip rend3 and write a small own viewport renderer; software rasterizer for `render_preview` (no crate needed).
8. Booleans: keep the PLAN's two-path strategy. Wrap `boolmesh` (MPL-2.0, pure Rust, active) as the exact polygon-mesh path behind a trait, add `manifold-rs` (C++) only as an optional fuzz oracle, and use `i_overlay` for 2D. Write the SDF path and sparse volumes yourself (`vdb-rs` for read-only VDB import).
9. Interop: adopt `openusd` 0.7 (pure Rust USD with usda/usdc/usdz) behind a feature flag with round-trip tests, `gltf`, `exr`; defer FBX/Alembic/MaterialX (no verified maintained Rust option; route via USD).
10. Windows GNU policy: pure-Rust by default, all C++-binding crates behind cargo features, MSYS2 prebuilt libs on D:, and a CI spike `cargo check` for both GNU and MSVC targets plus cross-target hash comparison of cooks (also validates rapier `enhanced-determinism` and the reduction-order contract).
