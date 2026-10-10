# Agent guide (Proteus)

You are driving a procedural 3D engine. Units are **meters**, **+Y is up**. Scenes are node
graphs; you write them as short programs in `hcs` (Python syntax) and the engine checks,
cooks and renders them deterministically. Same program + same seeds = bit-identical result.

## The loop

1. `run_hcs` with a complete program (it replaces the scene; a failed run changes nothing).
2. Read the diagnostics. Every error has a line, a message and a **fix**.
3. `render_preview` (try `sheet=true`, `mode="height"` or an attribute name) and LOOK at it.
4. `validate` / `spec(...)` lines are your definition of done. Add specs for size, grounding,
   counts and watertightness *before* you start tuning.
5. Fix what you see, re-run. Use `checkpoint`/`restore`/`undo` to try risky ideas.
6. `export` (glTF/OBJ) when the specs pass and the render looks right.

## hcs in 60 seconds

```python
ground = grid(size=[40, 40], rows=80, cols=80)          # node types are functions
hills  = noise(ground, amp=3, freq=0.08, octaves=4)     # positional args = input nodes
seeds  = scatter_poisson(hills, count=300, min_dist=1.2, seed=3)  # kwargs = parameters
rock   = sphere(radius=0.4)
rocks  = copy_to_points(rock, seeds, orient_to_normal=1, rot_y_random=360)
scene  = merge(hills, rocks)
spec(scene, "width", min=39, max=41)                    # checked after cooking
out(scene)                                              # marks the result
```

* Variable names become node ids. `id="name"` sets one explicitly.
* Python features: `def`, `for ... in range()`, list comprehensions, `if/else`, lists, dicts,
  `+ - * / // % **`, `min max abs sum round int float sin cos sqrt floor ceil lerp`.
  List math is elementwise: `[1, 2, 3] * 2`.
* Parameters may be expressions as strings: `"=2*$F"`, `"=ch(\"box1/size/0\") * 2"`
  (`node/param[/index]`). `$F` is the frame.
* `asset("name", output=node, inputs=[...], promote={"height": [shaft, "height"]})` packages
  a subgraph; use it with `hda(asset="name", values={"height": 5})`. `repeat` loops an asset.
* `spec(node, kind, min=, max=, name=)`; kinds: `height width depth points prims instances
  watertight finite grounded attribute`.

## Attributes, wrangles and the attribute-flow checker

Geometry has point / primitive / detail attributes (`P`, `N`, `Cd`, `pscale`, ...) and lazy
**instance tables** (`copy_to_points`, `array`, `radial_array`). Instances are not copied until
export; use `realize` only if you need real geometry.

`wrangle` runs a snippet per point (`class="point"`) or per instance (`class="instance"`):

```python
w = wrangle(geo, code="""
float h = fbm(@P * ch("freq"), 4.0) * ch("amp");
@P.y += h;
@Cd = mix(vec3(0.2, 0.3, 0.1), vec3(0.9), smoothstep(0.0, 2.0, @P.y));
""", channels={"freq": 0.3, "amp": 1.5})
```

Types: `float`, `vec3`. Reads and writes use `@name`. Functions: `sin cos tan abs sqrt floor ceil
fract exp ln sign min max pow step clamp mix smoothstep fit length normalize dot cross distance
atan2 atan asin acos mod radians degrees
noise fbm rand vec3`. Loops are bounded: `for i in 0..8 { ... }`. Everything is deterministic
and differentiable w.r.t. `ch()` values.

`check_hcs` / `check_graph` predict which attributes exist at every node **without cooking** and
flag mistakes such as `copy_to_points(..., orient_to_normal=1)` fed by points without `N`.

## Technique notes

* **Scale**: model in real meters (door 2.0, person 1.8, stair rise 0.18). Add a `height` spec.
* **Grounding**: end with `center(geo, mode="ground")` or spec `grounded`.
* **Symmetry kills realism**: squash non-uniformly, vary seeds, use `pscale_min/max`.
* **Layer noise**: large low-frequency shape, mid-frequency breakup, small detail.
* **Spacing**: prefer `scatter_poisson` over `scatter` for natural placement.
* **Instancing**: put repeats in `array`, `radial_array`, `copy_to_points`; nest them for grids.
* **Avoid pole pinching** on spheres: `subdivide(box)` then `@P = normalize(@P)`.
* **Normals** (`normals`) give smooth shading in previews; add them before `render_preview`.
* **Terrain**: noise octaves + a wrangle for large-scale shape, `smooth`, then filter scatter by
  slope (`@N.y`) and height with `group_by_expression` + `blast`.
* **Curves**: `curve(points=[[x,y,z],...])` and `helix` make polylines; `resample` evens them out;
  `sweep` turns a curve into a pipe (pipes, cables, springs, rails); `revolve` spins a
  (radius, height) profile around Y (vases, columns, bottles).
* **Deformers**: `bend`, `twist`, `taper` act on base points (use `realize` first if you have
  instances). `fit_size` scales to a target size, `point_jitter` breaks up regularity.
* **Placing on terrain**: `drop_to_surface(points, terrain)` lifts points onto the surface and writes
  `N`; filter with `group_by_expression("@N.y > 0.93")` to keep only flat spots. `switch` picks
  one of up to 4 variants.

## Tools (progressive disclosure)

`list_node_types`, `search_nodes`, `describe_node` -> discover. `run_hcs`, `check_hcs`,
`graph_to_hcs` -> author. `batch`, `create_node`, `set_param`, `connect` -> fine edits.
`cook`, `validate`, `check_graph`, `explain` -> inspect. `render_preview` -> look.
`list_recipes`, `get_recipe` -> start from proven examples. `export` -> deliver.

## Checking your own work (what the measurements say)

We measured how well different judges agree with the spec results on 10 tasks x 4 candidates
(`pt critique-bench`, `gym/critique_experiment.json`): a vision judge looking at same-camera sheets agreed
well (Spearman 0.84, 92% of pairwise rankings right), SSIM against a reference render moderately (0.54), the
generic `critique` rubric not at all (0.10, chance level). So:

* `spec(...)` lines are your definition of done; `critique` only catches framing/contrast problems.
* To judge a result by eye, use `render_compare` (2-4 nodes, one shared camera, true sizes in meters listed)
  instead of separate `render_preview` calls, whose auto-framing hides differences in scale.
* The shaded preview does not show colors from `Cd`; use `mode="Cd"` to look at color specs.

## Sparse SDF booleans and remeshing

`sparse_boolean(a, b, op=..., voxel=..., band=...)` and `sparse_remesh(mesh, voxel=..., offset=...)` take CLOSED meshes
and return a clean mesh with normals. They store only a narrow band around the surface, so `voxel` can be far smaller than
the dense `sdf_*` nodes allow (those refuse above 48M voxels). Keep `k` (smooth ops) and `|offset|` below `band * voxel`;
raise `band` for larger blends. Use them for carving, rounded joins and fixing non-manifold meshes; booleans between meshes
that overlap themselves are not supported (sign comes from ray parity).

## Matching a reference model (rebuild a professional asset as a recipe)

Goal: given a reference (`.glb`, `.gltf`, `.obj`), write a short parametric recipe whose cooked result matches it. Everything
below is measured, never judged by eye alone. Details and the scoring definitions: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B3.

```
pt describe <ref> [--brief]     text view: size, then cross-section islands along Y (and X, Z) with bounding boxes
pt parts <ref>                  size, triangle count, watertightness, and the parts (glTF nodes, then connected pieces)
pt match <ref> <recipe.hcs> [--report DIR] [--align bbox|reference]
                                F-score, Chamfer, silhouette IoU (10 views), normal consistency, validity, knob count, tier;
                                --report writes match.json and match.png (original left, ours right, two angles)
pt refine <ref> <recipe.hcs> [--evals N] [--out FILE] [--tune-integers]
                                CMA-ES over the NUMBERS in the recipe source; structure (loops, mirror, array) is untouched
pt export <recipe> <out.obj|glb|gltf|usda>     cook a recipe and export it (material names and instances included)
pt match-bench <manifest> --run NAME [--recipes DIR --only-recipes] [--resume] ...    batch with budgets and resume
pt corpus fetch|verify          licence ledger (corpus/LEDGER.csv): every file hashed, only CC0 accepted
pt report --baseline A.json --llm B.json --out DIR    the plain-English checkpoint page
```

Workflow that worked: `pt describe --brief` plus the pair image from `pt match --report`, then write the recipe with named
variables, then `pt refine`, then `pt match`. Conventions that matter:

* **Write continuous numbers with a decimal point** (`0.35`, `90.0`). `pt refine` only tunes those; whole numbers are treated as
  structure (`SHELVES = 3`, `count=4`, quarter turns). Use `--tune-integers` only for machine-written recipes.
* **A knob is a non-zero number in the source** (not inside a string, comment or `range(...)`, not `rows/cols/caps/seed`, not an
  integer factor next to `*` or `/`). Variables used many times count once, so name your dimensions (`W`, `D`, `LEG`) and
  derive the rest (`W / 2`). Silver needs 3 to 40 knobs; a good table or chair is 12 to 20.
* Use `mirror` (it keeps the original and adds the mirrored copy), `array` and `radial_array` for repeats; `merge` takes at most 4
  inputs, so nest it. A lying cylinder is `transform(tube(...), r=[0, 0, 90])`; a triangular prism (gable roof) is
  `tube(cols=3)` scaled with `s=[height, 1, width]` and rotated `r=[0, 0, 90]`.
* Size matters: the score normalises each shape by its bounding box, so a rebuild of the wrong overall size is capped at bronze
  (`size_ratio` must be within 10%).
* Thin sticks (frames, fences, ladders) are the hard case for silhouette IoU; thick boxy furniture is the easy case.

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
