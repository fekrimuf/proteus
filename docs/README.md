# Documentation index

Read in this order: [../CLAUDE.md](../CLAUDE.md) (role and reporting rules), then [STATUS.md](STATUS.md). Everything else is on demand.

| File | What it is | How it changes |
|---|---|---|
| [STATUS.md](STATUS.md) | current state, open items, next steps; 8 KB cap | rewritten at every wrap-up |
| [generated/state.md](generated/state.md) | every count and score, produced from the code and result files | `pt status --write`, never by hand |
| [PLAN.md](PLAN.md) | master plan: thesis, architecture bets, phases, risks, research decisions D1 to D10 | rarely |
| [PRO_QUALITY_PLAN.md](PRO_QUALITY_PLAN.md) | owner brief (Part A), runbook and tracks (Part B), north star | when the approach changes |
| [DECISIONS.md](DECISIONS.md) | decisions that stay fixed until new evidence (B11, B15, B16) | when a decision is made |
| [LOG.md](LOG.md) | append-only dated history and measured results | appended, never read at startup |
| [ENVIRONMENT.md](ENVIRONMENT.md) | installed tools, paths, hashes, gotchas by tool | when a tool or path changes |
| [TOOLS.md](TOOLS.md) | crates, commands, MCP tools, the docs harness | when a command is added |
| [AGENT_GUIDE.md](AGENT_GUIDE.md) | how an agent drives the engine: loop, language, checking, matching | when the agent surface changes |
| [WORLD_GUIDE.md](WORLD_GUIDE.md) | materials, light, real terrain, engine delivery, world lessons | a section per world lesson |
| [TECHNIQUE_NOTES.md](TECHNIQUE_NOTES.md) | notes from third-party tutorials | when a source is mined |

Outside `docs/`: the research behind the plan is in `reports/` and `research_notes/` (a fixed archive); corpus sources in `corpus/SOURCES.md`; checkpoint notes beside their results in `corpus/results/`.

Where a fact goes: intent and architecture, PLAN; a choice with its reason, DECISIONS; what is true now, STATUS; what happened on a date, LOG; a number that changes, nowhere by hand (it is generated).
