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
