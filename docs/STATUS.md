# Project status and handoff (read this first)

Updated: 2026-10-12 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6; the nine-capability order: [DECISIONS.md](DECISIONS.md) B18.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Worlds:** `scenes/valley` (analytic), `scenes/teton` (real USGS elevation, scorecard v4: judge brief fit 6.0 of 10 against a bar of 7) and `scenes/cascade` (the eye-level creek built for the owner's target picture; the engine rows measured 106 fps, 97.2% walkable ground, validator clean). The cascade east frame was found to have hovering moss and flat half-drowned stones; the placement fix is in the world but the frame is not yet re-rendered.
* **Assets:** Checkpoint 1 on the strict reading is 21 of 50 (development 17 of 40, hold-out 4 of 10); open repairs are the cactus arms, bench scrolls and tower score. Asset rebuilding is off the critical path (B17).
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md). Frames are compared with it by eye; the valley is not close.
* **Look-development render:** `pt render-scene ... --only CAMERA --cull METRES` drops what the chosen cameras cannot see, so a one-view render of the big world fits in memory; `pt ground-check` also reports where the base triangles are, per material.

## Open items (in this order; two in progress at most)

1. **In progress:** re-render the `east` frame (quick mode, one frame; it loads the GPU, so say so first) with the placement fix and the rock colour fix (`tune_authored` in `corpus/tools/render_scene.py` and the two `rock_moss_set_*` style entries: the scanned rocks render orange in direct sun), then review it with `pt ground-check --frames --annotate` and `pt review-tiles`. In the same tiles, the grass shows pale paper-like flakes (backlit blades under `foliage_translucency`) that read as bits floating in the air: find the cause on the grass material and fix it. Then move the `village` camera onto the lit side of the hut. The look is judged by the owner's eye against the reference (B17).
2. **In progress:** capability 1, geometry queries in the language (B18): a wrangle with a second input can ask for the nearest point or surface, neighbour counts and sums, ray hits and attributes there.
3. Ask the owner for the 10 blind pairs (our creek world against a hand-built baseline): the stage 1 gate, target 2026-10-17, once the east frame is re-rendered and reviewed.
4. Optional delivery check for the creek world, light by default: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`. The terrain bake is `--bake` and the vision judge `--judge`; both take the machine, so say so before using them.
5. Give `run_godot` a timeout: a stalled Godot import blocked the scorecard once. The tiled exporter removed that cause; the timeout removes the class.

## Capability track (DECISIONS.md B18, targets are plan dates)

1 geometry queries (10-13), 2 pixel pick (10-14), 3 simulation as set dressing (10-16), 4 fast pictures: region cook and a live render session (10-19), 5 stable identity and overrides (10-21), 6 layered world file with style variants (10-24), 7 nature that grows and the tree family (11-03), 8 procedural materials (11-10), 9 building and layout grammars (11-24). Each is finished and verified before the next starts.

## Next after the track

The scene composer that picks camera, sun and building sites by rule (stage 5); the engine look (terrain detail, water, grass) closer to the render; then systems (stage 4). Roadmap and dates: PRO_QUALITY_PLAN B12.6.

Later, only when the owner can no longer look at every world (rung A3): light automatic checks in place of the vision judge; `render_compare` as a gym observation; multi-step conversation edits and reference-similarity gym tasks; sparse volumes as a first-class `Geometry` attribute; compositing; pyro and FLIP through Blender; Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write` and `pt docs-check`.
