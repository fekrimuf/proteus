# Pro-Quality Plan: how Proteus reaches professional, human-grade output

Written 2026-10-09 after a final research round. **Part A is for the project owner (plain English). Part B is the
runbook for whoever builds it (starts with "Starting state").** If you only read one page, read Part A.

---

# Part A: for you (plain English)

## The goal

Make the software produce 3D assets that a professional would accept: right shape, clean structure, easy to edit,
and usable in a game engine or film pipeline. And do it in a way that **gets better with computer time, not with
your time or mine**.

## The idea in one paragraph

Professional work already exists in huge quantities, free and legally reusable. We use it as the answer key. The
computer takes a professional model, writes a recipe that rebuilds it, and checks the result with measurements, not
opinions. Recipes that work are kept. The computer then notices patterns that keep repeating and turns each pattern
into a new building block, so the next model is cheaper to rebuild. Over time a small learned helper takes over the
first-draft writing, so cost per model keeps falling. People are only needed for steering and for spot-checks.

## Is rebuilding hundreds of low-poly models enough to get professional, realistic worlds? No.

You are right. Rebuilding stylized props is **Level 1 of five**. It proves the mechanism (professional work as the answer
key, measured, with a library that learns), and it is the cheapest place to prove it. It does not by itself produce
realism, because realism comes mostly from things a mesh does not contain. The honest ladder:

| Level | What it adds | Why it matters for realism | Answer key (oracle) | Status today |
|---|---|---|---|---|
| 1. Assets | shape, structure, editable recipe | proportion and believable form | professional models (CC0) | plan below, Phase 0 to 3 |
| 2. Appearance | materials, lighting, rendering | usually the single biggest jump from "CG" to "photo" | real material scans and HDRI lighting (CC0), photos | **nothing yet: our exports carry no materials and no UVs** |
| 3. Systems | terrain, ecology, settlements that follow real processes | worlds look real because nature and cities follow rules | real elevation data, land-cover, street statistics | basic erosion nodes exist; nothing fitted to reality |
| 4. Composition | set dressing, detail hierarchy, focal points, camera | what makes a scene feel art-directed instead of random | photographs and reference scenes, plus your direction | none |
| 5. Delivery and scale | export to Blender/Unreal, instancing, LOD | pro pipelines are where the final image is made | pipeline referees (validators, Blender) | partial (glTF/USD geometry only) |

**The target is unchanged: an autonomous system that makes a finished AAA world from a short brief.** The five levels are
the stack that target needs; section "North Star" below says what "finished" means in measurable terms and how autonomy is
reached rung by rung. (An earlier draft of this plan narrowed the goal to "automate most of the work, a human directs the
rest". That was the planner's guess, not the owner's goal, and it has been removed.)

Two out-of-the-box moves keep this tractable:

- **Reality is the answer key at every level.** Not just models: real elevation data for terrain, real material scans
  for surfaces, real photographs for the final look. We match *statistics* (slopes, drainage, tree spacing, texture
  character), not copies, which is how procedural systems get unlimited variety that still looks real.
- **Don't rebuild what already exists in CC0.** Pros dress worlds with scanned rocks, plants and materials. Our job is the
  systems that place, vary and combine them, plus the procedural parts (terrain, buildings, plants, hard-surface props).
  We also don't need to invent a renderer: we export to Blender (and later Unreal) and use it as renderer and referee.

So tomorrow's plan now runs two tracks: **Track A** (the asset flywheel below) and **Track B** (a realism probe, then a
"photoreal valley" demo, section B12). Track B comes first because it answers your question with evidence within a day.
A third track, **Track C** (the world compiler and automatic QA, section B13), starts once the valley works.

## North Star: an autonomous system that makes a finished AAA world

**Style is part of the north star (owner, 2026-10-10):** the system generates in a style *matched from a reference*, or *invents a new style to the brief*
("a medieval village in a sense of sci-fi"), and every user and studio gets styles unique to them, so that no two studios end up with the same
visuals. Five fixed presets are explicitly not the goal. Section B16 is the design.

**"Finished" must be something we can measure, or the project can never tell whether it is getting closer.** So the
plan defines an **AAA Readiness Scorecard** with six families, each checked by a tool, not by opinion:

| Family | Plain-English question | Who checks it |
|---|---|---|
| Visual quality | does a frame look like a top-tier game or photo? | distance to real photos and reference renders, blind human pairs |
| Art coherence | does it have one consistent style and identity, with no obvious repetition? | an "art bible" built from references, style-match scores, a repetition detector |
| World function | can you actually play in it: no stuck spots, clear routes, landmarks you can see? | automatic playtest bots, navigation checks, sightline checks |
| Performance | does it run smoothly on target hardware? | the game engine's own profiler and build tools |
| Content breadth | enough unique, detailed things per square kilometre? | counts and variety measures |
| Delivery | does it open in Unreal or Unity with no errors and package into a build? | engine command-line builds and file validators |

**How autonomy is reached: the unit of work is a "vertical slice"**, the same unit real studios use: one square kilometre
(about a five-minute walk) at final quality. The rungs:

| Rung | Meaning | Human role |
|---|---|---|
| A0 (today) | tools an AI can drive | everything |
| A1 | assets rebuilt and generated on their own | approve libraries |
| A2 | given a brief and an approved art bible, the system builds a full slice | approve the art bible only |
| A3 | several slices joined into one consistent world | approve the brief |
| A4 | one paragraph in, finished world out | optional taps; veto only |

Each rung ends in a gate: the scorecard plus a blind comparison against a hand-made reference. A4 is the stated goal. Nobody has
shown it yet, so it is a research target; the rungs exist so that each step is real and measurable and so that failure
shows up early and cheaply.

**Why our approach can reach scale where others stall.** Meta's WorldGen (2025/26) generates traversable worlds from text but
only about 50 by 50 metres, and quality degrades within 3 to 5 metres of the centre. Studio practice (Horizon Zero Dawn) is
the opposite: art-directed procedural placement, data-driven, deterministic and stable, which scales to a continent. Our
engine is built the studio way (recipes, determinism, caching, checks), so scale is native; what we add is the autonomous
director, the judges and the playtest bots.

**The open problems, each with my routes** (nothing here is a reason to stop; it is where the research is):

1. *Hero-level asset realism (landmarks, creatures, characters).* Routes: (a) layered detail: a recipe for the large forms
   plus procedural micro-detail fitted to scan statistics, plus kitbashing from CC0 scans; (b) generate a base with an image or
   3D generator, then re-express it as a clean, editable recipe with the Track A flywheel; (c) environment first, characters
   later via licensed or CC0 bases with parametric variation.
2. *Taste and art direction.* Routes: (a) an "art bible compiler" that turns a reference set into measured palette, shape
   language, detail density, material set and lighting mood, enforced by two judges that fail differently; (b) have the system
   propose three directions and choose by judge consensus, with a human veto; (c) calibrate against the monthly taps.
3. *Is it fun and readable to play?* Routes: (a) measurable proxies: sightlines, landmark visibility from paths, pacing
   intervals, route clarity; (b) automatic playtest agents that explore, find stuck spots and report coverage; (c) a few human
   taps on "which route feels better".
4. *AAA scale and performance.* Routes: (a) use the engine's own headless builders (World Partition, HLOD, navigation data,
   PCG builder) as referees; (b) our own LOD and instancing generators; (c) budgets as hard constraints in the planner.
5. *Animation, audio, story.* Routes: define "world" as environment, props, lighting and gameplay spaces first; reuse CC0
   animation libraries; treat audio and narrative as later modules behind the same brief.
6. *Coherence across a whole world.* Routes: a plan that goes region, district, block under one art bible; a repetition
   detector on image embeddings; automatic seam and transition tests.
7. *Unknowns to verify, not assume:* engine licence terms, the exact Unreal headless/Python interfaces, and the licences of
   any reference scenes used for benchmarking.

## What the last research round changed (the upgrades)

1. **A proven shortcut exists.** Published systems (MeshCoder, CAD-Recode, ShapeCoder) turn shapes into editable code.
   Their trick: generate millions of random-but-valid recipes with the software itself, render each, and train a small
   model to go from shape back to recipe. We can generate those pairs already. This becomes Phase 3, the thing that makes
   the cost per model fall toward zero.
2. **Rebuild whole models in parts.** Splitting a model into its natural pieces and rebuilding each piece is what made
   the published results work. We do the same.
3. **Look up similar solved models first.** Each new model starts with the recipes of its closest already-solved
   neighbours as examples. Every solved model makes the next one easier.
4. **Judge "editable", not just "similar".** A copy that can't be edited is worth nothing. We also score how few
   controls the recipe needs and whether changing them still gives believable results. That is where this approach beats
   scanning or AI-generated meshes.
5. **Free referees for the technical half.** The Khronos glTF validator and headless Blender can check files
   automatically. A free image model (DINOv2) can measure "looks like the professional pile".
6. **Aesthetic score predictors are demoted.** I could not confirm their license, and found no evidence they work on
   renders of 3D models. They are a weak tie-breaker only until your spot-checks say otherwise.
7. **Legal hygiene is built in.** Every asset gets a license record. The first trial uses only assets that are confirmed
   public domain (CC0).

## What happens tomorrow

> **Status: done on 2026-10-10.** Phase 0, the Realism Probe and the first 50 assets ran; Checkpoint 1 is green. Results: section B14 and `corpus/results/checkpoint1/report.html`.

**What you do:** open a session and paste this one line:

> Read docs/PRO_QUALITY_PLAN.md and execute Phase 0 and the Realism Probe (B12), then run the first 50 assets and report at Checkpoint 1.

**What I do, in this order:**
1. Build the Phase 0 tools (download and license records, a reader for the common 3D file format, the measuring ruler,
   the test runner).
2. **Realism Probe (a few hours):** take one of our plain rocks and one public-domain scanned rock, give both a real
   public-domain material and real lighting, render both in Blender, and show you the four images side by side. This tells
   us how much of the "looks real" gap is appearance (materials and light) versus shape. It decides how much to
   invest in Level 2 first.
3. Download the first 50 public-domain models, try to rebuild them, and write you a plain-English report. Nothing needs
   your credentials or money (installing Blender and downloading public-domain files only).

**What you get:** a one-page report with traffic lights (below), pictures of originals next to our rebuilds, the probe
images, and a recommendation on whether to continue.

## The three checkpoints (traffic lights)

| Checkpoint | When | Green means | If red |
|---|---|---|---|
| 1. Does it work at all? | after 50 models | at least 3 of 10 rebuilt to within 10% of the original | switch effort to the "looks like the pro pile" judge and to part-by-part fitting, then retry on 50 new models |
| 2. Does it compound? | after 200 models | at least 20 new reusable building blocks found, and the effort per model in the last 50 is at least 40% below the first 50 | the library isn't learning: change how patterns are mined before spending more |
| 3. Does it look right to a human? | after checkpoint 2 | you are shown pairs (original vs our rebuild, same camera) and pick the original in no more than 7 of 10 pairs | adjust the finishing steps and the judges, retest |

No checkpoint commits you to anything. Each one ends with a recommendation you can decline.

The realistic-worlds track has its own gates (W0 to W4 in section B12.5): the Realism Probe, a "photoreal valley" built from
real elevation data, a distance-to-real-photos measure, a second biome, and a small town. Same rule: each ends in a
recommendation, never a commitment.

## Your fixed commitments (they do not grow with the size of the library)

- About **30 quick "which looks better?" taps a month**. They check that the automatic judges still agree with human
  taste. With 30 taps we can detect a drop in agreement of roughly 15 to 20 points.
- **Direction**: tell me the kind of assets you care about (props, buildings, rocks, plants...). The system executes;
  you steer.
- **Three decisions, later, not tomorrow:** a monthly cap on cloud computer rental for Phase 3, whether to use
  CC-BY (attribution) assets in addition to public-domain ones, and what to do about the Infinigen asset license.

## Honest unknowns

- I don't know yet what fraction of professional models the system can rebuild. Checkpoint 1 measures it.
- The published results are strongest on man-made, structured objects (furniture, machine parts, buildings). Organic
  shapes (animals, people) are weaker; we start with props, buildings, rocks and vegetation.
- The "looks right to a human" question is only settled by checkpoint 3 and your taps, not by any score.

---

# Part B: runbook

## B1. Starting state (verified 2026-10-09)

- Repo: `D:\Fekri\Houdini-clone`, GitHub `fekrimuf/proteus` (private). `gh` CLI installed and logged in (scopes repo,
  workflow). CI (Windows/Linux/macOS + cross-OS hash compare) green. `docs/STATUS.md` has commands and gotchas.
- Shell setup: prepend `D:\Fekri\toolchain\cargo\bin;D:\Fekri\toolchain\mingw\mingw64\bin` to PATH. No Python, no Node.
  Everything installs on D:. Write Rust files with Write/Edit tools (the Bash tool collapses double backslashes).
- Exists and reusable: recipe language (`hcs`), 75 node types, gradient fitting (`fit`), CMA-ES (`pt-opt`),
  silhouette/image/chamfer objectives, variation engine (`vary`), content-addressed cook cache, gym (41 tasks),
  `pt critique-bench`, `render_compare`, `sparse_remesh`/`sparse_boolean`, OBJ import (`pt_core::io::load_obj`),
  export to OBJ/glTF/USD, software rasterizer and CPU path tracer, MCP server.
- **Built in Phase 0 (2026-10-10):** glTF/GLB importer and writer, reference-matching metrics (`pt match`), corpus download + licence ledger, part splitting, the batch runner with budgets and resume, recipe refinement (`pt refine`), `pt describe`, the checkpoint report. **Still missing:** motif mining, FD_DINOv2, Blender bridge for our exports, materials/UVs in exporters.
- **Missing for realism (Track B, section B12):** the exporters (`io.rs`) write geometry only: no materials, no UVs, no
  texture coordinates. There is no material system, no lighting description, no Blender bridge, no real-elevation import,
  and nothing that fits terrain parameters to real data. Existing terrain nodes: `hf_erode_thermal`, `hf_erode_hydraulic`,
  `hf_slope`, `hf_terrace`, noise, `scatter_poisson`, `drop_to_surface`.
- Never run or study Houdini or its files (EULA). Original names only.

## B2. Evidence base (what supports the approach, with caveats)

| Source | What it shows | Caveat |
|---|---|---|
| [MeshCoder, 2025](https://arxiv.org/abs/2508.14879) | shape to editable Blender code; trained on 10M synthetic part/code pairs; average IoU 86.75% vs 67.62% (PLAD) and 45.03% (Shape2Prog) | authors' own numbers; man-made objects; organic not covered |
| [CAD-Recode, ICCV 2025](https://arxiv.org/abs/2412.14042) | point cloud to CAD code with a small LLM trained on 1M synthetic programs; about 10x lower Chamfer than prior work; DeepCAD IoU 92.0 | CAD-style parts only; third-party leaderboard numbers |
| [ShapeCoder, 2023](https://arxiv.org/abs/2305.05661) | learns reusable abstractions (library learning) plus a recognition network; fewer free parameters, better compression | primitives-based shapes |
| [LL3M, 2025](https://arxiv.org/abs/2508.08228) | LLM agents writing Blender code with render-and-critique loops; retrieval of examples | original model retired; naive prompting gives failing or low-quality code |
| In-house critique experiment (`gym/critique_experiment.json`) | blind vision judge agrees with spec truth (Spearman 0.84); the generic rubric does not (0.10) | judge was Claude, n=10 tasks |

## B3. Metrics (define once, never change mid-trial)

All geometry is normalised first: centre, scale to unit bounding-box diagonal, fixed up-axis, same unit system.

| Metric | Definition | Why |
|---|---|---|
| F-score | 20k surface samples each side, threshold 0.02 of the diagonal; harmonic mean of precision/recall | main shape fidelity |
| Chamfer | mean nearest-neighbour distance, both directions, in diagonal units | continuous companion to F-score |
| Silhouette IoU | 6 axis views plus 4 diagonal views at 256 px, mean IoU | catches proportions and holes |
| Normal consistency | mean dot of nearest-surface normals | catches inverted or noisy surfaces |
| Editability | free numeric parameters in the recipe; plausibility of 8 random parameter variations (FD_DINOv2 vs the category, Phase 2) | the product advantage over scans |
| Pipeline validity | `validate` clean, watertight if the source is, glTF-Validator clean, polygon count within 2x of source | technical half of "professional" |
| Cost | LLM turns, CPU seconds, tokens per asset | the scalability KPI |

Tiers per asset: **bronze** F>=0.80 and IoU>=0.80; **silver** F>=0.90 and IoU>=0.90 and valid and 3 to 40 free
parameters; **gold** F>=0.95 and IoU>=0.95. "Solved" means silver or better. Budget per asset in Phase 0 and 1: at
most 8 LLM turns and 10 CPU minutes. Hold out 20% of every batch; the hold-out is never used for pattern mining and
is reported separately so we do not fool ourselves.

### B3a. Definitions as built (Phase 0, 2026-10-10; frozen for the trial)

The code in `crates/pt-opt/src/matching.rs` is the definition; these notes fix the places where B3 left room.

* **Normalisation:** each shape is centred on its own bounding box and scaled to a unit diagonal (`Align::Bbox`, the default of
  `pt match`). This hides pure scale errors, so the bench also reports `size_ratio` and **caps any rebuild whose overall size is
  off by more than 10% at bronze**. `Align::Reference` (reference's centre and scale) is also reported as `metrics_strict`.
* **Distances are exact:** F-score, Chamfer and normal consistency sample 20 000 surface points on one shape and measure the
  exact point-to-triangle distance to the other (BVH), so only the choice of sample points is random, not the distances.
  Identical inputs score exactly 1.0 (tests).
* **Silhouette IoU:** 10 views (6 axis, 4 diagonal), 256 px, one shared camera framed on the reference.
* **"Valid":** no validation errors, watertight if the reference is (welded at 1e-4 of the diagonal), and **at most 2x the
  reference's triangle count** (read as an upper bound; a coarser rebuild is fine).
* **Free parameters ("knobs"), counted on the recipe source:** every non-zero number except structural integers (`rows`, `cols`,
  `caps`, `seed`, `octaves`, `divisions`, `levels`, `iterations`), `range(...)` arguments, integer factors or divisors up to 10
  next to `*` or `/` (`W / 2`), and anything in strings or comments. A variable used ten times counts once. This was refined once,
  after the first recipes were written, to stop `W / 2` counting as a knob; it is frozen now.
* **Tiers and budgets** are as in B3. "Solved" = silver or gold. Hold-out = the 10 assets whose id hashes lowest (BLAKE3).
* **Hold-out caveat:** nothing is mined or learned yet, so the hold-out guards nothing in Phase 0 by construction; and its
  results were visible while the solver was being fixed (tessellation, parameter cap, polish, size gate), so it is **not** a clean
  test of those fixes. It becomes a real test at Phase 1 (mining) and Phase 3 (proposer).

## B4. Phase 0 (day 1): tools, with acceptance tests

1. **License ledger and downloader.** `corpus/LEDGER.csv` columns: id, source, url, license, license_url, sha256,
   fetched_at, attribution. Raw files go in `D:\Fekri\corpus\<source>\` (outside the repo; add to `.gitignore`). Each source
   gets its own new empty directory. Use `curl` or PowerShell, never run scripts shipped in archives, list archive
   contents before extracting, never open `.blend` files. Sources for the first 50:
   - Kenney (30 stylized props and buildings): official support page says all assets are CC0, attribution not required.
     Formats are listed per pack (GLB/OBJ/FBX); confirm per pack.
   - Poly Haven (10 props): assets CC0 per their public API repo; using the live API for a product requires a
     "Powered by Poly Haven" credit. API at `https://api.polyhaven.com` (docs linked from the Public-API repo).
   - Quaternius (10): **dropped 2026-10-10.** quaternius.com/license.html (updated 2026-08-28) now publishes the Quaternius Asset
     License v1.0, which is not CC0 (no resale or redistribution as standalone assets or packs), while the home and pack pages
     still say CC0, and downloads sit behind Google Drive. The 10 slots went to Kenney. See `corpus/SOURCES.md`.
   - Not in the trial: Objaverse/Objaverse-XL (per-object licenses; v1 is roughly 88% CC-BY, about 10% non-commercial,
     under 0.5% CC0 by my arithmetic from the published counts; Polycam data is academic only); ABC (dataset MIT, but each
     CAD model is owned by its creator under Onshape terms, so research-only); Infinigen outputs (code is BSD-3, output
     license not stated, treat as internal until clarified).
   - *Accept:* ledger has a row for every file; a script re-verifies hashes; zero non-CC0 rows in the trial set.
2. **glTF/GLB importer** in `pt-core::io` (positions, indices, node transforms, per-mesh grouping as parts; ignore
   materials at first). *Accept:* round-trip test (export our geometry to GLB, import, hashes match); a handmade tiny GLB
   in the test suite; Kenney GLBs import without panics.
3. **`pt match <reference> <recipe-or-geometry>`** implementing B3 plus `pt match --report` that writes JSON and a side
   by side PNG (reference left, ours right, same camera). *Accept:* identical input scores 1.0 on every metric;
   scores decrease monotonically as a recipe is perturbed (reuse `gym::perturb`); unit tests for every metric.
4. **Normalisation and part splitting** (connected components, then glTF node/mesh boundaries). *Accept:* each asset
   yields a list of parts with bounds; a unit test on a two-box model.
5. **Solver stack v0, in this order per part:**
   a) primitive baseline: greedy box/cylinder/sphere fitting with CMA-ES refinement (no LLM; handles the easy ones);
   b) LLM-written recipe from multi-view renders of the part, with the 3 nearest solved recipes as examples (retrieval
      by shape descriptor first; DINOv2 embedding once Phase 2 lands);
   c) parameter refinement with CMA-ES against the metrics; stop at the budget.
   *Accept:* on 5 hand-picked easy assets the stack reaches silver on at least 3.
6. **`pt match-bench <manifest>`** runs a batch with the budget, writes `corpus/results/<run>.json`, supports resume (skip
   finished ids by content hash), and logs a failure taxonomy (import error, wrong topology, proportion error, missing
   detail, search timeout). *Accept:* kill and restart resumes without redoing finished assets.
7. **Checkpoint report generator** producing the plain-English page from Part A (traffic lights, gallery of 12 random
   original-vs-ours pairs, cost per asset, failure reasons, recommendation).

## B5. Phase 1: 50 to 200 assets, library learning

- Run assets in difficulty order (single primitives, then compound props, then buildings and plants).
- **Motif mining:** after every 25 solved assets, find sub-graphs that occur in at least 3 recipes with the same shape of
  parameters, abstract them into an asset (`asset(...)`), and promote only if the mean parameter count of the recipes that
  use it drops and no golden hash changes. Log each promotion with before/after parameter counts. (ShapeCoder is the
  reference design: abstractions must remove degrees of freedom, not just rename code.)
- Keep `recipes/golden.json` and the gym green. Add solved recipes to `recipes/proposed/` via the existing
  `propose_recipe` pipeline; the owner accepts in batches.
- KPIs after every batch: solved rate (mining set and hold-out separately), promotions, parameters per recipe, cost per
  asset (rolling 25), failures by category.

## B6. Phase 2: judges and spot-checks

- **DINOv2 features in Rust** with `candle` (pure Rust, CPU; the repo has `dinov2`, `clip`, `siglip` examples). Weights
  reportedly Apache-2.0: confirm the license file in Meta's repo for the exact checkpoint, record it in the ledger. Report
  FD_DINOv2 with the backbone named (ViT-S/14 for speed). Uses: (1) distance of our renders to the category's pro
  cloud, (2) nearest-neighbour retrieval for solver step (b), (3) variation plausibility.
- **Pairwise judge** (Claude or any VLM) comparing our rebuild with the original under the same camera; log accuracy against
  the tap data. Pairwise beats absolute scoring in the in-house experiment.
- **Taste taps:** add a `/taste` page to the existing web UI. Shows two renders, one tap, stores to
  `corpus/taps.json`; fits a Bradley-Terry ranking; monthly audit is 30 pairs chosen where the judges are least sure.
  Pass line: judge-human agreement >= 80%.
- **Referees:** `gltf_validator` on every export (Khronos; confirm its license file); headless Blender as a separate
  process (`blender -b --factory-startup -P check.py -- in.glb`) for UV unwrap, bake, render and import tests. Only simple
  files cross the boundary (glTF in, PNG/JSON out). Forum guidance says this is arm's-length use of a GPL program; have a
  lawyer confirm before any commercial distribution that bundles Blender.
- **Goodhart guard:** use at least two judges that fail differently, keep the hold-out, and let the taps overrule any score.

## B7. Phase 3: make the cost fall toward zero

1. **Synthetic data engine.** Sample recipes from the library with randomised parameters (the variation engine already
   does this), render and sample points, store (shape, recipe) pairs. Target 1M pairs, parts first (MeshCoder's recipe).
2. **Proposal model.** Fine-tune a small open code model (about 1B parameters) to map a point-cloud/image encoding to a
   recipe. Needs a rented GPU; cap set by the owner. Keep CMA-ES refinement as the last step (CAD-Recode style).
3. **Closed loop:** proposer drafts, search refines, metrics verify, solved pairs feed the next training round.
4. **Infinigen as a second oracle (Linux box only):** BSD-3 code, outdoor/indoor generators with known seeds. Use for
   category coverage and as a test that our pipeline works on a procedural system we did not write. Settle the output
   license question before using any of it in anything shipped.
- *Gate:* proposal model reaches silver on at least 40% of hold-out parts with zero LLM calls, otherwise keep the
  LLM-in-the-loop path and re-examine the data.

## B8. Gates

| Gate | Pass | Else |
|---|---|---|
| G1 (50 assets) | silver on >= 30% of non-hold-out and >= 20% of hold-out | shift weight to DINOv2 judge and part-wise fitting, rerun on 50 new assets |
| *G1 measured 2026-10-10* | *stack 19/40 (48%) and 5/10 (50%); after a visual check 14/40 and 4/10* | *passed; see B14* |
| G2 (200 assets) | >= 20 promotions; cost per asset in the last 50 <= 60% of the first 50; hold-out silver within 10 points of mining set | change mining criteria (smaller motifs, parameter-aware matching) before scaling |
| G3 (human) | owner identifies the original in <= 70% of silver+ pairs | add finishing passes (bevel, wear, variation) and rejudge |
| G4 (Phase 3) | see B7 | keep LLM path |

## B9. Risks and routes

| Risk | Route |
|---|---|
| Photoscan-style assets (Poly Haven) are noisy and hard to express as clean recipes | start with Kenney-style stylized assets; use scans only as shape targets with a looser tier |
| Rebuilds match geometry but are not editable | the editability metric and parameter budget are part of "solved" |
| Judges get gamed | two judges, hold-out, human taps as the final authority |
| License mistakes | ledger row required before any file is used; CC0-only trial; attribution file for CC-BY later |
| Cost grows with library size | cost per asset is a gate (G2); retrieval and proposer reduce LLM calls |
| Mining promotes junk abstractions | promote only when parameters drop and goldens are unchanged |
| Windows-only results | CI keeps the cross-OS hash check; vendored math patches stay (see `vendor/README.md`) |
| Downloaded files are untrusted | separate directories, no execution, our own importers only |

## B10. Daily loop and resuming

1. `git pull`, read `docs/STATUS.md` and this file, run `cargo test --workspace --release` and `pt recipes`.
2. `pt match-bench corpus/manifest.json --resume` (after Phase 0).
3. Update the checkpoint report, commit results JSON (not raw assets), push, check CI.
4. Update `docs/STATUS.md` (counts, KPIs) and this plan's gate table with measured numbers.

## B12. Track B: from assets to realistic worlds

**Principle:** at every level the answer key is real-world data and the metric is statistical or perceptual, never "looks
right to me". Match distributions, not copies.

### B12.1 Realism Probe (day 1, a few hours; decides the order of Level 2 work)

1. Install Blender on D: (separate program; we only call it from the command line; see B6 for the licensing note).
2. Fetch from Poly Haven (CC0): one scanned rock model, one PBR rock material set, one HDRI. Record all three in the ledger.
3. Render 4 images with the same camera in Blender Cycles: (a) our current procedural rock, flat grey; (b) our rock with the
   real PBR material (box/triplanar mapping) under the HDRI; (c) the scanned rock with its own material; (d) the scanned
   rock flat grey.
4. Show the four side by side to the owner. **Question answered:** how much realism comes from appearance (b vs a) versus
   shape (c vs b)? If (b) already looks mostly real, Level 2 (materials and lighting) is the first investment; if not, shape and
   detail come first. Record the answer and the images in `corpus/results/probe/`.

### B12.2 Level 2: appearance (build order, cheapest first)

1. **Material binding, no UVs needed:** scenes declare a material *name* per surface group (e.g. `rock_granite_02`); a Blender
   script binds the matching CC0 PBR set with box (triplanar) projection. Add the name as a primitive attribute and carry it
   through glTF `extras` and USD. This sidesteps UV unwrapping until we need it.
2. **Lighting description:** HDRI name, sun angle, camera height and lens in the scene file; the Blender script builds the
   rig. Same description renders the same image (determinism goal; Cycles seeds fixed).
3. **PBR export:** glTF `pbrMetallicRoughness` and USD preview surface for engine round trips; UVs only where needed
   (hard-surface props), via Blender's unwrap referee at first.
4. **Procedural materials fitted to scans:** precedent is [MATch (SIGGRAPH Asia 2020)](https://match.csail.mit.edu/):
   differentiable procedural material graphs optimised to reproduce photographs; output is editable and resolution
   independent. This is a research-scale build (we have no material graph system today); do it after steps 1 to 3 prove value.
5. **Wear and imperfection:** curvature- and exposure-driven dirt, edge wear, per-instance colour and scale variation. Judged
   by the pro-fingerprint metric and the taps.

### B12.3 Level 3: systems (terrain first, then ecology, then settlements)

- **Terrain.** Oracle: real elevation tiles. USGS 3DEP is described by USGS as public domain (credit requested; catalog fields
  are inconsistent, so confirm the wording on The National Map download page and record it). Copernicus GLO-30 is free but
  needs the exact attribution and disclaimer, and a modified-data notice; WorldDEM-10 is excluded; some countries are
  missing. Metric: distribution distances (Wasserstein, as used for landscape misfit in
  [Morris et al., GRL 2023](https://doi.org/10.1029/2023gl103880)) between our terrain and the real tile for slope, curvature,
  hypsometric curve, drainage density and ridge/valley spacing. Method: CMA-ES (we have it) over erosion/noise parameters.
  **Open research:** I found no published method that fits erosion parameters to DEM summary statistics; related work is
  Bayesian inversion of landscape-evolution models and the gradient-based INVTERRA project. Treat as our research bet and
  keep the objective simple first (3 statistics).
- **Ecology.** Oracle: real forest and land-cover patterns (point-pattern statistics such as pair-correlation and
  nearest-neighbour distance, size distributions, species mix by slope/altitude/moisture). Data source to verify: ESA
  WorldCover (believed CC BY 4.0, **not verified this round**), national forest inventories. Method: rule-based scattering
  (we have `scatter_poisson`, group-by-expression, `drop_to_surface`) with parameters fitted to the statistics.
- **Settlements.** Oracle: street-network statistics (block sizes, intersection angles, connectivity). OpenStreetMap is the
  obvious source but is ODbL; whether statistics or models trained on it trigger share-alike is unsettled in community
  discussion. **Keep OSM out until counsel advises**; prefer public-domain city GIS open data where available.

### B12.4 Level 4 and 5: composition and delivery

- **Global realism judge.** FD_DINOv2 distance between our renders (at human eye height, same lens) and photographs of the
  same biome or place type, plus pairwise VLM judgments and the monthly taps. Photo sources must be permissively licensed
  (CC0, public domain or properly attributed); keep a ledger row per photo.
- **Detail hierarchy and dressing rules:** large forms, medium forms, small forms, then noise; focal points; clutter near
  edges and paths. Encoded as scatter rules and scored by the fingerprint metric.
- **Delivery:** USD scene with instancing, material names and lighting description; Blender script renders it; Unreal import
  test later. LOD and streaming only after the above works.

### B12.5 Milestones and gates (world track)

| Gate | What is shown | Pass | Else |
|---|---|---|---|
| W0 Realism Probe | 4-image comparison (B12.1) | answer recorded; owner decides order | none, it is a measurement |
| W1 Photoreal Valley | one 3 km tile of real elevation, terrain statistics fitted, CC0 scanned rocks and plants scattered by rules, CC0 PBR materials, HDRI, rendered in Blender | statistics within the agreed tolerance on 3 measures; owner picks it over a hand-set-up baseline of the same scene in at least 5 of 10 blind pairs | iterate on materials/light before more systems |
| W2 Photo-distance | FD_DINOv2 between 100 renders and 100 photos of that biome | distance falls monotonically across 3 improvement rounds and beats the flat-shaded baseline | re-examine camera, scale, atmosphere |
| W3 Second biome | repeat W1 for a different biome (desert, farmland, alpine) with no new human-written rules beyond parameters | passes W1 with parameter changes only | the systems are over-fitted; generalise the rules |
| W4 Settlement | a small town: street network fitted to public-domain statistics, buildings from the Level 1 generators | owner's blind pair test as W1 | back to Level 1 building generators |

### B12.6 Honest sizing

Level 1 is weeks of focused work to a first verdict. Levels 2 and 3 are a multi-month program; I cannot give a responsible
date, which is why every step has a gate and an early probe. The first realistic-looking demo (W1) mostly composes things
that already exist (terrain erosion nodes, scatter, `drop_to_surface`, CC0 assets, Blender), so it should arrive well before
the fitted-material and settlement work.

## B13. Track C: the autonomous world compiler and the AAA Readiness Scorecard

### B13.1 Scorecard (draft bars; calibrate every number against reference scenes before first use)

| Family | Measure | Draft bar |
|---|---|---|
| Visual | FD_DINOv2 of renders vs a reference set of the same biome/style; blind human pairs vs a hand-made reference | distance no worse than the distance between two different reference scenes of the same type; picked >= 40% of pairs (50% = indistinguishable) |
| Visual | material coverage, GI/shadows/atmosphere enabled, texel density band | 100% of visible surfaces have a material; density within the reference band |
| Art coherence | style-match to the art bible (palette distance in Lab, shape-language and detail-density statistics, embedding distance); repetition detector (near-identical crops within 50 m, one asset repeated in view) | style score within tolerance of the reference scenes' own scores; repetition below a calibrated limit |
| World function | navigation coverage of intended walkable area; stuck points found by explorer agents; landmark visibility from main paths; POI spacing/pacing; collision (raycast fall-through test) | >= 99% coverage; 0 stuck points over a fixed bot-hour budget; every landmark visible from a path within 300 m; pacing within target |
| Performance | frame rate on a defined target GPU and resolution; triangles, draw calls, VRAM; HLOD present; streaming hitches | engine capture meets the budget (e.g. 60 fps at 1440p on the chosen spec) |
| Content breadth | unique assets per km2; max repeats of one asset in view; LOD chain on every mesh above 2k triangles | calibrated from reference scenes; 100% LOD coverage |
| Delivery | engine opens the project with zero errors; HLOD/nav/PCG builds succeed headless; packaged build survives a 10-minute bot path; glTF/USD validators clean | all pass |

Publish the scorecard result on every build as one page (traffic lights plus the evidence images), same style as the
checkpoint report.

### B13.2 Architecture: a studio of agents with referees

```
brief -> ART BIBLE compiler -> WORLD PLANNER (region > district > block; road/river graphs; landmarks; POIs;
        blockout + navmesh) -> BUILDERS (terrain fitted to real stats, ecology, settlements, props, materials)
        -> DRESSING -> LIGHTING -> TECH ART (LOD, HLOD, nav, streaming budgets) -> QA LOOP
        (playtest bots, validators, engine profiler, judges) -> REPAIR (agent edits the recipe graph) -> DELIVERY
```

The recipe graph is the shared medium. Existing strengths to use, not rebuild: deterministic cooks, content-addressed cache with
early cutoff, provenance and `explain`, `check`/`validate`, `checkpoint`/`restore`, `diff`/`merge`, the `vary` engine, the
gym. Pattern borrowed from WorldGen: plan the functional skeleton first (blockout + navigation mesh), then realise it, so
the result is traversable by construction. Pattern borrowed from Horizon Zero Dawn: artist-directable, data-driven,
deterministic, locally stable rules, so that moving a mountain or changing a biome re-dresses the world automatically.

### B13.3 Track C tasks (ordered; none needed for tomorrow)

- **C0 Scorecard harness v0:** compute what we can today (material coverage, instance counts and repeats, repetition crops,
  grid-nav coverage on our own geometry, budgets). Unit-test each metric. Report page generator.
- **C1 Art bible compiler v0:** a folder of reference images (CC0/permissive only, ledger row each) to `artbible.json`
  (palette, detail frequency, shape language, material set, lighting mood) plus a judge that scores renders against it.
- **C2 Planner prototype:** brief to blockout (region graph, roads, landmarks) with a navmesh; the plan becomes recipe-graph parameters;
  constraints (budgets, slope limits, spacing) are checked by solvers, not by the language model.
- **C3 Playtest bot v0:** explorer agent on the navmesh (Go-Explore style): coverage, stuck detection, landmark visibility;
  results feed the scorecard.
- **C4 Engine bridge:** export USD/glTF with material names and the lighting description; headless Unreal scripts using the
  World Partition builder commandlets (HLOD, navigation data, PCG builder) as referees. **Verify first:** Unreal licence terms,
  exact command lines and the Python/automation interface for the installed engine version.
- **C5 Closed repair loop:** scorecard failures become tasks for the builder agent (edit, re-cook, re-score), with the gym
  scoring how well it repairs.
- **C6 Vertical slice trial (rung A2 gate):** one square kilometre from a brief and an approved art bible; scorecard plus a
  blind comparison against a hand-made reference slice.

### B13.4 Evidence for Track C (and its limits)

| Source | What it shows | Caveat |
|---|---|---|
| [WorldGen, Meta (arXiv 2511.16825)](https://arxiv.org/abs/2511.16825) | LLM plans a blockout and navmesh, a generative model realises it; traversable by construction | scenes about 50 by 50 m; quality degrades 3 to 5 m from the centre (Meta's own statement); secondary-source numbers unverified |
| [GPU-based procedural placement in Horizon Zero Dawn (GDC 2017)](https://www.guerrilla-games.com/read/gpu-based-procedural-placement-in-horizon-zero-dawn) | AAA open worlds are art-directed, data-driven, deterministic procedural placement | artists painted the layout; autonomy of the layout is exactly what we add |
| [Go-Explore for reachability testing (arXiv 2209.00570)](https://arxiv.org/abs/2209.00570) | automated agents cover navigation meshes and find reachability bugs; authors claim a 1.5 km square world in about 10 hours on one machine | research prototype; "fun" is not measured |
| [World Partition builder commandlets (Epic docs)](https://dev.epicgames.com/documentation/unreal-engine/world-partition-builder-commandlet-reference) | headless engine builds for HLOD, navigation data and PCG generation | exact flags vary by engine version; Python interface unverified |

## B11. Decisions already made (do not re-litigate without new evidence)

- **The owner's target is an autonomous system that makes a finished AAA world.** It is staged by the autonomy rungs
  (A0 to A4) and the scorecard; do not narrow the goal in any document without the owner's explicit say.

- Use professional public-domain work as the answer key rather than human ratings or hand-written specs, at every level (assets, materials, terrain, ecology, photos); match statistics, not copies.
- Level 1 (assets) is only the foundation; realism comes from Levels 2 to 5 (B12) and world function from Track C (B13). Do not claim realistic worlds from asset rebuilding alone.
- Hand-curated references and per-item human rating were rejected: they scale with people's time.
- Aesthetic predictors are weak tie-breakers only until validated against taps.
- Start with structured, man-made and plant/rock assets; characters and animals later.
- Export to Blender (and later Unreal) for the final render instead of building a production renderer; call Blender only as a separate program.
- Houdini is never run, opened or studied.

## Sources

- [MeshCoder](https://arxiv.org/abs/2508.14879), [CAD-Recode](https://arxiv.org/abs/2412.14042), [ShapeCoder](https://arxiv.org/abs/2305.05661), [LL3M](https://arxiv.org/abs/2508.08228)
- [Kenney support page (license statement)](https://kenney.nl/support), [Poly Haven Public API](https://github.com/Poly-Haven/Public-API)
- [Objaverse-XL on PyPI](https://pypi.org/project/objaverse-xl/0.1.3), [Infinigen](https://github.com/princeton-vl/infinigen), [candle examples](https://github.com/huggingface/candle/tree/main/candle-examples/examples)
- [Khronos glTF-Validator](https://github.khronos.org/glTF-Validator)
- [Infinigen (CVPR 2023)](https://arxiv.org/abs/2306.09310) (all assets procedural; read sections 3+ before copying techniques), [MATch (SIGGRAPH Asia 2020)](https://match.csail.mit.edu/)
- [USGS 3DEP data](https://www.usgs.gov/3d-elevation-program) and [Copernicus DEM licence](https://documentation.dataspace.copernicus.eu/APIs/SentinelHub/Data/DEM/resources/license/License-COPDEM-30.pdf)

## B14. Results log (measured; update after every run)

### 2026-10-10: Phase 0, Realism Probe, Checkpoint 1

Report page: `corpus/results/checkpoint1/report.html` (self-contained). Raw numbers: `corpus/results/baseline.json`,
`corpus/results/llm.json`, `corpus/results/audit.json`; tuned recipes: `corpus/recipes/tuned/{baseline,llm}/`; the
language-model-written sources: `corpus/recipes/llm_src/`. Solver version `primitive-v0a.5`, recipes tuned by `pt refine`'s search.

| Measure | Result |
|---|---|
| Trial set | 50 CC0 assets: 40 Kenney (6 packs), 10 Poly Haven; 11 single shapes, 28 multi-part props, 11 building pieces; hold-out 10 (BLAKE3 of the id) |
| Path A, no language model (primitive search + polish) | 13/50 silver or better (26%); development 10/40, hold-out 3/10; about 10 s per asset |
| Path B, language-model recipe + tuned numbers | tried on 41; 12 solved; 46 turns (1.1 per asset); about 4 s of compute each; 7 to 23 knobs (median 14) vs 6 to 40 (median 30) for path A winners |
| Stack (best of A and B per asset) | **24/50 solved (48%, 95% range 35 to 61)**: gold 4, silver 20; development 19/40, hold-out 5/10 |
| Same, after a visual check of the 24 silver pictures | 18/50 (36%); development 14/40, hold-out 4/10. Six pass the numbers but miss a feature (`eye_audit.json`) |
| Gate G1 | pass on both readings (needs 30% of development and 20% of hold-out) |
| Failure reasons among the 26 misses | extra surface 14, uncovered surface 11, outline or size 9, open mesh 1 |
| Probe (B12.1) | appearance (material + light) is the biggest cheap lift; shape and multi-scale detail are the second bottleneck; see `corpus/results/probe/README.md` |

What the misses are (each with its route; also in the report): thin sticks (outline IoU 0.80 to 0.90; the refiner now scores all ten
views), slanted and bevelled stone (needs wedge, arch, lathe pieces), scans with interior faces (add an outside-only reading),
busy or organic objects (part-by-part, then the Phase 3 proposer), one open mesh (vase).

Honest limits of this checkpoint: each asset was fitted to its own answer key (in-sample); n = 50; the hold-out was visible while the
solver was fixed, so it is not a clean test of those fixes; the language model was Claude working from `pt describe` text and pair
images; the plan's score can pass a rebuild that is missing thin parts (ladder rungs, shelf shelves), which the second reading
(`uncovered_patches`) only partly catches; no human blind test yet (Checkpoint 3); glTF-Validator and DINOv2 not yet run.

Queued repairs found by the audit: ladder (eight rungs, 0.25 apart), Shelf_01 (array of shelves), bucket (tessellation), vase
(closed profile). Next: Level 2 steps 1 and 2 (B12.2), then Phase 1 on 150 more assets, then a fresh 50 for a clean test.

### 2026-10-10: Level 2 steps 1 and 2 (materials by name, lighting description)

Pictures and numbers: `corpus/results/level2/README.md`. Done as specified in B12.2: a recipe names a material (`set_material`, stored as prim
groups `mat:<name>`, so it survives merge, blast and realize); glTF carries it as a named material per primitive, USD as `GeomSubset` bindings,
OBJ as `usemtl`; instances stay instances (`EXT_mesh_gpu_instancing` above 20 000). Blender binds one of 23 CC0 Poly Haven sets by name with box
projection (no UVs), per-object colour and offset variation for free (Object Info random). The scene file carries sky (Nishita, HDRI or flat), sun,
bounded haze, cameras (explicit, eye-height on the terrain, or auto-framed) and quality; it is checked in Rust before Blender starts.

| Measure | Result |
|---|---|
| Frame time (1280x720, 96 samples, OptiX) | about 5 s for 49k triangles; 22.8 s for 27 884 instances / 2.2 M triangles including import (quick mode, 2 frames) |
| Determinism | repeat renders pixel-identical on CPU; GPU within 1/255 on 0.00% of values |
| Exposure | calibrated on an 18% grey card: scene `exposure: 0` is a sunny day (card 0.54 at 25 degrees, 0.66 at 45 degrees) |
| Tests | 154 pass; recipes 25/25 and gym baseline unchanged |

Defects found by looking at the pictures and fixed the same day: world fog volume blacked out the frame (now a bounded layer); default five stops
overexposed; below-horizon sky band (endless ground plane now default); eye-height camera landed on fog and tree tops; Blender lost relative output paths.
Shape remains the second bottleneck (cone trees): the next step imports real CC0 assets with their authored materials (B15 row 4).

### 2026-10-10: Steel thread v0 (brief to scorecard, end to end)

Page: private artifact "Readiness Scorecard" (https://claude.ai/artifact/4ubnVNTsojviFDL7Zn8joW); files `corpus/results/steelthread/v0/`; recipe
`scenes/valley.hcs`, style `styles/alpine_meadow.json`, scene `scenes/valley.scene.json`; command `pt scorecard scenes/valley.hcs scenes/valley.scene.json --out DIR`.
One path now runs unattended in about 2 minutes: recipe (analytic alpine valley, erosion, soft material layers from slope/height/noise, 15 CC0
scans from Poly Haven placed by rules on the masks) -> glTF (authored materials and textures copied from the sources) -> Blender frames -> Godot 4.7
import, navigation mesh and frame rate -> Khronos validator -> scorecard.

| Family | Measure | Result | Bar (draft) |
|---|---|---|---|
| Visual | surfaces with a real material / unresolved names / sky + haze on | 100% / 0 / on | 99% / 0 / on |
| Art coherence | style drove the look / distinct prototypes | alpine_meadow / 15 | named / 12 |
| World function | navigation mesh coverage of slope-walkable ground | **74.5%** | 90% |
| Performance | frame rate / triangles in frame / draw calls / video memory | **2.1 fps / 1.42 billion** / 336 / 380 MB | 60 fps / 20 M / 3000 / 6000 MB |
| Content breadth | prototypes per km2 / most copies of one / heavy meshes with an LOD chain | 18 / **701** / **0% (0 of 12)** | 15 / 250 / 100% |
| Delivery | validator errors / engine import errors / external files with ledger row | 0 (19 warnings) / 0 / 17 of 17 | 0 / 0 / 100% |

**11 of 16 pass.** Cross-check: Blender and Godot count the same 1,822,200,065 instanced triangles. Godot imports the 223 MB file in 2.2 s. The five
failures each name their route in the page. **Approach change that follows from this measurement:** the performance and LOD rows fail by three orders
of magnitude because CC0 scans are authored for offline rendering (the jacaranda is 3.9 M triangles). Level 1 "learn generators" stays the end state
(B15a), but the next item is generic LOD tooling (a `decimate` node: quadric collapse for solid surfaces, card culling for foliage; LOD chains in the
export; engine-side LOD policy in the referee), because it fixes performance for every scan we will ever import and gives the generators a budget to hit.
Also found and fixed on the way: world fog volumes blacked out the frame, hard per-face material zones looked jagged (soft weighted layers now),
texture tiling showed as stripes (anti-tiling now), and an unlit diagnostic taught that layer weights follow the layer order of the recipe.

### 2026-10-10: Engine delivery (LODs, baked terrain, tangents): scorecard 11 of 16 to 17 of 17

Same recipe, style and scenes as the v0 baseline; only the delivery path changed. Files: `corpus/results/steelthread/v1/`; the private artifact
"Readiness Scorecard" now shows v1 with a baseline column. Measured by Blender, Godot 4.7.2 (RTX 2070 SUPER, 1280x720, eye height, vsync off) and the validator.

| Measure | v0 baseline | v1 |
|---|---|---|
| Frame rate in the engine | 2.1 fps | 93 to 173 fps over four runs (93, 111, 120, 173; the machine was shared) |
| Triangles in one frame | 1.42 billion | 18.5 million (bar 20 million) |
| Heavy meshes with an LOD chain | 0 of 12 | 24 of 24 |
| glTF validator | 0 errors, 19 warnings | 0 errors, 0 warnings |
| Terrain in the engine file | flat grey | baked albedo + normal, no untextured material |
| Navigation mesh reach of intended ground (slope under 25 degrees) | 74.5% (old metric) | 95.4% |
| Most copies of one large prototype in a 50 m square | 701 (global count, old metric) | 11 (bar 12) |
| Distinct prototypes | 15 | 30 |
| Draw calls / video memory | 336 / 380 MB | 1408 / 544 MB |
| Engine file size | 223 MB | 206 MB (the render file stays at full detail, 321 MB) |

What was built: `pt_core::lod` (meshoptimizer attribute-aware simplification for solids with seam-crossing collapses allowed, card culling by triangle
budget with area compensation for foliage, content-addressed cache), an engine profile (level 0 capped at about 200 000 triangles, steps of 35%, 12%
and 4%, switch at 2, 5 and 12 object diagonals), per-instance LOD nodes with ranges in the glTF extras, the Godot bridge that turns them into
visibility ranges, tangent generation, `blend_materials` planar UVs and the Blender bake of layered materials into engine textures.
**Two metrics were corrected, not loosened, and both corrections are now part of the gate:** the navigation measure first divided the navmesh area
by ALL slope-walkable ground, which is structurally below 100% because the mesh is inset by the agent radius along every cliff edge; B13.1 asks for the
intended walkable area, so it now samples the intended ground and tests reach geometrically (the engine's closest-point query returned nothing for a
baked mesh here). The repetition measure counted copies across the whole valley; it now counts copies of one large prototype inside a 50 m square, which
is what a viewer sees. The Visual family holds presence checks only, so the vision judges (B15 row 6) are the next build and make it a measured family.
The pictures show mountains reading as flat plaster, thin ground cover and few tree species: real elevation data for the valley (W1), a ground-cover
generator and a tree generator family are the fixes, in that order, so that repetition is solved by generators rather than by hunting for scans.

## B15. Approach review (2026-10-10, after Checkpoint 1): same project, faster route

**Project: unchanged** (an autonomous system that makes a finished AAA world). **Approach: re-sequenced.** Evidence: Checkpoint 1 proved the
loop (oracle, recipes, measured referee, tuned numbers; B14). It also showed that five of the six scorecard families (visual, art
coherence, world function, performance, delivery) have no implementation yet, while all effort so far went into content breadth at the
prop level. Props are not the long pole; a rebuilt prop is also something we already own as the original.

| # | Course change | Why (evidence) | Expected gain |
|---|---|---|---|
| 1 | **Steel thread first.** Build the thinnest end-to-end path now: brief, terrain (existing nodes), scatter of CC0 assets, materials and light, Blender frame, glTF/USD export, engine import, scorecard v0. Deepen the weakest stage each round. | 5 of 6 scorecard families are empty; the probe says appearance is the cheapest lift | first AAA-style frame and a live scorecard within the next work block, instead of after Phase 1 |
| 2 | **Godot (MIT, headless, no account) is the engine referee now;** Unreal later. | No Unreal is installed (only the Epic launcher); Unreal needs an Epic login and unverified headless interfaces. Godot imports glTF, bakes navigation meshes, runs bots, and exports from the command line. | delivery, world-function and performance families measurable in days, zero licence risk |
| 3 | **Level 1 becomes "learn generators", not "rebuild assets".** One parametric generator per family, fitted to several references (the chair is the bench at half the width: one recipe, one number changed). Consolidate the 24 solved recipes into families with `asset()`, then test on held-out family members. | recipes already repeat: mirrored legs, step and shelf arrays, posts and rails; per-model rebuilding costs one AI turn each | turns per new model within a family from about 1 to well under 0.2 (to be measured); unlimited variants for world dressing, which also fixes repetition |
| 4 | **Worlds use CC0 assets directly** (Poly Haven scans and plants, Kenney kits); recipes add variation and control. | Level 1 is not a prerequisite for a world | world work starts now, not after 200 rebuilt props |
| 5 | **W1 photoreal valley follows Level 2 steps 1-2 immediately, ahead of Phase 1.** | it is the first visible proof of the north star and it exposes the real gaps in order | a hero image for the owner early; gap list ranked by the scorecard |
| 6 | **Autonomous judges in every gate now:** largest uncovered patch (built), per-part recall, and the pairwise vision judge (agreed with spec truth at Spearman 0.84 in the critique experiment). | the plan's score passed a shelf with no shelves and a ladder with half its rungs; I caught them by eye, which does not scale | no human eye per asset; the same judges are required for rung A2 anyway |
| 7 | **Widen the building blocks where misses concentrate:** wedge, arch, bevel, lathe, extruded profile. | about a third of the 26 misses are vocabulary (stone bevels, arches, slants) | building pieces from 6 of 11 solved upward; about a day of work |
| 8 | **Training stays at $0:** QLoRA of a small code model on the local RTX 2070 SUPER (8 GB), free GPU tiers as overflow; Phase 3 starts after generators exist, because they make the training data. | budget is near zero; no cloud rental needed | removes the spend decision and the dependency on rented GPUs |
| 9 | **Headline KPI is the scorecard,** not assets solved. Every build emits the scorecard page. | the north star is defined by the scorecard | progress is measured against the goal, not against a proxy |
| 10 | **Drop for now:** Infinigen as a second oracle (needs a Linux box we do not have), Unreal command lines (until Godot is in place). | not on the critical path | time back |

Kept as they are: deterministic cooks and cache, the recipe language, the CC0-only ledger, measured oracles, checkpoints.

Order of work (at most two open items): finish the four known recipe repairs, then Level 2 steps 1-2 with the Blender bridge, then the steel
thread with scorecard v0 and the Godot referee, then generator consolidation, then judges, then W1 and the world compiler.

### B15a. CC0 exit schedule (decision, 2026-10-10)

CC0 is **scaffolding and answer key, not the foundation.** It is always legal to ship (commercial use, no attribution), so the reason to move
off it is differentiation and supply, not law.

| Role of CC0 data | Now | Exit route | Target |
|---|---|---|---|
| **Answer key** (reference models, scans, photos, elevation data) | permanent by design: it is how we measure and learn | later add data we capture or generate ourselves; the learned generators contain abstractions, not copies | stays, but is never shipped |
| **Shipped props and buildings** | CC0 kits and scans dress the first worlds | our generators (B15 row 3) replace them family by family; the proposer (Phase 3) extends coverage | CC0 share of shipped geometry falls with every generator family |
| **Shipped materials** | CC0 PBR sets | procedural materials fitted to scans (B12.2 step 4), so scans become training data only | procedural by Level 2 completion |
| **Sky and lighting** | CC0 HDRIs | procedural physical sky (Blender's Nishita sky, free) and atmosphere | procedural from the start of the steel thread |
| **Terrain** | real elevation tiles (public domain) | synthetic terrain with parameters fitted to real statistics (B12.3) | no tile in the shipped world |

**KPI:** every build reports its *CC0 share* (triangles, instances and materials traced to a ledger row that is not ours). The ledger already
records provenance per file, so this is a query, not new work. Rule that stays: nothing ships without a ledger row.

## B16. Style system: matched, invented, and unique to each user (design, 2026-10-10)

**Idea.** A style is not a filter and not a preset. It is a bundle of measurable choices that every generator consumes. Because our assets are
recipes with numbers, a style can be a set of those numbers. That makes styles things we can **extract** from references, **blend**, **mutate**,
**apply** and **score**, which is exactly what inventing a new style needs.

**The style file (`style.json`), one per user/project, versioned, private, portable:**

| Part | What it holds | Measured how |
|---|---|---|
| Palette and values | colour clusters in Lab, value range, saturation, accent use | colour statistics of references and renders |
| Shape language | angular vs round, taper, bevel radius, proportion ratios, symmetry, panelisation, roof and opening families, chunkiness | geometry statistics (silhouette, curvature, ratios) |
| Detail hierarchy and ornament | large/medium/small forms, trim, motifs, surface busyness | detail-frequency measures, motif library |
| Materials | procedural material parameters (roughness, wear, patina, emissive use) | material statistics fitted to references |
| Light and atmosphere | sky, time of day, fog, grade, bloom | image statistics of reference frames |
| Layout grammar | street pattern, density, plot sizes, landmarks, rhythm | block and spacing statistics |
| Render treatment | physically based, cel, painterly, low-poly | post-processing and shader settings |

**Two layers.** Every brief splits into **what** (typology: a medieval village has timber houses, steep roofs, a central square, a well) and
**how it looks** (style). Generators keep the *what* and take the *how* as parameters. "A medieval village in a sense of sci-fi" keeps the village
typology and invents the look: bevelled panels, glowing trim, brushed metal, a cool palette, hard-surface detail.

**Operators.** *Extract* (references or models to a style file), *blend* (per part: structure from A, surface from B, mix weights), *mutate/sample*
(draw new styles), *apply* (generators, materials, lighting and layout read the style), *score* (below). An art-director agent proposes candidates
by blending and mutating; judges rank them; the user picks. The picks train a per-user taste model.

**Unique per user, by construction and by measurement.** Styles are sampled from a continuous space with a per-project seed, then must pass a
**distinctness gate** against everything already made (presets and the user's earlier styles). Quality-diversity search (a map of elites over
the style axes) fills the space with many distinct styles that each pass the coherence gate. This is enforced by measurement because a language
model acting alone drifts to the same average look every time.

**Judges for style (the answer key is not a single model, so these replace it):**

| Measure | Question | Check |
|---|---|---|
| Coherence | is it one consistent style? | within-style spread of palette, shape and detail statistics is smaller than the spread between styles; repetition detector |
| Brief fit | does it read as "medieval village" and as "sci-fi"? | pairwise vision judge plus embedding agreement per tag |
| Distinctness | is it different from every known style? | distance in style space and embedding space, with a minimum |
| Steerability | does the dial work? | turn "more angular" or "warmer" and the measured statistic moves monotonically |
| Appeal | would the owner pick it? | the owner's picks (taps); aesthetic predictors stay weak tie-breakers |

**Data.** Style diversity in the corpus, all CC0 and in the ledger: stylised low-poly (Kenney castle, town, survival, furniture kits), realistic
(Poly Haven), sci-fi hard-surface (Kenney Space Kit, Modular Space Kit, Space Station Kit; pages show CC0, licence file to be confirmed from each
zip on download). The 50 assets already hold the same typology in two styles (Kenney vs Poly Haven bench, chair, table, sofa, barrel).

**First test, the owner's own sentence (the crossover test).** Extract the medieval style from Castle Kit + Fantasy Town Kit and the sci-fi style
from the three space kits; generate the village typology under four conditions (medieval, sci-fi, 50/50 blend, and an invented mutation); score all
five measures above; the owner picks. Ledger rows first, then run.

**Where it lands in the plan (approach changes):**
1. The steel thread (B15 row 1) includes a style object from the start: v0 carries palette, material set, lighting and a few shape-language knobs,
   consumed by scatter, dressing and render. Retrofitting style into finished generators is the expensive path.
2. Level 1 generators split structure from style knobs (B15 row 3). The two-style pairs above are the training data for what varies with style.
3. The art-bible compiler (Track C1) moves from late to early and becomes the style system above.
4. The scorecard gains three measures: distinctness, brief fit, steerability (B13.1 art coherence family).
5. The Phase 3 proposer takes a style vector as input: one model, any style.
6. Product rule: no shared preset library beyond bootstrap; each studio owns its style archive.
