# Project status and handoff (read this first)

Updated: 2026-10-12 (rewritten at every wrap-up, never appended; history goes to [LOG.md](LOG.md))

Codename **Proteus**: a free, agent-native, Houdini-class procedural 3D tool in Rust whose north star is an autonomous system that makes a finished AAA world from a short brief, in a style matched or invented and unique to each studio. Index of all documents: [README.md](README.md).

Numbers (tests, recipes, gym, ledger, scorecard runs) are not written here: they are in [generated/state.md](generated/state.md), produced by `pt status --write`. Machine, paths and gotchas: [ENVIRONMENT.md](ENVIRONMENT.md). Commands: [TOOLS.md](TOOLS.md).

## State

* **Engine and tools:** the kernel, language, simulation, gym, vision judges (local Qwen3-VL-8B), Blender Cycles renderer, Godot engine referee and the `pt scorecard` all work end to end. Cross-OS determinism holds in CI.
* **Worlds:** `scenes/valley` (analytic), `scenes/teton` (real USGS elevation, scorecard v4: judge brief fit 6.0 of 10 against a bar of 7) and `scenes/cascade` (the eye-level creek built for the owner's target picture; the engine rows measured 106 fps, 97.2% walkable ground, validator clean; its render frames hold up when opened).
* **Assets:** Checkpoint 1 on the strict reading is 21 of 50 (development 17 of 40, hold-out 4 of 10); open repairs are the cactus arms, bench scrolls and tower score.
* **Visual bar:** the owner's reference is a Skyrim-style creek with a timber mill (path in ENVIRONMENT.md). Frames are compared with it by eye; the valley is not close.

## Open items from the 2026-10-11 wrap-up (in this order)

1. Run the full scorecard for the creek world: `pt scorecard scenes/cascade.hcs scenes/cascade.scene.json --out out/steelthread_v5 --samples 128 --baseline corpus/results/steelthread/v4/scorecard.json` (it had rendered four frames and was baking terrain when the session ended). Then copy frames and `scorecard.json` to `corpus/results/steelthread/v5/`, add the entry to LOG.md, and update the published "Readiness Scorecard" page.
2. Render one frame with `tune_authored` in `corpus/tools/render_scene.py` and the two `rock_moss_set_*` style entries (they grey the scanned rocks that render orange in direct sun in `frame_east.png`).
3. Move the `village` camera onto the lit side of the hut.
4. Triangles in one frame were 20.6 million against a bar of 20; one more trim went in after that measurement and is not measured yet.
5. Give `run_godot` a timeout: a stalled Godot import blocked the scorecard once (the 270 000-node import). The tiled exporter removed that cause; the timeout removes the class.

Verified on 2026-10-12: `cargo check -p pt-cli` compiles the `scorecard --reuse` and bake-cache edits that were written but never compiled; the full test run is recorded in the log entry of that date. Everything up to this update is committed (CLAUDE.md: commit on your own judgement, locally).

## Next after the open items

1. A tree and ground-cover generator family on morphology knobs (DECISIONS.md B16a): closes repetition (15 copies of one tree in a 50 m square against a bar of 12), lowers the CC0 share, makes the look unique per studio.
2. A scene composer that picks camera, sun and building sites by rule (they were picked by hand).
3. A look-development version of a scene that renders in under a minute.
4. A building kit beyond the one hut; the engine look (terrain detail, water, grass) closer to the render.
5. Terrain statistics as a scorecard row, then the owner's blind pairs against a hand-built baseline (the W1 gate), then Phase 1 mining on 150 more assets and a fresh 50.

Later, in this order: a stronger local judge only if the fresh-50 test shows misses (Qwen3-VL-30B-A3B with expert offload to CPU); the DINOv2 embedding judge for style distinctness (DECISIONS.md B16); `render_compare` as a gym observation; multi-step conversation edits and reference-similarity gym tasks; sparse volumes as a first-class `Geometry` attribute; compositing (COP-lite), pyro/FLIP, Alembic/FBX. After any numeric change keep `recipes/golden.json` and `gym/baseline.json` in sync (`pt recipes --update`, `pt gym-baseline`).

## How this documentation stays healthy

Facts are filed by how long they stay true: never-changing intent in [PLAN.md](PLAN.md) and [DECISIONS.md](DECISIONS.md); this file for the current state; [LOG.md](LOG.md) for history; [generated/state.md](generated/state.md) for numbers. `pt docs-check` (also in CI) fails on a stale state file, broken links, a file over its size budget (this one: 8 KB), a banned heading, a mangled Windows path, or a STATUS older than the last commit. Wrap-up routine: move what happened to LOG.md, rewrite this file, run `pt status --write` and `pt docs-check`.
