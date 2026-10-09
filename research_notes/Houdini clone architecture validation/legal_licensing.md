# Legal, Licensing and Trademark Considerations for a Clean-Room, Houdini-Compatible Open-Source 3D Tool

Research notes, not legal advice. Search was limited (about 20 tool calls). Anything I could not source is under Gaps. Several search summaries were secondary; where I fetched a primary page I say so.

## 1. Copyright vs functional elements (Google v. Oracle, SAS v. WPL, Lotus v. Borland, clean-room precedents)

### Takeaway
US law has not settled whether API or command names are copyrightable. The Supreme Court assumed they were and still found reimplementation fair use. The EU's highest court says functionality, language and file formats are not protected. Both favour reimplementing behavior and names for compatibility, as long as no code, docs text or assets are copied.

### Cited Findings
- In Google v. Oracle (6-2, April 2021) the Court did not decide whether the Java API declaring code is copyrightable. It assumed it was and held Google's copying of about 11,500 lines was fair use. — [Jones Day](https://www.jonesday.com/en/insights/2021/04/supreme-court-google-wins-copyright-battle), [Mondaq](https://mondaq.com/unitedstates/copyright/1056046/supreme-court-holds-that-googles-use-of-oracles-java-api-is-fair-use)
- The Federal Circuit (2014) had held that API declaring code and structure can be copyrighted. That ruling is not binding nationwide and the Supreme Court did not adopt it. — [search summary of Oracle v. Google / Lotus](https://www.softwarefreedom.org/blog/2022/jan/18/google-oracle-endera/)
- Lotus v. Borland (1st Cir. 1995) held a menu command hierarchy is an uncopyrightable "method of operation". The Supreme Court affirmed by a 4-4 tie, so it is persuasive authority only. — [Lotus v. Borland](https://hallapproved.com/us/cases/ca1/1995/195952)
- CJEU SAS Institute v World Programming (C-406/10, 2 May 2012): copyright does not cover a program's functionality, programming language or data file formats. A licensee may observe, study and test the program to learn its ideas, and can write rival software using them. Copying manuals or other protected expression can still infringe. — [Computerworld](https://www.computerworld.com/article/2504590/eu--programming-languages-can-t-be-copyrighted.html), [Fieldfisher](https://www.fieldfisher.com/en/insights/sas-institute-inc-v-world-programming-limited)
- EU Software Directive 2009/24/EC Art 5(3) lets a lawful user observe, study or test a program's functioning to learn its ideas. Art 6 permits decompilation only when indispensable for interoperability, with conditions. CJEU Top System (C-13/20) allowed decompiling for error correction. — [Art 6 text](https://www.legislation.gov.uk/eudr/2009/24/article/6), [Cuatrecasas on Top System](https://www.cuatrecasas.com/en/spain/art/decompilation-of-software-for-error-correction-cjeu-judgment-in-top-system)
- ReactOS's clean-room rule: one person examines the implementation and writes documentation, another implements from that documentation. Contributors who saw leaked Windows code were barred from the matching modules. The audit results were never made public, and critics disputed the project's cleanliness. — [Linux.com](https://www.linux.com/?p=526307), [ReactOS list](https://lists.reactos.org/hyperkitty/list/ros-dev@reactos.org/message/VNIH5MJZDPFPNMKHFHVZS2O77RG4PFR5/), [OSNews](https://www.osnews.com/story/13467/reactos-explains-troubles/)
- Wine reportedly refuses contributions from anyone known to have seen Windows source or done decompilation. This came from a Hacker News comment, not Wine's own docs. — [HN](https://news.ycombinator.com/item?id=7558415) (unverified)

### Inferences
- Node names, parameter names and VEX function names are the functional "interface" layer. They are the strongest case for being unprotectable or fair use (Lotus, SAS, the Google v. Oracle reasoning). The risk is rising from "names" to "copying of expressive structure". Copying SideFX's documentation prose, example scenes, icons, shelf layouts or node help text is the real exposure.
- The PLAN's clean-room line (public papers, docs, observable behavior) matches the Wine, ReactOS and SAS model. See §3: the contract (EULA) is a larger risk than copyright.
- Using the same names for behavior-compatible nodes (e.g. `copytopoints`, `scatter`, `attribwrangle`) is likely lawful. Short, functional names also have weak originality, but that is my inference, not a cited holding.

### Gaps
- I found no primary-source court opinions (only law-firm and press summaries) and no decision on node-graph names in 3D software specifically.
- I did not research the status of Natron vs Nuke, OpenToonz, GIMP, Krita, Godot or Blender as legal precedents. None was litigated as far as I know, and I have no source. Treat them as practical evidence of tolerated ecosystems, not as legal authority.
- Whether the cited US law applies to the project depends on where the contributors and maintainers are located.

## 2. SideFX trademarks and naming

### Takeaway
SideFX holds the Houdini marks, and USPTO records for the word mark are only partly confirmed. SideFX's contract text bars use of its names and logos without written consent. Use the names only descriptively and sparingly, and keep them out of the product name, crate names, logo and domain.

### Cited Findings
- A word mark HOUDINI (Reg. 2157135, registered 1998, renewed) exists for computer animation software. The aggregator did not show the owner. A Foundry legal notice states Houdini is a registered trademark of Side Effects Software. — [Justia](https://trademarks.justia.com/746/83/houdini-74683775.html), [Foundry legal](https://learn.foundry.com/hiero/12.2v10/content/legal.html)
- Side Effects' "HOUDINI 3D ANIMATION TOOLS" logo registration (4623232) was cancelled in 2021 for lack of a Section 8 filing. Other filings (SIDEFX, KINEFX, KARMA XPU, SIDEFX SOLARIS) are listed with unclear status. — [Justia owner page](https://trademarks.justia.com/owners/side-effects-software-inc-2585776)
- The EULA (s.13.12) forbids using the other party's "name(s), logo(s) or trade-mark(s)" without written consent. The license changelog says "Removed grants of consent to use names and logos." — [SideFX license agreement](https://www.sidefx.com/legal/license-agreement)
- SideFX's logo guidelines (2016 PDF) cover its brand logos and colors. — [SideFX logo guidelines](https://www.sidefx.com:443/media/uploads/company/press/logos/sidefx_logo_usage_guidelines.pdf)
- SideFX's own file-extension conventions: .hip/.hda (commercial), .hiplc/.hdalc (Indie), .hipnc/.hdanc (Apprentice). — [SideFX forum](https://www.sidefx.com/forum/topic/90789/) and [RenderHub](https://www.renderhub.com/guides/3d-file-formats/hiplc) (secondary)
- OpenSubdiv and OpenUSD licenses both replace Apache's Section 6 so the license grants no trademark rights. Pixar and others take their marks seriously. — [OpenSubdiv license](https://graphics.pixar.com/opensubdiv/license.html), [OpenUSD LICENSE.txt](https://github.com/PixarAnimationStudios/OpenUSD/blob/dev/LICENSE.txt)

### Inferences
- Nominative use ("imports a subset of Houdini .hip files", "compatible with Houdini concepts") is the generally accepted approach under US law. I did not find a primary source on the nominative fair use test, so treat that as general knowledge.
- Do not use "Houdini" in the project name, binary name, crate names, logo, domain, or in "X for Houdini" forms that suggest endorsement. The PLAN's "HoudiniGym" and the repo name "Houdini-clone" should be renamed before any public release.
- "Solaris", "Karma", "KineFX", "PDG", "TOPs", "VEX", "VOP", "SOP", "HDA": the plan uses these as feature labels. Some are claimed marks (KineFX, Karma XPU, Solaris) or product names. "VEX" and "HDA" are not shown as registered in my results (unknown, not verified). Use our own terms in product UI (for example `hcs`, "wrangle", "assets").
- File extensions are not trademark issues in themselves, but giving our native format `.hip`/`.hda` would invite confusion. Use our own extension and have an importer that reads SideFX's.

### Gaps
- I could not reach USPTO TSDR or a SideFX trademark policy page; owner and status of the HOUDINI word mark and the status of KARMA, SOLARIS, VEX, PDG marks are unconfirmed.
- No source found for non-US trademark registrations (EU, Canada, where SideFX is based).

## 3. SideFX EULA, docs, HDK, Apprentice/Indie, and clean-room process

### Takeaway
This is the highest-risk area. The EULA (read from the primary page) contains clauses that bar using Houdini to build a competing product, to copy its features, or to run competitive analysis. That undermines the PLAN's "parity checks against observable Houdini behavior" and any use of Houdini by implementers. File-format work is only restricted when used to bypass licensing, but the license-tier formats must not be converted.

### Cited Findings
- Section 4.1(vii): no reverse engineering, decompiling or disassembling to discover source code of the Software. — [SideFX EULA (Oct 13, 2021 version)](https://www.sidefx.com/download/eula/)
- Section 4.1(viii): no reverse engineering or use of the file format of files the Software generates in order to bypass the Software's restrictions or requirements. The restriction is purpose-qualified. — [SideFX EULA](https://www.sidefx.com/download/eula/) (paraphrased by the fetch tool)
- Section 4.1(x): no converting a file generated under a Non-Commercial License into a file format generated under a Commercial License. — [SideFX EULA](https://www.sidefx.com/download/eula/)
- Section 4.1(ix) bars circumventing licensing mechanisms; 4.1(xi) bars tampering with usage information. — [SideFX EULA](https://www.sidefx.com/download/eula/)
- Section 4.2: Licensee will not (i) use the Software for competitive analysis of or with it, (ii) access the Software to compete, build a competing product or service, or impair its market, or (iii) copy any features, functions, graphics or other components. — [SideFX EULA](https://www.sidefx.com/download/eula/)
- The fetched EULA text does not mention the HDK. — [SideFX EULA](https://www.sidefx.com/download/eula/)
- The HDK headers are proprietary; HDK sample code is under a BSD-style license (2005-era forum post, current text unconfirmed). — [SideFX forum](https://www.sidefx.com/forum/topic/4298/)
- Apprentice is non-commercial only; Indie is limited to organizations under $100k revenue and $1M funding, and cannot be in the same pipeline as commercial Houdini. The Apprentice render-size cap is stated inconsistently on SideFX pages (720p vs 1080p). — [SideFX Indie FAQ](https://www.sidefx.com/faq/question/who-may-use-indie/), [Apprentice restrictions](https://www.sidefx.com/faq/question/apprentice-restrictions/)
- A US appellate decision on a different company's license (SAS v. WPL, 4th Cir. 2017) reportedly read a reverse-engineering ban broadly; contract terms can restrict study even when copyright law does not. — search summary only, unverified; see [SAS v. WPL (UK)](https://caselaw.nationalarchives.gov.uk/ewhc/ch/2013/69) for the separate UK case
- EU law: the Directive's Art 5(3) study right and Art 6 decompilation right are in the Directive; I did not verify whether they bar contract overrides for each. — [Bird & Bird commentary via search](https://books.twobirds.com/?p=163)

### Inferences
- Anyone who accepted the EULA and then uses Houdini to study node behavior for the clone risks breach of contract under 4.2(i)-(iii), regardless of copyright law. The EU Art 5(3) study right may help in the EU, but that is uncertain and the Apprentice/Indie licenses are what most contributors would have.
- Reading SideFX public documentation is a different act from using the Software. The EULA text I read does not cover documentation, and the SideFX Terms of Use page was not fetched. Using docs to learn behavior (SAS v. WPL) is lower risk, but copying doc text or images is not.
- Reading .hip/.hda files that users give us (such as ASCII-converted `hscript` or `.hip` text) is not obviously restricted by 4.1(viii), since our purpose is interoperability and not bypassing a licensing check. Do not touch the `lc`/`nc` license-enforcement parts, and do not offer conversion from nc/lc files into commercial formats.
- Independently documented behavior (published papers, Bgeo/`.geo` JSON docs on the SideFX website, user reports, tutorials) is the safe input. The PLAN §3.6 line "against observable Houdini behavior" should be limited to what a third party has published, or run by someone who is not a contributor.

### Gaps
- Not read: the full current EULA beyond sections 4.1, 4.2 and 13.12, the Terms of Use, the HDK license shipped with the SDK, the Houdini Engine license, and the Houdini docs license.
- I found no source on whether SideFX publishes the .bgeo/.geo format spec. In my own knowledge, SideFX documents the JSON geometry (.geo) format and the HDK exposes bgeo, but I have no source here.
- No SideFX statement or case law on third-party .hip parsers (hou-like tools, other open parsers) was found.
- I cannot say whether Apprentice-license users who observe behavior breach 4.2; ask a lawyer.

## 4. Open formats and libraries: license compatibility (MIT / Apache-2.0 project)

### Takeaway
USD, OpenVDB (since 12.0), Embree and OpenSubdiv are under Apache-2.0 or a variant, and are compatible with an MIT or Apache-2.0 project. Check OpenVDB's version (older releases were MPL-2.0). I could not source licenses for Alembic, MaterialX, OpenEXR, OSL, glTF.

### Cited Findings
- OpenUSD is under the "Tomorrow Open Source Technology License 1.0", identical to Apache 2.0 except Section 6 (trademarks). Includes the standard patent grant, which ends if you sue alleging the Work infringes a patent. — [OpenUSD LICENSE.txt](https://github.com/PixarAnimationStudios/OpenUSD/blob/dev/LICENSE.txt), [AOUSD forum](https://forum.aousd.org/t/upcoming-openusd-license-update/1561)
- OpenVDB moved from MPL-2.0 to Apache-2.0 with 12.0 (released 2024-10-31) with sign-off from major copyright holders. — [Phoronix](https://phoronix.com/news/ASF-OpenVDB-12.0), [openvdb.org](https://www.openvdb.org)
- OpenSubdiv is under a modified Apache 2.0 (Section 6 replaced). Pixar says it picked Apache to convey its patents to users and that all its subdivision-surface patents were released for public use. — [Pixar license](https://graphics.pixar.com/opensubdiv/license.html), [Pixar intro](https://graphics.pixar.com/opensubdiv/docs/intro.html)
- Embree is Apache-2.0. The old bundled-TBB GPLv2-with-exception worry was raised in a 2019 thread; Embree can be used without TBB. Rust wrappers have their own licenses (the `embree` crate is listed as MIT; `embree3` unconfirmed). — [Embree README mirror](https://github.com/horzelski/embree/blob/master/README.md), [Intel forum](https://community.intel.com/t5/Intel-Embree-Ray-Tracing-Kernels/License-Issue/m-p/1083455), [lib.rs embree](https://lib.rs/crates/embree)

### Inferences
- Apache-2.0 deps are fine in an MIT or Apache-2.0 project, and you keep notices (NOTICE file). If you pick MIT alone you lose the patent grant for contributors. Use Apache-2.0 (or MIT OR Apache-2.0, the Rust norm) so patent grants and retaliation are consistent with dependencies.
- Old OpenVDB MPL-2.0 is file-level copyleft; compatible if you keep OpenVDB files separate, but prefer 12.0+.
- Linking pure-Rust re-implementations (own sparse-volume code) avoids C++ build dependencies and licensing audit, and matches the PLAN's Rust-only stance.

### Gaps
- Alembic (believed BSD-3), MaterialX (believed Apache-2.0), OpenEXR (believed BSD-3), OSL (believed BSD-3), glTF (spec under Khronos terms; assets vary), OBJ (de facto, no license), FBX (Autodesk SDK is proprietary, not an open format): none verified in this session. The PLAN lists FBX as interop; the Autodesk FBX SDK terms must be checked, or use an independent reader.
- Intel oneAPI terms (relevant only if using oneAPI-distributed components such as Open Image Denoise; OIDN is believed Apache-2.0): unverified.
- Bevy (believed MIT OR Apache-2.0), egui (believed MIT OR Apache-2.0), wgpu (believed MIT OR Apache-2.0): unverified here; confirm from each repo.

## 5. Rust dependency hazards and cargo-deny practice

### Takeaway
Gate dependencies in CI with cargo-deny covering licenses, advisories, bans and sources. Allow permissive licenses, deny GPL/AGPL by default, and handle exceptions explicitly.

### Cited Findings
- cargo-deny has four check groups: advisories (RustSec DB), licenses (SPDX allow list), bans (specific crates, duplicate versions, wildcards) and sources (allowed registries and git hosts). `cargo deny init` generates a commented config. Official docs are at embarkstudios.github.io/cargo-deny. — [example deny.toml files](https://rust.googlesource.com/cargo/+/refs/heads/rust-1.97.0/deny.toml), [Atuin deny.toml](https://redirect.github.com/atuinsh/atuin/blob/main/deny.toml)
- Older examples use `unlicensed = "deny"` and `vulnerability = "deny"`; newer configs use `[licenses] version = 2` and a `confidence-threshold`. — [search summary](https://beto-core.googlesource.com/beto-core/+/refs/heads/main/common/deny.toml)
- Embree's bundled TBB GPLv2-with-exception question (see §4).

### Inferences
- Suggested allow list: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0 (or the current Unicode license), MPL-2.0 (file-level copyleft, allowed with care), CC0-1.0. Deny: GPL-*, AGPL-*, LGPL unless dynamically linked and reviewed, SSPL, BUSL, and "non-commercial" clauses.
- Pin `[sources]` to crates.io only. Review `build.rs`/`-sys` crates that fetch native code (Embree, OpenVDB) separately; they can drag in non-Rust licenses cargo-deny does not see.
- Because the project is agent-heavy, run `cargo deny check` in CI on every PR and also `cargo vet` or `cargo audit` for supply chain (my suggestion, not sourced).

### Gaps
- I did not verify the current cargo-deny config schema (the deprecation of `unlicensed`, etc.). Run `cargo deny init` with the pinned version.
- Not researched: LGPL/AGPL crates commonly pulled in by GUI/media stacks.

## 6. Patent landscape

### Takeaway
I found little verifiable patent information in this session. The Apache-2.0 relicensing of OpenVDB and Pixar's OpenSubdiv patent statement are the only sourced positions.

### Cited Findings
- OpenSubdiv: Pixar says its patents on subdivision-surface algorithms were released for public use. — [Pixar intro](https://graphics.pixar.com/opensubdiv/docs/intro.html)
- Apache-2.0 / TOST includes a contributor patent license and a termination clause if you sue over the Work. — [OpenUSD LICENSE.txt](https://github.com/PixarAnimationStudios/OpenUSD/blob/dev/LICENSE.txt)
- OpenVDB 12.0 under Apache-2.0 was agreed by DreamWorks, NVIDIA, Disney, ILM and others. — [Phoronix](https://phoronix.com/news/ASF-OpenVDB-12.0)

### Inferences
- Using OpenVDB's code under Apache-2.0 carries contributors' patent grants. A clean re-implementation of a VDB-like sparse tree has no such grant; I recall DreamWorks patent filings on VDB but did not verify them, so prefer using OpenVDB itself or a simple tiled-hash design that avoids VDB's specific tree claims, and have counsel check.
- FLIP, pyro, texture synthesis (including WFC, which was assigned an open license by its author but has related patent discussion): not verified.

### Gaps
- No sourced patent search for VDB, FLIP, pyro, texture synthesis, neural surrogates or WFC. Needs a patent search (Google Patents / counsel). Patent-free alternatives were not sourced.

## 7. LLMs, AI-generated recipes, DCO vs CLA

### Takeaway
Purely AI-generated output may not be copyrightable in the US, so recipes need meaningful human authorship to be protected and licensable. A DCO (sign-off) is the lightest-weight policy; projects pair it with explicit AI-disclosure rules.

### Cited Findings
- US Copyright Office Part 2 (29 Jan 2025): AI outputs are protectable only where a human determined sufficient expressive elements; prompts alone are not enough; AI-assisted works remain protectable for the human-authored parts. Part 3 (training) was forthcoming at that time. — [Copyright Office](https://copyright.gov/newsnet/2025/1060.html), [Crowell](https://www.crowell.com/en/insights/client-alerts/us-copyright-office-releases-part-2-of-artificial-intelligence-report-clarifying-copyrightability-of-generative-ai-outputs)
- GitLab uses the DCO for MIT code and CLAs for its enterprise directory, and requires disclosure of AI-generated content. — [GitLab](https://about.gitlab.com/community/contribute/dco-cla/)
- ownCloud moved from a CLA to the DCO (French source). — [Kiteworks](https://www.kiteworks.com/fr/kiteworks-news/lancement-ospo-gouvernance-dco-politique-ia/)
- Commentary: the DCO assumes the contributor knows the provenance, which is hard with AI-generated code. Several projects hold the human submitter accountable. — [Nodeland](https://adventures.nodeland.dev/archive/who-is-responsible-for-ai-generated-code) (secondary), [Kemitchell, DCO not CLA](https://writing.kemitchell.com/2021/07/02/DCO-Not-CLA)

### Inferences
- Recommendation: DCO plus an `Assisted-by:` trailer for AI use, no CLA unless a relicensing path is wanted (a CLA is the only way to relicense later without asking every contributor). Choose now, because the PLAN's agent-driven workstreams make copyright ownership of agent output murky.
- Require that agent-written code or recipes pass a "no SideFX material in context" rule: agents must not be given Houdini docs text, Houdini installations or `.hip` samples from the restricted pool (see checklist).
- The AI outputs' legal status for training/generated code is unsettled; keep human review in the loop.

### Gaps
- Not researched: Linux Foundation/kernel AI policy text, court decisions on training data (Part 3 of the Copyright Office report and cases), model-provider terms of service on outputs.

## 8. EU AI Act and other regulation

### Takeaway
Probably light: a free procedural-content tool is not obviously high-risk. The timeline moved in 2026, and open-source/agent specifics were not found.

### Cited Findings
- GPAI model obligations (Arts 53-55) applied from 2 Aug 2025 and are outside the Digital Omnibus delay. Annex III high-risk rules were deferred to 2 Dec 2027 (Annex I to 2 Aug 2028). Article 50 content-labelling grace period shortens to 2 Dec 2026. Adoption status was unclear in sources (Parliament plenary vote 16 June 2026). — [Sidley](https://datamatters.sidley.com/2026/06/22/eu-lawmakers-reach-provisional-agreement-to-delay-key-eu-ai-act-obligations/), [CASRAI](https://www.casrai.org/news/eu-ai-act-digital-omnibus-deadline-delay)

### Inferences
- If we ship only an MCP tool surface and no model, we are likely neither a GPAI provider nor high-risk. If we bundle a model (neural surrogates, VLM critic) or generate synthetic media, Article 50 marking may matter (inference).

### Gaps
- Open-source exemptions and agent/deployer duties were not found. Not checked: US state laws, export controls, GDPR for telemetry/shared CAS caches.

---

## Recommended clean-room policy (checklist for PLAN.md §8)

1. No contributor or agent may run, record, or screen-capture Houdini (any license) to learn node behavior for this project. EULA s.4.2 prohibits building a competing product with it and copying features ([EULA](https://www.sidefx.com/download/eula/)). Amend PLAN §3.6 "observable Houdini behavior" accordingly: use only published papers, third-party tutorials, public bug reports and published behavior numbers.
2. Two-team process (ReactOS/Wine model). "Spec writers" may read SideFX public docs; "implementers" read only the spec writers' original-wording specs and the cited papers. Specs record which source supplied each fact.
3. Never copy or closely paraphrase SideFX docs text, example scenes, icons, screenshots, help strings, or default parameter label tables wholesale. Node and parameter names may be reused for compatibility, with a short compatibility map in a separate file that can be removed (names are likely OK per Lotus/SAS/Google v. Oracle but unsettled in US).
4. No HDK headers or HDK sample code, no Houdini Engine, no SideFX binaries or libraries in the repo, tests or CI.
5. .hip/.hda/.bgeo support: build only from public format documentation and files supplied by users; never circumvent licensing (4.1(viii)-(ix)); do not convert Apprentice/Indie (nc/lc) files to commercial formats (4.1(x)); no code that strips or alters license metadata.
6. Contributors declare they have not seen Houdini internals or leaked SideFX source; anyone who has is barred from the affected modules (ReactOS rule). Keep a provenance log per module.
7. Agents: system prompts and retrieval corpora exclude SideFX docs text and leaked material; the agent-generated PRs carry `Assisted-by:` and a human signs off (DCO).
8. Licensing: project under Apache-2.0 (or MIT OR Apache-2.0); DCO sign-off; cargo-deny in CI (licenses, advisories, bans, sources); NOTICE file for Apache/TOST deps; review any C++ `-sys` dependency by hand.
9. Prefer OpenVDB 12+, OpenUSD (TOST), OpenSubdiv, Embree (all Apache-2.0 family); verify Alembic/MaterialX/OpenEXR/FBX SDK terms before adding; avoid the Autodesk FBX SDK.
10. Before public launch: counsel review of EULA exposure, trademark clearance of the final name, and a patent search for VDB-like structures and any simulation method planned.

## Naming recommendation
- Pick an original, unrelated name for the product, crates (`hc-*` is fine as an abbreviation if the expansion is not "Houdini clone"), binary, native file extensions, domain and logo. Rename the repo folder, `HoudiniGym`, and `hcs` expansions in docs before release.
- Describe compatibility only in factual, plain text: "imports a subset of Houdini .hip files" / "node concepts familiar to Houdini users", with a footer: "Houdini is a trademark of Side Effects Software Inc. This project is not affiliated with or endorsed by SideFX." Never use SideFX logos, orange/gradient branding or the "H" mark.
- Avoid in names and UI: Houdini, SideFX, Karma, Solaris, KineFX, PDG/TOPs (use "task graph"), and `.hip`/`.hda` as our native formats. Use own terms: "assets" for HDA, "wrangle" for VEX snippet, `hcs` for the language (do not call it VEX; consider calling ours something else).
- Generic industry terms (SOP-style "geometry operators", "VDB-like sparse volume", USD, MaterialX) are fine.
