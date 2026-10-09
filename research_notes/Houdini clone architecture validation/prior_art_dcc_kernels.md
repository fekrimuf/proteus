# Prior art for procedural DCC kernels (validation of docs/PLAN.md bets)

Research budget note: about 17 tool calls. Several primary sources (HDK COW internals, Gaffer, Lagrange data model, Unreal PCG internals, Bevy 2026) could not be confirmed and are listed under Gaps rather than asserted.

## Q1. What Houdini already does (packed prims, COW, attribute model, cook engine, PDG, Solaris, Karma)

### Takeaway
Houdini already has opt-in lazy instancing (packed primitives, with shared references, delayed-load disk prims, viewport instancing), paged attribute storage, data-ID-based cook optimizations, GPU/ML features in H21/H22 (including native Gaussian splats in H22), and an official MCP preview. PLAN.md's claims that "Houdini's packed prims are an opt-in afterthought" is fair, but "Houdini runs each node as a separate full pass with a full copy" is unverified and probably overstated (see flags).

### Cited Findings
- Packed primitives "express a procedure to generate geometry at render time"; they can embed in-memory geometry, point to a fragment of other geometry, or reference disk files; copying one "copies the reference rather than the geometry itself"; they cannot be edited without unpacking. — [SideFX Houdini 22 geometry/packed docs](https://www.sidefx.com/docs/houdini/model/packed.html)
- Same page: packed is "not compressed or smaller"; if every instance is unique there is no memory or performance benefit; the viewport draws the shared data multiple times with different transforms and can show stand-ins (point cloud, bbox). The page does not use the term "copy-on-write." — [packed docs](https://www.sidefx.com/docs/houdini/model/packed.html)
- Packed disk prims delay load (data stays on disk until display/render) and Packed Disk Sequences support motion-blur interpolation; instances of disk/in-memory packed prims cannot carry per-instance attributes except `material` and `vel`; packed fragments can, less efficiently. — [packed docs](https://www.sidefx.com/docs/houdini/model/packed.html)
- HDK: GA attribute data is stored in "paged" arrays; "The GA library works on pages of data rather than large contiguous arrays", for data locality. — [HDK GA usage](https://www.sidefx.com/docs/hdk/_h_d_k__g_a__using.html)
- HDK exposes `GA_PageArray` (storage-type per component, a `copy` template) used in sample `GU_Copy2` alongside `GU_PrimPacked`/`GU_PackedGeometry`. — [GA_PageArray](https://www.sidefx.com/docs/hdk/class_g_a___page_array.html), [GU_Copy2.C](https://www.sidefx.com/docs/hdk/_g_u___copy2_8_c_source.html)
- Houdini Python docs: SOP geometry is a read-only reference updated on recook; `hou.Geometry.freeze` returns a snapshot; since Houdini 18, "data IDs" must be incremented when geometry is modified outside cooking because internal optimizations (e.g. in `hou.SopVerb.execute`) depend on them. This shows Houdini already tracks per-attribute/topology change IDs for cache invalidation. — [hou.Geometry](https://www.sidefx.com/docs/houdini/hom/hou/Geometry.html)
- PDG/TOPs: work items have cache states (Clean cooks from cache; Dirty when node always cooks, output files missing, or an upstream item wrote new files); a missing upstream cache file invalidates downstream caches; TOPs recook only affected work. Cache is file-path/on-disk based, not content-hash based. — [PDG workItemCacheState](https://www.sidefx.com/docs/houdini/tops/pdg/workItemCacheState.html)
- Solaris/LOPs: every LOP outputs a fully composed USD stage; a node "creates modified copies of only the prims it changes", root layer stays empty with sub-layer stack. — [Solaris/USD docs](https://www.sidefx.com/docs/houdini/solaris/usd.html), [about_lops](https://www.sidefx.com/docs/houdini/solaris/about_lops.html)
- Houdini 21 (Aug 2025): 360+ new-feature bullets; focus on completing/stabilising toolsets; KineFX motion mixer/Autorig, muscles, Copernicus Flow and Pyro solvers (GPU), OTiS GPU solver, Neural Point Surface (ML meshing), new erosion toolset, ML terrain tools, GPU texture baking, scatter into Solaris. Large-dataset memory optimizations reported, no benchmarks. — [CGChannel](https://www.cgchannel.com/2025/08/sidefx-just-released-houdini-21-check-out-its-5-key-features/), [Digital Production](https://digitalproduction.com/2025/08/26/houdini-21-finally-lands-the-buzz-is-over-time-to-actually-use-it/)
- Houdini 22 (keynote 22 Jun 2026; release reported 16 Jul 2026 by CGChannel, one German source says 21 Jul, sources disagree): headline is a full 3D Gaussian Splat toolset (train, animate with standard deformation nodes, relight, render in Karma, reconstruct splats from Karma renders); SfM not inside Houdini; Vulkan viewport replaces OpenGL (GPU subdivision, volumetric fog); VFX Platform CY2026. — [CGChannel tag](https://www.cgchannel.com/tag/relight-gaussian-splat/), [RadianceFields](https://radiancefields.com/houdini-22-ships-making-native-gaussian-splats-generally-available), [SideFX talk](https://www.sidefx.com/learn/talks/h22-gaussian-splats-and-ml/), [CGChannel p=177317](https://www.cgchannel.com/?p=177317)
- SideFX + NVIDIA showed MCP-powered agent support in Houdini 22 as an "Apex Script MCP server" (curated Apex Script syntax/examples for rigging code), a SideFX Labs sneak peek, not a finished production tool. — [Jon Peddie Research](https://www.jonpeddie.com/news/sidefx-and-nvidia-bring-mcp-powered-ai-agents-to-houdini-22s-rigging-workflow-at-siggraph-2026/)
- Community Houdini MCP servers (HoudiniMCP, others) and a commercial plugin (NodeArchitect, v2 with built-in MCP server) already let LLMs build nodes, cook, write VEX, and verify visually. — [Glama Houdini MCP list](https://images.glama.ai/mcp/servers/integrations/houdini), [Product Hunt NodeArchitect](https://www.producthunt.com/products/nodearchitect-ai-agent/launches/nodearchitect-ai-agent-2); thesis: [Drexel](https://researchdiscovery.drexel.edu/esploro/outputs/graduate/991022057536804721)

### Inferences
- Packed prims give Houdini lazy, shared instancing today; our differentiator is only that instance tables are the default representation of `copy_to_points`/scatter and carry per-instance attributes uniformly (Houdini restricts per-instance attributes on packed disk/memory prims). Frame it as "default and uniform", not "Houdini lacks it".
- Houdini's data-ID machinery means incremental invalidation at attribute granularity exists internally; our novelty is content-addressed keys (shared across machines/agents) rather than in-process IDs or file-path PDG caches.
- "Agent-native Houdini" is no longer unique: SideFX and third parties ship MCP integrations. Our edge must be the typed language, static analysis, determinism and CAS, not MCP itself.
- Houdini 22 first-classing Gaussian splats weakens any claim that splats as a representation are novel; the claim should be one container with all representations plus cross-conversion.

### Gaps
- Could not confirm from public docs whether SOP cooks share unmodified attribute pages between input and output (COW at the page level). HDK pages found do not mention COW. Do not claim either "Houdini copies everything" or "Houdini shares everything" without reading GA_Detail/GA_PageArray HDK docs or SideFX forum statements.
- No official Houdini 21/22 release-notes page was fetched (secondary coverage only); Karma CPU/XPU internals not covered.
- Packed-primitive docs fetched describe Mantra and viewport; Karma handling not documented in what I retrieved (Karma goes through USD point instancers/Solaris, unverified here).

## Q2. Blender Geometry Nodes (fields, lazy eval, instances, implicit sharing, bundles/closures, perf lessons)

### Takeaway
Blender ships exactly the COW-shared columnar attribute idea (implicit sharing, since 3.6), deferred field evaluation, and instances; 5.0 added bundles, closures and sparse volume grids. This validates the store design, but also shows the sharing breaks easily (domain/type mismatches) and needs API-level plumbing.

### Cited Findings
- Blender 3.6 introduced "implicit sharing" to avoid copying large data chunks; copying geometry to change part of it is much faster; memory use improves (at least 25% in simple cases per release notes as summarized). — [Blender 3.6 release notes](https://developer.blender.org/docs/release_notes/3.6/nodes_physics/) (via search summary; page not fetched directly)
- Implicit sharing is applied node by node (Instance on Points, Instances to Points, Points to Vertices, Mesh to Points, Store/Capture Attribute); sharing only works when domain sizes and types match; a Mesh to Curve bug (#110485) crashed due to incorrect sharing when the index list did not cover every point. — [PR 106917](https://projects.blender.org/blender/blender/pulls/106917), [PR 107357](https://projects.blender.org/blender/blender/pulls/107357), [commit for #110485](https://demo.gitea.com/bartvdbraak/blender/commit/803589ca5a3ff6bfc60740ef841f98ba93cc55ba)
- Integration with the attribute API required changing `VArray` returns to `AttributeReader` to carry sharing info. — [Issue 107059](https://projects.blender.org/blender/blender/issues/107059), commit e45ed69349
- Fields: a field is a function from a `FieldContext` to values; fields are an immutable DAG of `FieldInput` and `FieldOperation` nodes; nothing is computed until a `FieldEvaluator` applies them; typed `Field<T>` and runtime-typed `GField`. — [Blender dev docs: fields](https://developer.blender.org/docs/features/nodes/fields/)
- Blender 5.0: Bundles (struct-like multi-value link; Combine/Separate/Join Bundle), Closures (Closure Zone + Evaluate Closure; inject nodes into a node group), also in Shader Nodes; bundles/closures non-experimental. — [Blender 5.0 GN notes](https://developer.blender.org/docs/release_notes/5.0/geometry_nodes/), [Sept 2025 workshop](https://code.blender.org/2025/10/geometry-nodes-workshop-september-2025/)
- Blender 5.0 volume grids: a grid socket type; Mesh/Points to SDF Grid, Voxelize/Prune, Grid to Mesh, Field to Grid, curl/divergence/laplacian/gradient, Advect Grid, SDF boolean/fillet/offset/median/mean filters; Viewer node accepts non-geometry data; `id` is now a generic attribute. — [5.0 GN notes](https://developer.blender.org/docs/release_notes/5.0/geometry_nodes/)
- Workshop notes: BVH implementation slow, hope to use Embree; instances being explored for running node tools on whole objects (needs string attributes); desired "list fields"; bundles proposed in the geometry set; XPBD hair solver planned. — [code.blender.org workshop](https://code.blender.org/2025/10/geometry-nodes-workshop-september-2025/)
- Blender caches a compute-context hash to avoid recomputation (hash-keyed evaluation context exists for logging/caching). — [commit a09accb496d](https://lists.blender.org/pipermail/bf-blender-cvs/2022-December/182395.html)
- Later versions exist (search reports 5.2 LTS released 14 Jul 2026); not reviewed here. — [releasebot summary via search](https://releasebot.io/updates/blender)

### Inferences
- Implicit sharing is a proven design but each node must opt in and verify domain/size/type compatibility; our `Arc` columns should centralize this in the kernel API (one `with_column_replaced` path) instead of per-node logic.
- Blender fields are evaluated per-domain on demand (lazy, fusable) which maps closely to the PLAN IR fusion bet; bundles/closures signal that graph-level abstraction (higher-order nodes) is wanted; PLAN has subnets/HDAs only, consider closures as a design item.
- Blender's volume grids appearing only in 5.0 suggests multi-representation geometry inside one container is hard; budget accordingly.

### Gaps
- Instances/Realize Instances doc pages and Blender's lazy-function-graph (multi-threaded scheduling) docs were not fetched (the geometry_nodes dev-docs URL returned 404). No quantitative Blender benchmarks gathered.

## Q3. Other procedural systems and OSS kernels

### Takeaway
Rust ecosystem has building blocks (Fidget for SDF/JIT, wgpu, Bevy ECS) but no mature open-source Rust procedural geometry kernel comparable to the PLAN; Blackjack is the only 3D Rust node-based modeler found and it is small.

### Cited Findings
- Unreal PCG: GPU processing runs connected GPU-enabled nodes as a Compute Graph; GPU support limited to a few nodes (Copy Points, Static Mesh Spawner, Custom HLSL); CPU-GPU transfer cost is significant; guidance is to group GPU nodes and keep transfers small; Attribute Sets convert to Point Data; Hierarchical Generation for large grids. — [Epic PCG GPU docs](https://dev.epicgames.com/documentation/en-us/unreal-engine/using-pcg-with-gpu-processing-in-unreal-engine)
- Fidget (Matt Keeter, Rust): deduplicated math expression graphs compiled to "tapes"; interval arithmetic used to simplify tapes; hand-written aarch64/x86_64 JIT (point, interval, SIMD, gradient); interpreter fallback required for wasm32; bytecode format; Manifold Dual Contouring meshing; crates incl. `fidget-wgpu`; repo notes LLM-assisted code and suggests pinning 0.4.1 to avoid it. — [github.com/mkeeter/fidget](https://github.com/mkeeter/fidget)
- Blackjack (Rust, rend3 + wgpu + egui): node-driven non-destructive low-poly modeler inspired by Houdini/Blender GN; "no promises regarding stability"; the page I fetched was a 0-star fork with 55 commits, activity unverified. — [Shchvova/blackjack fork](https://github.com/Shchvova/blackjack) (upstream setzer22/blackjack, per README)
- Graphite (Rust) is a 2D node-based editor with a procedural graph engine ("Graphene"), reported July 2025. — [Digital Production](https://digitalproduction.com/2025/07/14/graphite-open-source-vector-design-tool-with-a-twist/)
- Bevy ECS: table storage (default; fast iteration, slow add/remove) vs sparse sets per component; change detection via `Added<T>`/`Changed<T>`; bevy_ecs 0.19.x current on docs.rs. — [docs.rs bevy_ecs](https://docs.rs/bevy_ecs), [Bevy 0.5 notes](https://bevy.org/news/bevy-0-5/)
- Lagrange (Adobe): "robust geometry processing library", modular, interface decoupled from algorithms. Data-model details not retrieved. — [github.com/adobe/lagrange](https://github.com/adobe/lagrange)

### Inferences
- Fidget's tape + interval simplification is directly relevant to the "implicit field (SDF expression tree)" representation and to the PLAN Cranelift/JIT choice; Fidget already solves JIT-plus-interpreter-fallback for wasm, supporting the PLAN's reference-interpreter design.
- Unreal PCG's lesson: GPU graph partitioning with minimal transfers; PLAN's "WGSL from same IR" should plan fusion regions that are GPU-resident end-to-end.
- Bevy's table vs sparse-set tradeoff is analogous to dense columns vs sparse attribute storage; consider a sparse-column variant for rarely-set attributes.

### Gaps
- Not covered with sources: Unity, Geometry Script, Substance Designer/Painter evaluation, Gaffer (hash/lazy plug design; search returned nothing relevant), Cavalry, libigl, OpenSubdiv, OpenVDB/NanoVDB, Polyscope, rend3 status, Nodium. Recommend a follow-up pass, especially Gaffer (hash-before-compute is the closest existing analogue to the CAS bet) and OpenVDB/NanoVDB.

## Q4. Is anyone building an agent-native or open-source Houdini alternative (2025-2026)?

### Takeaway
No full open-source agent-native Houdini replacement was found. Agent-driving of existing Houdini is active (community MCP servers, NodeArchitect, SideFX/NVIDIA MCP preview in H22).

### Cited Findings
- SideFX/NVIDIA Apex Script MCP server announced at Houdini 22 keynote (June 2026) as a Labs preview. — [Jon Peddie](https://www.jonpeddie.com/news/sidefx-and-nvidia-bring-mcp-powered-ai-agents-to-houdini-22s-rigging-workflow-at-siggraph-2026/)
- Third-party MCP servers claim node creation, cooking, VEX, USD, rendering and visual verification inside Houdini. — [Glama](https://images.glama.ai/mcp/servers/integrations/houdini)
- Only Rust 3D open-source node modeler surfaced is Blackjack; generic "Houdini alternatives" lists are commercial tools. — [Blackjack](https://github.com/Shchvova/blackjack), [toolradar](https://toolradar.com/alternatives/houdini)

### Inferences
- The "agent-native" differentiator must be depth (typed language, static checks, determinism, gym) since MCP-over-existing-DCC is commoditizing. Houdini's weak point for agents is that scenes are GUI-state hip files, with no verifier; this is an inference, not sourced.

### Gaps
- Did not search for ComfyUI-style/agent 3D tools, Three.js/Blender LLM agents, or startups specifically; absence of results is not proof of absence.

## Q5. Flags on docs/PLAN.md

### Takeaway
Mostly sound; several statements are overstated or unverified relative to public sources.

### Cited Findings
- PLAN Bet 2: "Houdini's packed prims are an opt-in afterthought." Packed prims are a mature feature with disk delayed load, shared references and viewport instancing; calling them an "afterthought" is rhetoric, and PLAN should say "opt-in, with per-instance attribute restrictions". — [packed docs](https://www.sidefx.com/docs/houdini/model/packed.html)
- PLAN Bet 1: "Houdini runs each node as a separate full pass with a full copy." Not confirmed by any source found; HDK shows paged attribute arrays and Houdini tracks data IDs; Blender-style page/array sharing may exist internally. Reword as unverified or measure. — [HDK GA](https://www.sidefx.com/docs/hdk/_h_d_k__g_a__using.html), [hou.Geometry](https://www.sidefx.com/docs/houdini/hom/hou/Geometry.html)
- PLAN §1: "nothing is differentiable" and "a scene is not diffable" are directionally fine, but "cooking is serial and opaque" is overstated: Houdini cooks SOPs multithreaded internally, PDG distributes, and Solaris/USD layers are diffable text (usda). (Solaris layering source: [Solaris docs](https://www.sidefx.com/docs/houdini/solaris/usd.html); multithreading claim is general knowledge, not sourced here.)
- PLAN §3/§4 implies agent connectivity is a unique advantage; SideFX is shipping MCP in H22 and community MCP servers exist. — [Jon Peddie](https://www.jonpeddie.com/news/sidefx-and-nvidia-bring-mcp-powered-ai-agents-to-houdini-22s-rigging-workflow-at-siggraph-2026/)
- PLAN §5 lists splat rendering/import as native novelty; Houdini 22 ships native Gaussian splats (train, relight, Karma render). — [CGChannel](https://www.cgchannel.com/tag/relight-gaussian-splat/)
- PLAN Bet 2 "order of magnitude" memory/time drop on wide geometry: no public benchmark found; Blender reports "at least 25% in simple situations" from implicit sharing. Treat 10x as a hypothesis to prove with the Phase 1 gate. — [Blender 3.6 notes](https://developer.blender.org/docs/release_notes/3.6/nodes_physics/)
- PLAN §11 says "Rust 1.99 (GNU)": not verified against any source here; check that toolchain version string (outside research scope).
- PLAN Bet 3 "demand-driven region/LOD cooks, Houdini cooks whole networks": Houdini TOPs and Solaris (lazy USD stage/Hydra) have partial demand/dirty semantics; SOP-level claim is plausible but unsourced here. Blender's field evaluation is lazily demand-driven per-field, so "no DCC does demand-driven" would be wrong at attribute level.

### Inferences
- Safer phrasing for the plan: "Houdini offers packed prims and data-ID tracking; we make instance tables, shared columns and content-addressed caching the uniform default and share caches across processes."

### Gaps
- Houdini internal COW behaviour remains unverified; resolving it (a micro-benchmark in Houdini: chain of Attribute Wrangles on 10M points, measure memory per node) would settle the PLAN's central comparative claim.
