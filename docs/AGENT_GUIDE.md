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
