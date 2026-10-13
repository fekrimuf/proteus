# Project status and handoff (read this first)

Updated: 2026-10-13 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6; the nine-capability order: [DECISIONS.md](DECISIONS.md) B18.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Capabilities built (B18, guides in AGENT_GUIDE):** 1 geometry queries in the language; 2 pixel pick (`pt pick`); 3 set dressing by physics (`rbd_settle`, draped cloth); 4 fast pictures (the look-development tier `lookdev()` and `--lookdev`, a view crop, a cache of the cooked world); 5 stable identity (`scatter_stable`) and the owner's decisions (`pt edit`, `apply_edits`, `edits/*.json`); 6 the layered world file (`worlds/*.world.json`, `pt world`), without the USD export.
* **Grounding by the real shape (2026-10-13, [WORLD_GUIDE.md](WORLD_GUIDE.md) "Seating by the real shape"):** placement and the check share one measure of an object's underside (`pt_core::seat`): the node `seat` buries a standing object's whole base (the slab of scanned soil under a fir included), turns lying objects to the ground under their footprint and rests them on their body, and floats wood in the water line; `pt ground-check` fails a world on the same measure and prints each prototype's `soles`. The creek world has no hanging or floating object anywhere, in the evening and the grown variant. Every new world uses `seat` after its copy nodes.
* **Worlds:** `scenes/valley`, `scenes/teton`, and `scenes/cascade` (the eye-level creek; engine rows measured 106 fps, 97.2% walkable ground, validator clean; placement stable; three variants in `worlds/cascade.world.json`: evening, overcast, and grown). The east frame after seating is `out/lookdev/seat_east.png` (1920 x 1080, 96 samples, look-development tier), reviewed in full and in tiles.
* **Look-development renders** of the creek world fit this machine with `--lookdev --only CAMERA` and `PT_HEAVY_MEM_GB=10`; the owner-grade frames at full quality do not fit yet (ROUTE.md row 1).
* **Tooling (B19):** one command list (`crates/pt-cli/src/registry.rs`) feeds the command line, the agent's tools and [generated/commands.md](generated/commands.md); tool arguments are checked; a panic in a tool becomes an error; production code has no `unwrap`, `expect` or `panic!` and no platform-dependent maths; `cargo gate` is the check before every commit and CI runs its lint; `.mcp.json` is checked in. [ROUTE.md](ROUTE.md) ranks all open work with its cost.
* **Three work lanes: approved by the owner (2026-10-13), not started.** They begin on the owner's word (the owner on 2026-10-13, after the grounding fix: not yet). Lane A: ROUTE.md rows 1, 2, then 8, 9. Lane B: rows 3, then 6. Lane C: rows 4, 5, then 7. Each lane works in its own git worktree and build folder and merges after `cargo gate`; the lead alone edits STATUS and LOG.
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md), compared by eye. Against it our frames are clean but flat: no light shafts, smooth bank and slope surfaces without stones and gravel, a hut roof that reads as planks not thatch, no mill (lanes A and C).

## Open items (in this order; everything else, ranked with cost: [ROUTE.md](ROUTE.md))

1. **Lane A (next to start, on the owner's word):** owner-quality frames of the creek world (rows 1 and 2: the full-detail world fits this machine, then light and air), for the owner's side-by-side look against the reference. The blind pairs wait for views the owner has not seen (about 10-22; DECISIONS B20).
2. **Lane B (next to start, on the owner's word):** capability 7, nature that grows. Built: `tree_conifer` and a forest swap by style knob (`grown` variant). Next: finer sprays and needles for trees seen close, the ecology pass by rules on queries, a broadleaf and a ground-cover generator, and the two-brief gate (a "sci-fi village" brief gets no Earth trees).
3. **Lane C (next to start, on the owner's word):** capability 8, procedural materials (gravel and stones on the banks, thatch), then the building grammar and the mill.
4. `seat` in `scenes/valley.hcs` and `scenes/teton.hcs` (they still place by points only; the new check will list what hangs there), and the right slope of the creek's east view carries little tall grass since the stable placement of 10-13: raise the hero tile density there.
5. The USD export of the layer stack, the last part of capability 6; do it before an engine handoff (lane A, row 8).
6. Give `run_godot` a timeout, and drop buried objects in the placement rules: `pt ground-check` finds about 1 100 instances more than 60% under the ground (invisible: they cost memory and nothing else) (lane A, row 9).
7. Optional delivery check for the creek world: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`; `--bake` and `--judge` take the machine and run without asking.

## Capability track (DECISIONS.md B18, targets are plan dates)

Done: 1 to 5, and 6 except the USD export. Next, in the lanes: 7 nature that grows and the tree family, 8 procedural materials, 9 building and layout grammars; plan dates in [ROUTE.md](ROUTE.md).

## Next after the track

The scene composer that picks camera, sun and building sites by rule (stage 5); the engine look closer to the render; then systems (stage 4). Roadmap and dates: PRO_QUALITY_PLAN B12.6.

Later, only when the owner can no longer look at every world (rung A3): light automatic checks in place of the vision judge; sparse volumes as a first-class `Geometry` attribute; pyro and FLIP through Blender; Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write`, then `cargo gate`.
