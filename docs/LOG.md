# Log (append-only history, newest last; never read at startup)

Rule: new entries are added at the end with a date heading. Nothing here is edited later except to fix a wrong number (say so in the entry). The current state lives in [STATUS.md](STATUS.md); numbers that change live in [generated/state.md](generated/state.md).

## Results log from the pro-quality plan (was section B14)

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
| Same, after a visual check of the 24 silver pictures | **21/50 (42%); development 17/40 (42.5%), hold-out 4/10.** Nine passed the numbers but missed a feature (`eye_audit.json`): six found by my first look, three more (Barrel_01, painted_wooden_bench, trashcan) found by the part-counting vision judge and confirmed by eye. That read 15/50 and 11/40 (27.5%, under the bar) until six were repaired the same day (both barrels, ladder, shelf, bin lid, fountain; judge and numbers agree). The first read had said 18/50 and 14/40 |
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

### 2026-10-10: Vision judges (autonomous, local, free): Visual is now a measured family

Judge: Qwen3-VL-8B (Apache-2.0, Q4) run by llama.cpp b11552 on the local RTX 2070 SUPER; no spend, no external service (setup and hashes in STATUS). Code `crates/pt-cli/src/judge.rs`,
`judge_bench.rs`; commands `pt judge ...`; results `gym/critique_experiment_qwen3-vl-8b-q4_*.json`, `corpus/results/judge/`, `corpus/results/steelthread/v2/`. Calibrated on answers known by
construction before it was allowed into a gate. Bars were fixed before the runs (pairwise at least 80%, order flips at most 20%; defects found at least 4 of 6 with at most 4 false alarms of 18).

| Use | Question format | Result | Verdict |
|---|---|---|---|
| Rank candidates against a spec (10 tasks x 4, truth = task specs) | two separate images, both orders | Spearman 0.62, pairwise 79%, best pick 50%, 44% of pairs flip with the order | below bar |
| same | one labelled 2x2 sheet, scores per label, two orders | 0.54 / 76% / 57% | below bar |
| same | both candidates side by side in ONE picture with stamped labels 1 and 2, both orders | 0.69 / 82% / 63%, 42% flip | pairwise passes, flips do not; author model on the same set: 0.84 / 92% / 90% |
| Rebuild missing a feature (24 solved rebuilds, 9 known defects, controls) | leading yes/no question ("does the copy lack a feature?") | flags all 24, including 22 of 24 pictures compared with themselves | useless, code removed |
| same | judge COUNTS parts per column (rungs/rings/shelves, legs, arms/handles, openings, top shape); Rust compares | 8 of 9 defects found, 0 false alarms on 15 good rebuilds, 0 of 24 identical pairs flagged, 23 of 24 different objects detected | passes (development numbers) |
| World frames against a brief (steel thread v2) | 0 to 10 fit; forced choice between the brief and three other worlds, both orders; blank-frame control | valley frames: brief picked 9 of 9, 0 flips, blank scores 0; fit 4, 6, 4 (mean 4.7 of 10, bar 7) | works; the fail row is real |

**Approach change that follows (already in the order of work):** a small model is a measuring instrument only when its questions are closed and countable, asked in both orders, with labels inside
the picture, and calibrated against known answers. So: spec-level ranking of near-identical candidates stays with the `spec(...)` lines (exact), the part judge gates rebuilds, the frame judge gates
worlds, and the author model through `pt critique-bench` is the independent second reader for audits. A stronger local VLM (Qwen3-VL-30B-A3B with expert offload) is the upgrade if the fresh-50 test shows misses.

**Found on the way:** (1) the part judge flagged three rebuilds my eye audit had passed (Barrel_01 without its rings, painted_wooden_bench without its scrolled arms, trashcan without its domed lid);
the pictures confirm all three. The stricter visual reading of Checkpoint 1 fell to 15/50 with development 11/40 = 27.5%, 2.5 points under the G1 bar; six repairs the same day (both barrels, ladder, shelf, bin lid, fountain, each
verified by the part judge and the numbers) lifted it to 21/50, development 17/40 = 42.5%, hold-out 4/10. Open: cactus arms, bench scrolls, tower (shape rebuilt, score bronze). Lesson: a tuner shrinks
a feature the score cannot see (the first tuned barrel lost two hoops), so such structure goes in whole numbers (not tuned) or in one lathe profile; one lathe profile also avoids the hidden-overlap penalty of stacked solids. (2) The judge's sentences on the valley frames name the causes of the 4.7: stylised look, a flat olive band where the sky should be in the hero frame (the endless
ground plane reads as sky), grey mountains with little snow. (3) The first server settings filled the 8 GB card (7.7 GB at a 12k context); the context is now 8k, and the client waits for a loading server instead of starting a second.
**Scorecard v2: 19 of 20** (17 unchanged rows plus three Visual rows; brief fit fails at 4.7 of 10, brief-versus-foils 100%, judge reliability passes).

### 2026-10-11: Step 1, one world that looks real (real elevation, the Teton valley)

Files: `scenes/teton.hcs`, `scenes/teton.scene.json`, `styles/teton_summer.json`, `corpus/dem_manifest.json`, `corpus/results/steelthread/v3` and `v4`. Data: USGS 3DEP through Terrain Tiles on AWS
(public domain; wording verified at usgs.gov: "USGS-authored or produced data and information are considered to be in the U.S. Public Domain", credit requested; ledger class `PD-USGS`, 34 tiles, ledger 305 rows, 0 problems).
Built: `dem` (deterministic: its own logarithm series, no platform libm), `hf_flow` (priority-flood hydrology), `pt terrain` (statistics, map, valley search), the `playable` scene window, a layered-shader specular fix.
The world: near window 1.6 km at 2.5 m per cell blended into a far ring 24 km square at 25 m, creek and lake masks from real flow, layers by elevation and slope, conifers to the tree line. Terrain statistics of the near window: relief 761 m, median slope 30 degrees, 90th percentile 47 degrees.

| Measure | Analytic valley (v2) | Teton, first full run (v3) | Teton after fixes (v4) |
|---|---|---|---|
| Judge brief fit (0 to 10, bar 7) | 4.7 | 4.7 | **6.0** |
| Brief picked over three other worlds | 9 of 9 | 9 of 9 | 9 of 9 |
| glTF validator errors | 0 | 1 922 (zero normals on unused far-ring vertices) | 0 |
| Heavy meshes with LOD chain | 24 of 24 | 0 of 24 (LOD fell back above 80 000 nodes) | 24 of 24 |
| Distinct prototypes per dressed km2 (bar 15) | 36.1 | 0.1 (divided by the whole 576 km2 mesh) | 11.7 (measure now divides by the dressed area) |
| Most copies of one large prototype in 50 m (bar 12) | 11 | 28 | 25 |
| Engine rows (navigation, fps, triangles, draw calls, memory) | measured, pass | measured (navigation 0%: baked a 576 km2 mesh) | **not measured**: Godot import stalled over 20 minutes on 270 000 LOD nodes |

**Approach change (already the next item):** per-instance LOD nodes do not scale past about 100 000 nodes; the engine export becomes tiled `EXT_mesh_gpu_instancing` nodes (one per tile, prototype and level, with distance ranges per tile), which also makes the engine rows measurable at any world size. Expected gain: engine rows back within one work block, and the same exporter serves every later world.
**Found and fixed on the way:** the first window centre was on the wrong side of the range (a flat map was not enough; `pt terrain --find-valley` and 3D previews now choose it); the tile row was computed by hand and was off by three (tile numbers are now computed in code); 3 x 3 tiles cover only 3.4 km (5 x 5 now); far-ring relief did not fade at the seam and left a dark sliver; the far forest was painted three times too bright because style colours are linear; the rock texture is salmon orange (a per-channel tint cancels the hue); AgX pushed greens to cyan and highlights to pink (Khronos PBR Neutral in the style); per-km2 measures divided by the whole mesh (now the dressed area); the navigation bake gave up on 576 km2 (scene key `playable`).
**What the judge still says:** "stylized, low-poly ... artificial, blocky texture". The faceted far slopes (ridged relief on 25 m cells) and three pine variants are the visible causes; both are the next items after the tiled export.


## 2026-10-09 to 2026-10-10: kernel gates and early work (moved from STATUS)

### Measured gates

| Gate | Result |
|---|---|
| P1: 5M instances + terrain cook+preview | 0.89 s, 856 MB peak (`pt bench a`) |
| P1: incremental recook | 1 param change recomputes only the downstream cone |
| P1/D2: determinism | bit-identical across thread counts, and across Windows, Linux and macOS since the physics math stack was vendored and patched (CI hash compare) |
| P2: autodiff | forward-mode gradients match finite differences on 8 programs |
| P3: fit rock to held-out noisy scans | 98-101% of achievable gain, 1500 evals, 5 restarts (`pt optimize-demo`) |
| P3: 64-variant exploration | 17.6x faster with the shared cache (`pt vary-bench`) |
| P3: mesher robustness | surface nets alone non-manifold/open in 14/24 fuzz cases; `auto` fallback fixed 24/24 |
| P6: recover params from silhouettes only | IoU 0.998 / 0.992 / 0.988, coarse-to-fine (`pt recover-demo`) |
| P4: gym | oracle 1.00 / perturbed 0.74 / null 0.40 |

### Done recently (as of 2026-10-10, all verified by tests and CI)

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
* Vision judges (2026-10-10, `pt judge`, local Qwen3-VL-8B; files `gym/critique_experiment_qwen3-vl-8b-q4_*.json`, `corpus/results/judge/`): the same 10-task critique set
  with a judge that is not the author: separate images 0.62 / 79% / 50%, labelled 2x2 sheet 0.54 / 76% / 57%, side by side in one picture 0.69 / 82% / 63% (Spearman / pairwise / best pick;
  author model 0.84 / 92% / 90%), 42% to 44% of answers flip with the order. Fine spec-level ranking is therefore NOT a judge job (spec lines are). Where the judge works:
  world frames against a brief (valley frames: brief picked over three other worlds 9 of 9, 0 flips, blank frame scores 0) and part counting on rebuilds (`pt judge parts`: 8 of 9 known
  missing-feature defects, 0 false alarms on 15 good rebuilds, 0 of 24 identical pairs flagged). A leading question ("does the copy lack a feature?") flagged every rebuild, so questions are
  closed and countable and Rust compares the counts. It found three real defects my eye audit had missed (barrel rings, bench arms, bin lid), which dropped the strict visual reading of Checkpoint 1
  to 15/50 (development 11/40 = 27.5%, under the 30% bar). Six repairs the same day (both barrels, ladder, shelf, bin lid, fountain; each verified by the judge and the numbers; sources in `corpus/recipes/llm_src/`)
  lifted it to **21/50, development 17/40 = 42.5%, hold-out 4/10**. Still open: cactus arms, bench scrolls, tower (shape rebuilt with battlements, score bronze). Lesson: a tuner will shrink a feature the score cannot see
  (hoops 9 mm proud), so such structure is written as whole numbers (not tuned) or inside one lathe profile.

## 2026-10-10 and 2026-10-11: state paragraphs from STATUS, in the order they were written

### Wrap-up of 2026-10-10 (evening)

**Read [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) first.** North star (owner's goal): an AUTONOMOUS system that makes a FINISHED AAA world; staged by autonomy rungs A0-A4 and an AAA Readiness Scorecard. Plan covers five levels (assets, appearance, systems, composition, delivery; assets alone do NOT give realistic worlds). Track A: rebuild public-domain professional
assets as recipes, score by measurement, mine reusable building blocks, then train a proposer. Track B: realism probe, then a "photoreal valley" from real elevation data + CC0 scans/materials rendered in Blender. Tomorrow starts at Phase 0 + the Realism Probe.

**Approach review 2026-10-10 (PRO_QUALITY_PLAN B15) supersedes the order below:** steel thread first (brief to scorecard end to end), Godot as the free engine referee, Level 1 reframed as learning generators, worlds built from CC0 assets directly, autonomous judges in every gate, scorecard as the headline KPI, training at $0 on the local GPU.

**State after 2026-10-10:** Phase 0 tools, the Realism Probe and Checkpoint 1 are done: the stack solved 24/50 (48%) by the plan's score and 21/50 (42%) on the strict visual reading (9 defects: 6 by my eye, 3 found by the part-counting judge; 6 repaired the same day); development 19/40 and 17/40, hold-out 5/10 and 4/10, so G1 passes on both readings (PRO_QUALITY_PLAN B14, `corpus/results/checkpoint1/report.html`). **Level 2 steps 1 and 2 are done** (B14, `corpus/results/level2/README.md`): `set_material` names, material-carrying glTF/USD/OBJ, 23 CC0 PBR sets in the ledger, `pt render-scene` and MCP `render_scene` (Blender Cycles, sky/sun/haze/cameras from a scene file, calibrated exposure, repeat renders pixel-identical on CPU). **Steel thread is done and delivered to an engine** (B14, `corpus/results/steelthread/v0` and `v1`, private artifact "Readiness Scorecard"): `import_asset`, `copy_variants`, `blend_materials`, styles, `scenes/valley.hcs`, Godot 4.7.2 as engine referee, the glTF validator, `pt scorecard`, then the engine delivery path (`pt_core::lod` LOD chains with a per-level triangle cap, per-instance LOD nodes, Blender bake of layered terrain to textures, tangents). **Scorecard 11 of 16 (baseline) to 17 of 17 (v1), then 19 of 20 with the vision judge (v2, `corpus/results/steelthread/v2`; Visual is now a measured family: brief fit 4.7 of 10 against a bar of 7 is the one failing row, and the judge's own sentences name the causes: stylised look, flat olive band where the sky should be, grey mountains with little snow):** 2.1 fps to 93-173 fps, 1.42 billion to 18.5 million triangles in frame, LOD chains 0 of 12 to 24 of 24, validator 19 warnings to 0, terrain textured in the engine, 30 prototypes. Two metrics were corrected, not loosened (navigation reach on the intended walkable ground; repetition per 50 m square for large prototypes). The pictures show flat-plaster mountains, thin ground cover and few tree species. **Vision judges are done (step 1 below; see "Vision judges" above): frames against a brief, part counting on rebuilds, and a labelled-picture rule for every question; style distinctness (a judge on the style file, B16) comes with the style work.** **Next, in order (B15, re-sequenced 2026-10-10 evening):** (0) ~~the defect repairs that close G1 on the strict reading~~ done (open: cactus arms, bench scrolls, tower score); (1) ~~vision judges~~ done, (2) the hero camera and ground plane (the olive band) and real elevation data for the valley (W1: USGS 3DEP tile, ledger row, terrain statistics) replacing the analytic terrain, (2b) a terrain-and-placement toolkit (TECHNIQUE_NOTES gap list items 1-4: mask layers on the heightfield, hierarchical scatter with variants and exclusion volumes, biome rules, road/river carving), then (3) a tree and ground-cover generator family built on morphology knobs, not real species (B16a: repetition by generators, CC0 share falls, a "sci-fi village" brief must not get Earth trees; the judges in step 1 take two briefs from the start and the tree family must pass the two-brief gate), (4) the remaining queued repairs (bucket, vase), (5) Phase 1 mining on 150 more assets and a fresh 50 for a clean test.

### Wrap-up of 2026-10-11 (Step 1: one world that looks real)

**State after 2026-10-11 (Step 1: one world that looks real).** The valley is now built from real elevation: `scenes/teton.hcs` (a glacial valley in Grand Teton National Park from USGS 3DEP; near window 1.6 km at 2.5 m per cell blended at its edge into a far ring 24 km square at 25 m so the view runs to real ridgelines; creek and lake masks from real water flow on a 4 km window; layers by elevation and slope; style `styles/teton_summer.json`). New nodes `dem` and `hf_flow`, tool `pt terrain`, scene key `playable`, ledger class `PD-USGS`. **Judge brief fit 4.7 to 6.0 of 10 (bar 7); brief picked over three other worlds 9 of 9; validator 0 errors; LOD chains 24 of 24 (v4, `corpus/results/steelthread/v4`).** Not measured at world scale: the engine rows (navigation, frame rate, triangles, draw calls, memory): Godot did not finish importing the 420 MB engine file in over 20 minutes because the LOD export writes one node per instance per level (about 270 000 nodes), so the import was stopped and the rows show "not measured". Failing for real: prototypes per dressed km2 11.7 (bar 15) and 25 copies of one large prototype in a 50 m square (bar 12): three pine variants cannot fill a dense forest, which is what the generator families are for. The judge's own words on the frames: "stylized, low-poly ... artificial, blocky texture"; the faceted far slopes (my far-ring relief on 25 m cells) are the cause. **Next, in order:** (1) a tiled instancing exporter (one `EXT_mesh_gpu_instancing` node per 100 m tile, prototype and level, with distance ranges per tile) so the engine rows are measured again at any world size; (2) smooth the far ring (broader relief, no faceting) and add macro texture variation at distance; (3) tree and ground-cover generator families on morphology knobs, which also fixes variety and repetition; (4) terrain statistics as a scorecard row, then the owner's blind pairs against a hand-built baseline (the W1 gate); (5) the rest of the toolkit (mask operators, hierarchical scatter, biome rules, road and river carving; `hf_flow` is the first piece).

### Wrap-up of 2026-10-11, afternoon (Step 2: a world an eye-level camera can stand in)

**State after 2026-10-11, afternoon (Step 2: a world an eye-level camera can stand in). READ THIS BEFORE CONTINUING: the session was wrapped up mid-run.** The owner showed a target picture (an eye-level creek with a timber mill and bridge, see Environment) and judged the Teton valley far from it, so the visual check is now a side-by-side with that picture, by eye (`out/progress/side_by_side.html`, local only). A second world was built for it: `scenes/cascade.hcs` with `scenes/cascade.scene.json` and `styles/cascade_evening.json` (the calm reach of Cascade Creek below the forks, Grand Teton; four nested terrain levels from 0.5 m to a 28 km skyline, a carved creek with clear water, about 27 000 scanned firs and pines, grass as patches, a footbridge and a procedural thatched hut). New tools: `hf_fit` / `hf_hole` / `hf_refine`, river channels in `hf_flow`, `lod_hint`, `import_asset max_triangles`, `pt dem-manifest`, the tiled engine export with far tiles and plant shells, haze and mist, light through leaves, crisp terrain patchwork and relief in the shader, clear water, an import cache (AGENT_GUIDE, "A world an eye-level camera can stand in"). Frames at full quality: `out/steelthread_v5/frame_hero.png`, `frame_east.png`, `frame_valley.png` (all three hold up when opened), `frame_village.png` (half in deep shadow: the camera has to move).
Measured in the engine on the creek world (`out/sc_cascade_test`, no render): 106 fps, 97.2% of the intended ground walkable, validator 0 errors and 0 warnings, 1339 draw calls, 659 MB video memory, 21.1 prototypes per km2, 261 219 instances in 37 000 nodes imported in 8 s; triangles in one frame 20.6 million against a bar of 20 (one more trim went in after that measurement and is not measured yet); 15 copies of one tree in a 50 m square against a bar of 12 (nine tree models; the tree generator closes it). On the small valley the same export gives 243 fps and 9.8 million triangles (was 93 to 173 fps and 18.5 million).
**Not finished at wrap-up, in this order:** (1) the full scorecard run for the creek world (`pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`) had rendered its four frames and was baking terrain when the session was wrapped up, so the judge's number for this world and the final scorecard do not exist yet; (2) `scorecard --reuse` (keep a finished render and bake) and the bake reading the render's import cache are WRITTEN in `scorecard.rs` and `blender.rs` but were never compiled: run `cargo check -p pt-cli` first; (3) `tune_authored` in `render_scene.py` and the two `rock_moss_set_*` entries in the style (grey the scanned rocks, which render orange in direct sun in `frame_east.png`) are written but no frame has been rendered with them; (4) move the `village` camera onto the lit side of the hut; (5) `cargo test --workspace --release` has not been run since the last edits (pt-core 32 pass, the scorecard tests pass, pt-sop passed 44 before `lod_hint` and `max_triangles` went in); (6) nothing from 2026-10-10 evening onward is committed. Then copy frames and `scorecard.json` to `corpus/results/steelthread/v5/`, add the B14 entry in PRO_QUALITY_PLAN, and update the published "Readiness Scorecard" page.
**Next after that:** a tree and ground-cover generator family (closes repetition, lowers the 76% CC0 share, makes the look unique per studio); a scene composer that picks the camera, sun and building sites by rule (this turn they were picked by hand); a small look-development version of a scene that renders in under a minute; a building kit beyond the one hut; the engine look (terrain detail, water, grass) closer to the render.

## 2026-10-12: documentation harness rebuilt

* **Why:** an audit found STATUS (30 KB) had become a diary of stacked "State after…" paragraphs, the plan (76 KB) held four jobs, three files contradicted each other on cross-OS determinism and node counts, hand-typed test counts had drifted, a Windows path was mangled by backslash escaping (also in the memory copy), and the plan carried "Honest unknowns" style sections that CLAUDE.md bans.
* **Done:** documents split by lifetime (README index, STATUS 4 KB, LOG, DECISIONS, ENVIRONMENT, TOOLS, WORLD_GUIDE split from AGENT_GUIDE, plan trimmed from 76 KB to 44 KB, PLAN sections 10 and 11 corrected and reordered); `pt status --write` generates `docs/generated/state.md`; `pt docs-check` runs in CI (stale state, broken links, size budgets, banned headings, mangled paths, STATUS older than the last commit, unindexed documents); memory files thinned to pointers and durable lessons; the wrap-up routine and the rules are in CLAUDE.md.
* **Measured the same day:** `cargo test --workspace --release` 200 passed, 0 failed, 1 ignored (the earlier "189" in STATUS predated later tests). `cargo check -p pt-cli` compiles the `scorecard --reuse` and bake-cache edits that had never been compiled.

## 2026-10-12 (later): the owner's review of frame_east, the grounding check and the PC-load fix

* **Owner's note:** `out/steelthread_v5/frame_east.png` had hovering moss and flat stones in the water; I had called the three creek frames good after a single downscaled look. Own defect: the frame was not reviewed at full resolution.
* **Measured** with the new `pt ground-check` (CPU only): across the world 1805 objects mostly under water and 1613 hanging over a slope or bank; in the east view 272 and 40. Its annotated frame boxed the flat stones exactly (the check and the owner's eye agree).
* **Fixed:** `drop_to_surface footprint` (settle onto the lowest ground under the object) and `import_asset sink`, water-depth rules for stones, slope caps, in `scenes/cascade.hcs`. After: 6 mostly-under-water and 2 hanging across the world, none floating or hanging in any camera view. Not yet re-rendered.
* **Guard:** two scorecard rows, `pt review-tiles`, and a review rule in CLAUDE.md (B7: a frame is reviewed with the annotated check and enlarged tiles).
* **Same day:** DECISIONS B17 (the owner's eye is the quality gate) and `pt_cli::lowload` (heavy children at low priority, memory-capped, killed with `pt`; bake and judge opt-in).

## 2026-10-12 (evening): the capability track

* **Owner asked** what the big Houdini features would do for an agent driving the product. Two reviews of the engine as its user: plumbing (geometry queries, pixel pick, stable identity, ecology, fast pictures, building grammars) and the headline families (simulation dressing, layered world file, procedural materials). Checked in code: the language has no geometry queries, scatter writes only position, direction and size, `explain` needs an index, the rigid-body solver has a flat ground plane only.
* **Decided:** nine capabilities in a fixed order with plan dates (DECISIONS B18); stage 2's building kit moves to the grammar item, stage 3 is the materials item (PRO_QUALITY_PLAN B12.6 note). Parked with a route: water bake, smoke and fire (Blender's solvers), life, tiled world builds, signed-distance cliffs.
* **Same day, found uncommitted from the morning:** `--cull` for one-view look-development renders and per-material triangle stats in `pt ground-check` (verified and committed with this entry).
* **Capability 1 built (geometry queries):** a wrangle takes an optional second input and can ask it `near_dist`, `near_dist_other`, `near_pos`, `near_count`, `near_f/v/sum`, `surf_dist/pos/n/f/v` and `ray_dist/pos/n/f/v`; `pt_core::spatial` (point tree, triangle tree), `pt_vex::query`, `pt-sop/refindex.rs`. Checked before cooking (the attribute-flow check reads the second input), the same answer on 1 and 4 threads, brute-force agreement for nearest, within-radius, closest-surface and rays. Worked example `recipes/26_clearing_planted_by_asking.hcs` (it is also a new gym build task, so the gym baseline moved: perturbation agent 0.731, null 0.437, oracle 1.000). All workspace tests pass.
* **East frame, two attempts, both stopped by the 8 GB job cap** (Blender needs more than 7.3 GB for about 210 000 instance objects; `--cull 150` and `--cull 60` kept 218 595 and 210 367 of 256 588 instances). The cap did its job: no lag on the PC. `--cull` now prints the kept instances by prototype so the memory is found by measurement.
* **Capability 2 built (pixel pick):** `pt pick <recipe> <scene> <camera> <x> <y> [--radius PX] [--frame F.png] [--annotate OUT.png]` and the MCP tool `pick` cast the camera ray through a pixel into the cooked world (CPU only; box tree over instance bounds, a triangle tree per prototype, `pt_core::spatial`) and name the prototype, instance, creating node with parameters (provenance), height above the ground and the ground-check verdict; `--radius` counts the classes of object in a region. Tests: ray/projection round trip, nearest-hit and hiding, a provenance test through a real recipe.
* **Look-development memory, measured:** the cached creek scene holds 9.5 million triangles over 50 unique meshes (terrain 3.8 M, the merged far forest 1.1 M, three grass-tile prototypes 1.2 M), 10 369 objects and 403 MB of textures. A fresh import needed more than the 8 GB cap and the first render stalled at 5 GB with the cap's shared-memory headroom gone. `--cull` now also drops far stand-ins outside the view cone and the terrain cells nobody sees (the far level stays whole): 73% of the terrain cells and 75% of the far forest go for the east camera.

## 2026-10-13: east frame reviewed, black-card defect closed, capability 3 built

* **East frame (open item 1):** rendered from the lighter preview recipe (OptiX, 40 samples, half size) after the placement fix; reviewed as the full frame plus the enlarged tiles that held the defects; `pt ground-check`: nothing floating or hanging in the east view (2 hanging across the world, 62 mostly under water, 2 043 buried). The rocks now render grey (the colour fix works). Pick found the pale slab on the right slope to be a half-buried dead trunk (`h_logs`), part of the scene. The dark pointed shards and the pale flakes were one class defect: the seed-head cards of the tall `grass_medium_01` variants have their cut-out in the black background of a JPEG (no alpha), so empty card rendered solid black (pale under translucency). Fixed in the render material setup for every plant of that kind (`key_black_background`), found with `pt pick` and a ten-second lab render; the east frame was re-rendered and the shards are gone.
* **Look-development render on this machine:** only worked after three cuts (preview recipe, `--cull 60` incl. far stand-ins and terrain cells, and a 10 GB job cap for the one render); the numbers and the recipe are in ENVIRONMENT.md. This is the evidence for capability 4.
* **Village camera:** moved to the lit side of the hut (56, 27, yaw 72, sun behind the camera), chosen by casting five rays from each of four candidate viewpoints with `pt pick` (one cook, no render): the chosen one sees the hut on every ray and no tree in front. Not yet rendered.
* **Capability 3 built (set dressing by physics):** node `rbd_settle` (rigid bodies fall onto any mesh and come to rest; resting positions and turns; `settle_move`, `settled`; deterministic; only nearby ground is the collider), `pt_sim::rbd::settle`, recipe `27_woodpile_and_stones` (also a gym task: perturbation agent 0.732, null 0.435, oracle 1.000). Cloth drapes over any mesh through `sdf_from_mesh` (checked on a table); a rope is a bent curve (`cloth_sim` needs polygons). All workspace tests pass.
* **Capability 4 built (fast pictures, first two parts):** the look-development tier is a first-class switch (`lookdev(full, light)` in recipes, `Graph.lookdev`, `pt render-scene --lookdev`), `--lookdev --only CAMERA` crops the world to the view (instances, far stand-ins, terrain cells), and the cooked world is cached under a key (recipe, program bytes, tier, crop, cameras): a repeat render that changes only light, style, material script or scene file took 31 s instead of about 7 minutes (cache hit, import 0.8 s). The hand-edited preview copy of the creek recipe is retired. The village camera render (first fill of the cache): import 12.6 s on the cropped scene.
* **Capability 5 built (stable identity and the owner's decisions):** `scatter_stable` (a point is decided by the cell of the world and the seed: surviving points stay put and keep their `pid` when terrain, masks, density or anything elsewhere changes; spacing on the map), `apply_edits` (remove, move, scale, add by `pid`; points and instances), copy nodes key turn and variant on `pid`, `edits_of(file, scope)` in the recipe language, `pt edit` to record a decision, `pt pick` shows the `pid`, a shared surface-tree cache. The creek recipe now places everything with it (256 109 instances against 256 588 before; ground-check: 1 hanging, 52 mostly under water, 1 886 buried, none floating). Verified on the real world: record "remove" for a picked grass tile and "scale" for a fir, rebuild: one instance fewer, nothing else moved. Recipe `28_stable_orchard` (gym task).
* **Capability 6 built (layered world file), without the USD export:** style `knobs` read in recipes with `knob(name, default)` (plus `Graph.knobs`, set by every command that cooks a scene), a world file (`worlds/cascade.world.json`: recipe, scene, variants of style + knobs + owner layer) and `pt world info|scene|render|check|pick [--variant]`, the cooked-world cache keyed by knobs and by `edits/`. First variant pair on the creek: evening and overcast (`styles/cascade_overcast.json`: grey light, stronger mist, forest_density 1.2, log_density 2.2): 256 109 against 262 015 instances from one recipe. East frame on the stable placement rendered in the look-development tier and reviewed (hut now visible through the trees, grass and slope read naturally).
* **Capability 7 started (nature that grows): the tree family.** `tree_conifer` (a conifer from nine proportions plus seed: trunk, whorls of drooping branches, sprays of needle cards, about 45 000 triangles at detail 1) and the needle picture drawn by the program (`pt gen-texture needles`, library material `conifer_needles`, kind `foliage`, alpha cut-out), a lab world to judge trees in seconds (spruce and wide pine read as real conifers next to a scanned fir). The creek recipe takes `tree_source = generated` from the style (world variant `grown`): identical placement, only the prototypes change. Also: `scatter_stable` writes stable random numbers `u` and `v` for probabilistic rules; `pt pick` and `ground-check` take `--lookdev` (same tier as the frame); grass tiles settle on a larger footprint (no hanging tile in view after the lookdev tile density went up). Full-size look-development frame (1920 x 1080, 96 samples) of the east view fits memory and reads well: dense overlapping light grass tiles instead of dense heavy ones (3.5 million unique triangles of full tiles ran Blender out of memory).
* **Grown forest, first frame:** the `grown` variant of the east view (same placement, generated conifers): a dense, dark evergreen mass; mid-distance trees read as spruces, the foreground trees show cards as big leaf shapes (needs finer sprays). Recipe `29_conifer_grove` (gym task). All workspace tests pass.

## 2026-10-13: tooling pass from reading effectcraft (DECISIONS B19)

* **What was read:** effectcraft (an open-source After Effects clone in Rust, built by several agents at once): its single command list that feeds menus, command line and agent tools, its one gate command, its never-crash rule and last-resort guard, its end-to-end tests that drive the product through the agent's own tools, and its ranked gap list with costs calibrated on its own history.
* **One command list:** `crates/pt-cli/src/registry.rs` holds ten file-level commands (docs-check, edit, gate, ground-check, recipes, render-scene, review-tiles, scorecard, status, world), each declared beside its code. The command line refuses a mistyped option and a wrong argument count, the tool server lists each as a tool (so `ground_check`, `review_tiles` and `scorecard` reached the agent for the first time) and runs it as a child process, and `docs/generated/commands.md` is generated and checked by `pt docs-check`. The recipe list of the tool server is generated by `crates/pt-cli/build.rs`.
* **Strict arguments and a guard:** every tool call is checked against its schema (unknown, missing, wrong type) and refused with the accepted names; a panic in a tool returns an error and restores the scene; a malformed line on stdio gets an answer; a poisoned lock and a panic in a page request no longer end the session.
* **Platform-independent maths enforced for the first time:** the ban in `clippy.toml` found 61 breaches in production code (the tree and texture generators, hydrology, the smoke solver, the gradient fitter, camera maths). They now use `pt_core::math` (new: `sq`, `powi`, `sin`, `cos`, `tan`). Of 29 golden hashes one changed, `29_conifer_grove`, and was re-recorded; the other 28 and the gym baseline are unchanged.
* **Crash-prone shortcuts removed:** about 70 `unwrap`, `expect` and `panic!` sites in production code were replaced by errors or total matches; the workspace now denies them (`[workspace.lints.clippy]`), tests may use them. A first text counter missed one site that clippy found, so clippy is the authority.
* **Gate and CI:** `pt gate` (alias `cargo gate`) runs build, lint, docs-check and tests; CI gained the lint step. A deliberate `unwrap` was added and rejected to show the ban bites.
* **Tests through the agent tools** (`crates/pt-cli/tests/agent_tools.rs`): the command line and tool list agree; mistyped options and arguments are refused with the accepted names; a failing command returns what it printed; rocks placed by the footprint rule never float or hang over four seeds on a rugged hill, the same world without the rule hangs 10 to 16 rocks per seed, rocks lifted 2.5 m float, stones on a submerged bed are counted as flat lids.
* **Route map:** [ROUTE.md](ROUTE.md) ranks all open work with evidence, route, cost and plan dates. Calibration: capabilities 1 to 6 and the start of 7 landed in about two hours of commits (10-12 23:10 to 10-13 01:15), so the old dates (one step a day) are re-set at three times the measured pace.
* **Checked-in setup:** `.mcp.json` registers the tool server; CLAUDE.md gained "Running and looking at the product", the rule that a fix ships with a test that fails without it, and the registry rule.

## 2026-10-13: the owner approves the three lanes

* The owner said yes to three work lanes side by side (ROUTE.md "Lanes", DECISIONS B19 item 8) and asked that no work start yet. Recorded; nothing is in progress. The blind pairs were explained to the owner and their protocol written into ROUTE.md.

## 2026-10-13: the blind pairs wait; renders need no permission (DECISIONS B20)

* The owner pointed out that the creek renders have already been seen, so pairs made from them are not blind. Decision: the pairs move to views the owner has not seen (earliest about 10-22, after ROUTE.md rows 3 to 5); stage 1 is judged by the owner's side-by-side look at owner-quality frames in the meantime.
* The owner asked that renders never wait for permission (it collides with an autonomous system). The "say so first" rule is removed from CLAUDE.md, the tool descriptions, the generated command table, AGENT_GUIDE and STATUS; the protections stay in code (low priority, memory cap, killed with `pt`).

## 2026-10-13: reference images tested and documented, not built

* **Tested at the owner's request:** the web search tool has no `provider` or `categories` option and finds pages, not pictures; public SearXNG servers gave a captcha, rate limits or nothing, and a local one needs Docker or Python, which this machine lacks. Openverse and the Wikimedia Commons API work with no key and give direct image links, creators and licences; Commons "Featured pictures" and "Quality images" return 4 000 to 6 500 pixel photographs; Openverse has no marker for AI-made images. A downloaded timber-mill photograph was viewed through the file reader, so search, file and look all work.
* **Decided (DECISIONS B21, ROUTE row 14):** build it later, as a registry command, with a quality gate: AAA-grade game environments, high-end photography and credited concept art qualify; AI-generated images, low-budget game art and low resolution are refused; no licence filter on reference search (the owner: it is only a reference), while the corpus keeps CC0. Nothing was added to the code.


## 2026-10-13: seating by the real shape (the owner's three defects in the east frame)

* **The owner's note:** in `east2_east.png` firs on the right bank float, a dead trunk in the creek hangs over the water line, and green dressing sticks out rigidly over the left bank. I had passed the tree bases as "a black plate"; that was my mistake: it is the slab of scanned soil the fir stands on, seen from below.
* **Cause:** the check and the placement measured a bounding box and a typed radius. The fir scan brings a slab 1.4 m wide and up to 0.4 m thick under its trunk; the scanned trunk `dead_tree_trunk_02` stands half a metre up on one branch stub; grass tiles were tilted to the ground at their centre point only.
* **Built:** `pt_core::seat` (the underside of a prototype from its vertices: base outline, slab top, where the body starts; one verdict for the check and the placement), the node `seat` (a standing object's base and slab into the ground, `bury` for roots; a lying object turned to the plane under its footprint and resting on its body; wood floating at `draft` where the water is deep enough; `limit` removes what the ground falls away under), and `pt ground-check` on the same measure, printing `soles`.
* **Measured on the creek world:** 16 664 hanging and 4 floating objects by the new check before the rule (the old check saw none in view), none hanging and none floating anywhere in the world after it; 99.7% of the objects stay.
* **Tests:** seven in `pt-core` (the base of a tree is its root plate and not its crown; a tree on a slope; the slab of a scanned tree; a log on a stub; a lying object follows the slope; wood in deep and shallow water; the height lookup) and four through the agent tools (trees dropped by their centre fail the check on four seeds and pass when seated; logs on a stub fail on three seeds over land and water and pass when seated).
* **The two older worlds:** `scenes/valley.hcs` and `scenes/teton.hcs` placed by points only; with `rooted` and `seated` after their copy nodes the valley went from 576 floating and 2 859 hanging objects to none (94% of its objects stay) and the Teton world from 2 108 and 24 411 to none (91% stay; what goes stood where the ground falls away under it).
* **The log rule in a picture:** `out/lookdev/lab/log_creek.png` shows the scanned trunk in a lab creek, far row dropped by its lowest point (it hovers on its stub on the banks), near row seated (it lies on the bank and in the water line).
* **The check on every frame:** `pt render-scene` now reports `floating_or_hanging_in_view` and warns when it is not zero (a lab slope with unseated firs reports 6, the same slope seated reports 0).
* **On the way:** the first render ran beside my lint build and stopped with "out of GPU memory" (one heavy job at a time is the rule; I broke it), and the second lacked `PT_HEAVY_MEM_GB=10` (ENVIRONMENT.md). The tool server holds `pt.exe` open, so a rebuild fails silently until the file is renamed (ENVIRONMENT.md).
