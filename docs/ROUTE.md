# Route map: ranked work to the north star

Updated: 2026-10-13. Written for choosing the next item: biggest lever per agent-hour first. Intent is in [PLAN.md](PLAN.md), choices in [DECISIONS.md](DECISIONS.md), the stage gates and old dates in [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) B12.6, today's state in [STATUS.md](STATUS.md). This file holds what none of them hold: every open piece of work in one ranked list, with evidence, the route and a cost.

## Calibration of the hours

An agent-hour is one agent session's wall-clock hour, tests and guide included. Measured on this repository's own history: between the commits of 2026-10-12 23:10 and 2026-10-13 01:15 the capabilities 1 to 6 of [DECISIONS.md](DECISIONS.md) B18 landed and the tree family began, about two hours for six capabilities, 0.3 hours each. The plan dates in B12.6 and B18 were set at one gated step per working day, which is five to ten times slower than what the history shows, so the dates below are re-set from the measured pace. The planning basis is three times the measured pace: 1 hour for a small or medium step, 3 hours for a large one, plus the owner's turnaround where the owner's eye is the gate. Renders are limited by the machine, not the agent: about ten minutes a frame at owner quality, one at a time.

## The ranked route

| # | Work | Evidence | Route | Hours | Plan date (was) | Unlocks |
|---|---|---|---|---:|---|---|
| 1 | **Owner-grade frames fit this machine** | Look-development renders fit with a crop and a 10 GB cap; the full-quality world does not fit one Cycles import ([STATUS.md](STATUS.md)) | Load far stand-ins and near detail per camera tile; finish the live render session of capability 4 so a change sends only what moved | 3 to 6 | 10-15 | the owner's 10 blind pairs (stage 1 gate) |
| 2 | **Light and air** | The frames against the reference have no light shafts and flat haze | Volume scatter and sun shafts in the render script, driven by style knobs | 1 to 2 | 10-15 | the largest visible gap per hour; every later frame |
| 3 | **Nature that grows, finished** (capability 7) | Close trees show cards as leaf shapes; no ecology pass; one conifer only | Finer sprays; ecology rules on queries (saplings near mature trees, shade ferns, tree line); broadleaf and ground-cover generators; the two-brief gate | 4 to 8 | 10-16 (11-03) | stage 2 gate: a "sci-fi village" brief gets no Earth trees |
| 4 | **Procedural materials** (capability 8) | Smooth banks without gravel; roof reads as planks, not thatch | Texture graph in the same language, curvature and occlusion masks, fitted to a reference photo by the optimizer, baked for engines | 4 to 8 | 10-18 (11-10) | the exit from scanned materials; stage 3 |
| 5 | **Buildings and layouts from grammars** (capability 9) | No mill; one hut | Split, repeat and component rules, roofs, pads and curve carving, a water wheel | 6 to 12 | 10-22 (11-24) | the building kit; the "medieval village in a sci-fi sense" brief |
| 6 | **Style tools**: extract, blend, mutate, apply, score | Styles are files a world reads; nothing yet derives one from a reference or mixes two (B16) | `pt style` commands on the registry, each also an agent tool | 4 to 8 | 10-20 (11-03) | "unique to each studio", the second half of the stage 2 gate |
| 7 | **Scene composer** | Cameras and sun are placed by hand; `pt pick` already compares viewpoints by casting rays | Rules choose camera, sun and building sites, ranked by pick rays and the placement checks | 3 to 6 | 10-24 | the director needs no human for framing |
| 8 | **USD export of the layer stack** (tail of capability 6) | World file has variants, no USD sublayers | One sublayer per variant over the base geometry, edits included | 1 to 3 | 10-17 | engine handoff |
| 9 | **Delivery check, two fixes** | A stalled Godot import once blocked the scorecard; buried instances cost memory (`pt ground-check` lists them) | A timeout on `run_godot`; drop buried instances in the placement rule | 0.5 to 1 | 10-14 | a scorecard that always finishes |
| 10 | **Systems** (stage 4) | Terrain, masks, biomes, roads, rivers, settlements not built | As B12.6 | 15 to 30 | 12-08 | a second biome by parameters; a small town |
| 11 | **Autonomous director** (stage 5) | Art bible compiler, planner, playtest bots, repair loop not built | As B12.6 | 25 to 50 | 2027-01-15 | one square kilometre from a brief |
| 12 | **World scale** (stage 6) | Joins, streaming, HLOD, engine bridge not built | As B12.6 | 15 to 30 | 2027-03-01 | a 10 km walkable world |
| 13 | **One paragraph in, finished world out** (stage 7) | Needs the data the generators produce | A locally trained proposer on that data | 25 to 50 | 2027-05-31 | the north star |

Rows 1 to 9 total about 27 to 54 agent-hours. Rows 10 to 13 keep their plan dates: nothing measured yet says they move.

## Lanes

Run one after another, rows 1 to 9 take about four to nine working days. Their only dependencies are 5 after 4 (buildings use the materials), 7 after 5 (the sites are for buildings) and the blind pairs after 1 and 2, so three lanes can run side by side, each in its own git worktree with its own build folder, with the gate ([TOOLS.md](TOOLS.md)) as the merge test and the command registry keeping the lanes out of each other's files:

* **Lane A (the critical path to the owner's gate):** rows 1, 2, then the blind pairs, then 8 and 9.
* **Lane B (nature and style):** rows 3, then 6.
* **Lane C (surfaces and buildings):** rows 4, then 5, then 7.

The longest lane is C (13 to 26 hours), so three lanes bring rows 1 to 9 to about two to four working days. One job that takes the GPU or several GB of memory runs at a time, whichever lane asks. The lanes start when the owner agrees to the extra use of the agent allowance.

## What the owner decides

The 10 blind pairs when row 1 delivers the frames (about 10 minutes, date 10-15 to 10-17), the CC-BY yes or no by 2026-11-01 (default: CC0 only), the product name before any public release, and the go-ahead for the three lanes.
