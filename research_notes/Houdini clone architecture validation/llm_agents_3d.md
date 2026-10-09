# LLM/Agent-driven 3D and procedural content generation: MCP tooling, critics, gyms, interface design

Scope note: research done via web search/fetch on 2026-10-09; roughly 25 tool calls. Several items come from search-result summaries, not full-paper reads; those are flagged. Items I could not source are in Gaps, not asserted.

## 1. Existing MCP/agent integrations for DCCs (Blender, Houdini, Unreal, Unity)

### Takeaway
The dominant, most-adopted pattern is a thin "execute arbitrary host-language code + screenshot" bridge (Blender MCP, ~30k stars), not a rich typed tool surface. Its known weaknesses (no sandbox, timeouts on complex requests, a published code-injection CVE) are exactly what a typed, static-checked, deterministic Rust surface can differentiate against.

### Cited Findings
- ahujasid/blender-mcp shows 30.3k stars, 2.7k forks, MIT license, 244 commits (as of fetch). Architecture: Blender addon runs a socket server (default localhost:9876, JSON over TCP) + a Python MCP server. Nine tools: `execute_blender_code`, `look` (viewport/camera/auto-framed/animation-strip renders), `get_scene_info` (one line per object), `generate_3d` (Tripo/Hunyuan3D/Rodin), `search_assets`/`import_asset` (Poly Haven, Sketchfab, Poly Pizza), `get_addon_status`, `disable_telemetry`, `record_trajectory_feedback` — [GitHub](https://github.com/ahujasid/blender-mcp)
- Its own documented limitations: arbitrary Python, advice to save work first; socket has no auth/encryption; complex requests must be split into smaller steps and time out; optional "safe mode" script checker; opt-in telemetry of prompts/code/screenshots; a `record_trajectory_feedback` tool exists (i.e., the project is harvesting agent trajectories) — [GitHub](https://github.com/ahujasid/blender-mcp)
- CVE-2026-10688: code injection in `execute_blender_code` (`src/blender_mcp/server.py`), publicly disclosed, maintainer reportedly unresponsive, rolling release so no fixed version — [SentinelOne](https://www.sentinelone.com/vulnerability-database/cve-2026-10688/); summary via [Augment listing](https://www.augmentcode.com/mcp/blender-mcp)
- Houdini MCP servers are community projects, none official from SideFX: OtakuSquirrel/houdini-mcp (create/modify nodes, set params, run Python, viewport capture, render, health diagnostics, multi-instance pooling; MCP + RPyC; Windows, Houdini 20.5+), an oculairmedia server over `hrpyc` (default port 18811), plus a Houdini 21+ "bridge" exposing an HTTP API — [Glama listing](https://glama.ai/mcp/servers/bemmg7xfl7), [LobeHub search](https://lobehub.com/mcp?q=houdini)
- Unity: CoplayDev/unity-mcp is the most popular (6.7k stars in one March 2026 tracker, 9.2k in another; numbers from trackers and unreliable); smaller ones: ivanmurzak (399), isuzu-shiranui (131), akiojin (advertises 108+ tools, 32 stars). Unreal servers are smaller/newer: ziggymar/unreal-mcp (Blueprints), UEMCP via Unreal's built-in Python remote exec (`uvx uemcp`), runreal (23 stars), ChiR24 (Remote Control API) — [search roundup: oosmetrics/gittrend/playbooks](https://oosmetrics.com/repo/CoplayDev/unity-mcp), [PyPI uemcp](https://pypi.org/project/uemcp/), [playbooks](https://playbooks.com/mcp/runreal-unreal-engine)
- Houdini's DAG is argued to be "structurally isomorphic" to computational graphs LLMs generate (NaturalHoudini paper); it explicitly does not benchmark and lists validation as future work — [IJIRT paper](https://ijirt.org/publishedpaper/IJIRT201804_PAPER.pdf) (low-tier venue; treat as opinion)
- A fine-tuned Qwen3-8B Houdini-VEX model and a "HoudiniVexBench" (completion, doc-to-code, explanation) exist, but only aggregator-listed with no verified results — [Featherless](https://featherless.ai/models/anshulVashist/Qwen3-8B-Houdini-VEX-v10), [benchmarklist](https://benchmarklist.com/benchmarks/houdini_vex_bench/)

### Inferences
- Adoption is driven by zero-friction "write code in the host language" generality, not tool richness; our surface must match one-command connect and code-style authoring to compete.
- Every Houdini/Unreal/Blender bridge inherits the host's untyped Python API, so API-mismatch errors and unsafe exec are structural; a sandboxed typed language inside our own runtime removes both classes.
- `look`/viewport screenshot and trajectory logging in blender-mcp validate our `render_preview` and trajectory-recording plans.

### Gaps
- No verified numbers on active users/download counts for any DCC MCP server beyond GitHub stars. No published failure-rate study of blender-mcp sessions found. Houdini MCP star counts not retrieved. Maya MCP and Claude/GPT computer-use for 3D: not researched (no sources found within budget).

## 2. LLM-driven procedural generation research (3D-GPT, SceneCraft, BlenderAlchemy, MeshCoder, Holodeck, LayoutGPT, I-Design, CAD code)

### Takeaway
Across systems, the working recipe is: LLM emits code or relations in a constrained API, a deterministic solver/executor handles geometry, and a render+VLM (or metric) loop refines. Direct numeric placement by LLMs yields collisions; solver-based layout fixes physics but loses semantics; most failures are API mismatch and floating/disconnected parts.

### Cited Findings
- 3D-GPT (2023, pre-range but foundational): three agents (task dispatch, conceptualization, modeling); the modeling agent reads generator docs, infers parameters, writes Python for Infinigen; the repo plans to switch to emitting a config file instead of Python for editability. Critics say being tied to parametric Infinigen limits asset diversity and large-scale scenes — [arXiv](https://arxiv.org/pdf/2310.12945), [summary](https://www.sourcepulse.org/projects/1838868)
- SceneCraft (ICML 2024): scene-graph plan -> Blender code converting relations to numeric constraints -> VLM render critique loop; "library learning" accumulates reusable functions without finetuning; handles ~100 assets — [PMLR](https://proceedings.mlr.press/v235/hu24g.html)
- BlenderAlchemy (ECCV 2024): iteratively refines a Blender Python program; vision-based edit generator + state evaluator search over edits; uses generated "imagined" reference images; tasks: procedural material, geometry, lighting — [ECCV](https://www.ecva.net/papers/eccv_2024/papers_ECCV/papers/12578.pdf), [arXiv](https://arxiv.org/html/2404.17672v1)
- MeshCoder (NeurIPS 2025): hand-written Blender Python API set for detailed geometry, paired object/code dataset split into semantic parts, multimodal LLM trained from point clouds to scripts; 41 categories; claims code form eases editing and LLM reasoning about shape — [arXiv](https://arxiv.org/pdf/2508.14879)
- Holodeck (CVPR 2024): GPT-4 emits spatial relational constraints, a solver optimizes layout; 50K Objaverse assets; 680-participant human study preferred it over ProcTHOR on residential scenes — [arXiv](https://arxiv.org/pdf/2312.09067)
- Failure comparison (competitors' framing, so biased): LayoutGPT (direct coordinates) gives collisions/out-of-bounds; Holodeck satisfies hard physical constraints but often violates specified categories/relations and struggles with many objects; I-Design suffers agent-communication/grounding failures and local clustering — [Direct Numerical Layout (2506.05341)](https://arxiv.org/pdf/2506.05341), [Text-to-Scene with LRMs (2509.26091)](https://arxiv.org/html/2509.26091v2), [HOG-Layout](https://arxiv.org/pdf/2604.10772)
- 3DCodeBench (May 2026): 12 VLMs, text/image -> procedural 3D code; most failures are API mismatches; successful renders often have disconnected/floating components; larger thinking budgets and multi-turn refinement help; authors say models need higher-quality procedural coding data; also 3DCodeArena (pairwise human preference). Software targeted not confirmed from the abstract — [arXiv](https://arxiv.org/abs/2606.01057)
- ScenePilot (2026): retrieval of layout priors, add object groups incrementally, RL-trained multimodal "repair" policy emits move/rotate/scale actions after each insertion plus global repair; trained on SceneReverse-17k (perturbed good scenes with inverse ops as targets). No numbers in abstract — [arXiv](https://arxiv.org/abs/2608.30307)
- "Thinking in Blender" (2026): staged, verifiable sub-steps (init scene, model objects, textures/materials, compose, lighting) for inverse graphics — [arXiv](https://arxiv.org/pdf/2606.02580) (snippet-level only)
- CADCodeVerify (ICLR 2025): VLM generates and answers validation questions about the render, feeds corrections back; CADPrompt = 200 prompts with expert code; GPT-4 compile rate 96.5% vs Gemini 85% vs CodeLlama 73.5%; +7.30% point-cloud distance and +5.5% compile rate over prior work (v1 abstract says 5.0% success; versions differ) — [arXiv](https://arxiv.org/pdf/2410.05340)
- CAD-Coder (NeurIPS 2025): CadQuery target; SFT (110K text-CadQuery-3D triplets) then GRPO with Chamfer-distance geometric reward + format reward; chain-of-thought planning because many scripts yield equal geometry — [NeurIPS](https://proceedings.neurips.cc/paper_files/paper/2025/hash/564a224e88f2490f3c1deaae877d37e3-Abstract-Conference.html)
- LLM-written Rust compile error rates cited: o1-mini 18%, R1 39%, Claude 3.5 Sonnet 27%; OCaml/Haskell 40-60% — [Mündler et al.](https://arxiv.org/abs/2504.09246)

### Inferences
- Evidence favors an LLM-proposes-relations/constraints, solver-places pattern; our `assert min_clearance`, `no_self_intersection` specs plus a repair loop map to this. Offering a constraint-layout node (relations in, placed instances out) avoids the LayoutGPT failure class and a "soft semantic constraints" check guards against the Holodeck one.
- "Floating/disconnected components" as the top residual failure supports a built-in `connected_components`/`grounded` validation spec.
- Library learning (SceneCraft) and trajectory harvesting (blender-mcp) both match the plan's recipe-promotion pipeline.
- Direct evidence on DSL vs Python from these papers is thin: MeshCoder and 3D-GPT both interpose a curated/documented API layer rather than raw host API, and 3D-GPT moves toward config output. This supports "small curated API" over raw bpy-style surface; it is not a head-to-head DSL-vs-Python study.

### Gaps
- Not retrieved: Infinigen/Infinigen-Indoors, LLaMA-Mesh, Text2Room, ShapeAssembly/ShapeCoder, Text2CAD, CAD-Recode, LayoutGPT/I-Design primary papers, LLM Geometry Nodes/HDA studies (a search found no Geometry Nodes benchmark). No head-to-head DSL vs Python experiment found. No quantitative numbers from 3DCodeBench or ScenePilot (abstract pages only).

## 3. VLM critics/judges for 3D and images

### Takeaway
Off-the-shelf VLM judges agree with humans only moderately on 3D (best pairwise ~0.66 vs 0.79 human-human in BlenderGym), are better at prompt adherence than geometry, have position bias, and CLIP correlates poorly. Use VLMs as soft ranking inside search, with deterministic specs as the hard gate.

### Cited Findings
- BlenderGym (CVPR 2025): human-VLM pairwise agreement for Claude-3.5-Sonnet 0.66, inter-human 0.79, random 0.5; verifier itself improves with inference scaling — [arXiv HTML](https://arxiv.org/html/2504.01786v1)
- Position bias: a cross-VLM mesh study required querying both orders and keeping order-consistent verdicts; judge families agree with Cohen's kappa 0.66 — [DB-3DME](https://arxiv.org/pdf/2606.10142)
- VLMs align better with humans on Prompt Adherence than on Geometry; CLIP similarity Pearson with human 0.074 (Geometry) and 0.228 (Prompt Adherence) — [DB-3DME](https://arxiv.org/pdf/2606.10142)
- Gen3DEval: GPT-4o significantly worse than a fine-tuned 3D-specific evaluator at matching human judgment — [arXiv](https://arxiv.org/html/2504.08125v1)
- Defect-detection study: best of 12 VLM judges still lags trained humans; texture agreement drops with noisier labels — [3D-DefectBench](https://arxiv.org/abs/2607.10826)
- CADCodeVerify uses self-generated yes/no validation questions as a structured critic rather than free scoring — [arXiv](https://arxiv.org/pdf/2410.05340)

### Inferences
- Structured, question-decomposed rubrics (CADCodeVerify) and order-swapped pairwise comparison are the cheapest reliability wins; build both into `critique`.
- CLIP score alone should not be a reward; silhouette/depth/normal-based metrics from our own renderer (exact access to geometry) should dominate.
- Because VLM sees renders only, providing multi-view, section and attribute-heatmap images (already planned) targets the "visual encoding" bottleneck.

### Gaps
- GPTEval3D primary paper, LPIPS/SSIM reliability for procedural scenes, calibration (ECE) numbers, verbosity/self-preference bias for 3D specifically: not retrieved.

## 4. RL gyms/benchmarks and evidence RL helps

### Takeaway
BlenderGym is the closest published "gym" (245 start/goal pairs, 5 tasks); evidence for RL on procedural modeling exists mainly in CAD (CAD-Coder GRPO with geometric reward) and layout (MetaSpatial multi-turn RL), with generic RLVR environments showing multi-point transfer gains. There is no published SWE-style RL environment for a full procedural DCC found.

### Cited Findings
- BlenderGym: 245 handcrafted start-goal pairs, 5 tasks (placement, lighting, procedural material, blend shape, procedural geometry); best VLM vs human on placement PL 10.94 vs 0.423 (~26x), blend shape ~10x, material ~6x, geometry ~5x, lighting ~2x; several open-source VLMs produced no executable script on >75% of geometry instances — [arXiv HTML](https://arxiv.org/html/2504.01786v1)
- BlenderGym inference scaling: below ~30 queries, spend on generation (verify ratio 0.33 best); above ~100 queries, spend on verification (0.73 best); a scaled InternVL2-8B beat unscaled GPT-4o/Claude-3.5 on blend shape with 32-candidate trees — [arXiv HTML](https://arxiv.org/html/2504.01786v1)
- CAD-Coder: SFT+GRPO with Chamfer reward on Qwen2.5-7B-based model — [NeurIPS](https://proceedings.neurips.cc/paper_files/paper/2025/hash/564a224e88f2490f3c1deaae877d37e3-Abstract-Conference.html), [HF](https://huggingface.co/gudo7208/CAD-Coder)
- MetaSpatial (2025): multi-turn RL with physics-aware constraints and rendered-image evaluation; rule-based rewards for format, collision, physical violations via scene-graph; claims gains across scales, no figures retrieved — [arXiv](https://arxiv.org/abs/2503.18470)
- TRACE: 1,000 tasks over 277 scene grammars; RLVR improves macro-average over 24 external benchmarks by 3.51 (Qwen2.5-VL-3B) and 4.06 points (7B) — [HF papers](https://huggingface.co/papers/2607.19790.md) (procedural-environment RL, not 3D modeling)
- TRON: online generator-verifier environments with per-environment curricula — [arXiv](https://arxiv.org/pdf/2606.01599)
- ScenePilot builds supervised repair data by perturbing good scenes and inverting the perturbation — a free source of (state, correct action) pairs — [arXiv](https://arxiv.org/abs/2608.30307)

### Inferences
- A deterministic CAS runtime plus exact geometric metrics gives HoudiniGym cheap, noise-free verifiable rewards, the property that made CAD-Coder-style GRPO work; reward should be geometry/spec based, with VLM score as a shaping term only.
- Perturb-and-invert data generation can be done on our graphs for free (mutate params, record inverse edit) to bootstrap SFT before RL.
- Gym should expose split compute knobs (n candidates vs verifier calls) because BlenderGym shows the optimum shifts with budget.

### Gaps
- No evidence found for RL specifically improving agent performance on Blender/Houdini procedural editing end to end; CAD-Recode, SWE-style graphics envs, and Gymnasium conventions for such tasks not retrieved. Numeric RL gains for CAD-Coder/MetaSpatial not retrieved.

## 5. Agent-interface design lessons (code-mode vs tools, tool counts, MCP features, token efficiency)

### Takeaway
Writing code against a progressively-disclosed API beats loading many tool schemas and routing intermediate data through the model; tool-selection accuracy falls as visible tool count grows, mitigated by retrieval/search-tools. But gains are task-dependent and sandboxing is mandatory.

### Cited Findings
- Anthropic: agents "scale better by writing code to call tools"; example 150,000 -> 2,000 tokens (98.7%) on one workflow; recommends progressive disclosure (filesystem tree of tool files or `search_tools` with detail levels), filtering data in the execution environment, loops/conditionals in code, state persisted to files, saved code as reusable skills; caveat: needs sandboxing, resource limits, monitoring — [Anthropic Engineering](https://www.anthropic.com/engineering/code-execution-with-mcp)
- Cloudflare Code Mode: convert MCP tools to a TypeScript API and have the model write code; models have seen far more real TypeScript than tool-call examples; run in V8 isolates — [Cloudflare blog](https://blog.cloudflare.com/code-mode/)
- Independent test: regular MCP vs code execution, both 100% success; input tokens -78.5% (15,417 -> 3,310), output tokens +120% (87 -> 192), latency +7%; programmatic tool calling elsewhere reported -37% (43,588 -> 27,297) — [AIMultiple](https://research.aimultiple.com/code-execution-with-mcp)
- RAG-MCP: retrieving a tool shortlist raised tool-selection accuracy 13.62% -> 43.13% and cut prompt tokens >50%; thresholds like "10-15 tools" or "30+" appear only in secondary blogs and conflict — [arXiv 2505.03275](https://arxiv.org/html/2505.03275v1), [StackOne](https://www.stackone.com/blog/agent-tool-count-ceiling/)
- LiveMCPBench: 95 tasks, 70 MCP servers, 527 tools for tool retrieval at scale — [arXiv](https://arxiv.org/html/2508.01780v1)
- blender-mcp's own experience: complex requests time out and must be split into smaller steps (exec-style call is one blocking call) — [GitHub](https://github.com/ahujasid/blender-mcp)

### Inferences
- A `hcs`/code-style surface plus a small fixed tool set (search, describe, run, inspect, render) with progressive disclosure fits all three sources; the 100+ tool "fine-grained edits" list in the PLAN table should be exposed via `search_tools` or as code-API stubs, not as flat tool schemas.
- Because output tokens rise in code mode, keep `hcs` terse and make errors return fixes (compact), to avoid the output-token cost.
- Async cooks with progress events address the timeout failure seen in exec-style bridges.

### Gaps
- Not retrieved: OpenAI engineering posts, MCP resources/prompts adoption evidence, structured-error benchmarks, hard tool-count limits from primary papers, Claude/GPT computer-use performance on 3D GUIs.

## 6. Static feedback / type-checking benefits for LLM code generation

### Takeaway
Type-aware constraint and compiler feedback measurably reduce failures: type-constrained decoding cuts compile errors by more than half on HumanEval/MBPP, and the dominant failure of 3D code agents (API mismatch) is exactly what a static checker catches. Constrained decoding needs model internals, so for API-only models the practical route is compiler-in-the-loop with machine-actionable diagnostics.

### Cited Findings
- Mündler et al. (PLDI 2025): prefix automata + search over inhabitable types; reduces compilation errors by more than half and improves functional correctness for synthesis, translation, repair across model sizes/families; needs next-token distributions so does not apply to API-only models; some errors remain when the model fails to terminate properly — [arXiv](https://arxiv.org/abs/2504.09246), [PLDI](https://pldi25.sigplan.org/details/pldi-2025-papers/25/Type-Aware-Constraining-for-LLM-Code-Generation)
- Compile error rates for Rust cited at 18-39% for frontier models, 40-60% for sparse-data languages like OCaml/Haskell — [same paper](https://arxiv.org/abs/2504.09246)
- 3DCodeBench: most failures are API mismatches, i.e., statically detectable — [arXiv](https://arxiv.org/abs/2606.01057)
- CADCodeVerify: compile rate varies hugely by model (73.5% to 96.5%), and iterative verification improved compile rate by ~5 points — [arXiv](https://arxiv.org/pdf/2410.05340)

### Inferences
- Sparse-data-language finding is the main risk for a custom DSL: a novel `hcs` syntax has zero pretraining data, so error rates may resemble OCaml/Haskell unless mitigated (surface syntax close to Rust/Python/TypeScript, in-context grammar, examples, fast checker feedback, and eventually SFT/RL on trajectories). Type-constrained decoding is available only if we later host/fine-tune open models.
- Static diagnostics must be cheap (parse+check in ms, no cook) to allow many repair iterations.

### Gaps
- No study found comparing a novel DSL with Python for agent success on 3D tasks; no data on grammar-constrained decoding for DSLs in 3D (e.g., via llguidance/XGrammar); no measurement of LLM performance on Rhai/Lua/WASM-hosted scripting specifically.

## 7. Recommendations for PLAN.md "Bet 4" and the agent-surface table (including `hcs` vs embedding)

### Takeaway
Keep `hcs`, but as a thin, Rust/TypeScript-flavored surface over the typed graph IR, shipped with an LSP-style checker and a generated stub API, and add a Python/TS-like "code mode" binding as the on-ramp so day-one agent performance does not depend on a language no model has seen. Evidence directly comparing DSL vs Python is absent, so make it an experiment on the gym.

### Cited Findings
(see sections 1-6; no new sources)

### Inferences
Bet 4 refinements:
1. State the hypothesis explicitly and test it: "`hcs` beats embedded Python on gym success rate per token." Define the gym task suite (T1-T3) and run three arms: `hcs`, Python bindings (via hc-py), and flat MCP tools. Decide with data; the literature supplies no head-to-head result.
2. DSL risk mitigation, from the sparse-language evidence: (a) design `hcs` syntax to be near-isomorphic to Rust/TS/Python constructs models know; (b) ship a generated reference (from the `#[sop]` macro) in the system prompt or retrievable via `search_tools`; (c) checker returns structured diagnostics with fixes in <50 ms; (d) log all trajectories (blender-mcp already does) to build SFT/RL data; (e) offer grammar for constrained decoding (and type-constrained decoding if open-weight models are used).
3. Why DSL still wins on our axes over Python/Lua/Rhai/WASM embedding: determinism and content-addressing (a pure, total language with no I/O hashes cleanly), static attribute-flow/unit typing (Python cannot enforce it without a parallel checker), safe-by-construction execution (blender-mcp's unsandboxed exec and CVE-2026-10688 are the cautionary case), round-trip with the graph for diffing. Embedded Python remains valuable as an escape hatch and for model familiarity; WASM for plugin nodes only (as the PLAN already has). Rhai/Lua offer no pretraining advantage over a purpose-built DSL and no static types, so they are the weakest options.
4. Add a "validated-by-construction placement" primitive (relations -> solver -> instances) since direct LLM coordinates cause collisions; keep semantic soft constraints checked by specs.
5. Add default specs: `connected`/`grounded` (no floating components), `no_self_intersection`, `within_bounds`, units.
6. Critic: implement order-swapped pairwise judging and question-decomposed rubrics; weight VLM below deterministic metrics; calibrate on a small human-rated set and report agreement against the ~0.66/0.79 BlenderGym reference points. Avoid CLIP as reward.
7. Search: make the verify-vs-generate budget split a first-class `search` parameter (BlenderGym: generate-heavy at low budgets, verify-heavy above ~100 queries).
8. Gym: rewards from exact geometry (Chamfer/IoU/spec pass) plus format reward; use perturb-and-invert synthetic tasks; record the cost axis (tokens) as an official metric.

Agent-surface table changes:
- Connect: add loopback-only default, auth token, per-session sandbox; note blender-mcp's unauthenticated socket and CVE.
- Author: lead with `run_hcs(program)` and `check_hcs`; demote node-level tools to generated code-API stubs found via `search_api(query, detail=name|signature|full)` (Anthropic progressive disclosure); keep core visible tools to about a dozen (exact ceiling is unsourced; measure on gym).
- Discover: `search_nodes` etc. should be one retrieval tool with detail levels (RAG-MCP: retrieval tripled selection accuracy in its test).
- Check: make static check automatic on every `run_hcs` response, returning structured diagnostics with suggested fixes; add `grounded`, `connected`, `bounds` specs.
- See: multi-view + section + heatmap by default in one call (`look`-style), with auto-framing like blender-mcp's `look`.
- Search: add budget-split parameter; Scale: async with progress events to avoid blocking-timeout failures.
- Train/Eval: record trajectories (opt-in), perturb-and-invert generator, report tokens-to-success.

### Gaps
- Specific numeric targets for the above (tool count cap, DSL error rates) cannot be sourced and must come from our own gym runs.
