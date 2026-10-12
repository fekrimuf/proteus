# World guide (Proteus): materials, light, real terrain, delivery to an engine

Second half of the agent guide, split out because it grows with every world. The first half (the loop, the language, attributes, checking your work, matching a reference) is [AGENT_GUIDE.md](AGENT_GUIDE.md); the MCP tool `get_guide` serves both. Add a section here when a world teaches a lesson that applies to the next one; keep the story of the session in [LOG.md](LOG.md).

## Materials, light and photographic renders (Level 2)

A recipe names materials; it never builds textures. `set_material(geo, name="rock_cliff")` binds a NAME to every primitive (or to a
`group`; with `class="point"` a primitive is chosen when all its points are in the group). The name travels through every export (glTF
material name, USD `GeomSubset` binding, OBJ `usemtl`), and the Blender render binds a CC0 PBR set to it with box (triplanar)
projection, so nothing needs UVs. Paint **prototypes before `copy_to_points`**, or call `set_material` on the instanced geometry with no
group (it then reaches the prototypes too). Unpainted surfaces render with the `Cd` point colour if there is one, else a default grey.

```
render_scene(node, scene={...}, quick=false)     MCP: a photographic still, about 10 s on the GPU, plus a report
pt render-scene <recipe|file.glb> <scene.json> <out.png> [--quick] [--samples N] [--keep DIR]
pt export <recipe> out.glb                       materials and instances included (EXT_mesh_gpu_instancing above 20 000 instances)
```

Material names (corpus/materials/library.json, Poly Haven CC0, 1k; `scale_m` = metres per texture tile): `rock_boulder rock_cliff
rock_mossy grass grass_lush forest_floor dirt sand snow gravel cobblestone wood_planks bark brick_red stone_blocks stone_blocks_light
plaster plaster_smooth roof_clay roof_slate metal_plate metal_rust concrete`. An unknown name still renders (flat colour) and is listed
in the report as `unknown_materials`; give it a colour in the scene file: `"materials": {"my_ore": {"color": [0.4, 0.1, 0.5]}}`. Any
library entry can be tinted or rescaled per scene: `"materials": {"grass_lush": {"tint": [0.45, 0.7, 0.4], "scale_m": 3.0}}`.

The scene file (everything optional; typos are rejected with the fix; coordinates are metres, Y up):

```json
{
 "render":   {"samples": 64, "resolution": [1280, 720], "exposure": 0.0, "device": "auto", "smooth_angle": 35, "seed": 0},
 "lighting": {"sky": "nishita", "sun_elevation": 32, "sun_azimuth": 140, "fog_density": 0.0002, "fog_height": 150, "fog_extent": 400000,
              "fog_color": [0.75, 0.82, 0.95]},
 "cameras":  [{"name": "eye",  "eye": {"at": [-14, 12], "height": 1.7, "yaw": -35, "pitch": 3}, "lens": 28},
              {"name": "over", "location": [40, 22, 40], "look_at": [0, 1, 0], "lens": 35}],
 "ground":   {"material": "grass_lush"},
 "materials": {"grass_lush": {"tint": [0.45, 0.7, 0.4]}}
}
```

* `sun_azimuth` is the heading toward the sun in the XZ plane from +X toward +Z (degrees); `sun_elevation` is above the horizon.
  `exposure` is in stops around a calibrated sunny-day setting (0 = an 18% grey card in full sun at 45 degrees renders near mid grey).
* `eye` puts the camera at a standing height on the terrain (the base geometry) or the ground plane at `[x, z]`, never on a tree.
  No camera at all frames the whole model from a three-quarter view. With several `cameras` each frame is written as `<out>_<name>.png`.
* `ground` is an endless plane (default on, material `dirt`; `"ground": false` removes it) so the horizon is ground, not the dark lower
  half of the sky. Haze is a big box of scattering air whose density falls off exponentially with height (`fog_density` at the ground, `fog_height` = the height over which it drops to 1 e-th, `fog_extent` = box width; use a very large extent so no side of the box shows at the horizon); a world-wide volume would absorb the sun and turn the frame black.
* `sky: "hdri"` needs `"hdri": "<path under the corpus root or absolute>"`; `sky: "flat"` takes `color`.
* The same description gives the same image: pixel-identical on the CPU, within 1/255 on a handful of GPU pixels. Use `quick` while
  iterating (quarter samples, half size) and a final render once the picture is right.
* The report says `material_coverage` (share of triangles with a textured library material), `unresolved`, `unknown_materials` and any
  warnings. Aim for coverage 1.0 and empty lists before judging the picture.

### Real assets, soft material layers, styles, and the scorecard (steel thread)

```
import_asset(path="polyhaven/models/rock_09/rock_09_1k.gltf", part="rock_09", size=2.0)   # a CC0 scan with UVs, normals, AUTHORED materials
copy_variants(points, proto_a, proto_b, proto_c, weights=[3, 1, 1], rot_y_random=360)     # up to 3 prototypes, picked per point by weight
blend_materials(geo, name="terrain_blend", layers="ground_flat ground_forest ground_rock ground_snow")   # soft layers from w_<layer> point weights
pt scorecard <recipe> <scene.json> --out DIR [--quick] [--no-render] [--no-fps]          # one build measured by Blender, Godot and the glTF validator
```

* **`import_asset`** reads a file under the corpus root (every file has a ledger row; `pt corpus fetch corpus/models_manifest.json`, ids in
  `corpus/models.txt`). `height=` or `size=` (longest side) fit it in metres, `ground=1` puts it on y=0, `part=` picks one glTF node:
  Poly Haven plants and rock sets ship several variants side by side in a file (`fern_02_a..d`, `rock_moss_set_01_rock01..06`; a miss lists
  the names). Its materials keep their textures through `copy_to_points` and the exporters (the glTF writer copies the material, samplers
  and images from the source file), so a scattered scan renders and ships with its real look. Scans are heavy (the jacaranda tree is 3.9 M
  triangles): that is what the scorecard's LOD and triangle rows measure.
* **`blend_materials`** is how terrain gets soft transitions. Write one float point attribute per layer with a wrangle
  (`@w_ground_rock = smoothstep(33.0, 41.0, @slope_deg + 5.0 * noise(@P * 0.05));`), let each layer take what the earlier ones left
  (`rem = rem * (1.0 - rock)`), and the node normalises the weights and exports them as `_WEIGHTS_0/_1`; the render mixes the layers'
  textures by them. Layer names are ROLES, not library names:
* **Styles** (`styles/*.json`, set with `"style": "styles/alpine_meadow.json"` in the scene file) map roles to library materials and tints
  (`"ground_rock": {"use": "rock_cliff", "tint": [0.62, 0.68, 0.78]}`) and carry default lighting, render and ground. The scene file wins key by key.
  One recipe, many looks: change the style, not the recipe. `scenes/valley.hcs` is the worked example (analytic valley, erosion, soft layers,
  scanned trees/ferns/rocks placed by rules on the masks).
* **`pt scorecard`** writes `scorecard.json`, `scorecard.html` (and a `_fragment` for publishing) and prints each measure with pass or fail.
  Referees: Blender (material coverage, atmosphere), Godot headless and windowed (`engine/godot/referee.gd`: import errors, triangles and draw
  calls in frame, frame rate, terrain navigation-mesh coverage), the Khronos glTF validator, geometry audits (repeats, LOD chains) and the
  ledger. Paths: `PT_BLENDER`, `PT_GODOT` (console exe), `PT_GLTF_VALIDATOR`. All bars are drafts from B13.1 until calibrated.

### Delivery to an engine: LODs, baked terrain, and the referee

`pt scorecard` writes two glTFs: `world.glb` (full detail, what the Blender render sees) and `world.engine.glb` (what an engine gets).
The engine file differs in three ways, none of which needs anything in the recipe:

* **Levels of detail.** Every placed prototype above 2000 triangles becomes a chain (`pt_core::lod`): level 0 is capped at about 200 000
  triangles, then 35%, 12% and 4% of that, each with a switch-in distance (2, 5 and 12 times the object's diagonal) and a draw distance.
  Solid surfaces (trunks, rocks) are simplified with meshoptimizer, UVs and normals in the error metric; foliage cards (alpha-cut
  materials, flagged by `import_asset`) are culled by a triangle budget and the survivors enlarged so the canopy keeps its coverage.
  Each instance becomes one node per level, `inst_<i>__lod<l>`, with the range in its extras (`proteus_lod_range`, scaled by the
  instance). `pt export <recipe> out.glb --lod` writes uncapped LOD chains; the cache for decimated levels is `out/lod_cache`.
* **Baked terrain.** A layered material (`blend_materials`) has no portable form, so the scorecard bakes it with Blender into an albedo and a
  tangent-space normal texture (4096 px, through the planar UVs `blend_materials` writes) and the engine file carries those.
* **Tangents** are exported for authored and baked normal maps, so the validator reports 0 errors and 0 warnings.

The engine referee (`engine/godot/referee.gd`, Godot 4.7) turns the node ranges into visibility ranges, then reports import errors,
instances and triangles, a navigation mesh baked over the terrain and how much of the intended walkable ground (slope under 25 degrees)
it reaches, and, windowed at 1280x720 with vsync off, frame rate, draw calls, triangles in frame and video memory, plus a screenshot
(`engine_eye.png`) from the scene's first eye-height camera. Run it directly:
`Godot_..._console.exe --headless --path engine/godot --script referee.gd -- <file.glb> nav=1` or
`... --script referee.gd -- <file.glb> fps=1 eye_at=-250,-40 height=1.7 yaw=10 shot=<out.png>`.

### Real terrain, hydrology and the lessons from the Teton valley

* `dem(zoom, lat, lon, size, rows, cols, datum, far_zoom, blend, hole)` reads Terrarium PNG tiles from the corpus (US 3DEP, public domain) into a grid heightfield: +X east, +Z south, `y = elevation - datum`, absolute elevation in the point attribute `elev`. A near window can blend into a coarser zoom at its edge (`far_zoom`, `blend`), and a far ring can leave out a rectangle (`hole`) where the near window sits; when the hole's edges lie on far grid lines the two meshes meet exactly. Fetch tiles with `pt corpus fetch corpus/dem_manifest.json`; a 5 km window needs 5 x 5 tiles at zoom 14. Compute tile numbers from latitude and longitude in code, never by hand.
* `hf_flow` fills basins (priority-flood), drains every cell downhill and accumulates the catchment: point attributes `filled`, `fill_depth`, `flow_acc`, `twi`, `stream`, `lake`. Run it on a window larger than the area you dress, then copy the masks across with `attribute_transfer`. Water is a layer role (`ground_water`, library material `water`: glossy, no texture) driven by `max(lake, stream)`; keep trees and rocks out of it with `@stream < 0.12 && @lake < 0.5`.
* `pt terrain <recipe> [--out DIR] [--find-valley 450]` prints relief, slope percentiles and the hypsometric integral of the cooked mesh, writes a shaded top-down map, and lists valley-floor candidates (flat floor with steep walls on both sides). Look at the map before choosing a window: the first centre I picked was on the wrong side of the range.
* A far ring: relief finer than its cells cannot be shown, so add ridged relief with wavelengths of at least four cells, fade it to zero toward the seam, and use broader layer noise there (`ch("f1")` and friends). A forest seen from kilometres away is its canopy: paint it a dark conifer green (a role with `fallback` linear values around 0.016, 0.034, 0.015), not the forest-floor texture.
* Colour traps found the hard way: style `fallback` colours are LINEAR values (a "dark green" of 0.06, 0.11, 0.06 renders about three times too bright); a Poly Haven rock set can have a strong hue (`rock_boulder_cracked` is salmon orange), so cancel it with a per-channel `tint` (about 0.4, 0.57, 0.71 for grey) instead of only darkening it; AgX turns bright saturated colours pastel and shifts greens toward cyan, so landscape styles use `"view_transform": "Khronos PBR Neutral", "look": "None"`.
* Scene key `playable: {"size": [w, d]}` limits the engine navigation bake and its measure to the dressed window; per-km2 measures divide by the box around the placed instances, not the whole mesh.
* World scale: a node per instance and level stalls Godot's importer above roughly 100 000 nodes. The engine file is now tiled (next section).

### A world an eye-level camera can stand in (Cascade Canyon, `scenes/cascade.hcs`)

The worked example: the calm reach of Cascade Creek below the forks (Grand Teton National Park) with a carved channel and clear water,
firs on both banks, a footbridge and a hut, and Mount Owen's ridges behind. `styles/cascade_evening.json` is its look.

**Terrain in nested levels.** One grid cannot serve a camera at one metre and a skyline at ten kilometres.

```
far  = dem(zoom=12, size=[28000, 28000], rows=700, cols=700, ...)       # 40 m: the skyline
mid  = hf_fit(dem(zoom=14, size=[9600, 9600], rows=960, cols=960, ...), far, blend=400)   # 10 m: canyon walls at the survey's full detail
near = hf_fit(dem(zoom=14, size=[1600, 1600], rows=640, cols=640, ...), mid, blend=80)    # 2.5 m: the ground that is dressed
hero = hf_refine(near_with_masks, size=[240, 130], center=[80, 62.5], cell=0.5, blend=8)  # 0.5 m: where the eye-level cameras stand
ring = hf_hole(near_with_masks, size=[240, 130], center=[80, 62.5])     # the same rectangle cut out of the coarser level
```

* `hf_fit(inner, outer_full_grid, blend)` bends the inner level's edge band onto the outer grid's surface; `hf_hole` cuts the cells the
  inner level replaces; `hf_refine` makes the finer level FROM the coarser one (bicubic heights, every point attribute carried across,
  `@edge` = metres to the patch edge). The seam is exact because both sides are linear along a coarse grid line. Rectangles snap to grid lines.
* Do the coarse work first (masks, carving) and refine after: the patch inherits it. Add what a coarse grid cannot hold in a wrangle on the
  patch and fade it with `smoothstep(0.0, 8.0, @edge)`. Keep the coarse normals as `@Nc` and blend the patch's own normals back to them at the
  edge, or the shading shows the seam. A patch needs its own layered material NAME (same roles): the engine bake is per material.
* The mid and far levels use their own roles (`far_rock`, `far_forest`...), because at a kilometre rock is a face a hundred metres across.
* `pt dem-manifest <manifest> --id ID --lat .. --lon .. --zoom Z --size METRES` adds the tile list of a window (US only: it writes the USGS
  licence); then `pt corpus fetch`.

**Water.** `hf_flow(..., river_width=2.5, river_width_max=11, river_bank=3.5, river_freeboard=0.35, stream_area=150000)` gives streams a
channel: `river_sd` (metres to the channel edge, negative inside), `river_w`, `water_y` (water surface, never rising downstream) and `river`
(1 in the channel, falling across the bank). Then in the recipe: carve (`@P.y = mix(@P.y, min(@P.y, @water_y - depth), @river * gentle)`),
and build the surface from the same grid (`@P.y = @water_y`, keep `@river_sd < 7`, `set_material(name="stream_water")`); the ground hides its
edge. The library material `water_clear` (`"kind": "water"`) refracts, mirrors the sky at a grazing angle, ripples and tints with depth.

* Carve only real streams on gentle ground: a small catchment on a smooth slope drains in dead-straight diagonal lines (a known artefact of
  eight-direction drainage), so use `stream_area` of 150 000 m2 or more and multiply by a slope mask.
* `pt terrain <recipe that outputs the hf_flow grid> --out DIR --reaches 400 --reach-spacing 25` writes `water_map.png` and lists the channel's
  centre line with heights: find a reach with a fall of 1 to 2 percent before placing a camera. A creek at 10 percent is a cascade, not a mirror.
* Paint gravel only low and close to the water (`river_sd` under 2 to 3 m AND under half a metre above `water_y`); banks are grass down to the water.

**Assets at scale.** `import_asset(..., max_triangles=600000)` reduces a scan on import (cached); the firs are two million triangles each and
a forest of them did not fit in 16 GB. `lod_hint(proto, cull=4200, merge=1)` marks a far stand-in (the 20-triangle spikes of the forest
beyond the dressed window): an engine keeps it visible for kilometres, and an offline render reads all its instances as one merged mesh.
A meadow is thousands of GRASS TILES (a round 4 m patch of clumps realized into one mesh, three variants, `orient_to_normal=1`), not millions
of clumps. Dress the hero patch separately and densely (`scatter_poisson` on the patch); the rest of the window gets a tenth of that.

**The render (new scene and style keys).**

* Atmosphere in two parts. HAZE (`fog_density`, `fog_height`, `fog_base`): thin and kilometres deep, built from nested boxes of constant
  density, which Cycles integrates exactly. MIST (`mist_density`, `mist_height`, `mist_extent`, `mist_step`): dense, tens of metres deep,
  one box with the true density marched in `mist_step` metres; it separates one line of trees from the next and carries the light shafts.
  One box with a height-dependent density is marched in steps of a tenth of the box (kilometres); slabs alone show their lids as lines when
  a camera stands at a lid's height (lids now keep clear of the camera heights).
* `render`: `white_balance: [kelvin, tint]`, `glare`, `foliage_translucency` (0.35: scanned plants are opaque, and against the sun they render
  as black card without it), `transparent_bounces`, `volume_bounces`. With haze and a backlit view, AgX (`"look": "AgX - Medium High Contrast"`)
  rolls the bright sky off well; the earlier advice (Khronos PBR Neutral) holds for front-lit, saturated scenes.
* Per ROLE in a layered material: `breakup` and `breakup_m` (the soft vertex blend becomes a crisp patchwork; `breakup_by: "relief"` puts snow
  in the gullies of the rock relief), `macro` (slow brightness change over the material's `macro_m`), `relief` and `relief_m` (metres of ridged
  relief in the bump), `strata`, `saturation` (grey a scan whose hue belongs elsewhere). Heights are in metres (map x `bump_distance` x `bump`),
  so a 90 m aerial scan and a 2 m close-up scan bump correctly side by side.
* `pt render-scene ... --only hero,valley` renders some cameras. The imported scene is cached (`out/blend_cache/<stem>.<hash>.blend`): the first
  render of a model imports for minutes, later ones open in seconds, so look development is one render per try.
* Look at the scans before using them (`corpus` jpgs): `sparse_grass` is red soil, `rocky_terrain_02` is green meadow with stones,
  `aerial_rocks_02` is grey slab with moss, `rocky_trail` is orange, `forrest_ground_01` is green with twigs, `boulder_01` and the Namaqualand
  rocks are orange-pink. Two test renders were spent on orange slopes that one look at the files would have prevented.

**The engine file (tiled).** `pt scorecard` writes `world.engine.glb` with `to_glb_tiled`. What is in it and why:

* **Instanced tiles.** Instances are grouped by ground tile and prototype: ONE node per group and level with `EXT_mesh_gpu_instancing`,
  all under one parent node per prototype (`proto_<p>`). Near tiles (`tile_<p>_<tx>_<tz>__lod<l>`, 32 to 128 m, about twice the prototype's
  size; 32 m for ground cover) carry every level but the last; far tiles (`hlod_...`, 4 x 4 near tiles) carry the last level, and a near
  tile names its far tile (`proteus_hlod_parent`), so the two hand over without a gap or a double draw. Distance ranges are pushed out by half
  the batch's radius (`RANGE_PUSH`).
* **Levels sized for a forest** (`LodPolicy::engine()`): level 0 capped at 24 000 triangles for anything 12 m or taller and in proportion to
  height below that, then 20% and 4%, switching at 1.3, 3.5 and 8 diagonals. The last level of a PLANT is a shell (`pt_core::lod::shell`:
  80 triangles following the foliage's outline, one colour). GROUND COVER (under 2 m: grass patches, ferns, cobbles) is capped at 6 000,
  has three levels, is drawn to 90 m, casts no shadows, and its thinned cards may grow by a quarter at most (grass thinned to a seventh came
  out three times as tall when it grew like a canopy does).
* **The base mesh per material.** `geometry__<material>` nodes (terrain levels, water, the hut...), so an engine culls and shades them
  separately; a node wider than 2.5 km is marked `proteus_distant`.
* **Materials.** Layered terrain is baked to JPEG textures per material. A library scan on procedural geometry (the hut's log wall) is
  written as a tiled texture with box-projected UVs; a style's plain colour (water, a far stand-in) is written as that colour and marked
  `proteus_flat`. The exporter repairs zero normals (a merge zero-fills `N` for parts that had none) and computes tangents from the repaired ones.
* **The Godot bridge** (`engine/godot/referee.gd`) builds the MultiMeshes itself (Godot 4.7 imports an instanced node as a single mesh),
  finds nodes by NAME (the importer's node ids are stale after it swaps its mesh nodes), sets visibility ranges and visibility parents, turns
  shadow casting off for ground cover and terrain, and uses shadows to 150 m in two cascades. The navigation mesh is baked in 400 m tiles
  (one bake holds about 65 000 vertices and silently returns nothing above that), each with a filter box only as tall as its own ground
  (heights are 16-bit steps above the box floor). `playable.materials` in the scene names the ground's materials for the bake.
* The validator holds about ten times the file in memory: 377 MB validates in 4 seconds, 800 MB did not finish in 15 minutes on 16 GB. It
  has a time limit (`PT_VALIDATOR_SECONDS`, default 900).

Measured on the way (creek world, eye-level camera, RTX 2070 SUPER, 1280x720): a node per instance never finished importing; tiles under one
parent took 10 minutes (removing a child of a parent with 60 000 children is linear); grouped tiles import in 8 seconds. Triangles in one
frame went 160 million, 64, 49, 30, 21 as the levels, the ground-cover class and the terrain split went in.

## Placement that cannot float or drown (2026-10-12, from the east frame of the creek world)

Two defects were seen in `frame_east.png` after the world was called good: moss caps hovering over the water and flat stones lying in it. `pt ground-check` measured them: 1805 stones mostly under water and 1613 objects hanging over a slope or bank across the world (272 and 40 in the east view), and the yellow boxes of its annotated frame sat exactly on the flat stones. Two causes, two permanent rules:

* **Hanging objects:** a point is on the ground, but a 2 m rock or a 5 m grass tile is not a point, so on a slope or a cut bank one side hangs in air. `drop_to_surface footprint=R` settles each point onto the lowest ground under a ring of R x pscale metres and writes `settle_drop`; the scene rejects points that would need burying deeper than a limit. `import_asset sink=F` buries a fraction of an object's height. The scene's `settle(points, footprint, limit)` applies both to rocks, logs, bushes, ferns, weeds and grass tiles. Result: 1613 hanging objects fell to 2.
* **Drowned stones:** the creek bed lies 0.5 to 0.85 m under the water, so a stone shorter than that shows only a flat lid (or none, and reads as a flat patch through the refracting water). Stones are placed in the water only where the water depth is under 4 to 20% of their scale (`@water_y - @P.y < k * @pscale`). Result: 1805 fell to 6.
* **Guard:** `pt scorecard` has two rows (objects floating or hanging in a camera view: bar 0; stones mostly under water in a view: bar 10), and CLAUDE.md requires a frame to be reviewed with the annotated check and the zoom tiles before it is called good.

## The look-development tier (2026-10-13)

A creek-sized world does not fit a 16 GB machine as one Cycles scene, and a look decision should not wait ten minutes. Recipes therefore carry two values for what costs memory: `max_triangles=lookdev(0, 120000)` is 0 (no cap) in a final render and 120 000 in look development; `grass_tile(71, lookdev(760, 260), ...)` is 760 clumps or 260. Choose the light value so that placement stays identical (the tier changes assets and counts of clumps inside a tile, never the scatter seeds or rules), so a frame from the tier is the same picture with less detail. `pt render-scene ... --only east --lookdev --quick` renders one camera of the tier and crops the world to its view; a second render that changes only light, a style, a material script or the scene file reuses the cooked world and Blender's import and starts in about a minute. Final frames (no `--lookdev`) still need the full world; getting those through the memory limit is what view-dependent levels of detail are for (B18 item 4, next).

## Black cards from plants with a cut-out in the picture (2026-10-12, from the east frame)

Dark pointed shards on the creek's right bank were the seed heads of the tall `grass_medium_01` variants: the scan keeps its cut-out in the black background of a JPEG colour picture (a JPEG has no alpha channel, yet the asset says alpha-blended), so every empty part of a card rendered as solid black, and under backlight as pale flakes once `foliage_translucency` coloured them. `pt pick` named the node and prototype in one call; a ten-second lab world (one meadow patch, sun in front and behind) reproduced it. The fix is in the render material setup, so it holds for every plant of this kind: `key_black_background` in `corpus/tools/render_scene.py` takes the Alpha from the picture's brightness (black = empty) whenever it comes from a JPEG; the report lists `keyed_black_background` per material. The same lab recipe pattern (a small world, two sun directions, one variable changed) is the cheap way to settle any material question before a creek-sized render.
