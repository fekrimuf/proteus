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

## Asking the world questions (a wrangle's second input)

A wrangle can take a **second input**: geometry it may ask questions of (it is only read). Without one a wrangle sees nothing but its
own point, so every placement rule used to need a special node; with one, a rule is a line. Worked example:
`recipes/26_clearing_planted_by_asking.hcs` (trees stand on a hill, keep 2.5 m from a hut, and the crowded ones stay small).

```python
standing = wrangle(seeds, ground, code="""
vec3 top = @P + {0.0, 100.0, 0.0};
@P = ray_pos(top, {0.0, -1.0, 0.0});                 // stand on the ground
@slope = degrees(acos(clamp(ray_n(top, {0.0, -1.0, 0.0}).y, 0.0, 1.0)));
""")
asked = wrangle(standing, hut, code="@hut_gap = surf_dist(@P);")        # how far is the hut?
spaced = wrangle(kept, kept, code="@pscale *= 1.0 - 0.07 * clamp(near_count(@P, 4.0) - 1.0, 0.0, 5.0);")   # how crowded am I?
```

| Question | Answer |
|---|---|
| `near_dist(p)`, `near_dist_other(p)`, `near_pos(p)` | distance to / position of the nearest point of the second input (`_other` skips the point with index `@ptnum`: use it when both inputs are the same points) |
| `near_count(p, r)`, `near_sum("attr", p, r)` | how many points lie within `r` metres (the point itself counts when it is in the second input), and the sum of a float attribute over them |
| `near_f("attr", p)`, `near_v("attr", p)` | float / vec3 attribute of the nearest point |
| `surf_dist(p)`, `surf_pos(p)`, `surf_n(p)` | distance to, position on and face normal (turned toward `p`) of the closest point of the second input's base surface |
| `surf_f("attr", p)`, `surf_v("attr", p)` | a point attribute interpolated at that closest point |
| `ray_dist(o, d)`, `ray_pos`, `ray_n`, `ray_f("attr", o, d)`, `ray_v` | the first surface the ray meets, from either side (`ray_dist` is -1 and the others zero for a miss); the normal faces the origin |

* **One second input per wrangle**: chain wranglers for several questions (ground first, then the hut). `ref_class="instance"` makes the
  `near_*` questions measure the second input's instance origins and their attributes (put the trees there to ask "is a tree near?");
  `surf_*` and `ray_*` always use its base surface (`realize` instances first if they matter).
* Attribute names are quoted and checked before anything cooks (`check_hcs` says what the second input offers, with a did-you-mean).
  An empty second input answers 1e9 for distances and 0 for everything else.
* Answers are constants for `fit_wrangle` and `optimize` (a distance or a hit is not differentiated), and identical on every thread count
  and machine (equal distances go to the lower index).
* **Stand on the ground with a ray, not a guess**: `ray_pos(@P + {0.0, 100.0, 0.0}, {0.0, -1.0, 0.0})` is the surface height under a
  point on any mesh (`drop_to_surface` is the same idea for heightfields and also settles a footprint).

## Set dressing by physics (`rbd_settle`, draped cloth, hanging rope)

Real things rest where they fell. Do not place a log pile by hand or by slope rules: drop it.

```python
logs = copy_to_points(log, raised_points, rot_y_random=360.0)    # instances; origins raised so a pile falls from heights
lying = rbd_settle(logs, ground)                                 # rigid bodies onto ANY mesh: resting poses (position and turn)
```

`recipes/27_woodpile_and_stones.hcs` is the worked example (two woodpiles and stones on a hill). Each instance becomes a body (the convex
hull of its prototype, sampled to `hull_points`), falls from where it was put (never lower than `gap` above the ground under it), lands on
input 1, slides, tips and rests on the slope or against its neighbours. The output keeps the instances with new positions **and turns**
and adds instance attributes `settle_move` and `settled`; bodies that never came to rest, or have no ground under them, are removed
(`drop_lost=0` keeps them for inspection). It returns resting poses, not frames: the result is the same at every frame and
deterministic. Only the ground within `margin` metres of a body is the collider, so a whole terrain is fine. Limits that shape its use:
at most 6000 bodies per node (settle the hero piles, scatter the rest), convex hulls (a barrel is a barrel, a bowl is a puck),
`rolling` 6 keeps logs from rolling off (0 lets them roll like real ones).

* **Cloth draped over something** (a tarp on a cart, a cloth on a table): `cloth_sim(sheet, sdf_from_mesh(thing, voxel=0.04, padding=0.4))`
  at a later frame (`frame(100)`); the sheet falls onto the collider and stays on it. Pin points with `pin_group` for banners.
* **A rope between two posts**: bend a resampled curve with a wrangle, `@P.y = y0 - sag * (1.0 - u * u)` with `u` running from -1 to 1
  along it (`sag` of 3 to 8 percent of the span), then `sweep` it into a pipe. (`cloth_sim` needs polygons: use a thin ribbon grid for a
  pinned strap, not a polyline.)
* A settled body that lies on terrain passes `pt ground-check` by construction: it touches the ground and does not float.

## Placement that keeps its identity, and the owner's decisions (`scatter_stable`, `apply_edits`, `pt edit`)

A scatter that draws N points in order changes every object when anything upstream changes, so a world the owner approved is a
different world after the next rebuild. Use `scatter_stable` for everything that is placed:

```python
at = scatter_stable(land, min_dist=3.2, seed=11, density=0.7, pscale_min=0.8, pscale_max=1.4)   # blue noise, stable ids
mine = keep(at, "t1", "@slope_deg < 30.0")                                                       # masks as before
final = apply_edits(settle(mine, ...), land, edits=edits_of("edits/world.json", "trees"))         # the owner's decisions, by id
trees = copy_variants(final, a, b, c)                                                            # turns and variants follow the id
```

* A point belongs to a CELL of the world (`min_dist / 1.5` m) and is decided by the cell and the seed alone. Change the terrain's height,
  a mask, the density or anything elsewhere, and every surviving point is exactly where it was (xz) with the same integer `pid`
  (the packed cell coordinates: unique, never reused for another place). Spacing is measured on the map (x, z), so height never
  decides who stays. Lowering `density` only removes points. A hole in the surface changes only the points near it.
* `count` is not a parameter: the density thins the saturated set. To aim at a number use `density = min(1, count / (0.6 * area / min_dist^2))`
  (what `spots_on` in `scenes/cascade.hcs` does).
* `copy_to_points` and `copy_variants` key their randomness (turn, variant) on `pid` when the points have one, so removing or adding
  an object changes no other object's look.
* **The loop with the owner.** `pt pick ... x y` names an object and its `pid` and scope node; the decision is one command:
  `pt edit edits/cascade.json rocks 123456 remove` (also `move DX DY DZ [--to]`, `scale F`, `add X Y Z [PSCALE]`), then rebuild:
  the world is identical except that object. The edits file is `{ "scope": [edit, ...] }`, read in the recipe by
  `edits_of(path, scope)` (a missing file or scope is an empty list) and replayed by `apply_edits` (put it after the filters and
  `settle`, before the copy). Moved and added points are put back on the surface (`snap`, second input). Edits whose id no longer
  exists are ignored and reported on stderr. `apply_edits` also edits instances that carry a `pid` (remove, move, scale).
* Points are placed on what the sky sees (the first surface under a ray from above): a roof or a floor under a roof is not a place.

## One world, many looks and many studios: knobs and the world file (`knob`, `pt world`)

A world is layers: the BASE (a recipe), the SCENE (cameras), a STYLE (materials, light, render settings and **knobs**) and the OWNER
layer (`edits/*.json`, the decisions about single objects). A world file names them, and a **variant** picks a style, adds knobs
and may point at another owner layer. The same creek on a grey day with a thicker forest and more fallen wood is a variant, not a copy.

```python
FOREST = knob("forest_density", 1.0)            # in the recipe: ask the active style; the default is what a final recipe means
EDITS  = knob("edits", "edits/cascade.json")     # even the owner layer is a knob, so a studio can have its own
tall_pts = keep(spots(24000 * FOREST, 6.8, 21, 0.8, 1.45), ...)
```

* A style file (and the scene file, which wins key by key) may carry `"knobs": {"forest_density": 1.2, ...}`. `knob(name, default)` returns
  the style's value when there is one (it must be the same kind as the default: number, string, list) and the default otherwise. Every
  command that cooks a scene (`render-scene`, `ground-check`, `pick`, `scorecard`) sets the knobs first, so they all see the same world.
  Knobs are also how a style changes SHAPE (tree morphology, building proportions) once the generators exist, not only colour and light.
* The world file `worlds/<name>.world.json`: `{recipe, scene, default_variant, variants: {name: {style, knobs, edits}}}`.
  `pt world worlds/cascade.world.json info|scene|check|render OUT.png|pick CAMERA X Y [--variant NAME]` expands the variant into a scene file
  under `out/world/` and runs the usual command on it (`--only`, `--lookdev`, `--radius`, `--frame`... pass through).
* Because placement is stable (`scatter_stable`), two variants differ only where their knobs differ: the rocks stand where they stood.
  Measured on the creek: evening 256 109 instances, overcast 262 015 (more trees and logs), same ids elsewhere.
* The cooked-world cache of `render-scene` is keyed by the knobs and by everything under `edits/`, so a changed decision or variant is
  never rendered from a stale world.

## Trees made from knobs (`tree_conifer`, `pt gen-texture`)

A forest of scans repeats and carries the shape of someone else's trees. `tree_conifer` grows a conifer from nine proportions, so a spruce,
a fir, a wind-bent pine and a tree that never grew on Earth are points in one knob space (DECISIONS B16a), and a style can move them.

```python
fir = tree_conifer(height=24.0, crown_width=0.27, droop=30.0, lift=18.0, crown_power=0.85, whorl_spacing=0.46, branches=8, seed=101)
pine = tree_conifer(height=11.0, crown_width=0.38, droop=8.0, lift=34.0, crown_power=0.6, seed=107)
```

* Knobs: `height`, `crown_base` (clear trunk below the first branches, share of height), `crown_width` (longest branch, share of height:
  spruce 0.2, pine 0.4), `crown_power` (1 = cone, less = fuller), `whorl_spacing`, `branches`, `droop` and `lift` (branch angle at the foot and at the
  top of the crown), `curl`, `asymmetry`, `gaps`, `lean`, `wobble`, `spray_width/length`, `sprays_per_m`, `detail` (0.4 = a distant tree, 1.5 = hero),
  `seed` (every tree of the same knobs differs). About 45 000 triangles at detail 1; materials are the roles `bark` and `conifer_needles`.
* The needle cards carry a picture drawn by the program: `pt gen-texture needles corpus/generated/needles_a.png --seed 3` (RGBA, no scan).
  The library entry `conifer_needles` has `"kind": "foliage"` (an alpha cut-out card material, double sided, translucent against the sun);
  LOD treats it as foliage by its name.
* The creek recipe swaps scans for grown trees with the style knob `tree_source = "generated"` (the world variant `grown`:
  `pt world worlds/cascade.world.json render OUT.png --variant grown --only east --lookdev`): placement, sizes and the owner's decisions
  are identical, only the prototypes change.
* Look in a lab world before a creek-sized render: a few trees, a ground, the sun in front and behind (`out/lookdev/lab/tree.hcs` is the pattern).

## What is at this pixel? (`pick`)

When the owner (or you) points at a frame ("that moss floats", "this rock is orange"), do not hunt for the object: ask.
`pick` casts the camera's ray through the pixel into the cooked world on the CPU (no render, no GPU) and answers in one call:

```
pt pick scenes/cascade.hcs scenes/cascade.scene.json east 1210 640 --frame out/lookdev/east.png [--radius 40] [--annotate pick.png]
MCP:   pick(node, scene, camera, x, y, radius?, frame_size?)
```

* The result names the **prototype** (materials, triangles, far stand-in or not), the **instance number**, the **node that placed it with its
  parameters** (provenance: for scattered things this is the scatter node, so the fix goes to the rule, not the object), the node that made
  the prototype, its attributes (`pscale`, `N`, `settle_drop`...), its origin's height above the terrain, and what `ground-check` says
  about it (`float`, `overhang`, `shallow`, `buried`). A pixel on terrain or water names the material, slope and the node that built it.
  The first output line is one plain sentence.
* `--radius PX` also lists the **classes of object** within that many pixels with their share, nearest example and flags: "that patch" is
  one node and one prototype, so fix the class (a rule or an engine node plus a check), never one instance.
* Pixel coordinates refer to the frame's own size: pass `--frame` (the PNG you were looking at; its size is read) or `--size WxH`; quick
  renders are half size. A camera must be a standing one (`eye` in the scene file). Hits use the geometry of the cooked world, so an
  alpha-cut leaf card counts as a hit where its texture is clear.

## Looking at a world from every side (`survey`, `walk`)

Do not render to find flaws. An offline frame costs half a minute to ten minutes and shows one side; the game engine loads the same
world once and draws it at a hundred frames a second (DECISIONS B22). Survey first, render when the survey is clean.

```
pt survey scenes/cascade.hcs scenes/cascade.scene.json [--at X,Z --radius M] [--stations N] [--closeups N] [--passes lit,flags,kinds]
MCP:   survey(recipe, scene, out?, at?, radius?, stations?, closeups?, passes?, size?, lookdev?, plan_only?)
```

* **What it takes, in one engine launch:** a walk (standing points spread over the area the scene's cameras look at, four views
  each), the area from above (four sides and a plan), the scene's own cameras, and a close-up pair of every object the ground check
  flags (floating, hanging, mostly under water): one low view that shows the air under it and one higher from another side, each from
  the clearest of twelve directions, at most three objects of one class.
* **What you open:** the contact sheets in the output folder (default `out/survey/<recipe>/`), twelve labelled views each:
  `sheet_walk_N.png`, `sheet_overview_N.png` (above and cameras), `sheet_flaws_N.png` (close-ups, target boxed in the defect's
  colour). The report (also `survey.json`) lists every sheet's cells and what each close-up shows. Open a single picture
  (`lit_<view>.png`, 1280 x 720) when a cell shows something.
* **Passes:** `lit` is the world as the engine draws it. `flags` paints every flagged object in its colour over the same view (red
  floats, orange hangs, yellow is mostly under water, magenta is buried), so you see which defects a visitor can see and from where.
  `kinds` (on request) gives every kind of object one colour of its own: what belongs to what, where one kind repeats.
* **Asking about a pixel:** `survey.scene.json` holds every view as a camera, so
  `pt pick <recipe> out/survey/<recipe>/survey.scene.json <view> X Y --size 1280x720` names the object, its stable id and the node
  that placed it. Fix the class (the rule), record single decisions with `pt edit`.
* **What it is for:** placement, shape, scale, gaps, repeats. The engine's light is plain, its ground shows the layers' flat colours
  and its meshes are the engine's reduced levels (kept three times further out than in a game, without the far shells): judge light, air and surfaces in a render (`pt render-scene`).
* **Cost on the creek world:** 67 seconds for 90 pictures of a changed world (cook 35 s, engine 16 s); the engine file is kept while
  the world is the same. `--plan-only` plans the views without the engine.

`pt walk <recipe> <scene>` opens the same world in a window for the owner to walk or fly through: the only safe way to open a world
by hand (low priority, the memory cap, a time limit on the load; never open a full world in Blender by hand). Controls are on screen
(H). A left click notes "this looks wrong", X notes "remove this"; when the window closes each note is named like a pick (object,
`pid`, placing node, ground-check verdict) in `out/walk/<recipe>/notes.json` with the picture taken at the click. Read the notes, fix
the class, and record a single decision with `pt edit`. `--check` proves the walk works with nobody at the keyboard.

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
* **Seating what was placed**: `seat(instances, terrain)` after every copy node. It lowers each
  instance by its real shape until its whole base is in the ground (a tree's root plate on a slope;
  `bury=0.2` puts roots in the soil), lays a lying object on its body instead of its lowest stub,
  and with `seat(instances, terrain, water, draft=0.55)` wood lies in the water line. It removes an
  instance the ground falls away under (`limit`). What it seats passes `ground_check`, which uses the
  same measure and prints each prototype's `soles` (base width, where the body starts).

## Tools (progressive disclosure)

Every tool call is checked against its schema before it runs: an unknown, missing or wrongly typed argument is refused with the accepted names, so a typo never silently changes nothing. The frame-review and world commands (`ground_check`, `review_tiles`, `world`, `edit`, `render_world`, `scorecard`, `gate`, `recipes`, `docs_check`, `status`) are tools too, listed by `tools/list` like the session tools; the heavy ones say so in their description (no permission needed: they run at low priority under a memory cap, one at a time). A tool that fails returns an error whose text is what the command printed.

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

### Vision judges (local, free, autonomous)

`pt judge ...` runs a vision-language model on the local GPU (llama.cpp `llama-server` + Qwen3-VL-8B Q4, Apache-2.0; about 6.5 to 7.7 GB of
the 8 GB card, so never alongside Blender or Godot; the scorecard runs it last). Setup paths and hashes are in STATUS.md; override with
`PT_JUDGE_SERVER`, `PT_JUDGE_MODEL`, `PT_JUDGE_MMPROJ`, `PT_JUDGE_PORT`. Answers are JSON-schema constrained, temperature 0, and cached by exact
request in `out/judge_cache`, so a re-run of a report is instant and identical. The server starts on the first cache miss (about 50 s to load)
and stops with the command; start one by hand to reuse it across commands.

| Command | Question it answers | Measured |
|---|---|---|
| `pt judge frames IMG... --brief T [--foil T]...` | does the frame read as the brief (0 to 10), and as the brief rather than as other worlds (forced choice, both orders), plus a blank-frame control | valley frames: brief picked over 3 foils 9 of 9, 0 order flips, blank frame scores 0 |
| `pt judge parts PAIR.png` | does the rebuild have the same parts as the original? The judge only COUNTS (rungs/rings/shelves, legs, arms/handles, openings, top shape) per column; Rust decides what differs | 8 of 9 known missing-feature defects found, 0 false alarms on 15 good rebuilds, 0 of 24 self-comparisons flagged, 23 of 24 different objects detected (development numbers; the clean test is the fresh 50 of Phase 1) |
| `pt judge compare A.png B.png --brief T` | which of two renders satisfies the brief better (one picture, labels 1 and 2, both orders) | critique set: Spearman 0.69, 82% pairwise, picks best 63%, 42% of answers flip with the order; author model 0.84 / 92% / 90% |
| `pt judge rate`, `which`, `critique`, `audit`, `status` | single-image rating, which-description, and the calibration runs | `gym/critique_experiment_*.json`, `corpus/results/judge/` |

Rules that came out of the measurements (follow them when you add a judge question):
* **Closed, countable questions beat leading ones.** "Does the copy lack a feature?" flagged all 24 rebuilds, including 22 of 24 pictures compared with themselves.
  Counting each object separately and comparing the numbers in code flagged none of the identical pairs.
* **Ask in both orders and treat a flip as a tie.** The 8B model flips on about 42% of close pairwise questions, so near-identical candidates are NOT a judge job:
  spec lines stay the definition of done, and a second reader (the author model through `pt critique-bench`, or a human tap) audits.
* **Put labels in the picture** (stamped letters), not in the prose ("the second image").
* **Calibrate before a judge gates anything:** score it on answers that are known by construction (specs, flagged defects, identical pairs, a blank frame) and record the file.

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
* **Round and stepped objects (barrels, fountains, vases, towers): one lathe profile.** `curve` (radius, height) then `revolve` gives a single clean surface; stacked or overlapping
  solids carry hidden faces that the surface score counts as extra (a ring added as a separate tube cost a barrel its gold). Put the axis points at exactly `0` (a pole at 0.001 leaves the mesh open and caps the tier at bronze).
  A square tower is `revolve(..., divisions=4)` turned 45 degrees (radius = half-width x 1.414).
* **Thin features must be structure, not knobs.** The score cannot see a 9 mm hoop, so `pt refine` will shrink it away if its position is a decimal. Write positions of rings, rungs, shelves and
  hoops as whole numbers (`B1 = 57` then `B1 / 1000`) or as an `array(count=N)`; run `pt judge parts` on the pair picture after every tuning, not only `pt match`.


## Building worlds

Materials, light, photographic renders, real assets, styles, the scorecard, engine delivery, real terrain and the eye-level creek world are in [WORLD_GUIDE.md](WORLD_GUIDE.md). The `get_guide` tool returns both files.
