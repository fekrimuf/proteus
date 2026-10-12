## In plain terms

- We took 50 professional models, all public domain: 40 low-poly props and building pieces from Kenney, and 10 furniture and props from Poly Haven. For each one the computer tried to write a short recipe that rebuilds it, then measured the result against the original.
- **It works.** 24 of the 50 (48%) reached "silver" or better by the plan's score. The plan asked for 3 in 10. On the 10 models held back for testing it was 5 of 10. I also looked at all 24 pictures: 6 of them missed a feature you would notice, and the part-counting vision judge (built afterwards) found 3 more that my first look had missed (a barrel without its rings, a bench without its scrolled arms, a bin without its domed lid; pictures re-read and confirmed). On that stricter reading the count fell to 15 of 50, with development 2.5 points under its bar. I then repaired six of the nine the same day (both barrels, the ladder, the shelf, the bin lid, the fountain), each checked by the judge and by the numbers, so the stricter reading now stands at **21 of 50 (42%): 17 of 40 on development and 4 of 10 on the hold-out**, both passing the gate. Three remain: the cactus arms, the bench's scrolled arms, and the tower (its shape is rebuilt with battlements but its score is bronze, so the plain block still stands for it).
- **Two routes were used.** Route A is plain computer search with no AI in the loop: 13 of 50 (26%), about 10 seconds each. Route B has an AI model write the structure of the recipe (loops, mirrors, named dimensions) from a text description and pictures of the original, then the computer tunes the numbers: 12 of the 41 models it was tried on, about 1.1 AI turns and 4 seconds of computer time each. Together they solve 24.
- **The recipes are small and easy to edit.** Route B's winners have 7 to 23 editable numbers (middle value 14); route A's winners have 6 to 40 (middle value 30). The bench recipe has 13 numbers: change the width once and the legs, rails and stretcher follow.
- **What works best:** furniture and containers made of boxes and tubes. 7 of the 11 single shapes were solved by route A alone, and route B added bookcase, sofa, table, chair, bench (two kinds), stall, ladder, fence gate, door wall and gable roof.

## Why the other 26 stopped, and the route for each

- **Thin sticks and posts** (fence, signpost, lantern, tent, flag): the surface matches (often 0.95 to 1.00) but the outline overlap lands at 0.80 to 0.90 against a bar of 0.90. About ten of the 26 are within 0.03 of the bar on one of the two measures, for example the Poly Haven table (outline 0.898) and the Kenney fence section (outline 0.898). The bar stays where the plan put it. Route: the tuning step now scores all ten outline views, which already moved several models over the line; next are thickness-aware fits (tubes along a skeleton) and more tuning restarts.
- **Slanted and bevelled stone** (castle wall, tower, gate, stairs): the building blocks are boxes, so bevels and arches cost surface match. Route: add wedge, arch and lathe pieces. A triangular prism already solved the gable roof in 7 numbers, and motif mining will promote the step array and the arch.
- **Scanned furniture with surfaces inside** (cabinet, television): the original has interior faces that a solid recipe does not reproduce, so coverage stops near 0.8. Route: report an "outside surface only" reading next to the main score (kept separate so the main score never moves), or model the hollow.
- **Busy or organic objects** (rock formation, canoe, cart, campfire, catapult): Route: solve part by part, one AI turn per part, then the learned proposer of Phase 3. The catapult (more than 30 parts) was not attempted by route B.
- **One fails only on a mesh check:** the vase matches closely (surface 0.97, outline 0.97) but is an open shell where the original is closed. Route: close the profile; mechanical.

## What these numbers do and do not show

- Each model was fitted to its own answer key. The result shows that the tools can express and match these models, not that the system predicts unseen ones. Route: Phase 3 trains a proposer on synthetic data and tests it on held-out parts with no AI calls (gate G4).
- The plan's score can pass a rebuild that is missing a thin part: the ladder has four rungs instead of eight and the shelf has no shelves, and both scored silver. A second automatic check, the largest uncovered patch of surface, agrees on the ladder and the tower but not on the shelf, where each missing shelf is a small patch. Route (built and used): a part-counting vision judge (`pt judge parts`) that found 8 of the 9 known missing-feature defects with no false alarms on the 15 good rebuilds; it is how the ladder, the shelf, the barrels, the bin lid and the fountain were repaired and verified. It runs on every rebuild from here on, next to your blind pairs.
- 50 models is a small sample. The 95% range for 24 of 50 is 35% to 61%. Route: Phase 1 runs 150 more models.
- The hold-out (5 of 10) is only a partial test, because its results were visible to me while I fixed the solver. Route: a fresh 50 models for a clean test; the hold-out becomes strict once pattern mining starts.
- The AI model here is Claude, with 46 turns in total for 41 models and no turn spent on the 9 models route A had already solved. Route: record turns per model (done) and compare other models by dropping their recipes into the same folder; Phase 3 removes the AI from the loop.
- No person has looked yet. Checkpoint 3 (you pick the original in at most 7 of 10 pairs) is the real test of "looks right". The twelve pairs below were picked by a fixed random seed, and several of them are failures.
- Not built yet: the Khronos glTF validator and the DINOv2 judge (Phase 2), and a Blender import test of our exports. Route: both are planned downloads, no credentials needed.

## One source was dropped

Quaternius changed its licence on 28 August 2026 to a custom one that is not CC0. Their pack pages still say CC0, so the claims contradict each other. The 10 planned slots went to more Kenney models. Everything in the trial is CC0 and every file has a hash and a licence row in the ledger.

## Recommendation (you can decline)

- **Continue.** Checkpoint 1 is green.
- Suggested order: first Level 2 steps 1 and 2 from the plan (material names and a lighting description that Blender renders), a day or two of work, because the probe below shows appearance is the cheapest lift and our exports carry no material or light at all. Second, Phase 1: 150 more models with library learning; the recipes already repeat the same motifs (a leg mirrored twice, an array of steps or shelves, posts and rails). Third, a fresh 50 for a clean test.
- Needed from you: nothing now. Please look at the twelve pairs and the probe image and tell me if any look wrong to you.
