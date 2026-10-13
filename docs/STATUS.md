# Project status and handoff (read this first)

Updated: 2026-10-13 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6; the nine-capability order: [DECISIONS.md](DECISIONS.md) B18.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Capabilities built (B18, guides in AGENT_GUIDE):** 1 geometry queries in the language; 2 pixel pick (`pt pick`); 3 set dressing by physics; 4 fast pictures (the look-development tier, a view crop, a cache of the cooked world); 5 stable identity and the owner's decisions (`pt edit`, `edits/*.json`); 6 the layered world file (`pt world`), without the USD export.
* **Looking at a world (DECISIONS B22, 2026-10-13, AGENT_GUIDE "Looking at a world from every side"):** flaws are found in the game engine, not in offline frames. `pt survey` takes a walk, the area from above, the scene's cameras and a close-up pair of every flagged object in one engine launch and writes contact sheets of twelve labelled views (`out/survey/<recipe>/`); `pt pick` answers for any pixel of any survey view. Measured on the creek world: 90 pictures of a changed world in 67 seconds (cook 35, engine 16), 3.3 GB peak memory. `pt walk` is how the owner opens a world (a window to walk through under the PC protections; a click notes an object and it is named afterwards). Offline frames are for light, air and surfaces, and for the owner's side-by-side look.
* **Grounding by the real shape ([WORLD_GUIDE.md](WORLD_GUIDE.md) "Seating by the real shape"):** placement and the check share one measure of an object's underside (`pt_core::seat`, the node `seat`); `pt ground-check` fails a world on it and every `pt render-scene` report carries its count. The creek, valley and Teton worlds have no hanging or floating object anywhere. Every new world uses `seat` after its copy nodes.
* **Worlds:** `scenes/valley`, `scenes/teton`, and `scenes/cascade` (the eye-level creek; 125 fps in the engine, validator clean; placement stable; three variants in `worlds/cascade.world.json`). The east frame after seating is `out/lookdev/seat_east.png`, reviewed in full and in tiles.
* **Look-development renders** of the creek world fit this machine with `--lookdev --only CAMERA` and `PT_HEAVY_MEM_GB=10`; the owner-grade frames at full quality do not fit yet (ROUTE.md row 1).
* **Tooling (B19):** one command list (`crates/pt-cli/src/registry.rs`) feeds the command line, the agent's tools and [generated/commands.md](generated/commands.md); production code has no `unwrap`, `expect` or `panic!` and no platform-dependent maths; `cargo gate` is the check before every commit; `.mcp.json` is checked in. [ROUTE.md](ROUTE.md) ranks all open work with its cost.
* **Three work lanes: started on the owner's word (2026-10-13).** Lane A (this PC): ROUTE.md rows 1a, 1, 2, 2a, then 8, 9, 15. Lane B (cloud): rows 3, 6, then 5a. Lane C (cloud): rows 5, 4, then 7. A cloud machine has no graphics card: code and tests there, pictures on this PC when a branch comes back. Executors run on Sonnet 5.5 at medium effort from the lead's brief; the lead reviews and merges (ROUTE.md "Lanes"). In progress: the heavy-job lock (`lane/lock`, this PC), the certificate's completeness fix (`lane/certificate`, cloud), the building shell from a floor plan (`lane/c-buildings`, cloud).
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md), compared by eye. The owner (2026-10-13) judges the nature and light of the hut frames high quality. Still to close: a hut without door or window, near-black foreground shadow, no mill, thatch, bank gravel or light shafts.
* **Private (DECISIONS B23):** nothing is shown in public until the owner says. Rival products, features taken and the money estimate: [MARKET.md](MARKET.md).

## Open items (in this order; everything else, ranked with cost: [ROUTE.md](ROUTE.md))

1. **Next: fix what the first survey of the creek showed, by class (ROUTE.md row 1a).** Confirmed with `pt pick`: the `village` camera stands 0.8 m from a fir sapling's branches (node `young`): keep placed objects out of arm's reach of every standing camera; half-buried rocks and ferns still show on the east slope (nodes `h_rocks`, `h_ferns`): remove or raise them in the placement rule; 40 plants and rocks in the area stand mostly under the creek's water. To check: a coarse ground triangle at the bank beside the `hero` camera. Then survey again and render the frames.
2. **Lane A:** owner-quality frames of the creek world (rows 1 and 2), for the owner's side-by-side look against the reference; then the engine look (row 2a). The blind pairs wait for views the owner has not seen (about 10-22; DECISIONS B20).
3. **Lane B:** capability 7, nature that grows. Built: `tree_conifer` and a forest swap by style knob. Next: finer sprays and needles for close trees, the ecology pass, a broadleaf and a ground-cover generator, vines, and the two-brief gate; then the style tools and interiors (row 5a).
4. **Lane C:** the building grammar from the inside out and the mill (row 5), then capability 8, procedural materials (bank gravel, thatch, worn ground at buildings).
5. The right slope of the creek's east view carries little tall grass since the stable placement of 10-13: raise the hero tile density there.
6. The USD export of the layer stack, the last part of capability 6; do it before an engine handoff (lane A, row 8).
7. A walk-through note marked "remove this" is applied by the agent with `pt edit` (it needs the scatter's scope name): record the scope on every placed object so the note becomes the edit by itself.
8. Optional delivery check for the creek world: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`; `--bake` and `--judge` take the machine and run without asking.

## Capability track (DECISIONS.md B18, targets are plan dates)

Done: 1 to 5, and 6 except the USD export. Next, in the lanes: 7 nature that grows and the tree family, 8 procedural materials, 9 building and layout grammars; plan dates in [ROUTE.md](ROUTE.md).

## Next after the track

The scene composer that picks camera, sun and building sites by rule (stage 5; the survey's standing points and sight lines are its first rules); the engine look closer to the render; then systems (stage 4). Roadmap and dates: PRO_QUALITY_PLAN B12.6.

Later, only when the owner can no longer look at every world (rung A3): light automatic checks in place of the vision judge; sparse volumes as a first-class `Geometry` attribute; pyro and FLIP through Blender; Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write`, then `cargo gate`.
