# Decisions (design choices that stay fixed until new evidence changes them)

Section numbers (B11, B15, B15a, B16, B16a) are kept from the plan so references in code comments and the log still resolve. The plan: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md). Dated results: [LOG.md](LOG.md).

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


## B15. Approach review (2026-10-10, after Checkpoint 1): same project, faster route

**Project: unchanged** (an autonomous system that makes a finished AAA world). **Approach: re-sequenced.** Evidence: Checkpoint 1 proved the
loop (oracle, recipes, measured referee, tuned numbers; B14). It also showed that five of the six scorecard families (visual, art
coherence, world function, performance, delivery) have no implementation yet, while all effort so far went into content breadth at the
prop level. Props are not the long pole; a rebuilt prop is also something we already own as the original.

| # | Course change | Why (evidence) | Expected gain |
|---|---|---|---|
| 1 | **Steel thread first.** Build the thinnest end-to-end path now: brief, terrain (existing nodes), scatter of CC0 assets, materials and light, Blender frame, glTF/USD export, engine import, scorecard v0. Deepen the weakest stage each round. | 5 of 6 scorecard families are empty; the probe says appearance is the cheapest lift | first AAA-style frame and a live scorecard within the next work block, instead of after Phase 1 |
| 2 | **Godot (MIT, headless, no account) is the engine referee now;** Unreal later. | No Unreal is installed (only the Epic launcher); Unreal needs an Epic login and unverified headless interfaces. Godot imports glTF, bakes navigation meshes, runs bots, and exports from the command line. | delivery, world-function and performance families measurable in days, zero licence risk |
| 3 | **Level 1 becomes "learn generators", not "rebuild assets".** One parametric generator per family, fitted to several references (the chair is the bench at half the width: one recipe, one number changed). Consolidate the 24 solved recipes into families with `asset()`, then test on held-out family members. | recipes already repeat: mirrored legs, step and shelf arrays, posts and rails; per-model rebuilding costs one AI turn each | turns per new model within a family from about 1 to well under 0.2 (to be measured); unlimited variants for world dressing, which also fixes repetition. Families are built on morphology knobs, not real species, so styles can leave the real world (B16a) |
| 4 | **Worlds use CC0 assets directly** (Poly Haven scans and plants, Kenney kits); recipes add variation and control. | Level 1 is not a prerequisite for a world | world work starts now, not after 200 rebuilt props |
| 5 | **W1 photoreal valley follows Level 2 steps 1-2 immediately, ahead of Phase 1.** | it is the first visible proof of the north star and it exposes the real gaps in order | a hero image for the owner early; gap list ranked by the scorecard |
| 6 | **Autonomous judges in every gate now:** largest uncovered patch (built), per-part recall (**built 2026-10-10: `pt judge parts`**), and the vision judge (**built: `pt judge frames`, in the scorecard**; the author model's 0.84 on the critique set does not transfer to a local 8B: 0.69, see B14). | the plan's score passed a shelf with no shelves and a ladder with half its rungs; I caught them by eye, which does not scale | no human eye per asset; the same judges are required for rung A2 anyway |
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

### B16a. Generators are built on morphology, not species (decision, 2026-10-10)

**Finding.** The valley is dressed with real-world CC0 assets, so a "sci-fi village" brief would get Earth trees: the style object (palette, materials,
light) reaches the render, but nothing yet changes the *shape* of the vegetation or props. A generator named after real species (oak, pine, birch) would
repeat the flaw.

**Course change.**
1. **Generator families are parameterised by morphology and surface.** Trunk curve and taper, branching rule and angle, canopy form (cone, disc,
   hanging, crystalline, none), proportions, material, palette, emissive use. Real species are named presets, i.e. points in the knob space; an alien
   tree is another point, outside the real-species region. The same applies to ground cover, rocks, buildings and props.
2. **Shape-language knobs in the style file (B16) are consumed by the generators,** so the style chosen from the brief changes what is generated, not
   only how it is lit and coloured. The shape-language measures (angularity, taper, silhouette statistics) are the link.
3. **Two-brief gate on the first generator family (trees).** The same system runs "sci-fi village" and "realistic valley". The vision judges (B15 row
   6) must rate the sci-fi trees as clearly not Earth-realistic, the two worlds must pass the distinctness gate against each other, and the
   per-brief repetition measure must hold across seeds. The judges are therefore specified with two briefs from the first build, not retrofitted.
4. **CC0 scans stay the answer key and the first-world dressing** (B15a); `blend_materials` and `copy_variants` can re-style them as a stopgap, but the
   exit route is the morphology generators.

**Effect on order of work (B15):** unchanged order (vision judges, real elevation, tree and ground-cover generators, recipe repairs, mining). Content
change: the judges take two briefs from the start, and the tree family is designed on morphology knobs with the two-brief gate as its pass condition.
