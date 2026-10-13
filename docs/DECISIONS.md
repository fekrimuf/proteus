# Decisions (design choices that stay fixed until new evidence changes them)

Section numbers (B11, B15, B15a, B16, B16a, B17, B18, B19, B20, B21, B22) are kept from the plan so references in code comments and the log still resolve. The plan: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md). Dated results: [LOG.md](LOG.md).

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

*Rows 6 and 9 are superseded by B17 (2026-10-12).*

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

## B17. The owner's eye is the quality gate; machine judges and the scorecard are on demand (decision, 2026-10-12)

**Evidence.** The creek world the owner loved on 2026-10-11 was made from real elevation, scanned CC0 assets and a Cycles render, compared by eye with the reference picture; no judge score and no scorecard took part in making it. The vision judge scored the Teton valley 4.7 to 6.0 of 10 while the owner judged it far from the target, so its number did not track the owner's taste. The scorecard's terrain bake held about 10 GB and made the owner's PC lag for a period, even after the session ended. Where the machinery did earn its place: the scorecard found the engine failures (2.1 fps to 93 to 173 fps, 1.42 billion to 18.5 million triangles in frame) and the part judge found three defects in rebuilt props that the eye missed.

**Course change (supersedes B15 rows 6 and 9).**
1. How a world looks is judged by the owner's eye: side-by-side frames with the reference, and blind pairs. Nothing blocks on a judge score.
2. The scorecard becomes an on-demand delivery check (validator, navigation, frame rate, repetition counts), run at milestones. The terrain bake (`--bake`) and the vision judge (`--judge`) are opt-in because they take the machine.
3. Asset rebuilding for its own sake (Phase 1 mining on 150 more assets, the fresh 50, the part judge as a gate) leaves the critical path: worlds use CC0 scans directly and generators supply variety and uniqueness.
4. The photo-distance judge (FD_DINOv2, gate W2) and the 7 of 10 judge bar are retired as gates. Light automatic checks return at rungs A3 and A4, when the owner cannot look at every world.
5. Every heavy child program starts at low priority, under a memory cap, and is killed when `pt` exits (`pt_cli::lowload`; `PT_HEAVY_MEM_GB`, `PT_HEAVY_THREADS`). A job that would swap the machine fails by itself instead.

**Effect:** about three weeks off the timeline (B12.6) and no GPU-heavy step in the normal loop.

## B18. The capability track: nine agent capabilities, in this order (decision, 2026-10-12)

**Evidence.** Reading the engine as the agent that drives it, five things cost the most turns: the language cannot ask the world a question (a wrangle sees only its own point, so each placement rule became a special node: `settle`, the water-depth rules); nothing links a pixel of a frame to the instance and rule that made it (`explain` needs an index); scatter writes only position, direction and size, so a changed count or seed reshuffles every instance (the owner's approved placements do not survive a rebuild); the rigid-body solver knows only a flat plane; a look-development render pays a whole-world import. The big-name Houdini families (RBD/Vellum dressing, USD layers, Copernicus-style materials, ecology, building grammars) were parked as "later" although an engine ships their baked results, not live simulations.

**Order (each item is finished and verified before the next; at most two open at once). Targets are plan dates from the measured pace of one gated step a day; ROUTE.md re-sets them from the faster pace the history shows.**

| # | Capability (Houdini analogue) | Size | Why here | Target |
|---|---|---|---|---|
| 1 | **Geometry queries in the language**: nearest point/surface, neighbour count and sums, ray hits, attribute lookups on a second input (VEX queries) | S-M | foundation: every later rule is a one-liner on it; replaces the hand-made `settle` and water-depth nodes | 2026-10-13 |
| 2 | **Pixel pick**: a frame pixel to prototype, instance, creating node and rule, height over ground (viewport pick, spreadsheet) | S | the owner's note on a frame becomes a fix at the rule in one query | 2026-10-14 |
| 3 | **Simulation as set dressing**: rigid bodies settle on the real terrain, cloth drapes, ropes hang; resting poses saved as instances (RBD, Vellum, grains) | S-M | solvers exist; first big-name result visible in a frame; floating stones fixed by physics | 2026-10-16 |
| 4 | **Fast pictures**: cook and render only a region, keep one render session open and send only what changed (IPR, Hydra delegate) | M | every later item is judged by renders; the uncommitted `--cull` is the first slice | 2026-10-19 |
| 5 | **Stable identity and overrides**: every instance gets an id from its rule and place; keep, move, remove and add edits are stored against it and replayed on rebuild (stable `id`, Edit, Takes) | M | an approved world must survive a rebuild | 2026-10-21 |
| 6 | **Layered world file**: base layer, style variant, owner-overrides layer, exported to USD (Solaris, USD layers and variants) | M | one recipe many looks; each studio owns its layer; needs 5 | 2026-10-24 |
| 7 | **Nature that grows**: seeds compete for light, water and slope, grow and die; size-aware spacing; the tree and ground-cover family on morphology knobs (Solver loops, Labs Biome) | M-L | closes repetition and the CC0 share; the stage 2 gate; uses 1, 3, 5 | 2026-11-03 |
| 8 | **Procedural materials**: texture graph in the same language, masks from curvature and occlusion, fitted to a reference photo by the optimizer, baked to engine textures (Copernicus) | M-L | the CC0 exit and the studio-unique surface; stage 3 | 2026-11-10 |
| 9 | **Buildings and layouts from grammars**: split, repeat and component rules, roads, lots, roofs, terrain pads and curve carving (Labs Building Generator, PolyPath) | L | the "medieval village in a sci-fi sense" brief needs it; uses 1, 4, 6, 8 | 2026-11-24 |

**Reasons for the order.** 1 to 3 are small and make everything after them cheaper and visible in frames. 4 to 6 protect the owner's approved work and speed every iteration. 7 to 9 are the biggest and use everything before them. Dependencies: 5 before 6 (overrides are keyed by identity), 1 before 7 and 9, 4 before 7 to 9 (they need many renders), 8 after 6 so a style is a layer.

**Parked behind, route ready** (they stay in PLAN.md): water bake (a shallow-water pass on the heightfield for flow and foam maps, Blender's fluid simulator for hero splashes), smoke and fire (Blender's solvers driven from the recipe, baked to flipbooks), life (herds, flocks and fish from the particle solver, CC0 animated models), tiled resumable world builds (`pt top`), cliffs with overhangs and caves from signed-distance terrain (`sparse_boolean`). Rule for all: published methods and open-source code only, Houdini is never run (B11); Blender is the heavy simulator and is called as a separate program, results cached by content hash.

**Effect on B12.6.** Stage 1 keeps its date. Stage 2's building kit moves to item 9 (it pulls the settlement work of stage 4 forward; stage 4 keeps its date); stage 2's gate is trees and ground cover. Stage 3 is item 8.

## B19. One command list, one gate, no crash-prone shortcuts (decision, 2026-10-13)

**Evidence.** Read against an open-source After Effects clone written in Rust by several agents at once (effectcraft), this repository showed six gaps that cost agent turns and risked the session. Tools were wired by hand in two big files, so three review commands (`ground-check`, `review-tiles`, `scorecard`) were not in the agent's tool list and two agents adding a command or a recipe met in the same lines. A mistyped tool argument was silently ignored. A panic inside a tool would end the live session. The ban on platform-dependent maths in `clippy.toml` was written down but never run, and the first run found 61 breaches in production code, among them the new tree and texture generators (the conifer grove is the one golden hash of 29 that changed when its maths was made platform-independent). There were about 70 crash-prone sites in production code, one of them missed by a text counter and found by clippy. No single command said whether a change could be committed.

**Course change.**
1. **One command list.** Commands that work on files are declared once, beside their code, in `crates/pt-cli/src/registry.rs`. The command line, the agent's tool list and `docs/generated/commands.md` all read it; a registered command runs as a child `pt` process for the agent, so what it prints or a crash in it cannot reach the live session. New commands go there. The recipe list in the tool server is generated by `crates/pt-cli/build.rs`, so adding a recipe edits no list.
2. **Strict arguments.** Every tool call is checked against its schema first (unknown name, missing required, wrong type) and refused with what is accepted; so is a mistyped command-line option.
3. **A crash is an error.** A panic in a tool becomes an error that names the tool and restores the scene; the stdio loop answers a malformed line; a poisoned lock does not end the session.
4. **No crash-prone shortcuts in production code.** `[workspace.lints.clippy]` in the root `Cargo.toml` denies `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` and `unimplemented!` in every crate; tests may use them (`clippy.toml`); a line that truly cannot fail carries `#[allow(clippy::...)]` with the reason. The production code is at zero.
5. **One gate.** `pt gate` (alias `cargo gate`) runs build, lint (the maths rule of D2 and the ban above), `pt docs-check` and the tests; CI runs the same lint step. Every commit passes it.
6. **Every defect fix ships with a test that fails without it**, written against the class (the placement rule, not the one stone), through the agent tools where an agent can see the defect. `crates/pt-cli/tests/agent_tools.rs` holds the floating, hanging and drowned classes, and shows the check fails where it should.
7. **A fresh agent needs no setup.** `.mcp.json` is checked in; CLAUDE.md carries the commands to run and look at the product.

8. **Three work lanes side by side** (ROUTE.md, "Lanes"): approved by the owner on 2026-10-13; each lane works in its own git worktree with its own build folder, merges only after `pt gate` passes, and the lead alone edits STATUS and LOG. Not started; they begin on the owner's word.

**Effect on the plan.** The dates of B12.6 and B18 were set at one gated step per working day; the history shows capabilities 1 to 6 in about two hours of commits. [ROUTE.md](ROUTE.md) re-sets the dates from that pace and ranks all open work with its cost; the stage gates and the north star are unchanged.

## B20. The blind pairs wait for views the owner has not seen; heavy jobs need no permission (decision, 2026-10-13)

**Evidence.** The owner has already seen our creek renders, so a pair built from those views is not blind: the composition and the faults they know give ours away. The owner also asked that renders never wait for permission, because an autonomous system cannot stop to ask.

**Course change.**
1. The stage 1 blind pairs (B12.6, protocol in ROUTE.md) are delayed until there are views the owner has not seen: new cameras, or a world that has changed a lot. The earliest such point is after rows 3 to 5 of ROUTE.md (about 2026-10-22), where the pairs also serve the stage 2 gate. Meanwhile stage 1 is judged the way B17 says: owner-quality frames (rows 1 and 2) that the owner compares with the reference by eye.
2. Renders, the terrain bake, the vision judge and every other heavy job run without asking and without a warning first. The protections are in code (`lowload`: low priority, a memory cap, killed with the process); one heavy job runs at a time; the report says which ran; none is left running at wrap-up; if the owner reports lag, mine are stopped first.

## B21. Reference images: how they are found and what qualifies (decision for later, 2026-10-13)

Not built and not scheduled: ROUTE.md row 14. The decision is recorded now so the later build starts from the owner's rules.

**Evidence (tested 2026-10-13).** The web search tool finds pages, not pictures (a query for a medieval watermill returned museum prints and a toy design). The tool option `provider: "searxng"` with `categories: "images"` does not exist here, and public SearXNG servers answered a captcha, rate limits or nothing; running one needs Docker or Python, which this machine lacks. Two free sources work with no key: Openverse (direct image link, creator, source page and licence per image) and the Wikimedia Commons API (image links and sizes; the categories "Featured pictures" and "Quality images" return 4 000 to 6 500 pixel photographs). A downloaded 1024 pixel photo of a timber mill with a wheel beside a waterfall was viewed through the file reader, so the whole loop (search, direct file, look) works. Openverse records carry no marker for AI-made images.

**Rules.**
1. **What a reference is for:** looking, and measuring (palette, shape and layout statistics), never copying: B11 stays, "match statistics, not copies". A reference picture is never committed or shipped. It lives under `out/refs/` (git-ignored) with a note beside it: source page, creator, licence, size, tier and why it was chosen.
2. **No licence filter on reference search** (owner, 2026-10-13: the picture is not sold, it is only a reference). Anything that enters the asset library or the corpus keeps the CC0 and ledger rules of B15a; the CC-BY question of 2026-11-01 is unchanged.
3. **Only high-quality references qualify:** AAA-grade game environments (official screenshots and art), high-end photography of real places, credited concept art by established artists, and anything the owner names; the owner's own reference picture is always accepted. A tier is recorded for each; the style tools and the art bible compiler (ROUTE rows 6 and stage 5) take only references above the lowest tier with a recorded provenance.
4. **Refused, with the reason recorded:** AI-generated images; low-budget game screenshots and asset-flip art (a source that is not on the quality list); low resolution (long side under 1920 pixels); watermarked previews; pictures with no human creator or source page. The system never builds a world from a refused reference.
5. **How AI-made images are caught** (Openverse gives no flag): the content-credentials marker where a file carries one, tags and titles such as "ai generated", "midjourney", "stable diffusion" and "dall-e", a block list of sources that publish mostly AI output, a missing human creator, and the agent's own look at the picture before it is accepted (CLAUDE.md rule 7).
6. **Where quality comes from:** for natural scenes the Commons "Featured pictures" and "Quality images" categories and Openverse's large-photograph filter; there is no free interface for AAA game art, so those references come from the owner, or from official press pages the agent finds by search and adds by direct image address through the same gate.

## B22. The game engine finds the flaws; the offline render makes the frames the owner judges (approach change, 2026-10-13)

**Evidence.** Looking was the slow step. One offline frame of the creek world costs 31 seconds at draft quality and about ten minutes at the owner's quality, it shows one side of the world, and every attempt to make it faster made it worse; the owner found floating firs and rigid dressing that a one-angle review had passed. Measured on the same world in the game engine (Godot, 2026-10-13): the world is up in 10 seconds, the engine peaks at 3.5 GB of memory under the 8 GB cap and draws 125 frames a second, and one launch loaded the world and wrote 123 pictures in 17 seconds. A whole survey of a changed world (cook, engine file, 90 pictures, contact sheets) took 67 seconds, 35 of them the cook. Blender opened by hand on the full world froze the owner's PC for about an hour, because nothing limits it there.

**Course change.**
1. **Two jobs, two tools.** Flaws of placement, shape, scale, gaps and repeats are found in the engine, from many sides (`pt survey`). Light, air and surfaces are judged in offline frames, and only those go to the owner for the side-by-side look (B17 stays: the owner's eye is the gate).
2. **Survey first.** A world is surveyed before any offline frame of it is rendered and again after every change to placement. A world whose survey shows a floating or hanging object is fixed before a frame of it is rendered.
3. **Flaws choose the cameras.** Every object the ground check flags gets a close-up pair from the two sides with the clearest sight, at most three of one class, because the fix goes to the class (the rule), not the object.
4. **The owner opens a world with `pt walk` and nothing else.** It runs under the protections of B19 item 5 and B20 (low priority, the memory cap, stopped with `pt`) and a time limit on the load. Blender is never opened by hand on a full world.
5. **Every engine launch has a time limit** (`PT_GODOT_SECONDS`, default 600): a stalled import is stopped with the engine it started and reported as such.
6. **One camera everywhere.** A survey view, `pt pick`, the boxes drawn on pictures and the engine put the same point on the same pixel (a test through the agent tools holds them together), so any pixel of any survey picture can be asked what it is.

**What it replaces.** The "live render session" half of ROUTE.md row 1 (send Blender only what moved) is dropped: its purpose was fast looks, and the engine gives them. Row 1 keeps "the full-detail world fits this machine" for the owner's frames. The engine timeout of row 9 is done.
