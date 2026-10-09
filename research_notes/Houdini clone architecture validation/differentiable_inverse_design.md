# Differentiable geometry, rendering, simulation and inverse procedural modeling: what is proven for an `optimize()` feature

Method note: about 16 search/fetch calls. Search snippets were thin for 2025-2026 work; several claims below come from search-result summaries rather than full-paper reads, and are flagged. Items I could not source are in Gaps, not in findings. Most strong evidence is 2020-2024; 2025-2026 coverage is sparse.

## 1. Differentiable rendering (Mitsuba/Dr.Jit, nvdiffrast, SoftRas, Slang.D, Gaussian splatting): costs, limits, CPU viability

### Takeaway
Physically based differentiable rendering is mature (Mitsuba 3 / Dr.Jit has an LLVM CPU variant; path-replay gives constant memory, linear time), but visibility/edge gradients need special estimators, and published speed numbers are GPU-only. For a GPU-free `optimize()` the realistic path is a soft or stochastic rasterizer plus SDF/silhouette losses first, with CPU differentiable path tracing treated as a later, slow option.

### Cited Findings
- Mitsuba 3 differentiable rendering treats the renderer as a function from scene parameters to an image. Autodiff is built on Dr.Jit JIT array types, and you enable it by picking an `_ad` variant such as `cuda_ad_rgb` or `llvm_ad_rgb`. So a CPU (LLVM) differentiable variant exists. — [Mitsuba docs](https://mitsuba.readthedocs.io/en/stable/src/inverse_rendering/gradient_based_opt.html)
- Path replay backpropagation (Vicini, Speierer, Jakob, SIGGRAPH 2021) differentiates light paths with constant memory and linear time. Reparameterization of discontinuous integrands (Loubet, Holzschuch, Jakob 2019) handles visibility discontinuities that otherwise give incorrect gradients. — [search summary citing both; Mitsuba docs](https://mitsuba.readthedocs.io/en/stable/src/inverse_rendering/gradient_based_opt.html); [Dr.Jit paper](https://arxiv.org/pdf/2202.01284)
- Jakob's Dec 2025 talk frames the core problem: visibility creates step discontinuities in the integrals, and tape-based autodiff exhausts memory on large intermediate state. — [CERN talk page](https://indico.cern.ch/event/1543104/contributions/6751660/)
- 2025 work: "Radiative Backpropagation with Non-Static Geometry" (arXiv 2504.05750) addresses time-varying geometry. I saw only the title/reference list, not the abstract. — [arXiv](https://arxiv.org/pdf/2504.05750)
- 2024: a Dr.Jit-based differentiable SDF renderer compares against SDF convolution, SDF reparameterization and projective sampling, and reports the highest PSNR across area light sizes in its tests. — [arXiv 2405.08733](https://arxiv.org/pdf/2405.08733)
- 2024: a method turns a non-differentiable rasterizer into a differentiable one via stochastic gradient estimation (relevant to wrapping our own software rasterizer without writing analytic edge gradients). Abstract details not verified. — [arXiv 2404.09758](https://arxiv.org/pdf/2404.09758)
- Slang.D (SIGGRAPH Asia 2023) adds built-in autodiff to Slang, handling arbitrary control flow, dynamic dispatch, generics and higher-order differentiation. NVIDIA reports a real-time path tracer made differentiable reusing 90% of the Slang code. Reported per-sample cost on RTX 4090: forward-mode 12 ms, reverse-mode 54 ms, versus 2.4 ms for the primal pass (about 5x and 22x overhead). — [NVIDIA Research](https://research.nvidia.com/labs/rtr/publication/bangaru2023slangd); [NVIDIA blog](https://developer.nvidia.com/blog/differentiable-slang-a-shading-language-for-renderers-that-learn/)
- 3DGS rasterizer is tile-based, sorts once per image, and backpropagates over any number of blended Gaussians with constant per-pixel memory overhead. — [3DGS paper](https://arxiv.org/pdf/2308.04079)
- gsplat (JMLR 2025) is a PyTorch front end over CUDA kernels. Reported: up to 4x less memory / 10% less training time in the abstract-level claim, and 4.3 GB vs 7.7 GB and 19.39 vs 26.19 min in another summary of the same paper (different comparisons; check paper before quoting). MCMC densification reduces Gaussian count and memory. — [JMLR](https://jmlr.org/beta/papers/v26/24-1476.html); [alphaxiv overview](https://alphaxiv.org/overview/2409.06765v1)
- AMD ROCm gsplat benchmark: 7k-iteration model 4.52 GB, 159.96 s, 27.61 dB PSNR; 30k-iteration 6.31 GB, 872 s (MI300X). — [ROCm gsplat benchmarks](https://rocm.docs.amd.com/projects/gsplat/en/latest/reference/benchmark-evaluation.html)
- 2025 hardware-rasterization paper: backward rasterization over 10x faster than naive atomics, 3x over canonical tile-based, 3.07x full-pipeline speedup with float16 on RTX 4080. — [arXiv 2505.18764](https://arxiv.org/pdf/2505.18764)
- DiffCSG (2024) provides differentiable CSG via rasterization for fixed CSG structure, explicitly intended to refine continuous parameters after discrete structure is found. — [arXiv 2409.01421](https://arxiv.org/pdf/2409.01421)

### Inferences
- The only sourced CPU differentiable renderer here is Mitsuba's `llvm_ad` variant. No source gave CPU timings, so any CPU speed target is unmeasured. Our own plan item (soft rasterizer on CPU, silhouette/depth/normal losses) is consistent with the literature but I found no sourced CPU benchmark for it.
- Slang.D-style overheads (about 5x forward, about 22x reverse relative to primal) are a reasonable planning ratio for reverse-mode over a renderer on GPU; for our IR autodiff, expect the same order (reverse-mode costing a few to tens of primal evaluations) but this is an inference.
- Path-replay plus reparameterization is the proven recipe; implementing it from scratch in a Rust CPU tracer is a multi-month project, supporting the plan's "soft rasterizer first, path tracing later" ordering.
- Silhouette/SDF-based losses sidestep the hardest part (edge gradients), so geometry fits are cheaper than photometric fits.

### Gaps
- No sourced CPU-only timings for any differentiable rasterizer, path tracer or 3DGS.
- nvdiffrast, SoftRas, PyTorch3D and Nvdiffrec: no 2025-2026 sources retrieved; I did not fetch their papers, so no claims are made here.
- Abstract-level contents of arXiv 2504.05750 and 2404.09758 not verified.

## 2. Differentiable simulation (DiffTaichi, Warp, Newton, DiffPD, cloth, rigid, fluid)

### Takeaway
Differentiable simulators work for short horizons and smooth dynamics (cloth, soft bodies via projective dynamics) but gradients become wrong or useless with collisions (time-of-impact), frequent contact, stiffness contrasts and chaotic long rollouts. Treat gradient-through-solver as a "local refinement tool" backed by gradient-free fallback.

### Cited Findings
- Newton is an open-source GPU physics engine built on NVIDIA Warp, initiated by Disney Research, Google DeepMind and NVIDIA, announced at GTC 2025, Linux Foundation project, Apache-2.0. Differentiability, OpenUSD and extensibility are core design goals; MuJoCo Warp is the primary solver backend. A dev version string seen was 1.5.0.dev0 and Disney's Kamino solver was beta, so the API is not stable. — [NVIDIA blog](https://developer.nvidia.com/blog/announcing-newton-an-open-source-physics-engine-for-robotics-simulation/); [Newton page](https://developer.nvidia.com/newton-physics); [PyPI](https://pypi.org/project/newton/)
- NVIDIA-reported (vendor) MuJoCo Warp speedups over MJX: 252x locomotion, 475x manipulation on RTX PRO 6000 Blackwell. These are GPU robotics figures, not cloth/fluid for DCC. — [NVIDIA blog](https://developer.nvidia.com/blog/?p=113754)
- Warp tape rules: do not overwrite earlier results while a tape is recording; gradients accumulate and need `Tape.zero()`; atomic adds are accommodated; lambdas, recursion and exceptions are unsupported in kernels. A Warp developer advised writing each layer's output to a new array. — [Warp differentiability docs](https://warp.readthedocs.io/en/latest/user_guide/differentiability.html); [Warp limitations](https://nvidia.github.io/warp/latest/user_guide/limitations.html); [NVIDIA forum](https://forums.developer.nvidia.com/t/warp-library-what-can-be-done-to-big-input-arrays-to-keep-differentiability/220065)
- Warp-based research handles memory by gradient checkpointing (recompute forward during backward, via CUDA graph capture/replay); a user-side pattern, not a Warp feature. — [arXiv 2412.12089](https://arxiv.org/pdf/2412.12089)
- DiffTaichi states that differentiating simulators does not always give useful gradients; collisions can make gradients entirely incorrect, remedied by computing exact time of impact. A follow-up shows discarding TOI gives a completely wrong position gradient and reducing dt does not fix it. — [DiffTaichi](https://cdnjs.deepai.org/publication/difftaichi-differentiable-programming-for-physical-simulation); [arXiv 2207.05060](https://arxiv.org/pdf/2207.05060)
- DiffTaichi limits (secondary course slides): only code inside Taichi kernels is differentiable; global-data-access rules; high memory without checkpointing. — [course slides](https://www.cse.iitd.ac.in/~narain/courses/2301-cov877/presentations/abhay.pdf) (secondary source)
- "Gradients are Not All You Need": exact gradients can be useless due to chaos (Jacobian spectrum) in physics simulation and RNNs. — [arXiv 2111.05803](https://arxiv.org/pdf/2111.05803)
- DiffCloth: Projective-Dynamics cloth with dry frictional contact; demos system ID, assisted-dressing trajectory optimization, closed-loop control, inverse design, real-to-sim, with gradient information giving a substantial speedup for most. Authors concede frequent contact and self-collision gradients are not fully resolved. — [arXiv 2106.05306](https://arxiv.org/pdf/2106.05306)
- DiffPD: differentiable projective dynamics reusing the prefactorized Cholesky; 4-19x faster than standard Newton's method; penalty and complementarity contact models. A later paper (DiffPhD) says DiffPD struggles with extreme stiffness contrasts, large-deformation hyperelasticity and contact-rich interaction. — [arXiv 2101.05917](https://arxiv.org/pdf/2101.05917v2); [catalog entry](https://vufind.lboro.ac.uk/PrimoRecord/cdi_unpaywall_primary_10_1145_3490168)
- Review: broad survey of differentiable simulators (not read in detail). — [arXiv 2407.05560](https://arxiv.org/pdf/2407.05560)

### Inferences
- The plan's "cloth/rigid/particles first" is consistent with where gradients are best behaved (PD cloth, smooth soft bodies). Rigid-body with impacts and cloth self-collision are the failure-prone cases; design them as "gradient-assisted, search-verified".
- Fluids: no sourced differentiable-fluid evidence retrieved; the plan's "adjoint or surrogate" remains unvalidated.
- Warp's array-overwrite rule is directly compatible with a CAS/pure-function runtime (each node writes fresh outputs), which is a design point in our favor.

### Gaps
- No sourced numbers for CPU differentiable simulation; Warp has a CPU backend but I did not retrieve its performance.
- No sourced evidence on adjoint methods for FLIP/smoke (Stable Fluids adjoint, differentiable MPM) beyond the general chaos warning.
- Newton's differentiability maturity (which solvers support gradients) was not verified.

## 3. Inverse procedural modeling (MATch, DiffMat, ADPM, DI-PCG, ShapeCoder, Infinigen, ProcTHOR)

### Takeaway
Gradient-based fitting of procedural graphs works when nodes are differentiable (or proxied) and the graph structure is fixed; structure and discrete choices are handled by search, retrieval or learned priors. The best-documented 2020-2023 systems are materials (MATch, DiffMat v2) and node-based geometry (ADPM); 2024-2025 trend is feed-forward/diffusion inverse networks for fixed generators (DI-PCG).

### Cited Findings
- MATch (SIGGRAPH 2020): DiffMat translates large procedural material graphs into differentiable node graphs; gradient optimization against a style-transfer loss; a neural feature-based method selects graphs and initializes parameters. It assumes a procedural graph is given. — [MIT DSpace](https://dspace.mit.edu/entities/publication/c1487622-b0ae-4002-9a59-4f26c9388494); [MatFormer](https://arxiv.org/pdf/2207.01044)
- Node Graph Optimization Using Differentiable Proxies (Hu et al., SIGGRAPH 2022) uses neural proxies for non-differentiable nodes; MATch was limited to differentiable operations. — [search summary of Hu et al.](https://par.nsf.gov/biblio/10367720)
- DiffMat v2 / end-to-end procedural material capture (SIGGRAPH 2023): adds generic noise/pattern generator nodes and per-pixel filters, and pairs gradient optimization with gradient-free (mixed-integer, proxy-free) search, reporting a closer match than MATch and neural proxies. — [CFG MIT](https://cfg.mit.edu/publications/diffmatv2); [citedrive](https://www.citedrive.com/en/discovery/end-to-end-procedural-material-capture-with-proxy-free-mixed-integer-optimization/)
- Generating procedural materials from text or image prompts (2023): generate candidate graphs from a model, then refine top candidates with differentiable optimization. — [Yale PDF](https://graphics.cs.yale.edu/sites/default/files/generating_procedural_materials_from_text_or_image_prompts.pdf)
- Beichen Li's 2025 MIT thesis addresses limited training data, token-space-only supervision and approximation-free parameter post-optimization; claims transfer to other inverse procedural graphics problems (abstract-level only). — [thesis](https://cdfg.csail.mit.edu/pdf/theses/phd/li-beichen-2025.pdf)
- ADPM (Automatic Differentiable Procedural Modeling, EG/CGF 2022): converts a node-based procedural model into a differentiable computational graph, pushes output edits back to input parameters, detects under-determined edits and proposes multiple parameter configurations, using cheap derivatives to explore the region of optimality. — [Adobe Research](https://research.adobe.com/publication/automatic-differentiable-procedural-modeling); [EG DL](https://diglib.eg.org/items/7fb46ca6-06ee-4d66-afdd-5709641d77b1/full)
- DI-PCG (CVPR 2025): 7.6M-parameter diffusion transformer treats procedural parameters as the denoising target conditioned on an image; about 30 GPU hours to train. Six Infinigen generators used, with 48 (chair), 19 (table), 12 (vase), 14 (basket), 9 (flower), 15 (dandelion) controllable parameters. Abstract reports accurate recovery and generalization to in-the-wild images, with no quantitative numbers on the page I fetched. — [arXiv 2412.15200](https://arxiv.org/abs/2412.15200); [CVPR supplemental](https://openaccess.thecvf.com/content/CVPR2025/supplemental/Zhao_DI-PCG_Diffusion-based_Efficient_CVPR_2025_supplemental.pdf)
- Search-summary note: older inverse procedural modeling used genetic algorithms, and one approach used gradients within a memetic algorithm with differentiable rendering and differentiable procedural generators. — [DI-PCG related-work summary, unverified](https://arxiv.org/pdf/2412.15200)
- NNProc: trains a network to approximate a procedural model and its inverse, agnostic to the generator internals (venue/year not confirmed). — [Carleton PDF](https://people.scs.carleton.ca/~olivervankaick/pubs/NNProc.pdf)
- Sforza 2025 doctoral thesis: recovering parameters of procedural vector patterns by differentiating signed distance fields; includes version control for procedural node graphs. — [thesis repository](https://tesidottorato.depositolegale.it/handle/20.500.14242/210521)
- ShapeCoder (SIGGRAPH 2023): discovers reusable abstraction functions and programs from unstructured primitives using a learned recognition network plus e-graphs with conditional rewrites; reports better dataset compression than alternatives, shorter programs, fewer free parameters. — [arXiv 2305.05661](https://arxiv.org/pdf/2305.05661)
- Infinigen Indoors (CVPR 2024): 79 randomized procedural object generators in Blender, constraint DSL plus solver for layout, parameters randomized by default or overridable, BSD license. I found no inverse fitting of scene-level Infinigen parameters except DI-PCG's per-asset use. — [arXiv 2406.11824](https://arxiv.org/pdf/2406.11824)
- ProcTHOR (NeurIPS 2022): procedural generation of interactive houses from a room spec; scale demonstrated at 10,000 houses. Forward generation, not inverse. — [NeurIPS](https://proceedings.neurips.cc/paper_files/paper/2022/hash/27c546ab1e4f1d7d638e6a8dfbad9a07-Abstract.html)

### Inferences
- A "fit a procedural rock to a scan" gate is analogous to ADPM / DiffMat-style fixed-graph fitting plus a learned or retrieved initialization. The literature supports it for roughly tens of continuous parameters when nodes are differentiable; it is much less supported for a fully automatic "no manual tuning" claim across arbitrary graphs.
- A pretrained inverse network like DI-PCG needs a per-generator training run (about 30 GPU-hours reported) and a large synthetic dataset, so it fits Phase 7 "learned priors", not Phase 3.
- Mixed gradient + gradient-free search (DiffMat v2) is exactly the plan's "hybrid solver" and has published support.
- ADPM's multi-solution reporting for under-determined fits is a useful UX target for `optimize()` (return a set of configurations, not one).
- I found no sourced Houdini or Blender Geometry Nodes parameter-fitting system and no sourced LLM image-to-Geometry-Nodes work; that approach is unvalidated by my search.

### Gaps
- No retrievable evidence of failure statistics (failure rates, local minima frequencies) for MATch/DiffMat in these snippets.
- ShapeAssembly, "Differentiable Procedural Models for Single-view 3D" and Neural Procedural Models were not retrieved.
- ProcTHOR/Infinigen detail on controllability beyond what is above not verified.

## 4. Gradient-free optimizers and relaxations for discrete/topological parameters

### Takeaway
Evidence is general black-box optimization, not procedural-graph specific: Bayesian optimization wins at small evaluation budgets, CMA-ES holds up with larger budgets and parallel evaluations, and BO degrades beyond roughly 15 dimensions. Fuzzy-logic soft booleans make CSG operator choice differentiable, but structure search remains mostly evolutionary.

### Cited Findings
- BBOB study (24 noiseless functions, 10-60 variables): BO beats CMA-ES at limited budgets; BO performance is often reported to suffer above about 15 dimensions; trust regions are the most promising BO improvement. — [arXiv 2303.00890](https://ar5iv.labs.arxiv.org/html/2303.00890)
- Symbolic-regression study: BO substantially worse than CMA-ES in accuracy and time, attributed to high-dimensional difficulty. — [arXiv 2602.20557](https://arxiv.org/pdf/2602.20557)
- CMA-ES compared against GP-BO, TPE and SMAC for DNN hyperparameters in sequential and parallel settings (result not retrieved). — [arXiv 1604.07269](https://arxiv.org/pdf/1604.07269)
- Unified differentiable boolean operator for implicit solids (fuzzy-logic t-norms/t-conorms over soft occupancy) is differentiable with respect to operator type, enabling continuous optimization of primitives and boolean operations. — [Toronto fuzzyBoolean](https://www.dgp.toronto.edu/~hsuehtil/pdf/fuzzyBoolean.pdf)
- DiffCSG assumes fixed discrete structure; suggested use is with iterative program synthesis for joint discrete/continuous solving. A reference notes CSG tree degrees of freedom grow exponentially and discrete search has mostly used evolutionary algorithms. — [arXiv 2409.01421](https://arxiv.org/pdf/2409.01421)
- Neural CSG predictors (BSP-Net, Neural Parts) rely on networks to predict trees, with computational overhead. — [Fluid-SDF listing](https://arxiv.org/pdf/2607.18646)
- ShapeCoder uses recognition network + e-graph rewriting for program search (see section 3). — [arXiv 2305.05661](https://arxiv.org/pdf/2305.05661)

### Inferences
- With a deterministic, cached runtime, cooks per evaluation are cheap, which favors CMA-ES with population batches (parallel evaluations share the CAS) over GP-BO beyond about 15 free parameters; BO is better for small-parameter, expensive-cook cases. Treat this as a hypothesis to benchmark inside the project.
- MCTS/beam search over graph edits: I found no sourced PCG-specific results; the closest sourced analogues are ShapeCoder-style program search and DiffMat v2's mixed-integer search.
- Soft SDF booleans (smooth-min, fuzzy operators) are the sourced route for relaxing blends; straight-through estimators on booleans: no sourced evidence retrieved.

### Gaps
- No PCG-specific benchmark of CMA-ES vs BO vs MCTS found.
- No source for straight-through estimators in procedural graphs.

## 5. Neural surrogates for simulation (smoke, cloth, FLIP, erosion)

### Takeaway
I found little verifiable 2025-2026 evidence supporting the plan's "within 5% of exact at 50x speed" gate. What sources show are generalization limits (unseen geometry/domains, lost small-scale detail) and the hybrid mitigation of feeding network output back into a physical solver.

### Cited Findings
- Oct 2025 arXiv study evaluates neural accuracy/speed for Navier-Stokes around obstacles on unseen layouts (measured speedups not in the snippet). — [arXiv 2510.22976](https://www.arxiv.org/pdf/2510.22976)
- 2020 3D fast neural fluid surrogate reports real-time on 128x64x64 and generalization to unseen domain geometries. — [paperswithcode entry](https://cs.paperswithcode.com/paper/fast-fluid-simulations-in-3d-with-physics)
- Failure modes reported: dependence on massive training data and complex pipelines, poor generalization to new fluid domains; dictionary-based smoke upsampling failed to match high-res fields and missed vortex rings. — [CFDNet](https://arxiv.org/pdf/2005.04485); [dictionary smoke](https://www.cjig.cn/en/article/doi/10.11834/jig.180681/)
- CFDNet hybrid: network prediction is fed back into the physical solver, allowing generalization without relaxing convergence constraints. — [arXiv 2005.04485](https://arxiv.org/pdf/2005.04485)
- Non-learned erosion baseline: GPU hydraulic and thermal erosion runs interactively on heightfields (Jako and Toth 2011). — [EG DL](https://diglib.eg.org/handle/10.2312/EG2011.short.057-060)

### Inferences
- Plan's confidence-gated fallback to the exact solver matches the hybrid precedent; the "solver-in-the-loop" pattern is the safer target than pure surrogate.
- For erosion, the non-learned GPU solver is already interactive, so a surrogate's value is lower there than for smoke/FLIP; the plan's "smoke, erosion" surrogate pair should prioritize smoke.
- The 5%/50x gate is unsupported by what I found; set metric definition and a held-out distribution before committing.

### Gaps
- No 2025-2026 sourced numbers for learned smoke, cloth, FLIP or erosion surrogates (graph-network simulators, neural operators, diffusion rollouts).
- No sourced learned-erosion paper found; only a forum idea post.

## 6. Realistic benchmarks, iteration counts, and recommendations on the Phase 3-6 gates

### Takeaway
Gradient-based fits of fixed procedural graphs are demonstrated; fully automatic "no manual tuning" fits across arbitrary graphs, and photo-to-scene parameter recovery, are research-grade. Iteration/time numbers are mostly missing from retrieved sources, so gates should be framed with explicit budgets measured in our own harness.

### Cited Findings
- Per-sample reverse-mode overhead of roughly 22x primal (Slang.D on RTX 4090). — [NVIDIA Research](https://research.nvidia.com/labs/rtr/publication/bangaru2023slangd)
- DiffCloth reports gradient information gave a substantial speedup in solving most of its applications (no iteration counts in snippet). — [arXiv 2106.05306](https://arxiv.org/pdf/2106.05306)
- DiffPD: 4-19x faster than standard Newton's method. — [arXiv 2101.05917](https://arxiv.org/pdf/2101.05917v2)
- 3DGS training reference: 7k iterations about 160 s, 30k about 872 s on MI300X. — [ROCm benchmarks](https://rocm.docs.amd.com/projects/gsplat/en/latest/reference/benchmark-evaluation.html)
- DI-PCG's argument against sampling-based methods is that they need numerous sample iterations; its inference time was not on the page fetched. — [arXiv 2412.15200](https://arxiv.org/abs/2412.15200)

### Inferences (calibrated recommendations; not from sources unless noted)
Phase 3 (fields, volumes, optimize):
- Realistic: `optimize` with IR autodiff on smooth SDF/heightfield/noise graphs, losses on SDF L2, Chamfer, silhouette; CMA-ES for discrete/rough parameters; `fork/eval/score/merge` with CAS sharing. Fixed-structure fitting is the strongest-supported case (ADPM, DiffMat v2, DiffCSG).
- Aggressive: "fit a procedural rock to a reference scan with no manual tuning". Reword to: with a recipe whose parameter count and ranges are declared, reach a stated Chamfer/SDF-L2 threshold within a fixed cook budget on N held-out scans, with multi-start initialization and a gradient + CMA-ES hybrid; report restarts needed. Add a parameter-identifiability report (ADPM-style multiple solutions).
- Needs change: "64-variant exploration >=10x faster than naive" is plausible only if variants share upstream cones; define the benchmark to reflect shared-prefix structure.
- Mark topology-changing nodes `Differentiability::None` and let CMA-ES/beam handle them; use smooth-min/fuzzy booleans as the relaxation (sourced route).

Phase 5 (simulation, differentiable subset, surrogates):
- Realistic: differentiable particles and PD-style cloth for short horizons with gradient checks against finite differences; always verify gradient-found solutions with a forward cook and fall back to gradient-free search when gradient norms are chaotic or contact-dominated.
- Aggressive: rigid body with impacts (needs time-of-impact handling), cloth self-collision (acknowledged open by DiffCloth authors).
- Needs change: surrogate gate "within 5% at >=50x" lacks supporting evidence; define it per metric on a held-out distribution, prefer solver-in-the-loop hybrid, and treat smoke as the first target. Add the confidence-fallback rate as a gate metric.

Phase 6 (differentiable renderer, photo recovery):
- Realistic: differentiable soft/stochastic rasterizer on CPU for silhouette/depth/normal/shaded-diffuse losses; recover a handful of geometric/material parameters of a synthetic render of the same graph.
- Aggressive: differentiable path tracing with correct visibility gradients (reparameterization or path-replay) in the Rust CPU tracer.
- Needs change: "recover scene parameters from a single rendered photo" should be split into (a) synthetic render from our own renderer (achievable), (b) real photo (research-grade; needs learned prior initialization as in DI-PCG-style networks or material-capture pipelines). Consider embedding Mitsuba 3 (`llvm_ad`) as an out-of-process oracle for validating our gradients rather than building a full differentiable tracer first.

Phase 2 dependency: Phase 3 relies on the Phase 2 IR autodiff; the finite-difference gradient-check gate is well supported by the sourced "gradients can be wrong" literature (DiffTaichi, arXiv 2207.05060), so keep it strict.

### Gaps
- No sourced wall-clock/iteration counts for fitting a procedural rock or terrain to a scan; the project should measure its own baseline.
- No sourced evidence on LLM-agent-driven parameter search outperforming CMA-ES on procedural graphs.
- Coverage of 2025-2026 literature was thin overall because the search tool returned mostly older papers; a follow-up targeted at SIGGRAPH 2025/Asia 2025 and CVPR 2026 is advisable before locking gates.
