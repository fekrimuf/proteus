# Technique notes from third-party tutorials

Allowed spec source under PLAN §12 D1 (technique only: no videos, project files or assets are stored in the repo; transcripts live in
`D:\Fekri\corpus\tutorials\`, outside the repo). Nobody runs Houdini. Each note: what the technique is, what we already have, what is missing.

## Source 1: "Environments in Houdini, Full Course FREE" (YouTube playlist PL2V35R-U_sjOT1l66y7zulIw9Qe7twt5F, 21 videos, about 11 hours)

**Level of the source:** a beginner generalist course that builds one small stylised environment (house, tower, podium, crate, rocks, pavers, grass)
and takes it through layout, lighting, materials, cameras, compositing and multi-shot variants. It is a workflow teacher, not an example of AAA
realism, so we take its production habits and take the realism techniques from other sources (real elevation data, scans, B12).
Transcripts fetched with yt-dlp (subtitles only): lessons 01-04 and 06-20 present; lesson 05 (crate) was rate-limited and is retried.

| # | Technique in the course | What we have | Gap and action |
|---|---|---|---|
| 1 | **Two modes, chosen per element:** art-directed (hand placed, hand modelled) vs fully procedural; hero items directed, bulk items procedural; changing one house must not require redoing the scene | graph with cached cook and early cutoff; recipes with `asset()` | make the split explicit in the world compiler: every world element is tagged `directed` or `procedural`, directed ones are agent-edited, procedural ones regenerate from rules |
| 2 | **Scatter recipe:** scatter points on the ground, delete points inside exclusion volumes (near buildings, towers, other props) with a noise-broken edge, set orientation attributes, randomise, then copy | scatter and instance nodes, `copy_variants`, 5M-instance bench | add an exclusion-volume step with noisy edge to the scatter family and test it with the world-function checks (nothing intersecting buildings) |
| 3 | **Orientation contract:** one normal says which way the prop's up axis points, a separate up-vector sets rotation around it, so props stay on the ground but turn randomly; pavers lie flat, rocks tumble freely | `N`/`up` style attributes on points (to confirm in `pt-sop`) | write the contract in the scatter guide; add a per-prop `align: ground\|free` flag |
| 4 | **Scale by rule:** `pscale` driven by a ramp of distance, height or noise (large far from the centre, small near it), plus randomise | attributes and VEX | expose as a scatter option so density and size follow the style file (B16) |
| 5 | **Materials by name with wildcards:** primitives carry names (`tower_stone_1`..), one wildcard assigns a material to every tower; separate assignment nodes for environment, props, building so one can be restyled alone | `set_material` names, material-carrying glTF/USD | adopt the naming rule and wildcard assignment in the world compiler: the style swap in B16 changes one assignment node per class |
| 6 | **Blended materials from a stored rest position:** noise read from the unchanged position blends two materials (clean/worn, stone/moss), with extra variation controls | `blend_materials` with planar UVs | check it uses a rest position so blends do not swim under deformation and LOD; add variation knobs |
| 7 | **One scene, many shots:** a second camera or a shake variant on the same scene; day and night lighting set-ups switched by one control | scene files with cameras, sky, sun | add shot variants and lighting presets (day, dusk, night) to the scene file; the scorecard renders all of them |
| 8 | **Preview budget trick:** small fixed render window while working, final resolution only at the end | `--quick` render | already equivalent |
| 9 | **Compositing pass** (lesson 19): render passes combined and graded afterwards | Blender frame only | needed for the finished-frame level; take the pass list from lesson 19 into the Level 4 plan (not yet read in full) |

**Decisions from this source**
1. Items 1-4 and 7 become work items inside the tree and ground-cover generator step (B15 step 3) and the world compiler. Item 2 (exclusion
   volumes with noisy edges) is the biggest visible gain for scatter, because it ends props growing through walls.
2. Items 5-6 are small and go into the style swap (B16a), so a style change restyles one class at a time.
3. This course does not move realism. The realism route stays: real elevation data (W1), scans and procedural materials, vision judges.

## Next sources to mine
Environment or terrain tutorials aimed at game or film quality (erosion layering, flow maps, biome masks, hero versus background budgets, set-dressing
rules). Every one is read as text; the technique goes into this file, never the content.

## Source 2: web research on how game environments are built in Houdini (2026-10-10)

Sources: SideFX heightfield docs (21.0 and 22.0 Copernicus), SideFX Labs and Realtime FX pages, the SideFX "Procedural Desert" tutorial, 80.lv production
write-ups (procedural forest with UE4), the Guerrilla GDC 2017 talk "GPU-Based Run-Time Procedural Placement in Horizon Zero Dawn" (slides on
guerrilla-games.com). Several are UE4-era; no AAA studio talk on Houdini specifically was found beyond Guerrilla's placement system (that one is
their own engine, not Houdini). Studio-specific claims for Ghost of Tsushima and Far Cry were not found and are not used.

**The common pipeline (consistent across sources):** (1) terrain as a heightfield with named layers (height, mask, flow, debris), (2) cascaded erosion,
big features first then finer passes, each node taking a mask input so it acts only where wanted, (3) derive masks from the terrain (slope,
curvature, height, flow, occlusion), (4) scatter driven by those masks, hierarchical so large items (trees) are placed before small ones (rocks, grass)
and never overlap, with variant attributes choosing the species or rock shape, (5) bake textures and masks for the engine (flow maps, splat/weight
maps, vertex animation textures for wind), (6) tile the world and cook tiles in parallel (PDG), (7) package as an HDA for the engine, or hand the
masks to the engine's own placement system. Guerrilla's version moves step 4 into the engine at run time: placement rules are data, deterministic and
locally stable (moving a mountain does not re-dress the world), and read "world data maps" (humidity, temperature, distance to river).

**Gap list against our 79 nodes** (our terrain is a grid with point attributes: `hf_slope`, `hf_erode_thermal`, `hf_erode_hydraulic` which writes
`erosion`, `deposit`, `water`, `flow`, and `hf_terrace`; plus `scatter`, `scatter_poisson`, `copy_to_points`, `copy_variants`, `attribute_transfer`,
`color_by_height`, the SDF family, LOD chains and the Blender bake):

| # | Missing piece | Why it matters | Route |
|---|---|---|---|
| 1 | **Mask layers on terrain:** a mask input on every terrain op, and mask operators (curvature, occlusion, height band, blur, expand, combine, remap) | masks are the control language of every source above; they drive erosion, scatter, materials and biomes | extend the grid heightfield with named layers; add `hf_curvature`, `hf_occlusion`, `hf_mask_*`; give `hf_*` nodes an optional mask input. Biggest lever, build first |
| 2 | **Hierarchical scatter with variants:** place by size class, forbid overlap across classes, choose variant by mask or attribute, exclusion volumes with noisy edges | ends floating and intersecting props; the course (TECHNIQUE_NOTES source 1) and Horizon both rely on it | one `scatter_layers` node that takes ordered classes, radii, mask rules and variant tables, on top of `scatter_poisson` |
| 3 | **Biome rules:** a rule table from masks and world data (height, slope, wetness from `flow`, distance to water) to asset classes and densities | this is where style enters the layout (B16: layout grammar) | a `biome_rules` node whose table is part of the style file |
| 4 | **Linear features:** roads, rivers, paths carved into terrain along a curve, with banks and blended edges | every game world has them; curves exist, carving does not | `curve_to_terrain` (carve, bank, mask output) |
| 5 | **Engine texture outputs:** splat/weight maps, flow maps, height and normal tiles, vertex-animation or wind attributes for foliage | the engine needs textures, not only meshes; the Blender bake covers layered materials but not masks as deliverables | `export_maps` (EXR/PNG layers) plus wind attributes in the glTF extras |
| 6 | **Tiled worlds:** cook and export in tiles with matching edges, stitched LOD between tiles | 4 km valleys cannot be one cook; the TOP farm exists, tile logic does not | `tile_world` using `pt top`; edge-matched erosion is the hard part, plan overlap-and-trim |
| 7 | **Modular building kits and layout:** wave-function-collapse or grammar-driven building and street generator, trim-sheet UVs, material slots per part | W4 settlement; AAA towns are kit-built | `wfc_solve` and a grammar node; kits come from the generator families (B16a) |
| 8 | **Foliage specifics:** branching tree generator, impostor/billboard bake for far LOD, wind attributes | step 3 of the plan; our LOD has card culling but no impostor bake | part of the tree family and an `impostor_bake` node |
| 9 | **Run-time placement (Horizon style):** ship the rules, not the instances | cuts world size and allows streaming | later; our deterministic cook and rule tables are already the right shape for it |
| 10 | **Small shape ops:** polybevel, convex hull, lattice (already listed), plus a general `boolean` on meshes beyond sparse SDF | detail on hard-surface props | follow `crates/pt-sop/src/deform.rs` |

**Answer to the question:** yes, the software is missing nodes that every game-environment pipeline uses; the missing ones are items 1 to 6 and
they are all in the terrain-and-placement layer, not in the core. Core strengths already match or exceed the sources: deterministic cook with
cache, 5M instances in under a second, hydraulic erosion that already writes the flow layer, LOD chains, engine delivery.

**Approach change (placed in the order of work):** a "terrain and placement toolkit" step (items 1 to 3, then 4) goes between real elevation data
(step 2) and the tree/ground-cover generators (step 3), because the generators need mask-driven scatter to look right and the elevation data needs
masks to be dressed. Items 5 and 6 follow with W1; 7 to 9 follow with W4.
