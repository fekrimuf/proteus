# Project status and handoff (read this first)

Updated: 2026-10-13 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6; the nine-capability order: [DECISIONS.md](DECISIONS.md) B18.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## How work runs now (DECISIONS B24, 2026-10-13)

**One agent session at a time, one row of [ROUTE.md](ROUTE.md) per new session, no executors in the background.** A row is built on Sonnet 5.5 at medium effort; planning and the review of a finished row are done on Opus 5.5 in a short session of their own. Work on the row's branch with its own build folder, and make a checkpoint commit at every passing test run. The three lanes used up the five-hour agent allowance in forty minutes and were cut off mid-work; what they built is recovered (items 1 and 2).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Capabilities built (B18, guides in AGENT_GUIDE):** 1 geometry queries in the language; 2 pixel pick (`pt pick`); 3 set dressing by physics; 4 fast pictures (look-development tier, view crop, cache of the cooked world); 5 stable identity and the owner's decisions (`pt edit`); 6 the layered world file (`pt world`), without the USD export.
* **Looking at a world (B22):** flaws are found in the game engine. `pt survey` writes contact sheets of a world from many sides in about a minute (`out/survey/<recipe>/`), `pt pick` answers for any pixel, and the owner opens a world with `pt walk`. Offline frames are for light, air and surfaces, and for the owner's side-by-side look.
* **Grounding by the real shape ([WORLD_GUIDE.md](WORLD_GUIDE.md)):** placement and the check share one measure (`pt_core::seat`, the node `seat`); `pt ground-check` fails a world on it and every render report carries its count. The creek, valley and Teton worlds have no hanging or floating object.
* **Built 2026-10-13, on main:** the machine-wide heavy-job lock (`pt heavy-status`; one render, bake, engine launch or judge at a time across every worktree) and the clean-title certificate (`pt certificate`: every file a world reads, with source, licence and hash; it fails on a file the ledger lacks).
* **Worlds:** `scenes/valley`, `scenes/teton`, and `scenes/cascade` (the eye-level creek; 125 fps in the engine; three variants in `worlds/cascade.world.json`). Look-development renders of the creek fit this machine with `--lookdev --only CAMERA` and `PT_HEAVY_MEM_GB=10`; owner-grade frames at full quality do not fit yet (ROUTE.md row 1).
* **Tooling (B19):** one command list (`crates/pt-cli/src/registry.rs`) feeds the command line, the agent's tools and [generated/commands.md](generated/commands.md); `cargo gate` is the check before every commit.
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md), compared by eye. The owner (2026-10-13) judges the nature and light of the hut frames high quality. Still to close: a hut without door or window, near-black foreground shadow, no mill, thatch, bank gravel or light shafts.
* **Private (B23):** nothing is shown in public until the owner says. Rivals and the money estimate: [MARKET.md](MARKET.md).

## Open items (in this order; the whole queue: [ROUTE.md](ROUTE.md))

1. **Next: finish row 1a, the creek's placement faults** (branch `lane/a-1a`, worktree `.claude\worktrees\agent-a5034bac352a8f5b0`, build folder `D:\Fekri\proteus-targets\lane-a`). Built and tested: `seat` keeps objects out of a camera's face, out of the ground and out of the water, and `pt ground-check` fails on each. Creek: near a camera 24 to 0, buried 5,337 to 0, under water 35 to 3. Left: the last 3 under water, the valley and Teton worlds, `pt survey` with the sheets opened and copied to `out\survey\cascade_1a\`, the coarse ground triangle beside the `hero` camera, the guide section, golden hashes and gym baseline, the gate, the merge. Details: LOG 2026-10-13, last entry.
2. **Then: finish the building shell** (row 5, first slice; branch `lane/c-buildings`, worktree `.claude\worktrees\agent-ab0694ac8226a9b35`). Built and tested: the `building` node (a house from a floor plan), open windows, entrance steps. Left: `pt status --write`, the cottage's pictures from five cameras with tiles (the first frames, `D:\Fekri\proteus-targets\review-c\out\review\`, showed black rooms and no step: both now fixed in code, not yet seen in a picture), golden hashes and gym baseline, the guide section, the gate, the merge; then the cottage replaces the plank hut in `scenes/cascade.hcs`.
3. **Rows 1 and 2:** owner-quality frames of the creek, then light and air, for the owner's side-by-side look. The blind pairs wait for views the owner has not seen (B20).
4. **Row 5, the rest:** the mill and the water wheel.
5. **Capability 7, nature that grows** (row 3). Built: `tree_conifer` and a forest swap by style knob. Next: finer sprays and needles, the ecology pass, a broadleaf and a ground-cover generator, vines, the two-brief gate.
6. The right slope of the creek's east view carries little tall grass since the stable placement of 10-13: raise the hero tile density there.
7. The USD export of the layer stack (row 8), before an engine handoff.
8. A walk-through note marked "remove this" needs the scatter's scope name: record the scope on every placed object so the note becomes the edit by itself.
9. The folder of the finished lock work (`.claude\worktrees\agent-a5d0e824feb7584c7`, clean, its work on main) stays until the owner says to remove it; the leftover branches were deleted on the owner's word (2026-10-13).

## Capability track (B18)

Done: 1 to 5, and 6 except the USD export. Next, in the queue: 9 building and layout grammars (begun), 7 nature that grows, 8 procedural materials; plan dates in [ROUTE.md](ROUTE.md).

## Next after the track

The scene composer (stage 5; the survey's standing points and sight lines are its first rules); the engine look closer to the render; then systems (stage 4). Later, only when the owner can no longer look at every world: light automatic checks in place of the vision judge; sparse volumes as a first-class `Geometry` attribute; pyro and FLIP through Blender; Alembic/FBX. Optional delivery check of the creek: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write`, then `cargo gate`.
