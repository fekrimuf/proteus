# Corpus sources and licence decisions

Raw files live outside the repo (`D:\Fekri\corpus\<source>\`, override with `PT_CORPUS`). Every file used has a row in
[LEDGER.csv](LEDGER.csv) (sha256, URL, licence, fetch time). `pt corpus verify` re-hashes them and rejects any
licence that is not `CC0-1.0`. The list of assets is [manifest.json](manifest.json).

| Source | Status | Evidence (checked 2026-10-10) |
|---|---|---|
| Kenney | **used** (40 assets, 6 packs) | Each pack's own `License.txt` says "Creative Commons Zero, CC0" (kept as ledger rows `kenney/licenses/*_License.txt`); https://kenney.nl/support (sha256 `4c71d011...2f9`) says all assets are public domain (CC0). |
| Poly Haven | **used** (10 props + 3 probe inputs) | https://polyhaven.com/license (sha256 `6f4f617c...bba`): "Our assets are all licensed as CC0". The public API terms are separate; they ask for a "Powered by Poly Haven" credit when the API is used in a product. We fetch a few dozen specific files by name, no crawling. |
| Quaternius | **dropped** | Their licence page https://quaternius.com/license.html (sha256 `d1b56be5...b0d`, "Last updated: 8/28/2026") now publishes the **Quaternius Asset License (QAL) v1.0**, not CC0: free for personal/educational/commercial use with no credit, but it forbids redistributing the assets as standalone assets, asset packs or "similar product". The pack pages and the home page still say "CC0", which contradicts the licence page. The plan's rule is "CC0 only, otherwise drop", and the downloads sit behind Google Drive folders, so the 10 planned Quaternius slots were filled with 10 more Kenney assets. Revisit only if Quaternius states per-pack CC0 terms in writing. |

Not in the trial (unchanged from the plan): Objaverse/Objaverse-XL (per-object licences), ABC (research use),
Infinigen outputs (output licence unstated).

## Decisions

* Hold-out: the 10 assets (20%) whose id hashes lowest with BLAKE3 (`pt match-bench` prints and stores the list). They are
  never used for motif mining and are reported separately.
* The trial list was fixed before any solver ran, to span single shapes ("primitive"), multi-part props ("compound") and
  "building" pieces.
