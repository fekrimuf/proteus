# Project status and handoff (read this first)

Updated: 2026-10-12 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md)). Roadmap and dates: [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6.

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Worlds:** `scenes/valley` (analytic), `scenes/teton` (real USGS elevation, scorecard v4: judge brief fit 6.0 of 10 against a bar of 7) and `scenes/cascade` (the eye-level creek built for the owner's target picture; the engine rows measured 106 fps, 97.2% walkable ground, validator clean; its render frames hold up when opened).
* **Assets:** Checkpoint 1 on the strict reading is 21 of 50 (development 17 of 40, hold-out 4 of 10); open repairs are the cactus arms, bench scrolls and tower score.
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md). Frames are compared with it by eye; the valley is not close.

## Open items (in this order)

1. Render one frame with `tune_authored` in `corpus/tools/render_scene.py` and the two `rock_moss_set_*` style entries (they grey the scanned rocks that render orange in direct sun in `frame_east.png`), then move the `village` camera onto the lit side of the hut. The look is judged by the owner's eye against the reference (DECISIONS.md B17), so these come first.
2. Ask the owner for the 10 blind pairs (our creek world against a hand-built baseline): the stage 1 gate.
3. Optional delivery check for the creek world, light by default: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json`. The terrain bake is `--bake` and the vision judge `--judge`; both take the machine, so say so before using them. Triangles in one frame were 20.6 million against a bar of 20; one more trim went in after that measurement.
4. Give `run_godot` a timeout: a stalled Godot import blocked the scorecard once (the 270 000-node import). The tiled exporter removed that cause; the timeout removes the class.

Verified on 2026-10-12: `cargo check -p pt-cli` compiles the `scorecard --reuse` and bake-cache edits that had never been compiled. Heavy child programs (Blender, Godot, the validator, the judge server) now start at low priority, under a memory cap, and die with `pt` (`pt_cli::lowload`); a test proves the cap stops a runaway child.

## Next after the open items

1. A tree and ground-cover generator family on morphology knobs (DECISIONS.md B16a): closes repetition (15 copies of one tree in a 50 m square against a bar of 12), lowers the CC0 share, makes the look unique per studio.
2. A scene composer that picks camera, sun and building sites by rule (they were picked by hand).
3. A look-development version of a scene that renders in under a minute.
4. A building kit beyond the one hut; the engine look (terrain detail, water, grass) closer to the render.
5. Stage 3 appearance depth (procedural materials and sky) beside stage 2; then systems (stage 4). Roadmap and dates: PRO_QUALITY_PLAN B12.6.

Later, only when the owner can no longer look at every world (rung A3): light automatic checks in place of the vision judge; `render_compare` as a gym observation; multi-step conversation edits and reference-similarity gym tasks; sparse volumes as a first-class `Geometry` attribute; compositing (COP-lite), pyro/FLIP, Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write` and `pt docs-check`.
