# Project status and handoff (read this first)

Updated: 2026-10-13 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6; the nine-capability order: [DECISIONS.md](DECISIONS.md) B18.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Capabilities built (B18):** 1 geometry queries (a wrangle's second input answers `near_*`, `surf_*`, `ray_*` questions), 2 pixel pick (`pt pick`, MCP `pick`: a frame pixel to prototype, instance, creating node, ground verdict), 3 set dressing by physics (`rbd_settle` onto any mesh, draped cloth, hanging rope). Guides: AGENT_GUIDE.
* **Worlds:** `scenes/valley`, `scenes/teton`, and `scenes/cascade` (the eye-level creek for the owner's target picture; engine rows measured 106 fps, 97.2% walkable ground, validator clean). The east frame has been re-rendered with the placement and rock-colour fixes and the black-card fix, and reviewed (full frame and tiles): nothing floats, rocks are grey, the grass is clean. The `village` camera now stands on the lit side of the hut, not yet rendered.
* **Look-development renders** of the creek world fit this machine only with the lighter preview recipe, `--cull 60` and a 10 GB job cap (ENVIRONMENT.md); the owner-grade frames at full quality do not fit yet: this is capability 4.
* **Assets:** Checkpoint 1 on the strict reading is 21 of 50; asset rebuilding is off the critical path (B17).
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md), compared by eye. Against it the east frame is cleaner but flat: no light shafts, smooth bank and slope surfaces without stones and gravel, no mill (the building kit is capability 9, surfaces are capability 8).

## Open items (in this order; two in progress at most)

1. **In progress:** capability 4, fast pictures (B18): the look-development tier made first-class (`lookdev()` in recipes instead of a hand-edited copy, a cache of the cooked world so a material change does not recook for three minutes, view-dependent levels of detail so owner-grade frames fit in memory) and then a render session that stays open.
2. Render the creek world's four cameras for the owner's 10 blind pairs (our world against a hand-built baseline): the stage 1 gate, target 2026-10-17; it needs item 1's frames at owner quality, so say before the render (it takes the machine) and ask the owner for the baseline pairs when the frames exist.
3. Optional delivery check for the creek world, light by default: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`; the terrain bake is `--bake` and the vision judge `--judge`; both take the machine, so say so before using them.
4. Give `run_godot` a timeout: a stalled Godot import blocked the scorecard once; the timeout removes the class.
5. Buried objects: `pt ground-check` finds 2 043 instances more than 60% under the ground world-wide (103 in the east view); they are invisible, so they cost memory and nothing else: drop them in the placement rules (`settle` limit) instead of exporting them.

## Capability track (DECISIONS.md B18, targets are plan dates)

Done: 1 geometry queries, 2 pixel pick, 3 set dressing by physics. Next: 4 fast pictures (10-19), 5 stable identity and overrides (10-21), 6 layered world file with style variants (10-24), 7 nature that grows and the tree family (11-03), 8 procedural materials (11-10), 9 building and layout grammars (11-24). Each is finished and verified before the next starts.

## Next after the track

The scene composer that picks camera, sun and building sites by rule (stage 5; the pick tool already compares candidate viewpoints by casting rays); the engine look closer to the render; then systems (stage 4). Roadmap and dates: PRO_QUALITY_PLAN B12.6.

Later, only when the owner can no longer look at every world (rung A3): light automatic checks in place of the vision judge; sparse volumes as a first-class `Geometry` attribute; pyro and FLIP through Blender; Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write` and `pt docs-check`.
