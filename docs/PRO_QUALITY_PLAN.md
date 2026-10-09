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

**What you do:** open a session and paste this one line:

> Read docs/PRO_QUALITY_PLAN.md and execute Phase 0, then run the first 50 assets and report at Checkpoint 1.

**What I do:** build the tools in Phase 0 (download and license records, a reader for the common 3D file format, the
measuring ruler, the test runner), download the first 50 public-domain models, try to rebuild them, and write you a
plain-English report. Nothing needs your credentials or money.

**What you get:** a one-page report with traffic lights (below), pictures of originals next to our rebuilds, and a
recommendation on whether to continue.

## The three checkpoints (traffic lights)

| Checkpoint | When | Green means | If red |
|---|---|---|---|
| 1. Does it work at all? | after 50 models | at least 3 of 10 rebuilt to within 10% of the original | switch effort to the "looks like the pro pile" judge and to part-by-part fitting, then retry on 50 new models |
| 2. Does it compound? | after 200 models | at least 20 new reusable building blocks found, and the effort per model in the last 50 is at least 40% below the first 50 | the library isn't learning: change how patterns are mined before spending more |
| 3. Does it look right to a human? | after checkpoint 2 | you are shown pairs (original vs our rebuild, same camera) and pick the original in no more than 7 of 10 pairs | adjust the finishing steps and the judges, retest |

No checkpoint commits you to anything. Each one ends with a recommendation you can decline.

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
- **Missing (Phase 0 builds these):** glTF/GLB importer, reference-matching metrics (`pt match`), corpus download +
  license ledger, part splitting, the batch runner with budgets and resume, motif mining, FD_DINOv2.
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

## B4. Phase 0 (day 1): tools, with acceptance tests

1. **License ledger and downloader.** `corpus/LEDGER.csv` columns: id, source, url, license, license_url, sha256,
   fetched_at, attribution. Raw files go in `D:\Fekri\corpus\<source>\` (outside the repo; add to `.gitignore`). Each source
   gets its own new empty directory. Use `curl` or PowerShell, never run scripts shipped in archives, list archive
   contents before extracting, never open `.blend` files. Sources for the first 50:
   - Kenney (30 stylized props and buildings): official support page says all assets are CC0, attribution not required.
     Formats are listed per pack (GLB/OBJ/FBX); confirm per pack.
   - Poly Haven (10 props): assets CC0 per their public API repo; using the live API for a product requires a
     "Powered by Poly Haven" credit. API at `https://api.polyhaven.com` (docs linked from the Public-API repo).
   - Quaternius (10): described as CC0 in secondary sources only. **Confirm on quaternius.com and record the page in the
     ledger before use; otherwise drop these 10 for more Kenney.**
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

## B11. Decisions already made (do not re-litigate without new evidence)

- Use professional public-domain work as the answer key rather than human ratings or hand-written specs.
- Hand-curated references and per-item human rating were rejected: they scale with people's time.
- Aesthetic predictors are weak tie-breakers only until validated against taps.
- Start with structured, man-made and plant/rock assets; characters and animals later.
- Houdini is never run, opened or studied.

## Sources

- [MeshCoder](https://arxiv.org/abs/2508.14879), [CAD-Recode](https://arxiv.org/abs/2412.14042), [ShapeCoder](https://arxiv.org/abs/2305.05661), [LL3M](https://arxiv.org/abs/2508.08228)
- [Kenney support page (license statement)](https://kenney.nl/support), [Poly Haven Public API](https://github.com/Poly-Haven/Public-API)
- [Objaverse-XL on PyPI](https://pypi.org/project/objaverse-xl/0.1.3), [Infinigen](https://github.com/princeton-vl/infinigen), [candle examples](https://github.com/huggingface/candle/tree/main/candle-examples/examples)
- [Khronos glTF-Validator](https://github.khronos.org/glTF-Validator)
