# Project status and handoff (read this first)

Updated: 2026-10-13 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6; the nine-capability order: [DECISIONS.md](DECISIONS.md) B18.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Capabilities built (B18, guides in AGENT_GUIDE):** 1 geometry queries in the language; 2 pixel pick (`pt pick`: frame pixel to prototype, instance, `pid`, creating node, ground verdict); 3 set dressing by physics (`rbd_settle`, draped cloth); 4 fast pictures (the look-development tier `lookdev()` and `--lookdev`, a view crop, a cache of the cooked world: a repeat render takes 31 s); 5 stable identity (`scatter_stable`, ids survive rebuilds) and the owner's decisions (`pt edit`, `apply_edits`, `edits/*.json`); 6 the layered world file (`worlds/*.world.json`, variants of style, knobs and owner layer, `pt world`), without the USD export.
* **Worlds:** `scenes/valley`, `scenes/teton`, and `scenes/cascade` (the eye-level creek; engine rows measured 106 fps, 97.2% walkable ground, validator clean; placement stable; three variants in `worlds/cascade.world.json`: evening, overcast, and grown, whose forest is made from knobs instead of scans). The east frame is reviewed in full and in tiles, rendered at 1920 x 1080 and 96 samples in the look-development tier; the `village` camera stands on the lit side of the hut and renders.
* **Look-development renders** of the creek world fit this machine with `--lookdev --only CAMERA` and a 10 GB job cap; the owner-grade frames at full quality (all recipe assets at full size) do not fit yet.
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md), compared by eye. Against it our frames are clean but flat: no light shafts, smooth bank and slope surfaces without stones and gravel, a hut roof that reads as planks not thatch, no mill (surfaces are capability 8, the building kit capability 9).

## Open items (in this order; two in progress at most)

1. **In progress:** the owner's blind pairs (stage 1 gate, target 2026-10-17): frames of the creek world at owner quality against a hand-built baseline. The render of the four cameras takes the machine (about ten minutes each at the 10 GB cap, then low priority), so say before it starts; ask the owner for the baseline pairs once the frames exist.
2. **In progress:** capability 7, nature that grows. Built: the conifer generator `tree_conifer` (knobs, seed, needle picture drawn by the program) and a forest swap by style knob (`grown` variant). Next in it: finer sprays and needles for trees seen close (the foreground trees of the grown frame show their cards as large leaf shapes), then the ecology pass by rules on queries (saplings cluster near mature trees, no two trees through each other, ferns in shade, trees shrink toward the tree line; stable random numbers `u` and `v` are there for it), a broadleaf and a ground-cover generator, and the two-brief gate (a "sci-fi village" brief gets no Earth trees).
3. The USD export of the layer stack (base geometry with the owner's edits, one sublayer per variant for materials and light), the last part of capability 6; do it before an engine handoff.
4. Optional delivery check for the creek world, light by default: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`; the terrain bake is `--bake` and the vision judge `--judge`; both take the machine, so say so before using them.
5. Give `run_godot` a timeout: a stalled Godot import blocked the scorecard once; the timeout removes the class.
6. Buried objects: `pt ground-check` finds about 1 900 instances more than 60% under the ground (invisible: they cost memory and nothing else); drop them in the placement rules (the `settle` limit) instead of exporting them.

## Capability track (DECISIONS.md B18, targets are plan dates)

Done: 1 to 5, and 6 except the USD export. Next: 7 nature that grows and the tree family (11-03), 8 procedural materials (11-10), 9 building and layout grammars (11-24). Each is finished and verified before the next starts.

## Next after the track

The scene composer that picks camera, sun and building sites by rule (stage 5; `pt pick` already compares candidate viewpoints by casting rays); the engine look closer to the render; then systems (stage 4). Roadmap and dates: PRO_QUALITY_PLAN B12.6.

Later, only when the owner can no longer look at every world (rung A3): light automatic checks in place of the vision judge; sparse volumes as a first-class `Geometry` attribute; pyro and FLIP through Blender; Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write` and `pt docs-check`.
