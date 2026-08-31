# Arche Work Log

**Status:** Living operational ledger. `arche_comprehensive_design_document.md` defines the contracts; this file records what has actually shipped.
**Current focus:** M27 is the active umbrella milestone. M27-A, M27-B, M27-C0, and M27-C1 are Done. M27-C2 is Doing through short reviewed slice PRs recorded in the evidence ledger below. M27-D through M27-L are Backlog. M28 (Arche 0.1 release) follows M27.

## Ledger rules

- A gate or contract change is closed only when its row below links a merged pull request whose four protected checks (`Proof / Native Linux`, `Proof / Windows`, `Proof / Physical >4-GiB Source`, `Proof / Sparse >4-GiB Executable`) passed on the exact head. Local results are not evidence.
- One row per closed gate or promoted contract change: date, what closed, PR, exact-head CI run(s). No prose closure narratives; the PR holds the detail.
- The board keeps at most two issues in Doing. Backlog may inventory the promoted gates of an umbrella milestone without activating them.
- Every post-M24 milestone must name the fixture-specific production assumptions it removes and include a structurally distinct source program that needs no program-specific compiler change.
- History lives in git. This file is rewritten, not appended, whenever it exceeds about 250 lines.

## Board

### Doing

| Issue | Title | Notes |
|---|---|---|
| M27-C | General language semantics and verified generic Core | Slices C0 and C1 are Done. C2 is Doing: const-independent type checking, explicit conversion selection, symbolic type-level-const obligations, static trait selection, orphan/coherence and `impl default`, operator dispatch, const-independent exhaustive decision trees, and checked `NeedsCtfe` pattern leaves, with exact positive/negative goldens for both `tests/m27c2/v1` workspaces. Still owed before C2 closes: golden rendering of the C2-resolved declaration/owner shapes and corpus growth to exercise the remaining coercion kinds, logical-not, float comparison, and environment-corpus impls, plus the pattern surfaces absent from both corpora (unsigned ranges, pending array-length tests, ref-mut bindings, symbolic slices and arrays, opaque and unsupported domains); the corpus freeze binds fixtures, not the printer. C3–C6 follow in order. M27-C stays Doing until C6's exact-head closure evidence is recorded; M27-D cannot start early. |

M27-C slice state: C0 **Done**, C1 **Done**, C2 **Doing**, C3–C6 **Backlog**. Closure boundaries for every slice are in design document §0.7.

### Ready

Empty.

### Backlog

Promoted M27 gates in dependency order. Required results for each are in design document §0.7.

| Gate | Title |
|---|---|
| M27-D | Monomorphization, layout, and ARCHEOBJ v1 |
| M27-E | Dynamic runtime values and reentrant worlds |
| M27-F | Entity lifecycle and structural commands |
| M27-G | General native AOT and runtime |
| M27-H | Standard library and core public workflows |
| M27-I | Complete public developer tooling |
| M27-J | Managed toolchains and production registry |
| M27-K | Integrated acceptance and production soak |
| M27-L | Exact-head M27 closure |

M28 (authoritative Arena server, 1,024-world Grid Pursuit, trainer protocol v1) is sequenced after M27-L; see design document §0.8.

## Closed gates and promoted contract changes

Newest first. Run links are the exact-head `Proof` workflow runs; "merged main" is the run on the merge commit.

| Date | Closed | PR | Exact-head CI |
|---|---|---|---|
| 2026-08-31 | M27-C2 slice: every staged declaration judgment implemented and hardened through ten adversarial review rounds (alias transparency, readiness recanonicalization, three-valued head unification) — both v1 corpora terminate as fully checked C2 workspaces | [#28](https://github.com/noahsabaj/arche/pull/28) | PR head [33429022324](https://github.com/noahsabaj/arche/actions/runs/33429022324), merged main [33430512570](https://github.com/noahsabaj/arche/actions/runs/33430512570) |
| 2026-08-31 | M27-C2 slice: closure and generator-factory values, associated-method inference, the reserved resume postfix, corpus repair — every body in both v1 corpora authority-complete with zero diagnostics | [#27](https://github.com/noahsabaj/arche/pull/27) | PR head [33408087022](https://github.com/noahsabaj/arche/actions/runs/33408087022), merged main [33410255862](https://github.com/noahsabaj/arche/actions/runs/33410255862) |
| 2026-08-31 | M27-C2 slice: constructor-inference dispatch, while loop frames, the bare-break rule, honest isolated-subtree break gaps (C2 stays Doing) | [#26](https://github.com/noahsabaj/arche/pull/26) | PR head [33405352124](https://github.com/noahsabaj/arche/actions/runs/33405352124), merged main [33407819405](https://github.com/noahsabaj/arche/actions/runs/33407819405) |
| 2026-08-30 | M27-C2 slice: catch arms typed against declared singleton throws sets | [#25](https://github.com/noahsabaj/arche/pull/25) | PR head [33340706142](https://github.com/noahsabaj/arche/actions/runs/33340706142), merged main [33341365716](https://github.com/noahsabaj/arche/actions/runs/33341365716) |
| 2026-08-30 | M27-C2 slice: opaque pattern domains and ordinary `for` iterator selection | [#24](https://github.com/noahsabaj/arche/pull/24) | PR head [33338274817](https://github.com/noahsabaj/arche/actions/runs/33338274817), merged main [33338911658](https://github.com/noahsabaj/arche/actions/runs/33338911658) |
| 2026-08-30 | M27-C2 slice: method selection and generic-actual inference in bodies | [#23](https://github.com/noahsabaj/arche/pull/23) | PR head [33336619306](https://github.com/noahsabaj/arche/actions/runs/33336619306), merged main [33337169755](https://github.com/noahsabaj/arche/actions/runs/33337169755) |
| 2026-08-30 | M27-C2 slice: embedded callables in C2 bodies | [#22](https://github.com/noahsabaj/arche/pull/22) | PR head [33333034488](https://github.com/noahsabaj/arche/actions/runs/33333034488), merged main [33333732748](https://github.com/noahsabaj/arche/actions/runs/33333732748) |
| 2026-08-30 | Line-ending normalization and the lean work-log ledger | [#20](https://github.com/noahsabaj/arche/pull/20) | PR head [33326608328](https://github.com/noahsabaj/arche/actions/runs/33326608328), merged main [33326943395](https://github.com/noahsabaj/arche/actions/runs/33326943395) |
| 2026-08-11 | M27-C2 predecessor contract amendment (design + `.gitattributes` only; C2 stays Doing) | [#18](https://github.com/noahsabaj/arche/pull/18) | PR head [31538099580](https://github.com/noahsabaj/arche/actions/runs/31538099580), merged main [31540228289](https://github.com/noahsabaj/arche/actions/runs/31540228289) |
| 2026-08-11 | M27-C1 closed; C2 promoted to Doing | [#17](https://github.com/noahsabaj/arche/pull/17) | merged main [31528567212](https://github.com/noahsabaj/arche/actions/runs/31528567212) |
| 2026-08-11 | M27-C1 complete retained syntax, immutable source authority, package-aware symbolic HIR, type-shape/inventory skeleton | [#16](https://github.com/noahsabaj/arche/pull/16) | PR head [31525196910](https://github.com/noahsabaj/arche/actions/runs/31525196910), merged main [31526456910](https://github.com/noahsabaj/arche/actions/runs/31526456910) |
| 2026-08-11 | M27-C0/C1 predecessor identity and grammar contract amendment | [#15](https://github.com/noahsabaj/arche/pull/15) | merged main [31515711289](https://github.com/noahsabaj/arche/actions/runs/31515711289) |
| 2026-08-09 | M27-C0 closed; C1 promoted to Doing | [#14](https://github.com/noahsabaj/arche/pull/14) | merged main [31287781512](https://github.com/noahsabaj/arche/actions/runs/31287781512) |
| 2026-08-09 | M27-C0 general language and verified-generic-Core contract frozen | [#13](https://github.com/noahsabaj/arche/pull/13) | PR head [31287217870](https://github.com/noahsabaj/arche/actions/runs/31287217870), merged main [31287416249](https://github.com/noahsabaj/arche/actions/runs/31287416249) |
| 2026-08-08 | M27-B evidence ledger closed | [#12](https://github.com/noahsabaj/arche/pull/12) | merged main [31268809089](https://github.com/noahsabaj/arche/actions/runs/31268809089) |
| 2026-08-08 | M27-B schema-1 manifests, workspaces, deterministic resolution and `Arche.lock`, streamed modules, package-aware resolved HIR, public `arche check` | [#11](https://github.com/noahsabaj/arche/pull/11) | merged main [31268239040](https://github.com/noahsabaj/arche/actions/runs/31268239040) |
| 2026-08-08 | M27-A evidence ledger closed | [#10](https://github.com/noahsabaj/arche/pull/10) | merged main [31262507624](https://github.com/noahsabaj/arche/actions/runs/31262507624) |
| 2026-08-08 | M27-A platform contract promotion, licenses, Rust workspace boundaries, public `arche` command shell, status taxonomy, ID domains | [#9](https://github.com/noahsabaj/arche/pull/9) | merged main [31262003302](https://github.com/noahsabaj/arche/actions/runs/31262003302) |
| 2026-08-01 | M26 evidence ledger closed | [#8](https://github.com/noahsabaj/arche/pull/8) | merged main [30709763744](https://github.com/noahsabaj/arche/actions/runs/30709763744) |
| 2026-08-01 | M26 verified-Core execution closure and audit remediation; static x86-64 Linux PIE; strict Linux/Windows/WSL CI; >4-GiB source and artifact proofs | [#7](https://github.com/noahsabaj/arche/pull/7) | PR head [30709163804](https://github.com/noahsabaj/arche/actions/runs/30709163804), merged main [30709348257](https://github.com/noahsabaj/arche/actions/runs/30709348257) |
| 2026-07-13 | CI checkout action upgraded to v6 (Node 24) | [#6](https://github.com/noahsabaj/arche/pull/6) | merged main [29292852847](https://github.com/noahsabaj/arche/actions/runs/29292852847) |
| 2026-07-13 | M25 descriptor-generic native world execution across two unrelated programs | [#5](https://github.com/noahsabaj/arche/pull/5) | merged main [29292272687](https://github.com/noahsabaj/arche/actions/runs/29292272687) |
| 2026-07-13 | M25 acceptance checkpoint frozen | [#4](https://github.com/noahsabaj/arche/pull/4) | merged main [29280018164](https://github.com/noahsabaj/arche/actions/runs/29280018164) |
| 2026-07-13 | M24 native ECS storage catalog (M24-001 through M24-005) | [#3](https://github.com/noahsabaj/arche/pull/3) | merged main [29278398832](https://github.com/noahsabaj/arche/actions/runs/29278398832) |
| 2026-07-13 | Historical audit ledger bytes preserved | [#2](https://github.com/noahsabaj/arche/pull/2) | merged main [29274164544](https://github.com/noahsabaj/arche/actions/runs/29274164544) |
| 2026-07-13 | `archec0` audit remediation baseline; Windows and native Linux proof gates required | [#1](https://github.com/noahsabaj/arche/pull/1) | PR head [29273455071](https://github.com/noahsabaj/arche/actions/runs/29273455071), merged main [29273762821](https://github.com/noahsabaj/arche/actions/runs/29273762821); tag `archec0-audit-baseline-2026-07-13` |
| ≤ 2026-07-13 | M0–M23 bootstrap milestones (repository harness, ELF64 emission, lexer/parser/checker, Core and verifier, layouts, native ECS tables, systems, queries, schedules) | — | Pre-GitHub history before the baseline tag. Issue-level detail is preserved in `WORK_LOG.md` at commit `dbd58c9` and is not maintained here. |

## Archive tags

Archive tags preserve history that is deliberately not on any branch. None of them is closure evidence.

- `archive/2026-08-30-c2-agent-dump` — unreviewed agent output (`9070f8f`) that flipped 68 files to CRLF, broke `cargo test`, stubbed public commands, removed the Windows PowerShell 5.1 proof, and claimed M27-C through M27-L closed without a PR or CI run. Its genuine C2 progress was re-committed on `m27/c2-semantics`; everything else in it is unreviewed D–L scaffolding.
- `archive/m27-c1-pre-amendment` — WIP snapshot taken before the C0/C1 contract split; superseded by PRs #16 and #17.
- `archec0-audit-baseline-2026-07-13` — the audit remediation baseline landed by PR #1.

## External prerequisite status

These prerequisites are not inferred from code and are not renamed around. Each stays Open until exact external evidence replaces that status; the owning gate cannot close before then. None blocks in-repository M27-C work.

| External prerequisite | Evidenced status | First blocked gate |
|---|---|---|
| Control of `arche-lang.org` and the `packages.arche-lang.org` production DNS/service identity | Open | M27-J production registry |
| Production infrastructure credentials and recovery access | Open | M27-J deployment and M27-K restore/failover |
| GitHub App OAuth device-flow and trusted-publisher OIDC configuration | Open | M27-J authentication/publishing |
| Offline signing roots and release-signing ceremony | Open | M27-J signed records and M27-L release closure |
| Required distribution namespaces, including the `arche-lang-env` Python name | Open | M28 Python/package publication |

## Known gaps

Runtime growth, system-time structural mutation, entity lifecycle, archetype transitions, and command buffers are deliberate post-M26 work owned by M27-D through M27-G. Events, relations, optional/change-detection queries, parallel schedules, user FFI, trait objects, associated types, and native Windows output are explicitly excluded from M27 and M28 (design document §0.9).

## Working rules

Every issue must produce at least one of: a binary that runs, a compiler command that works, a test that passes, a runtime behavior that can be observed, or a verifier that catches a real invalid program. Anything else is too vague and must be split.

Each session: pick one issue from Ready or the active slice; read the relevant design section; write or update the acceptance test first; implement the smallest change; run the test; commit; record evidence; promote the next unblocked issue. Never spend a session "working on the language" in the abstract.

Local verification commands are in `README.md`. Local success is not acceptance; only the exact-head hosted checks in the table above close a gate.
