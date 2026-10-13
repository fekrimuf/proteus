# Market: rival products, what is worth taking, and the money

Checked: 2026-10-13, from public pages, trade press, papers and company filings only ([DECISIONS.md](DECISIONS.md) B23). Every figure here is dated research, not a measurement of ours; check a figure again before relying on it in a contract or a pitch. What the route took from this is in [ROUTE.md](ROUTE.md).

## The products

| Group | Products looked at | What they sell | What they leave open |
|---|---|---|---|
| Artist tools | Gaea, World Creator, SpeedTree, Substance | one piece each (terrain, trees, materials), about $99 to $1,499 a licence, an artist at the controls throughout | no finished world, no autonomy |
| AI generators | Meta WorldGen (research), World Labs Marble ($20 to $95 a month) | a small scene from a sentence: WorldGen about 50 by 50 metres in about five minutes, as ordinary meshes with a walkable map | not changed by changing a rule; every customer gets the model's look; small |
| Engine maker | Epic, Unreal 5.8 | its own procedural tool driven by an AI model through a tool server; the City Sample demo was rebuilt this way, without outside procedural software | an assistant for an artist inside the editor: the artist directs and judges every step |
| Open research | Infinigen and Infinigen Indoors (BSD), ProcTHOR, Holodeck, SceneCraft, Blender's node system | rule-based nature and interiors for AI training; agents that write scenes | not built for games or for a studio's own style |

No product looked at takes a brief to a large, finished, engine-ready world in a style that belongs to one studio. Epic is the nearest in design (an agent driving a procedural tool) and the one to watch. Epic's own page refused the fetch, so its details come from search summaries of that page.

## Features worth taking

| Feature | Seen in | ROUTE row |
|---|---|---|
| Detail objects placed only near the camera | World Creator 2026.3 | 1 |
| Leaves hidden in a tree's shaded interior removed | SpeedTree 10 | 1 |
| A tree stored as reused parts | Unreal 5.7 foliage | 1 |
| Ground that changes around a placed object, terrain that shapes itself under it | World Creator 2026.3 | 4 and 5 |
| Vines that hang and drape by physics | SpeedTree 10 | 3 |
| Layout in three passes (landmarks, medium, small), walkability decided on the rough layout | Meta WorldGen | 7 |
| A river that meanders along a drawn guide line | Gaea 3.0 (announced) | 10 |
| Accepted results saved into a library the agent reuses | SceneCraft, WorldCraft | the recipe library |
| Buildings planned from one model of inside and outside | TU Delft | 5 |
| Rooms furnished by rules and a solver, large pieces first | Infinigen Indoors | 5a |

Interiors are the ground where the AI generators do best, because a room is small. Our ground is the join: many buildings, each with its own interior in the studio's style, entered from the landscape in one world.

## What the market has paid

| Evidence | Figure | Kind of source |
|---|---|---|
| AMD agreed to buy World Labs, 2026-09-28 | about $8.2 billion, all stock | AMD newsroom |
| World Labs funding, 2026-02 | $1 billion, at about $5 billion (reported) | trade press |
| Meshy funding, 2026-07 | nearly $400 million at $1.5 billion | company press release |
| Tripo funding announced in 2026 | about $846 million | trade press |
| Adobe bought the maker of Substance, 2019 | about $160 million | Adobe's filings |
| Unity bought Weta's tools, 2021 | $1.625 billion | company press release |
| SideFX yearly revenue | $26 to 34 million | data aggregators, unaudited |
| Keywords Studios: 2023 revenue; sale to EQT, 2024 | $844 million; £2.1 billion | trade press |
| One AAA budget (Spider-Man 2) | $315 million | leaked sheet, reported |
| Red Dead Redemption 2 | about 1,600 developers, $170 to 240 million for development | analyst estimates |
| Art's share of a AAA budget | 25 to 30% | one vendor article |
| Labelled-data market for AI training, 2026 | $0.7 to 2.5 billion, growing 25 to 31% a year | market-research firms that disagree |
| Game professionals who say generative AI harms the industry | 52%; 30% of studio staff use it | GDC survey 2026, 2,300 answers |

A procedural tool sold by itself is a business of tens of millions a year. Autonomous world building is priced in billions, in deals struck before those companies showed revenue.

## The lead's estimate (assumptions, not measurements)

| Revenue line | Arithmetic | Per year |
|---|---|---:|
| Mid-size studios | 150 at $30,000 (under a third of one environment artist; base salary about $80,000) | $4.5M |
| AAA studios | 5 at $500,000 | $2.5M |
| Worlds on demand (we run it, the studio receives the world) | 100 at $20,000 | $2.0M |
| Labelled worlds for AI laboratories | 5 at $200,000 | $1.0M |
| Total | | $10M |

At five to ten times revenue that is $50 to 100 million; the Substance sale puts the top of that case at about $160 million; the AI world builders' prices are the case above $1 billion. Prices and customer counts are the lead's assumptions. They become firm with a demo and about five private talks with studios. The business goal and the free and paid split are the owner's, later (B23).

What follows for the approach: sell worlds and the director, not the tool (Epic gives its tool away); worlds on demand is the shortest way to a first sale and keeps the director on our machine, where it cannot be taken apart; present it as a procedural tool that artists direct, with a clean title (the GDC figures).

## Sources

* Rivals: [Gaea 3.0](https://www.cgchannel.com/2025/12/quadspinner-unveils-gaea-3-0/), [World Creator 2026.3](https://www.cgchannel.com/?p=174659), [SpeedTree 10](https://80.lv/articles/speedtree-10-has-been-released/), [Unreal PCG 5.6 and 5.7](https://80.lv/articles/what-s-new-in-pcg-in-ue5-6-ue5-7), [Unreal 5.7 foliage](https://digitalproduction.com/2025/11/12/unreal-engine-5-7-foliage-pcg-and-in-editor-ai/), [Epic City Sample](https://www.unrealengine.com/learning/city-sample-gets-a-major-update-with-pcg-and-unreal-mcp-workflows), [Meta WorldGen](https://arxiv.org/html/2511.16825v1), [Marble](https://www.deeplearning.ai/the-batch/world-labs-makes-its-marble-generative-world-model-public-adds-chisel-editing-tool), [SceneCraft](https://arxiv.org/abs/2403.01248v1), [Horizon Zero Dawn placement](https://www.guerrilla-games.com/read/gpu-based-procedural-placement-in-horizon-zero-dawn).
* Interiors: [Infinigen Indoors](https://arxiv.org/abs/2406.11824v1), [TU Delft, consistent buildings](https://graphics.tudelft.nl/Publications-new/2011/TSLDB11a), [ProcTHOR](https://procthor.allenai.org/), [Holodeck](https://arxiv.org/html/2312.09067v2).
* Money: [AMD and World Labs](https://newsroom.amd.com/news/amd-acquire-world-labs/), [World Labs round](https://www.crowdfundinsider.com/2026/02/262836-ai-firm-world-labs-raises-1-billion-at-5-billion-valuation/), [Meshy](https://tools.prnewswire.com/en-us/live/20813/release/20260721EN06857), [Tripo](https://ai2.work/blog/tripo-ai-raises-446m-as-3d-foundation-models-draw-big-capital), [Adobe filing](https://www.sec.gov/Archives/edgar/data/796343/000079634319000142/adbe10qq219.htm), [Unity and Weta](https://www.businesswire.com/news/home/20211109006514/en/Unity-Announces-Intent-to-Acquire-Weta-Digital/), [Keywords](https://www.pocketgamer.biz/keywords-studios-sells-for-21-billion-in-2450-per-share-eqt-buyout), [Spider-Man 2 budget](https://mp1st.com/news/marvels-spider-man-2-budget-was-315-million-leaks-reveal), [Red Dead Redemption 2](https://en.wikipedia.org/wiki/Development_of_Red_Dead_Redemption_2), [AAA cost shares](https://juegostudio.com/blog/everything-you-need-to-know-about-aaa-game-development-costs), [environment artist pay](https://www.payscale.com/research/US/Job=Environment_Artist/Salary), [labelled-data market](https://www.meticulousresearch.com/product/synthetic-data-generation-market-6933), [GDC survey 2026](https://gdconf.com/article/gdc-2026-state-of-the-game-industry-reveals-impact-of-layoffs-generative-ai-and-more/).
