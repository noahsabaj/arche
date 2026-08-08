# Arche Comprehensive Design Document

**Document version:** 0.3
**Date:** 2026-08-08
**Status:** Normative M27 platform and M28 Arche 0.1 contract; implemented and accepted M26 contract retained as history
**Primary goal:** Define Arche as an independent, native, ECS-first programming language and software platform.

Section 0 is the normative contract for M27, M28, and Arche 0.1. It supersedes every conflicting "initial," 0.0.x, post-M26, open-question, example, and future-facing sketch elsewhere in this document. The detailed M26 clauses remain normative descriptions of the closed historical milestone only; they do not constrain an explicit M27 hard cut. All other sections are supporting rationale or retained design history unless Section 0 adopts them explicitly.

This document defines intended contracts. `WORK_LOG.md` alone records promoted implementation gates, exact-head evidence, and acceptance state. No future-facing clause in this document is a claim that the feature is implemented.

---

## Table of Contents

0. [Normative M27 and M28 Contract](#0-normative-m27-and-m28-contract)
1. [Vision](#1-vision)
2. [Non-Negotiable Design Principles](#2-non-negotiable-design-principles)
3. [What Arche Is](#3-what-arche-is)
4. [What Arche Is Not](#4-what-arche-is-not)
5. [Target Product Experience](#5-target-product-experience)
6. [First Target Platform](#6-first-target-platform)
7. [System Overview](#7-system-overview)
8. [Arche Execution Model](#8-arche-execution-model)
9. [Core Runtime Concepts](#9-core-runtime-concepts)
10. [Entity Model](#10-entity-model)
11. [Component Model](#11-component-model)
12. [Resource Model](#12-resource-model)
13. [Tag Model](#13-tag-model)
14. [System Model](#14-system-model)
15. [Query Model](#15-query-model)
16. [Schedule Model](#16-schedule-model)
17. [Command Buffer Model](#17-command-buffer-model)
18. [Event Model](#18-event-model)
19. [Relations](#19-relations)
20. [Memory Model](#20-memory-model)
21. [Arche Runtime Kernel](#21-arche-runtime-kernel)
22. [Arche ABI](#22-arche-abi)
23. [Component Identity and Linking](#23-component-identity-and-linking)
24. [Arche Core](#24-arche-core)
25. [Arche Object Format](#25-arche-object-format)
26. [Arche Executable Format Strategy](#26-arche-executable-format-strategy)
27. [Compiler Architecture](#27-compiler-architecture)
28. [Frontend](#28-frontend)
29. [Semantic Analysis](#29-semantic-analysis)
30. [ECS Access Checking](#30-ecs-access-checking)
31. [Layout Planning](#31-layout-planning)
32. [Query Planning](#32-query-planning)
33. [Schedule Planning](#33-schedule-planning)
34. [Backend Architecture](#34-backend-architecture)
35. [x86-64 Backend](#35-x86-64-backend)
36. [ELF64 Writer](#36-elf64-writer)
37. [Arche Linker](#37-arche-linker)
38. [Startup and Boot](#38-startup-and-boot)
39. [Standard Library](#39-standard-library)
40. [Package and Build System](#40-package-and-build-system)
41. [Debugger](#41-debugger)
42. [Profiler](#42-profiler)
43. [Testing Strategy](#43-testing-strategy)
44. [Diagnostics](#44-diagnostics)
45. [Toolchain Commands](#45-toolchain-commands)
46. [Language Surface](#46-language-surface)
47. [Example Programs](#47-example-programs)
48. [Historical Bootstrap Roadmap (Superseded)](#48-historical-bootstrap-roadmap-superseded)
49. [Bootstrap and Self-Hosting](#49-bootstrap-and-self-hosting)
50. [Risks and Mitigations](#50-risks-and-mitigations)
51. [Historical Open Design Questions](#51-historical-open-design-questions)
52. [Appendix A: M26 Grammar Boundary](#52-appendix-a-m26-grammar-boundary)
53. [Appendix B: Initial Runtime Structs](#53-appendix-b-initial-runtime-structs)
54. [Appendix C: Initial Arche Core Example](#54-appendix-c-initial-arche-core-example)
55. [Appendix D: Milestone Acceptance Tests](#55-appendix-d-milestone-acceptance-tests)

---

# 0. Normative M27 and M28 Contract

## 0.1 Product north star and authority

Arche is a standalone, native, general-purpose ECS language and platform. Games, simulations, authoritative servers, tools, deterministic environments, and other ECS-shaped software are equal intended uses. Machine learning is an important application frontier—particularly for running many reproducible simulation worlds—but it is not Arche's identity and does not introduce an ML-specific language boundary before 0.1.

The roadmap to the first public release is:

1. **M26 — closed historical substrate.** Generic verified-Core reference/native agreement, metadata-authoritative ARCHEECS v2 execution, ARCHEOBS2, static x86-64 Linux PIE output, and the required strict/large-file gates are accepted.
2. **M27 — general platform foundation.** One umbrella milestone, implemented through mandatory internal gates M27-A through M27-L, creates the general language, reentrant ECS runtime, artifact pipeline, package ecosystem, public toolchain, standard library, and source-package registry.
3. **M28 — Arche 0.1 release proof.** No broad language feature is added. Two structurally different applications prove the completed platform: an authoritative multiplayer arena server and a deterministic 1,024-world Grid Pursuit environment with external trainer interoperability.

Every internal gate uses a short-lived pull request and exact-head evidence. Passing an internal gate does not close M27. M27 closes only after M27-L; Arche 0.1 ships only after M28's exact protected release commit passes both application proofs and every release gate.

## 0.2 Targets, modules, worlds, and entrypoints

`Arche.toml` schema 1 defines library, binary, and environment targets:

- A library target has `src/lib.arc` by default, cannot declare a root world, and has no process entrypoint.
- A binary target has `src/main.arc` by default, links exactly one explicit root world, and exports `fn main(app: &mut App<RootWorld>, caps: Caps<...>) requires {...} throws {...} -> i32`. Its returned `i32` becomes the process status through the low eight bits.
- An environment target uses a manifest-listed source root, links exactly one explicit root world, has no `main` or source `exit`, and names reset, step, and self-play schedules in its manifest profile.

World declarations use `world Name { init { ... } }`. Components, resources, tags, systems, schedules, functions, and types are package/module items. `init` is data-only resource and spawn initialization. A binary drives schedules through `App`; an environment's lifecycle and schedules are driver-owned.

Module discovery is explicit and deterministic:

- `mod physics` resolves only `physics.arc`; a child `mod collision` declared there resolves `physics/collision.arc`.
- There is no `mod.arc`, wildcard discovery, path attribute, or duplicate module loading.
- `use`, `pub`, `pub(package)`, `pub(super)`, and `pub(in path)` define imports and visibility.
- Source identifiers use Unicode XID normalized to NFC. Filename aliases, case-fold collisions, and normalization collisions are errors.
- Public package scope and name segments are strict lowercase ASCII and use `scope/name`; official packages use `arche/*`.

M26 `startup`/final-`exit` source is not accepted by M27 target semantics. It receives an explicit migration diagnostic; no source compatibility shim remains.

## 0.3 General language contract

### 0.3.1 Values and numerics

The scalar set is `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, 64-bit `isize`/`usize`, `f32`, `f64`, `bool`, Unicode-scalar `char`, and 64-bit `entity`. The aggregate and owned-value set includes tuples, fixed arrays, slices, `str`, named structs, payload enums, generic `Option` and `Result`, `String`, `Vec<T>`, ordered `Map<K,V>`, `Box<T>`, `Rc/Weak`, `Arc/Weak`, and `Pin`. Arche has no garbage collector.

Integer arithmetic and shifts wrap in two's complement/modulo arithmetic. Shift counts are masked to the width; signed right shift is arithmetic. Division or remainder by zero and signed minimum divided by `-1` trap. No implicit numeric or boolean conversion exists. `From` and `TryFrom` express safe conversions; `as` is reserved for unsafe pointer/address casts.

Floating-point operations use round-to-nearest-even, masked exceptions, FTZ/DAZ disabled, no contraction, preserved subnormals and signed zero, and canonical arithmetic NaNs for `f32` and `f64`. Ordered comparisons with NaN are false except `!=`, which is true.

### 0.3.2 Functions, generics, traits, and patterns

Arche supports direct and mutual recursion, real call frames, stack probes, and a noncatchable stack-overflow trap. Generics accept type, lifetime, and integer-const parameters.

Traits use static dispatch and required methods only. Trait objects, associated types, default methods, supertraits, and user negative impls are outside 0.1. The orphan rule permits an implementation when its package owns the trait or the outermost nominal target type. Overlap is legal only when the parent is explicitly `impl default` and the child is strictly more specific; incomparable matches are errors. Operator traits use explicit input/output generic parameters and cannot throw. All conversions remain explicit.

Pattern matching supports nested struct, tuple, enum, reference, and slice patterns; literals; integer and character ranges; or-patterns; `name @ pattern`; guards; match ergonomics; `if let`; `while let`; `let ... else`; and catch patterns. Matches are exhaustive. Guards run in source order and do not contribute to exhaustiveness. Every alternative of an or-pattern binds the same names with the same types/modes. Float and map structural patterns are rejected.

### 0.3.3 Ownership, unsafe code, and allocation

Arche uses Rust-like ownership semantics without Rust source, crate, or ABI compatibility:

- Moves, `Copy`, `Clone`, nonthrowing `Drop`, shared/mutable references, nonlexical lifetimes, explicit lifetime parameters, raw pointers, strict provenance, `unsafe`, and `MaybeUninit` are language concepts.
- Assignment completely evaluates and owns its right-hand side before dropping and replacing the old destination.
- `Drop` cannot throw. It may panic; panic during an existing unwind aborts with status `134`.
- Fallible allocation APIs return ordinary `Result<_, AllocError>` values. Infallible or bootstrap allocation failure is an infrastructure failure with status `1`; structural-command allocation failure follows the transactional rule in Section 0.4.1. Compiler-generated cleanup edges destroy every initialized owned value exactly once.
- Unsafe violations are undefined behavior. A checked-unsafe diagnostic mode may detect violations but does not change the language contract.

Closures infer captures or use `move` and implement anonymous, statically dispatched `Fn`, `FnMut`, or `FnOnce` types. Generators are pinned stackless pull state machines with typed resume, yield, return, `throws`, and `requires` sets. A generator may borrow across yield only through its pinned lifetime rules. Async/futures remain deferred.

Threads use structural `Send`/`Sync`, scoped borrowing, atomics with Relaxed/Acquire/Release/AcqRel/SeqCst orderings, and nonpoisoning mutexes, read/write locks, condition variables, and channels. Consume ordering is unavailable. `App` and live world access are `!Send + !Sync`; the simulation thread exclusively owns its world. Environment stepping remains sequential in 0.1.

### 0.3.4 Exceptions, effects, capabilities, and compile-time evaluation

Checked exceptions use explicit canonical `throws {E...}` sets. Capability effects use separate canonical `requires {Capability...}` sets. Exported functions, traits, closures, generators, function pointers, schedules, Core bodies, and ABI hashes include both sets. Recursive call graphs are solved to a fixed point. A schedule exposes the union of every dispatched system's sets.

`throw`, propagation, and exhaustive `catch` implement recoverable exceptions. An exception escaping the entrypoint unwinds initialized values, discards the current unflushed structural-command epoch, preserves earlier committed effects, emits an enabled observation, writes the reserved diagnostic, and exits `71`. Panic and semantic traps are uncatchable and exit `70` after the same committed-state rule; a panic during unwind aborts `134`.

Capabilities are unforgeable, non-static, nonserializable driver-supplied values. General binaries may receive explicit capabilities for arguments, environment, standard I/O, files, subprocesses, wall/monotonic clocks, TCP/UDP, threads, atomics, and synchronization. Environment reset/step/self-play call graphs are statically checked and dynamically guarded against ambient/nondeterministic host effects, raw address observation, unsafe host calls, and threads.

Compile-time evaluation runs the full hermetic language subset, including recursion, allocation-backed values, traits, caught exceptions, closures/generators, and Drop. It cannot access ECS worlds, capabilities, threads, host I/O, FFI, or observable addresses. `include_bytes` and `include_str` are explicit hashed package inputs. Published manifests pin step, call-depth, and heap budgets; scaffold defaults are 10,000,000 steps, depth 1,024, and 64 MiB. Budget exhaustion is a compiler-resource error, not an Arche result.

## 0.4 ECS and world-lifecycle contract

### 0.4.1 Entities and structural commands

`entity` packs a nonzero 32-bit index in the low half and a nonzero 32-bit generation in the high half. Fresh entities start at generation one. Despawn increments the generation before deterministic LIFO slot reuse; overflow permanently retires the slot. Zero is invalid. `Option<entity>` expresses absence—there is no sentinel entity.

Systems request structural access with an explicit `cmd: commands` parameter:

```arche
cmd.spawn { Position { x: 0.0, y: 0.0 } } -> entity
cmd.despawn(e)
cmd.add(e, Velocity { x: 1.0, y: 0.0 })
cmd.remove<Velocity>(e)
```

`cmd.spawn` immediately reserves and returns a handle. That handle may be stored or targeted by later queued commands, but the entity is not query-visible until flush. Structural commands flush exactly once at the implicit end of a schedule; no public intermediate flush exists.

Command emission order is schedule order, system order, query table/row order, then statement order. Each command is atomic. Earlier valid commands remain committed if the first stale, duplicate, conflicting, or allocation-failing command stops the flush. The failing command publishes no partial effect, later commands remain unapplied, and every owned queued payload is dropped exactly once.

### 0.4.2 Tables, queries, and isolated worlds

Queries visit materialized tables in table-creation order and live physical row slots in row order. Spawn and archetype transition append to the destination table. Removal performs deterministic swap-remove and repairs the moved entity's location. Materialized empty tables remain observable because their catalog position affects future iteration.

Required query terms preserve source binding order; exclusions do not bind; tags and other zero-sized required terms bind only `_`; mutable tags remain invalid. M27/M28 do not add optional terms, change detection, nested query loops, events, relations, or parallel schedules.

Every `WorldContext` owns an independent allocator, resources, table catalog, rows/columns, entity locations and generations, free list, retired slots, command buffer, and allocation ledger. No mutable heap allocation crosses world instances. One immutable linked world template and shared code image may create many reentrant instances.

### 0.4.3 ECS-storable values

`EcsValue` is a compiler-sealed eligibility judgment revalidated after monomorphization. Scalars, entities, eligible arrays/tuples/structs/enums/Option/Result, `Box`, `String`, `Vec`, and ordered `Map` are eligible when every transitive child is owned, sized, `'static`, canonically encodable, and safely droppable.

References, raw pointers, `Rc`/`Arc`/`Weak`, `Pin`, closures, generators, synchronization/interior-mutable values, and operating-system/runtime handles are transitively ineligible. `EcsKey` is separately compiler-sealed and supplies canonical structural ordering. Floats and arbitrary user `Ord` implementations cannot become ECS map keys.

Reference and native execution consume the same decoded/linked metadata but remain independent semantic implementations. The reference executor interprets verified instantiated Core; native execution runs AOT bodies. Exact parity is required for deterministic programs without ambient host effects. Live networking and thread scheduling use separate behavioral conformance tests rather than byte-identical execution claims.

## 0.5 Compiler, artifacts, native runtime, and observation

### 0.5.1 Semantic pipeline and identities

The compiler pipeline is:

```text
streamed source snapshots
  -> AST
  -> resolved HIR
  -> typed generic MIR (move paths, NLL, patterns, calls, effects, cleanup/unwind edges)
  -> VerifiedGenericCore
  -> deterministic link-time instance graph and monomorphization
  -> VerifiedInstanceCore
  -> direct reference execution or Machine IR/AOT
```

Machine IR is never a semantic authority. Domain-separated 128-bit identities cover package, definition, type, concrete instance, interface, layout, ABI, and Core body. Stable definition identity uses registry origin, scoped package name, module path, declaration, and declaration shape—not package version. Resolved package instances separately include version and source digest.

The M27 domains are exact ASCII byte strings including their trailing NUL:

```text
ARCHE-PACKAGE-ID\0
ARCHE-DEF-ID\0
ARCHE-TYPE-ID\0
ARCHE-INSTANCE-ID\0
ARCHE-INTERFACE-HASH\0
ARCHE-LAYOUT-HASH\0
ARCHE-ABI-HASH\0
ARCHE-BODY-HASH\0
```

Every M27 canonical identity preimage begins with the selected domain followed by fingerprint encoding version `2` as `u32le(2)` before the domain-specific fields. No domain may be reused for another identity kind. M27 retains the exact M26 ABI/body domain strings but changes their prefix from the historical M26 `u32le(1)` to `u32le(2)` and uses the new domain-specific preimage contract; Section 23.1's M26 version-1 preimages and vectors remain frozen historical M26 contracts rather than being reinterpreted.

### 0.5.2 Versioned hard cut

M27 makes one fail-closed compatibility cut:

- `ARCHEOBJ` v1 represents one target-specific package target and contains imports/exports, type/trait/impl descriptors, generic Core templates, concrete instances, relocations, source maps, and identity hashes.
- Canonical-Core encoding v2 is the sole serialized Core contract.
- `ARCHEECS` v3 is the fully linked executable/environment metadata contract for the type graph, root world, initializers, schedules, queries, function links, environment profiles, canonical values, source spans, and package provenance.
- Binary `ARCHEOBS` v3 is the semantic observation contract.
- Canonical Value v1 is the shared logical-value codec.
- Environment protocol v1 is the trainer process protocol.

ARCHEOBJ, Canonical Core, ARCHEECS, ARCHEOBS, and Canonical Value share this exact little-endian 64-byte directory envelope:

| Offset | Width | Field | Empty-vector value |
|---:|---:|---|---:|
| `0` | `8` | format magic | format-specific ASCII bytes |
| `8` | `4` | version (`u32`) | format-specific version |
| `12` | `4` | header size (`u32`) | `64` |
| `16` | `8` | flags (`u64`) | `0` |
| `24` | `8` | total length (`u64`) | `64` |
| `32` | `8` | directory offset (`u64`) | `64` |
| `40` | `8` | directory count (`u64`) | `0` |
| `48` | `8` | directory entry size (`u64`) | `64` |
| `56` | `8` | reserved (`u64`) | `0` |

The format magic/version pairs are exact:

| Format | 8-byte magic | Version |
|---|---|---:|
| Arche package object | `ARCHEOBJ` | `1` |
| Canonical Core | `ARCHECOR` | `2` |
| Linked executable metadata | `ARCHEECS` | `3` |
| Semantic observation | `ARCHEOBS` | `3` |
| Canonical Value | `ARCHEVAL` | `1` |
| Environment protocol frame | `ARCHEENV` | `1` |

The byte-exact empty vector for each of the five directory-envelope formats is:

```text
magic[8]
|| u32le(version)
|| u32le(64)
|| u64le(0)
|| u64le(64)
|| u64le(64)
|| u64le(0)
|| u64le(64)
|| u64le(0)
```

That is exactly 64 bytes and fixes, in order, the magic, version, header size, flags, total length, directory offset, directory count, directory entry size, and reserved field. Gate-owned directories and sections may specialize content and validation but may not reinterpret any envelope field. Nonempty directories use 64-byte entries, checked `u64` arithmetic, reserved-zero validation, canonical ordering, complete cross-reference validation before mutation, and explicit rejection of unknown versions or sections.

ARCHEENV is the deliberate framing exception: each protocol frame is exactly 64 bytes, so a directory envelope cannot precede or consume its message fields. M27-A freezes its foundation vector as `ARCHEENV || u32le(1) || u32le(64) || zero[48]`. M28 assigns opcode, flags, sequence, lengths, and payload references within bytes `16..64`; until then those bytes are reserved and must be zero. The frame decoder uses checked `u64` values for every length, sequence, and external payload reference and rejects nonzero reserved fields. This distinction preserves the fixed-frame trainer contract without reinterpreting a directory header.

ARCHEECS v2, ARCHEOBS2, M26 source entry syntax, and later incompatible pre-1.0 versions require rebuild/migration; no production compatibility shim remains.

Canonical Value begins with stable `TypeId`, checked payload length, and flags, then type-directed logical bytes. Strings preserve exact UTF-8, vectors logical order, maps sealed key order, enums variant plus payload, and `Box` its pointee. It never exposes pointers, padding, spare capacity, allocator state, or hash buckets. Decode builds in staging storage and publishes only after full validation.

### 0.5.3 Observation and outcomes

OBS3 is opt-in for ordinary binaries. Program stdout/stderr remain entirely program-owned. `arche run --observation PATH` uses a private Linux pipe and atomic publisher; Windows/WSL uses a private mode-restricted WSL sidecar followed by atomic publication. Direct PIE execution without the observation control block emits no observation. Environment snapshots travel inside the framed trainer protocol.

An OBS3 snapshot includes package/lock/profile identity and descriptor dictionaries; resources in schema-ID order; materialized tables in creation order including empty tables; physical rows with table ordinal, row ordinal, entity handle, and birth ordinal; columns in canonical schema-ID signature order; next fresh entity index; every slot generation; retired slots; exact free-list order; and every dynamic value through Canonical Value. Addresses, capacities, and allocator implementation details are absent.

Reserved process statuses are:

| Status | Meaning |
|---:|---|
| low eight bits | returned binary `main` value |
| `1` | infrastructure, metadata, link, bootstrap allocation, or observation-I/O failure |
| `2` | CLI usage/configuration error or unsafe output target |
| `64` | trainer protocol violation |
| `70` | semantic trap or panic |
| `71` | uncaught checked exception |
| `72` | environment/profile invariant violation |
| `134` | double panic or explicit abort |

A source return matching a reserved status remains distinguishable by the absence of the reserved diagnostic.

### 0.5.4 Native target

The generated target remains x86-64 Linux static PIE: real calls and guarded stacks, compiler-managed unwinding, DWARF source/ECS metadata, TLS, direct Linux system calls, no interpreter/dynamic relocation/text relocation, no writable-executable segment, and a nonexecutable stack. All output remains streamed, checked-`u64`, far-safe, and atomically published. User native-library FFI is unavailable in 0.1; only the trusted runtime uses its private native ABI.

## 0.6 Packages, tools, registry, and distribution

### 0.6.1 Manifests, workspaces, and dependency resolution

`Arche.toml` schema 1 supports package identity, binary/library/environment targets, explicit workspace membership, registry/path dependencies, const-eval budgets, declared capabilities, and environment profiles. Workspaces use sorted explicit member paths with no globs, nesting, or outside-root members; they share one lockfile, cache, and target directory and may declare default members.

Dependencies are SemVer requirements from the one official registry or local paths. The resolver selects one version of each registry package identity across the graph; highest compatible non-yanked versions win and prereleases require an explicit prerelease requirement. Git/URL/custom-registry, build-script, feature, optional, and target-conditional dependencies are rejected in 0.1. A publishable path dependency must also name and match a registry package/version; packaging removes the path.

`Arche.lock` canonically and without timestamps pins exact versions, archive/source digest, complete dependency graph, registry identity, provenance/inclusion record, exact toolchain, and release-manifest digest.

### 0.6.2 Public toolchain

The public `arche` CLI owns:

- `new`, `check`, `build`, `run`, `test`, `inspect`, `fmt`, `doc`, `lsp`, `debug`, and `profile`.
- `add`, `remove`, `update`, `search`, `package`, `publish --dry-run`, `login`, `logout`, `whoami`, scope/owner/trusted-publisher management, and yank/unyank.
- `toolchain install`, `toolchain list`, `toolchain default`, and `toolchain remove`.
- Workspace selection, locked/offline/frozen operation, human and NDJSON messages, and deterministic atomic output publication.

`archec0` remains the authoritative Rust bootstrap compiler through 0.1 but becomes an internal component behind `arche`. Linux supports the full toolchain. Windows supports compilation and static tooling; run/test/debug/profile use an explicitly configured, version-matched WSL helper. Debugging wraps LLDB/GDB with Arche source maps and ECS inspection. Profiling wraps Linux `perf` plus systems/schedules/queries/commands/allocation/drop instrumentation.

### 0.6.3 Public registry and toolchain distribution

The official registry at `packages.arche-lang.org` distributes canonical source-only `ARCHEPKG` v1 archives; ARCHEOBJ and native code are local rebuildable caches only. Archives reject traversal, links, devices, duplicate entries, case/NFC aliases, undeclared files, and unsafe expansion. Operational quotas are server-advertised and adjustable rather than compiler product caps.

Human authentication uses a [GitHub App OAuth device flow](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app) and short-lived, scoped registry sessions with no upload permission. Humans reserve names, administer scopes/roles/policies, and yank versions; production publishing is trusted-CI-only. [GitHub Actions OIDC claims](https://docs.github.com/en/actions/reference/security/oidc) are validated against issuer, audience, immutable owner/repository IDs, workflow, ref/environment, version, digest, expiry, and replay ID. A single-use capability publishes exactly one immutable package version.

Scopes bind to immutable GitHub user/organization IDs with Owner, Maintainer, and Publisher roles. Scope transfer and sole-owner recovery require notification, a seven-day delay, two-administrator approval, and append-only audit records. Published versions are immutable and yankable. Security may quarantine or tombstone malware or legally prohibited content; a locked fetch fails rather than receiving substituted bytes.

The production service uses stateless OCI services, managed PostgreSQL, S3-compatible immutable blobs, a CDN/sparse index, append-only transparency, signed attestations, tested backups, and a public status surface. Monthly read and authenticated-write availability are each at least 99.9%; RPO is at most 15 minutes and RTO at most four hours. M27 closure requires a restore/failover exercise and a complete 30-day production soak meeting the SLO.

Managed historical toolchains use expiring, rollback-protected, threshold-signed release metadata. Noncompromised releases remain installable; revoked releases fail with an explicit migration path. First-party toolchain/runtime/registry/packages/templates use `MIT OR Apache-2.0`; generated output carries no Arche copyright claim. Third-party packages declare their own valid SPDX license. Client telemetry is off by default and sends no installation ID, command usage, source, observation, crash dump, or profile.

## 0.7 M27 implementation gates

M27 is one platform milestone with the following mandatory, ordered internal gates:

| Gate | Required result |
|---|---|
| **M27-A — promotion/foundation** | Promote this contract; update README/design/ledger; add first-party licenses; establish shared Rust workspace boundaries, a public CLI shell, status taxonomy, ID domains, empty format vectors, and retain both required greater-than-4-GiB checks. |
| **M27-B — packages/modules/HIR** | Implement schema-1 manifests, target kinds, explicit modules, workspaces, dependency resolution/locks, package-aware name resolution, root-world linking, and resolved HIR. |
| **M27-C — language semantics** | Implement the selected types, traits/specialization/operators, patterns, ownership/NLL/drop/unsafe, recursion, exceptions/effects, closures, generators, thread semantics, CTFE, and `VerifiedGenericCore`. |
| **M27-D — separate compilation** | Implement deterministic monomorphization, `VerifiedInstanceCore`, layouts/ABI, ARCHEOBJ v1, linker/coherence validation, promoted constants, and object corruption matrices. |
| **M27-E — values/reentrant runtime** | Implement process/world allocators, move/drop/codec glue, Canonical Value v1, reentrant `WorldContext`, dynamic ECS values, and the OBS3 envelope. |
| **M27-F — entity lifecycle** | Implement generation/reuse/location repair, deferred structural commands, archetype transitions, physical ordering, complete OBS3 world records, exception/trap epoch behavior, and direct-reference lifecycle proofs. |
| **M27-G — generic native AOT** | Implement calls, recursion, unwind, dynamic values, explicit world contexts/commands, system calls, blocking I/O/networking, TLS, threads/atomics, and deterministic reference/native parity. |
| **M27-H — standard product path** | Stabilize `core`/`alloc`/`std`, package cache, `check/build/run/test`, official packages, capability APIs, environment-target synthesis, and deterministic/offline builds. |
| **M27-I — developer tools** | Complete `fmt/doc/lsp/inspect/debug/profile`, source/debug metadata, workspace UX, WSL transport, observation side channels, and human/NDJSON contracts. |
| **M27-J — registry/toolchains** | Deliver managed toolchains and production registry packaging, sparse resolution, GitHub login, trusted OIDC publishing, roles, yanks/tombstones, transparency, backup, monitoring, and status service. |
| **M27-K — integrated acceptance** | Run clean-host, security/corruption, allocation-failure, offline-rebuild, WSL-fidelity, telemetry-capture, registry-restore, load/failover, and public-soak proofs. |
| **M27-L — closure** | Require exact-head protected Linux/Windows/WSL/large-file/registry CI, close documentation/evidence, verify the production SLO, and only then mark M27 complete. |

No gate may weaken or silently defer a contract assigned to it. If an external service, hosted runner, production domain, credential, signing root, or package namespace required by a gate is unavailable, that gate and M27 remain blocked.

## 0.8 M28 Arche 0.1 release proof

M28 introduces no broad language feature. It validates M27 with two equal general-purpose ECS applications.

### 0.8.1 Authoritative Arena Server

The Arena proof is a headless 60 Hz authoritative simulation with one exclusive ECS simulation thread. It includes a versioned UDP input/snapshot protocol, logical player/session IDs, reconnects, projectiles, health, teams/tags, entity transitions, resources, dynamic values, replay files, and server configuration.

A network thread validates/fragments input into a channel; the simulation canonicalizes accepted messages by tick, client ID, and sequence. Missing input becomes no-op, duplicates are rejected, late input is dropped and counted, and 0.1 has no rollback. Deterministic acceptance runs eight clients for 10,000 replay ticks with byte-identical reference/native replication, state, and status. A separate loopback run uses four clients for 1,000 ticks with loss, reordering, and reconnect injection. Windowing, graphics, audio, and local input are not required.

### 0.8.2 Seeded Grid Pursuit

Grid Pursuit uses one immutable template and shared code image for 1,024 isolated world contexts. Each 17×17 world has Runner and Chaser agents, a 64-step horizon, explicit seeded RNG, explicit RESET, and no automatic reset. Its ECS model materially uses `String`, `Vec`, ordered `Map`, enums, tags, structural transitions, despawn/drop, and Canonical Value observation.

The same executable runs counts 1, 2, 17, and 1,024 without recompilation or code-size growth. Acceptance performs 1,024 × 256 ordered world transitions plus a second variable-population environment to prevent fixture-shape specialization. Same seed/action streams are byte-identical across runs and reference/native execution; changing one action for world 513 may affect only that world.

### 0.8.3 Trainer protocol and Python adapter

Environment protocol v1 uses fixed 64-byte little-endian frames and HELLO, RESET, RESET_RESULT, STEP, STEP_RESULT, SNAPSHOT, SNAPSHOT_RESULT, CLOSE, CLOSED, and ERROR messages. It permits one outstanding request, requires exact monotonic sequence numbers, validates a complete frame before mutation, treats protocol errors as fatal, and performs no stream resynchronization.

Initial RESET covers every world; later RESET selects explicit subsets. No terminal world advances until explicitly reset. A STEP names an exhaustive action set for every ready agent/world. If world `k` fails, lower-index worlds remain committed; `k` preserves earlier committed effects but discards its pending structural epoch; higher-index worlds remain untouched.

Ship pure-Python `arche-lang-env` with a general subprocess/Canonical-Value client, one-world `ArcheParallelEnv`, batched `ArcheVectorParallelEnv`, and distinct protocol/trap/exception/environment/infrastructure exception classes. The acceptance lock pins [PettingZoo 1.26.1](https://pypi.org/project/pettingzoo/) and [Gymnasium 1.3.0](https://pypi.org/project/gymnasium/) and passes PettingZoo's documented [`parallel_api_test(..., num_cycles=1000)`](https://pettingzoo.farama.org/main/content/environment_tests/) for the one-world adapter.

### 0.8.4 Release gate

Arche 0.1 is released only when the exact protected commit:

- Creates both applications through public `arche` commands in real workspaces.
- Resolves an immutable official registry dependency, rebuilds offline from source-only cache, and reproduces ARCHEOBJ and PIE bytes.
- Passes check/build/test/fmt/doc/LSP/inspect/debug/profile on clean Linux and Windows+WSL hosts.
- Repeats deterministic native runs under ASLR with identical state/observation/status.
- Proves world/client isolation, stale handles, canonical physical ordering, allocation/drop balance, and ordered-stop state.
- Keeps the complete core proof below 20 minutes, combined compiler/reference/native RSS below 1 GiB, and scratch below 12 GiB.
- Keeps Python plus its native child below 2 GiB RSS and its proof below 20 minutes.
- Reports throughput without inventing a marketing performance floor before measurement.
- Passes every release, registry, clean-room, arena-server, and environment gate at the exact publication commit.

## 0.9 Post-0.1 direction and explicit exclusions

After 0.1, use time-bounded 0.x releases with workload-backed acceptance. Advance two equal application tracks over the shared compiler/runtime:

- **Game platform:** public unsafe C ABI, static/dynamic native linking, first-party window/input/render/audio packages, asset pipeline, client-game examples, and later editor workflows.
- **Native ML:** tensors, shape/dtype/device semantics, CPU kernels, reverse-mode autodiff, optimizers, GPU kernel compilation, and end-to-end native training workloads.

Neither track becomes Arche's exclusive identity. Self-hosting begins after 0.1: compiler-support/linker pieces move into Arche, the Rust seed builds `archec1`, `archec1` builds `archec2`, and reproducible or conformance-equivalent stage-1/stage-2 output is mandatory before 1.0.

M27/M28 explicitly exclude user FFI, graphics/audio/windowing, async/futures, user macros, trait objects, associated types, garbage collection, events, relations, optional/change-detection queries, nested query loops, parallel schedules, native Windows output, other architectures, and native tensor/autodiff/GPU facilities. General binaries may use explicit threads and ambient-I/O capabilities; deterministic environment execution rejects them.

Pre-1.0 releases may hard-cut source, manifests, object files, executable metadata, observations, registry APIs, and trainer protocols. Every contract remains explicitly versioned and old data is never silently reinterpreted. Stable compatibility begins only with 1.0.

External prerequisites include control of `arche-lang.org`, production infrastructure credentials, GitHub App/OIDC configuration, signing roots, and distribution namespaces. A missing prerequisite blocks the relevant gate rather than causing an unreviewed rename or weakened acceptance contract.

---

# 1. Vision

Arche is an independent, native, ECS-first programming language.

The language is not a general-purpose language with an ECS library attached. It is a language whose fundamental execution model is:

```text
world
  contains entities
  arranged into archetype tables
  storing component columns
  processed by systems
  selected by typed queries
  ordered by schedules
  mutated structurally through command buffers
```

The final goal is a complete Arche software platform:

```text
Arche source code
  ↓
Arche compiler
  ↓
Arche object files
  ↓
Arche linker
  ↓
native executable
  ↓
Arche runtime kernel
```

A finished user experience should be:

```bash
arche new asteroids
cd asteroids
arche run
arche build --release
arche test
arche debug
arche profile
```

Arche should stand on its own in the same sense that other language ecosystems stand on their own: it should have its own compiler, runtime model, build tool, package model, debugging tools, profiling tools, and standard library.

The first implementation should be low-level and final-goal-oriented. The earliest investments should go into:

```text
native executable emission
ECS runtime kernel
component metadata
query planning
compiled system loops
schedule execution
object format
linker model
```

The high-level syntax exists to express these concepts, not to hide them.

---

# 2. Non-Negotiable Design Principles

## 2.1 ECS is the language core

Arche must treat ECS concepts as first-class language semantics:

```text
entity
component
tag
resource
system
query
schedule
command buffer
event stream
relation
```

These are not framework APIs. The compiler must understand them.

## 2.2 Native execution from the beginning

Arche should produce native executables. The first target is deliberately narrow, but native:

```text
x86-64 Linux
static ELF64 position-independent executable (PIE)
ET_DYN with no interpreter or dynamic relocations
no required libc dependency
_start entrypoint
syscall-based process exit
```

## 2.3 No transpilation dependency

Arche should not depend on generating Rust, C++, JavaScript, or another language as its permanent implementation strategy.

Generated source diagnostics, host language limitations, and framework constraints should not define the Arche experience.

## 2.4 Runtime kernel, not VM

Arche will have a runtime, but the runtime is not a virtual machine. It is the ECS kernel that manages:

```text
world memory
entity generations
archetype tables
component columns
resources
queries
commands
events
schedules
profiling hooks
debug hooks
```

Compiled Arche systems execute as native machine code.

## 2.5 Data-oriented by default

The default storage strategy is archetype-table columnar storage.

Entities are opaque IDs. Components are plain data. Systems are behavior. Queries are compiled data-access patterns.

## 2.6 Structural mutation is explicit

Spawning, despawning, adding components, and removing components are structural operations. During system iteration, they are deferred through command buffers.

## 2.7 Access determines scheduling

System signatures define what data a system reads and writes. The compiler and scheduler use this to detect conflicts and plan execution.

## 2.8 The low-level substrate is permanent

Arche should not begin with a temporary high-level representation that later gets thrown away. The project should begin by defining permanent artifacts:

```text
ABI
Arche Core
runtime kernel
object format
linker metadata
query descriptors
schedule descriptors
component descriptors
```

---

# 3. What Arche Is

Arche is:

```text
A native programming language.
An ECS-native programming language.
A data-oriented execution platform.
A compiler and runtime kernel.
A build and package ecosystem.
A debugging and profiling environment designed around ECS.
```

Arche programs are organized around worlds, components, resources, systems, queries, and schedules.

A minimal Arche program:

```arche
world Main

startup {
    exit 42
}
```

A minimal ECS Arche program:

```arche
world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}

schedule Main {
    run Move
}

startup {
    resource Time { delta: 1.0 }

    spawn {
        Position { x: 0.0, y: 0.0 }
        Velocity { x: 2.0, y: 3.0 }
    }

    run Main
    exit 0
}
```

This program compiles into native machine code containing:

```text
component metadata
resource metadata
system function Move
query descriptor for [mut Position, Velocity]
schedule descriptor for Main
runtime boot code
world initialization
compiled query loop
```

---

# 4. What Arche Is Not

Arche is not:

```text
A Rust ECS framework.
A C++ ECS framework.
A scripting language embedded into an engine.
A general-purpose OO language with components added later.
A VM-first language.
A transpiler-first language.
A language whose semantics depend on LLVM or another host ecosystem.
```

Arche may eventually support foreign function interfaces, multiple backends, and external tooling, but its core identity should remain independent.

---

# 5. Target Product Experience

A finished Arche project should look like this:

```text
asteroids/
  Arche.toml
  src/
    main.arc
    player.arc
    enemies.arc
    physics.arc
    render.arc
  assets/
  tests/
```

`Arche.toml`:

```toml
[package]
name = "asteroids"
version = "0.1.0"

[target]
default = "x86_64-linux"

[dependencies]
math = "0.1"
render2d = "0.1"
```

Common commands:

```bash
arche check
arche build
arche build --release
arche run
arche test
arche debug
arche profile
arche inspect target/debug/asteroids
```

The build tool should eventually orchestrate:

```text
source discovery
package resolution
incremental compilation
object generation
Arche metadata linking
native binary linking
runtime selection
debug metadata generation
profile metadata generation
```

---

# 6. First Target Platform

The M26 output target is intentionally narrow:

```text
Architecture: x86-64
Operating system: Linux
Executable format: static ELF64 PIE (ET_DYN)
Entry point: _start
Runtime dependency: linked Arche runtime kernel only
Interpreter/dynamic relocation dependency: none
C library dependency: none
```

Windows is a compiler and test host, not an M26 executable target. PE/COFF, Mach-O, AArch64, WebAssembly, native Windows output, and dynamic linking are later work.

## 6.1 M26 authority boundary

Executable semantic checking has one branded result: `VerifiedExecutableCore`. Only the executable verifier can construct it. Bare `archec0 SOURCE` and `archec0 SOURCE --check` both require a buildable executable and emit no artifact. `--emit-core`, `--emit-machine`, and `-o` use the same executable checker; `--emit-ast` remains syntax-only and `--inspect-components` remains declaration-only.

The syntax-only AST view contains no derived identity. Core and Machine diagnostic output displays every schema, system, query, and schedule ID as 32 uppercase hexadecimal digits alongside the separate checked `u64` Core index. Machine links also display the distinct ABI and Core-body hashes used by AOT dispatch. These views expose linkage for testing and inspection without making a dense index into stable identity.

Compiler status is `0` for success, `1` for parse, semantic, or build failure, and `2` for usage or unsafe output-target errors. Lexer-captured EOF and closing-brace positions give missing startup, missing final exit, and incomplete literals stable, precise spans.

Verified Core is authoritative for program semantics and represents ordered resource initialization, spawn, schedule dispatch, scalar operations, blocks, branches, loops, query iteration, resource and component access, and exit. Machine IR is a generalized AOT lowering detail derived from verified Core, not a second semantic authority.

The reference executor interprets Core directly and independently of native lowering. Reference and native execution both consume metadata decoded and linked through the same ARCHEECS v2 contract. Metadata selects and orders schemas, startup operations, payloads, schedules, query bindings, and native functions. AOT machine code remains authoritative for system-body instructions. Separate ABI and verified-Core-body hashes bind each metadata function link to its body.

No production behavior may depend on fixture names, declaration ordinals, fixed descriptor, term, or lane counts, whole-blob equality, a compiler-side execution-shape recognizer, or a source-derived host-only plan.

## 6.2 M26 language boundary

Stored fields and locals support only `i32`, `f32`, and `bool`. They have layouts 4/4, 4/4, and 1/1 respectively. Integers use little-endian two's-complement bits, floats use IEEE binary32 bits, and booleans encode only as `0` or `1`. There are no implicit numeric or boolean conversions.

Components and resources may contain zero fields. `tag Name` is a zero-sized, alignment-one schema whose v2 flags field is exactly `TAG = 0x1`; component and resource schema flags are `0`. Tags and other zero-sized schemas participate in archetype signatures and query matching without payload allocation. M26 accepts `{}` literals, initialized and uninitialized empty resources, tag attachment such as `Enemy {}`, and `spawn {}` for an empty-archetype entity.

Every component or resource literal names each declared field exactly once. Unknown, duplicate, and omitted fields are errors. Named fields can appear in any order; initializer expressions evaluate in source order and payload bytes are stored in declaration order.

Startup and systems share the same typed scalar expression semantics:

```text
primary
unary:             - ~ !
multiplicative:    * / %
additive:          + -
shift:             << >>
relational:        < <= > >=
equality:          == !=
bitwise:           & then ^ then |
logical:           && then ||
```

Parentheses, mutable `let` locals, direct assignment, and typed field initialization are supported in both startup and systems; systems additionally support component and resource field reads and writes. `i32` supports arithmetic, remainder, unary negation, shifts, comparisons, equality, and bitwise operations. `f32` supports arithmetic, unary negation, comparisons, and equality. `bool` supports literals, negation, short-circuit conjunction/disjunction, and equality.

Integer addition, subtraction, multiplication, unary negation, left shift, and bitwise operations wrap in two's complement. Right shift is arithmetic and shift counts use the low five RHS bits. Division or remainder by zero, plus `i32::MIN / -1` and `i32::MIN % -1`, trap. `-2147483648` is the one magnitude interpreted after unary negation; all other integer literals must fit after unary interpretation.

At compiler process entry, before semantic folding, and at emitted-program entry, before execution, the floating-point environment is round-to-nearest-even, exceptions are masked, and FTZ/DAZ are disabled. Operations are not fused. Subnormals and signed zero are preserved. Every arithmetic NaN result is canonicalized to bits `0x7FC00000`. Ordered comparisons with NaN are false except `!=`, which is true.

Startup is straight-line and source ordered. It gains the shared expressions and mutable-local assignment, including `+=`, but not `if` or `while`. Systems support lexical blocks, locals, assignment, `if`/`else`, `while`, multiple query loops, and `+=` for mutable numeric locals, resource fields, and query component fields. Query loops can occur inside control flow but cannot nest, and there is no language-level loop iteration cap. Other compound assignments are outside M26.

## 6.3 M26 ECS boundary

System resources use `read Resource` or `mut Resource`. Duplicate read-only aliases are valid; an alias set containing mutable access is rejected conservatively. Queries support required reads `T`, mutable terms `mut T`, and exclusions `!T`. Source term order determines bindings. Exclusions do not bind, so an exclusion-only query uses the empty binding list `for ()`. A required tag or other zero-sized term can bind only `_`; `mut Tag` is invalid. Including and excluding the same schema is invalid.

The executable checker rejects duplicate queryable schemas, resources, systems, schedules, fields, parameters, and active lexical bindings. It also proves before every startup schedule run that every resource read or mutated by each dispatched system has already been initialized. Metadata decoding repeats this resource-flow check before any world mutation.

Entity handles, optional terms, change detection, nested query loops, structural mutation from systems, command buffers, events, relations, and parallel schedules remain deferred.

## 6.4 M26 metadata and identity

The executable carries one hard-cut little-endian metadata format: `ARCHEECS` version 2. Earlier ARCHEECS versions and ARCHECMP are not executable compatibility paths. Both legacy forms exit `1` before world mutation. Their exact diagnostics are `arche: unsupported ARCHEECS version 1; rebuild with archec0` and `arche: unsupported ARCHECMP artifact; rebuild with archec0`.

The v2 header is 64 bytes and records magic, version, header size, flags, total length, directory offset/count/row size, and zeroed reserved fields. Each directory row is 64 bytes and records kind, flags, offset, byte length, record count, record stride, alignment, and zeroed reserved fields. Canonical sections contain strings, world, schemas, fields, systems, parameters, queries, terms, schedules, items, startup operations, payloads, function links, and source spans.

All offsets, counts, sizes, lengths, strides, slice references, layout values, dense indexes, and Core IDs are checked `u64`. IDs remain raw 16-byte values; variable data lives in strings or payloads. The decoder validates the complete directory and all cross-references before world creation: checked arithmetic, bounds, alignment, overlap, stride, UTF-8, canonical and required section policy, unique IDs, dense-index bounds, layouts, payloads, query access, schedule targets, startup/resource flow, spans, function offsets, ABI hashes, and body hashes. Structural validation is not a signature or authenticity guarantee.

Schemas use `SchemaId([u8; 16])`; systems, queries, and schedules use separate domain-separated 128-bit declaration IDs; function linking uses distinct 128-bit ABI and Core-body hashes. Dense runtime indexes are assigned only after canonical ID sorting and are not stable identity.

Stable identifiers and link hashes use pinned pure-Rust BLAKE3 1.8.5. Every preimage starts with its NUL-terminated domain bytes and fingerprint version `1` encoded as a little-endian `u32`; this is distinct from the little-endian `u64` canonical-Core encoding version inside ABI/body payloads. Schema, declaration, ABI, and body payloads are specified byte-for-byte in Section 23.1. The first 16 digest bytes are stored verbatim and displayed as 32 uppercase hexadecimal digits.

## 6.5 M26 observation and failure behavior

After a source exit or semantic trap, successful world observation is streamed to stdout as canonical uppercase ASCII `ARCHEOBS2`. Resources sort by schema ID; tables sort lexicographically by sorted schema-ID signatures; rows retain committed spawn order; columns follow signature order. Observation includes initialized and uninitialized resources, initialized empty payloads, tag and zero-sized membership, empty-archetype rows, and zero-length columns. Empty payloads print as `-`; allocator capacity is not semantic state.

The record grammar is:

```text
ARCHEOBS2
RESOURCE <ID32> UNINITIALIZED
RESOURCE <ID32> INITIALIZED <LEN> <HEX-or-->
TABLE <KEY_COUNT> <IDS...> <ROW_COUNT>
ROW <ROW_INDEX> <SPAWN_ORDINAL> <COLUMN_COUNT>
COLUMN <ID32> <LEN> <HEX-or-->
END
```

On an integer trap, prior committed effects remain and the trapping write is not applied. The process emits and flushes the full observation, writes `arche: trap[<KIND>] <basename>:<line>:<column> bytes <start>..<end>` to stderr, and exits `70`. Metadata, link, allocation, or observation-I/O failure exits `1` without claiming a complete observation. A source `exit 70` remains distinguishable because it has no trap diagnostic. Normal source exit uses the low eight bits of its `i32` value.

## 6.6 M26 source and publication boundary

`SourceSpan` carries checked `u64` byte boundaries and lexer-captured `u64` line/column endpoints. Compilation first snapshots the complete input into a private temporary spool, retains the original `SourceIdentity`, and then reads only the immutable snapshot. The lexer is incremental over `BufRead`; the parser keeps two-token lookahead; diagnostic snippets are bounded; the spool is cleaned on every outcome. AST/Core storage is proportional to semantic content and identifiers are interned instead of copying source slices.

Formatters and encoders accept streaming `Write`; metadata and ELF writers accept `Write + Seek`. Conversion to `usize` occurs only at a host allocation or slice boundary and can fail explicitly for overflow, address-space, filesystem, or allocation limits. There is no fixed product cap.

Output publication calls a producer on a unique sibling temporary, synchronizes it, preserves executable permissions, repeats exact-path, relative/canonical, symlink, and hard-link source/output alias defenses, cleans failures, and atomically replaces the target. The promise is atomic visibility only, not parent-directory synchronization or power-loss durability.

## 6.7 M26 ELF contract

The emitted static PIE is `ET_DYN` with no interpreter, dynamic relocations, or text relocations. It has distinct read-only header, read-execute text, read-write non-executable data/BSS/world-state, and read-only metadata `PT_LOAD` segments, plus a read-write non-executable `PT_GNU_STACK`. Page alignment and file-offset/virtual-address congruence are preserved and no segment is writable and executable.

Native code establishes a RIP anchor and represents metadata, function, and data targets as checked 64-bit image-relative deltas. Calls and jumps use far-safe indirect sequences. Conditional branches invert over a local short skip before the far transfer, eliminating late `rel32` range failures. ELF output is streamed and backpatched; sparse holes are created with seeking instead of zero-filled buffers.

## 6.8 M26 closure boundary

M26 is not complete merely because these requirements are documented or local tests pass. Closure requires byte-identical ARCHEOBS2 and equal status from the independent Core reference path and native PIE for the primary multi-system fixture and structurally different Arena fixture, plus an exact trap proof. Both greater-than-4-GiB Linux job contexts must be registered as required checks, and those jobs, strict Linux and Windows Clippy, the complete proof suite, dependency audit, and exact-head CI must pass and be recorded in `WORK_LOG.md`.

The large-source contract covers a semantically ordinary program with very large input bytes; it does not promise that billions of declarations fit under the CI memory bound. Host address-space, filesystem, allocation, and kernel limits are explicit failures. M26 adds no signing, sandboxing, or crash-durability guarantee.

---

# 7. System Overview

The complete Arche platform contains:

```text
arche        main project/build/package command
archec       compiler
archeas      Arche Core / assembly tool, if separated
archeld      Arche linker
archefmt     formatter
archedb      ECS-aware debugger
archeprof    ECS-aware profiler
```

Primary compilation pipeline:

```text
.arc source files
  ↓
lexer
  ↓
parser
  ↓
AST
  ↓
semantic analysis
  ↓
Arche Core
  ↓
layout planner
  ↓
query planner
  ↓
schedule planner
  ↓
backend code generation
  ↓
Arche object files (.aco)
  ↓
Arche linker
  ↓
native executable
```

Runtime execution pipeline:

```text
_start
  ↓
arche_boot
  ↓
initialize allocator
  ↓
create world
  ↓
register metadata
  ↓
execute startup block
  ↓
execute schedules
  ↓
flush commands
  ↓
shutdown world
  ↓
exit process
```

---

# 8. Arche Execution Model

The execution model is ECS-native.

## 8.1 Entities

Entities are opaque handles.

```text
Entity = index + generation
```

Entities do not contain behavior. They do not have methods. They do not own components directly from the language perspective.

## 8.2 Components

Components are typed data attached to entities.

```arche
component Position {
    x: f32
    y: f32
}
```

Components are stored in columns inside archetype tables.

## 8.3 Tags

Tags are zero-sized components.

```arche
tag Player
tag Enemy
tag Frozen
```

Tags affect queries and archetype membership but do not occupy component column storage.

## 8.4 Resources

Resources are singleton world-level data.

```arche
resource Time {
    delta: f32
    elapsed: f32
}
```

## 8.5 Systems

Systems are functions that declare ECS access through their parameters.

```arche
system Move(
    time: read Time,
    q: query[mut Position, Velocity]
) {
    for (pos, vel) in q {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}
```

## 8.6 Queries

Queries select entities by component membership and produce references to component columns.

```arche
query[Position]
query[mut Position, Velocity]
query[Player, mut Health]
query[Position, !Frozen]
query[entity, Enemy, Position] // post-M26 entity handles
```

## 8.7 Schedules

Schedules define execution order and barriers.

```arche
schedule Main {
    run Input
    run Move
    run ApplyDamage
    flush // post-M26 command-buffer barrier
    run Render
}
```

---

# 9. Core Runtime Concepts

The core runtime concepts are:

```text
World
EntityStore
ArchetypeStore
ArchetypeTable
ComponentColumn
ResourceStore
QueryDesc
QueryPlan
QueryIterator
CommandBuffer
EventStore
ScheduleDesc
SystemDesc
Allocator
```

The runtime kernel should be small, explicit, and inspectable.

It should expose stable internal functions to compiled systems:

```text
world_create
world_destroy
entity_alloc
entity_free
resource_get
resource_insert
query_prepare
query_begin
query_next_chunk
commands_append
commands_flush
schedule_run
```

The compiled code should not use a high-level runtime API for hot loops. It should use query plans and direct column access wherever possible.

---

# 10. Entity Model

## 10.1 Representation

An entity is a 64-bit value:

```text
bits 0..31   index
bits 32..63  generation
```

Conceptual C representation:

```c
typedef struct ArcheEntity {
    uint32_t index;
    uint32_t generation;
} ArcheEntity;
```

Packed representation:

```c
typedef uint64_t ArcheEntityBits;
```

## 10.2 Entity store

The entity store maps an entity index to its current location:

```c
typedef struct ArcheEntityLocation {
    uint32_t generation;
    uint32_t alive;
    uint64_t archetype_index;
    uint64_t row;
} ArcheEntityLocation;
```

Entity creation:

```text
1. Pop free index or append new location.
2. Increment or initialize generation.
3. Insert entity into destination archetype table.
4. Store location.
```

Entity destruction:

```text
1. Validate generation.
2. Remove row from archetype table using swap-remove.
3. Update moved entity location if a swap occurred.
4. Increment generation.
5. Push index to free list.
```

## 10.3 Stale handles

Generation counters allow stale handle detection:

```text
entity handle says: index=42, generation=3
entity store says:  index=42, generation=4
result: stale entity
```

Queries should never produce stale entities. Direct entity operations must validate handles.

---

# 11. Component Model

## 11.1 Component declaration

```arche
component Position {
    x: f32
    y: f32
}
```

## 11.2 M26 component field types

M26 components and resources are plain data with this complete field-type set:

```text
bool
i32
f32
```

Other integer widths, `f64`, entity handles, arrays, and nested plain structs are later additions.

Delay:

```text
strings
heap arrays
references
custom destructors
generics
trait objects
closures
managed pointers
```

This keeps layout and runtime behavior predictable.

## 11.3 Component descriptor

Each component emits metadata:

```c
typedef struct ArcheComponentDesc {
    uint128_t stable_id;
    uint64_t dense_id;
    const char* name;
    uint64_t size;
    uint64_t align;
    uint64_t field_count;
    const ArcheFieldDesc* fields;
    uint64_t flags;
} ArcheComponentDesc;
```

Field descriptor:

```c
typedef struct ArcheFieldDesc {
    const char* name;
    uint64_t type_kind;
    uint64_t offset;
    uint64_t size;
    uint64_t align;
} ArcheFieldDesc;
```

Example:

```text
Component Demo.Position
  stable_id: fingerprint(Demo.Position schema)
  dense_id: assigned by linker/runtime
  size: 8
  align: 4
  fields:
    x: f32 offset=0
    y: f32 offset=4
```

## 11.4 Storage

Components are stored in columns:

```text
Position column:
  [Position, Position, Position, ...]
```

For a component size of 8 and row `i`:

```text
address = column_base + i * 8
```

Field access:

```text
pos.x = column_base + i * sizeof(Position) + offset(x)
```

---

# 12. Resource Model

Resources are singleton values stored in the world.

```arche
resource Time {
    delta: f32
    elapsed: f32
}
```

Resource access in systems:

```arche
system Move(
    time: read Time,
    q: query[mut Position, Velocity]
) {
    ...
}
```

Access modes:

```text
read Time
mut Time
```

Resource conflicts:

```text
read Time + read Time = safe
read Time + mut Time  = conflict
mut Time  + mut Time  = conflict
```

Resource descriptor:

```c
typedef struct ArcheResourceDesc {
    uint128_t stable_id;
    uint64_t dense_id;
    const char* name;
    uint64_t size;
    uint64_t align;
    uint64_t field_count;
    const ArcheFieldDesc* fields;
} ArcheResourceDesc;
```

---

# 13. Tag Model

Tags are zero-sized components.

```arche
tag Player
tag Enemy
tag Frozen
```

Tag descriptor:

```text
size: 0
align: 1
flags: TAG
```

Tags participate in archetype signatures:

```text
Archetype<Player, Position, Velocity>
Archetype<Enemy, Position, Health>
Archetype<Enemy, Frozen, Position, Health>
```

Tags are useful for:

```text
entity classification
query filters
exclusive sets
state markers
phase markers
```

Future feature:

```arche
exclusive tags Player, Enemy, Projectile
```

This lets the compiler prove disjointness between queries.

---

# 14. System Model

A system is a native function plus ECS metadata.

Source:

```arche
system Move(
    time: read Time,
    movers: query[mut Position, Velocity, !Frozen]
) {
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}
```

System metadata:

```text
name: Demo.Move
function: pointer to native machine code
reads resources: Time
writes resources: none
reads components: Velocity
writes components: Position
excludes components: Frozen
structural write: false
queries: Move.q0
```

M26 also permits `mut Resource` parameters. Duplicate aliases are accepted only when all aliases are read-only; any alias set containing mutable access is rejected conservatively across the system.

System ABI:

```c
typedef void (*ArcheSystemFn)(
    ArcheWorld* world,
    ArcheFrame* frame,
    ArcheCommandBuffer* commands
);
```

Every system receives:

```text
world pointer
frame pointer
command buffer pointer
```

The compiler can omit or ignore unused parameters internally, but the public ABI remains stable.

---

# 15. Query Model

Queries are typed ECS access patterns.

## 15.1 Query syntax

```arche
query[Position]
query[mut Position]
query[Position, Velocity]
query[mut Position, Velocity]
query[Position, !Frozen]
query[entity, Enemy, Position]  // future entity binding
query[?Velocity, Position]       // future optional term
query[Changed<Position>]         // future change detection
query[Added<Enemy>]              // future added detection
query[Removed<Health>]           // future removed detection
```

## 15.2 Query terms

| Syntax | Meaning |
|---|---|
| `T` | Required read access to component/tag `T` |
| `mut T` | Required write access to component `T` |
| `!T` | Entity must not have component/tag `T` |
| `entity` | Include entity handle in iteration, post-M26 |
| `?T` | Optional component, future |
| `Changed<T>` | Component changed since last run, future |
| `Added<T>` | Component recently added, future |
| `Removed<T>` | Component recently removed, future |

## 15.3 Query descriptor

```c
typedef enum ArcheQueryAccess {
    ARCHE_QUERY_READ,
    ARCHE_QUERY_WRITE,
    ARCHE_QUERY_EXCLUDE,
    ARCHE_QUERY_OPTIONAL,
    ARCHE_QUERY_ENTITY
} ArcheQueryAccess;

typedef struct ArcheQueryTerm {
    uint128_t stable_component_id;
    uint64_t dense_component_id;
    uint8_t access;
} ArcheQueryTerm;

typedef struct ArcheQueryDesc {
    const char* name;
    uint64_t term_count;
    const ArcheQueryTerm* terms;
} ArcheQueryDesc;
```

M26 preserves term order for bindings. Exclusions never bind. Tags and other zero-sized required terms can bind only `_`. Mutable tags and simultaneous inclusion and exclusion of one schema are invalid. Query loops may appear in system control flow but cannot nest.

## 15.4 Query plan

At runtime or link time, a query descriptor becomes a query plan:

```c
typedef struct ArcheQueryTablePlan {
    uint64_t archetype_index;
    uint64_t entity_column_present;
    uint64_t* component_column_indices;
} ArcheQueryTablePlan;

typedef struct ArcheQueryPlan {
    const ArcheQueryDesc* desc;
    uint64_t table_count;
    ArcheQueryTablePlan* tables;
} ArcheQueryPlan;
```

## 15.5 Query lowering

Source:

```arche
for (pos, vel) in movers {
    pos.x += vel.x * time.delta
}
```

Core form:

```text
for_chunks movers.plan {
    pos_col = column Position
    vel_col = column Velocity
    len = chunk_len

    for i in 0..len {
        pos = pos_col + i * sizeof(Position)
        vel = vel_col + i * sizeof(Velocity)

        pos.x = pos.x + vel.x * delta
    }
}
```

The compiler should emit the hot inner loop directly.

---

# 16. Schedule Model

Schedules describe system execution.

```arche
schedule Main {
    run PlayerInput
    run Move
    run ApplyDamage
    flush // post-M26
    run Render
}
```

A schedule is compiled into:

```text
system nodes
access sets
manual ordering constraints
barriers
flush points
execution batches
```

Schedule descriptor:

```c
typedef struct ArcheScheduleNode {
    uint64_t system_index;
    uint64_t dependency_count;
    uint64_t* dependencies;
} ArcheScheduleNode;

typedef struct ArcheScheduleBatch {
    uint64_t node_count;
    ArcheScheduleNode* nodes;
    uint8_t has_flush_after;
} ArcheScheduleBatch;

typedef struct ArcheScheduleDesc {
    const char* name;
    uint64_t batch_count;
    ArcheScheduleBatch* batches;
} ArcheScheduleDesc;
```

Early implementation may execute every schedule sequentially. The representation should still be batch-oriented so parallel scheduling can be added without redesign.

Conflict rules:

```text
write component A conflicts with read component A
write component A conflicts with write component A
read component A does not conflict with read component A
write resource R conflicts with read/write resource R
read resource R does not conflict with read resource R
structural writes require flush visibility
```

---

# 17. Command Buffer Model

Structural mutations are deferred during system execution.

Examples:

```arche
cmd.spawn {
    Position { x: 0.0, y: 0.0 }
    Velocity { x: 1.0, y: 0.0 }
}

cmd.despawn(e)
cmd.add<Health>(e, Health { hp: 100, max: 100 })
cmd.remove<Frozen>(e)
```

Binary command format:

```c
typedef enum ArcheCommandKind {
    ARCHE_CMD_SPAWN,
    ARCHE_CMD_DESPAWN,
    ARCHE_CMD_ADD_COMPONENT,
    ARCHE_CMD_REMOVE_COMPONENT,
    ARCHE_CMD_SET_RESOURCE,
    ARCHE_CMD_EMIT_EVENT
} ArcheCommandKind;

typedef struct ArcheCommandHeader {
    uint16_t kind;
    uint64_t align;
    uint64_t size;
} ArcheCommandHeader;
```

Command buffer layout:

```text
[header][payload][header][payload][header][payload]
```

Despawn payload:

```c
typedef struct ArcheCmdDespawn {
    ArcheEntity entity;
} ArcheCmdDespawn;
```

Spawn payload conceptually:

```text
component_count
component_id[component_count]
component_payload_blob
```

At a flush point:

```text
1. Read command stream.
2. Apply despawns.
3. Apply spawns.
4. Apply add/remove component moves.
5. Update entity locations.
6. Invalidate affected query caches.
7. Clear command buffer.
```

---

# 18. Event Model

Events are typed streams.

```arche
event Damage {
    target: entity
    amount: i32
}
```

Emission:

```arche
system DetectHits(
    out damage: emit Damage,
    bullets: query[entity, Bullet, Position],
    enemies: query[entity, Enemy, Position]
) {
    ...
    damage.emit(Damage { target: enemy, amount: 10 })
}
```

Reading:

```arche
system ApplyDamage(
    damage: events Damage,
    health: query[entity, mut Health]
) {
    for d in damage {
        if let Some(h) = health.get_mut(d.target) {
            h.hp -= d.amount
        }
    }
}
```

Event descriptor:

```c
typedef struct ArcheEventDesc {
    uint128_t stable_id;
    uint64_t dense_id;
    const char* name;
    uint64_t size;
    uint64_t align;
    uint32_t lifetime;
} ArcheEventDesc;
```

Event lifetimes:

```text
stage
frame
manual
```

Events should be added after systems, queries, schedules, and command buffers are stable.

---

# 19. Relations

Relations represent typed edges between entities.

```arche
relation ParentOf {
    parent: entity
    child: entity
}

relation EquippedBy {
    item: entity
    owner: entity
}
```

Relations are useful for:

```text
hierarchies
ownership
attachments
inventories
graphs
links between simulated objects
```

Potential relation storage:

```text
edge table per relation type
source index
target index
optional payload columns
```

Example future query:

```arche
system PropagateTransforms(
    graph: relation ParentOf,
    locals: query[LocalTransform],
    worlds: query[mut WorldTransform]
) {
    for edge in graph.topological() {
        ...
    }
}
```

Relations should be delayed until the basic ECS runtime and query system are reliable.

---

# 20. Memory Model

## 20.1 Primary storage layout

Arche uses archetype tables by default.

Entity with:

```text
Position
Velocity
Health
```

belongs to:

```text
Archetype<Position, Velocity, Health>
```

Storage:

```text
entities: [e0, e1, e2, e3]
Position: [p0, p1, p2, p3]
Velocity: [v0, v1, v2, v3]
Health:   [h0, h1, h2, h3]
```

## 20.2 Structural changes

Adding a component moves an entity between archetype tables.

Before:

```text
Archetype<Position, Velocity>
```

After adding `Health`:

```text
Archetype<Position, Velocity, Health>
```

Process:

```text
1. Find or create destination archetype table.
2. Copy/move existing components to destination row.
3. Initialize new component.
4. Remove old row using swap-remove.
5. Update locations.
```

## 20.3 Column allocation

Column allocation should eventually support:

```text
alignment
capacity growth
custom allocators
page/block allocation
SIMD-friendly alignment
sparse component layout
large blob layout
```

Initial version:

```text
contiguous heap allocation
power-of-two capacity growth
component stride equals aligned size
```

## 20.4 Borrowing and references

Component references are valid only during query iteration.

Invalid:

```arche
var cached: &Position

system Bad(q: query[Position]) {
    for pos in q {
        cached = &pos
    }
}
```

Valid:

```arche
component Target {
    entity: entity
}
```

Store entity handles, not component references.

---

# 21. Arche Runtime Kernel

The runtime kernel is the permanent low-level execution engine.

## 21.1 Responsibilities

```text
memory allocation
world lifecycle
entity lifecycle
component registration
resource registration
archetype table management
query cache management
command buffer application
event stream management
schedule execution
profiling hooks
debug hooks
```

## 21.2 Runtime module layout

```text
runtime/
  kernel/
    allocator
    world
    entity
    component
    resource
    archetype
    query
    command
    event
    schedule
    debug
    profile

  platform/
    linux_x86_64
    windows_x86_64
    macos_aarch64
```

## 21.3 Runtime API classes

The runtime API exposed to compiled systems should be minimal.

```c
ArcheWorld* arche_world_create(void);
void arche_world_destroy(ArcheWorld* world);

void* arche_resource_get(ArcheWorld* world, uint64_t dense_resource_id);
void arche_resource_insert(ArcheWorld* world, uint64_t dense_resource_id, const void* value);

ArcheQueryPlan* arche_query_prepare(ArcheWorld* world, const ArcheQueryDesc* desc);
void arche_query_begin(ArcheWorld* world, ArcheQueryPlan* plan, ArcheQueryIter* out);
bool arche_query_next_chunk(ArcheQueryIter* iter, ArcheChunkView* out);

void arche_commands_append(ArcheCommandBuffer* buffer, const void* command, uint64_t size);
void arche_commands_flush(ArcheWorld* world, ArcheCommandBuffer* buffer);
```

Compiled systems may eventually bypass some runtime calls for known-safe, preplanned queries.

---

# 22. Arche ABI

The ABI defines stable binary expectations.

## 22.1 Primitive sizes

| Arche type | Size | Alignment | Notes |
|---|---:|---:|---|
| `bool` | 1 | 1 | M26, stored only as 0 or 1 |
| `i8` | 1 | 1 | Future |
| `u8` | 1 | 1 | Future |
| `i16` | 2 | 2 | Future |
| `u16` | 2 | 2 | Future |
| `i32` | 4 | 4 | M26, little-endian two's-complement bits |
| `u32` | 4 | 4 | Future source type; metadata uses checked `u64` |
| `i64` | 8 | 8 | Future |
| `u64` | 8 | 8 | Future source type; metadata uses checked `u64` |
| `f32` | 4 | 4 | M26, IEEE binary32 bits |
| `f64` | 8 | 8 | Future |
| `entity` | 8 | 8 | Future packed index + generation |

Zero-field components and resources have size zero and alignment one. Tags have the same layout and additionally carry the TAG metadata flag.

## 22.2 Struct layout

Initial layout rules:

```text
Fields are laid out in declaration order.
Each field is aligned to its natural alignment.
Struct alignment is the maximum field alignment.
Struct size is rounded up to struct alignment.
```

Example:

```arche
component Position {
    x: f32
    y: f32
}
```

Layout:

```text
x: offset 0, size 4, align 4
y: offset 4, size 4, align 4
struct size: 8
struct align: 4
```

## 22.3 System ABI

The long-term public system ABI may expose:

```c
typedef void (*ArcheSystemFn)(
    ArcheWorld* world,
    ArcheFrame* frame,
    ArcheCommandBuffer* commands
);
```

M26 links its internal generated functions through ARCHEECS v2 function records and validates both ABI and Core-body hashes before world mutation. The command-buffer parameter has no language-level operations in M26. A future public ABI can use the following x86-64 convention:

```text
rdi = world
rsi = frame
rdx = command buffer
```

## 22.4 Query chunk view ABI

```c
typedef struct ArcheChunkView {
    uint64_t len;
    ArcheEntity* entities;
    void** columns;
} ArcheChunkView;
```

`columns[n]` corresponds to the nth non-excluded component term in the query descriptor.

---

# 23. Component Identity and Linking

Arche needs both stable identity and fast runtime identity.

## 23.1 Stable IDs

Stable IDs are used across:

```text
packages
object files
save files
debug metadata
linking
schema comparison
```

A component, resource, or tag has `SchemaId([u8; 16])`. Systems, schedules, and queries use the same raw 16-byte representation as `DeclId`; native links use raw 16-byte `AbiHash` and `BodyHash` values. All four types use this normative construction:

```text
stable128(domain, payload) = BLAKE3(domain || u32le(1) || payload)[0..16]
```

The BLAKE3 operation is unkeyed. `domain` includes its trailing NUL byte. `u32le(1)` is the four bytes `01 00 00 00`; it is the fingerprint-format version for every ID and hash domain. The first 16 digest bytes are copied verbatim, without integer reinterpretation, and display in wire order as 32 uppercase hexadecimal digits.

The canonical byte vocabulary is:

| Notation | Bytes |
|---|---|
| `u8(x)` | one byte |
| `bool(x)` | `00` for false or `01` for true |
| `u32le(x)` | four bytes, least significant first |
| `u64le(x)` | eight bytes, least significant first |
| `id128(x)` | the 16 raw bytes of the referenced ID or hash |
| `str(x)` | `u64le(UTF-8 byte length)` followed by the UTF-8 bytes, with no terminator |

Counts and Core IDs are `u64le`. Sequences are count-prefixed only where the grammar below explicitly shows a count. Records are concatenated with no padding or alignment bytes.

The exact domains are:

| Purpose | ASCII bytes | Hex bytes |
|---|---|---|
| schema ID | `ARCHE-SCHEMA-ID\0` | `41 52 43 48 45 2D 53 43 48 45 4D 41 2D 49 44 00` |
| system ID | `ARCHE-SYSTEM-ID\0` | `41 52 43 48 45 2D 53 59 53 54 45 4D 2D 49 44 00` |
| schedule ID | `ARCHE-SCHEDULE-ID\0` | `41 52 43 48 45 2D 53 43 48 45 44 55 4C 45 2D 49 44 00` |
| query ID | `ARCHE-QUERY-ID\0` | `41 52 43 48 45 2D 51 55 45 52 59 2D 49 44 00` |
| ABI hash | `ARCHE-ABI-HASH\0` | `41 52 43 48 45 2D 41 42 49 2D 48 41 53 48 00` |
| Core-body hash | `ARCHE-BODY-HASH\0` | `41 52 43 48 45 2D 42 4F 44 59 2D 48 41 53 48 00` |

Declaration-ID payloads are:

| ID | Payload after domain and fingerprint version |
|---|---|
| schema | `u8(kind) || str(world) || str(local_name) || u64le(field_count) || field*` |
| schema field | `str(field_name) || u8(primitive_type)` |
| system | `str(world) || str(local_name)` |
| schedule | `str(world) || str(local_name)` |
| query | `id128(parent_system_id) || str(parameter_name)` |

Schema kinds are component `1`, resource `2`, and tag `3`. Primitive types are `i32 = 1`, `f32 = 2`, and `bool = 3`. Schema fields remain in declaration order; names are encoded separately rather than concatenated into an ambiguous qualified string.

Golden vectors fixed by the implementation are:

| Input | Uppercase wire-order result |
|---|---|
| component schema `Demo.Position { x: f32, y: f32 }` | `E6E38FA83F96A32AA6CA26FCD8E29FED` |
| system `Demo.Move` | `30B49813C21A4FE2AC3AB5EC91762525` |
| schedule `Demo.Main` | `A84CD595F7D399E6B08123EF5DAA90F5` |
| query parameter `movers` under the preceding `Demo.Move` ID | `CB0E807161BB02CAB685F0AC9C9BF4DC` |
| system world/name strings `Δ` and `名` | `F47D45318BA812D92B71FC7CA4C51302` |

For a low-level domain-separation vector, let `S` be the schema ID for resource `Demo.Time { delta: f32 }` and let `P = u8(7) || u64le(0x0102030405060708) || id128(S) || str("Move")`. Then `stable128(ARCHE-ABI-HASH\0, P)` is `9271D60459527794206B07D1568A7D94`, while `stable128(ARCHE-BODY-HASH\0, P)` is `45054B4897A991ABA55F35AEBF36CAF1`.

### Canonical ABI and body encoding

ABI and body hashes use `stable128` with their respective domains. Their payload begins with canonical-Core encoding version `u64le(1)`, the eight bytes `01 00 00 00 00 00 00 00`. This version is independent of the outer fingerprint-format `u32le(1)` and must change if any table or ordering rule in this subsection changes.

The ABI payloads are:

| Function | Payload |
|---|---|
| startup | `u64le(1) || u8(1) || u64le(0) || u8(1)`; function kind `1`, zero parameters, result type `i32` |
| system | `u64le(1) || u8(2) || id128(system_id) || u64le(parameter_count) || parameter*` |

System parameters remain in declaration order. Each parameter begins `str(name) || u8(kind)`: kind `1` (read resource) and kind `2` (mutable resource) append `id128(resource_schema_id)`; kind `3` (query) appends `u64le(term_count)` and each source-ordered term as `u8(access) || id128(schema_id)`. Query access codes are read `1`, mutable `2`, and exclusion `3`.

The startup-body root is:

```text
u64le(1) || u8(1) || str(function_name) || u64le(entry_block_id)
|| u64le(local_count) || local*
|| u64le(block_count) || block*
```

Locals are sorted by ascending Core local ID and encode as `u64le(local_id) || str(name) || u8(type)`. Blocks are sorted by ascending Core block ID and encode as `u64le(block_id) || u64le(instruction_count) || instruction* || terminator`. Instructions within a block retain verified-Core order. The type codes are `i32 = 1`, `f32 = 2`, and `bool = 3`.

A literal is `u8(tag) || value`: `i32` tag `1` plus `u64le(zero_extend_u32(two_complement_bits))`, `f32` tag `2` plus `u64le(zero_extend_u32(binary32_bits))`, or `bool` tag `3` plus `bool(value)`. Thus an `i32` or `f32` scalar contributes eight value bytes even though its semantic representation is four bytes.

Startup instruction encodings, including their leading `u8` discriminant, are:

| Tag | Instruction | Bytes after the tag |
|---:|---|---|
| 1 | initialize resource | `id128(resource_schema_id) || u64le(field_count) || field*`; each field is `str(name) || u64le(evaluation_id) || literal` |
| 2 | spawn | `u64le(component_count) || component*`; each component is `id128(schema_id) || u64le(field_count) || field*`, with fields encoded as above |
| 3 | run schedule | `id128(schedule_id)` |
| 4 | `i32` constant | `u64le(result_value_id) || u64le(zero_extend_u32(two_complement_bits))` |
| 5 | `i32` binary | `u64le(result) || u8(binary_op) || u64le(left) || u64le(right)` |
| 6 | `i32` unary | `u64le(result) || u8(unary_op) || u64le(operand)` |
| 7 | `f32` constant | `u64le(result) || u64le(zero_extend_u32(binary32_bits))` |
| 8 | `f32` unary | `u64le(result) || u8(unary_op) || u64le(operand)` |
| 9 | `f32` binary | `u64le(result) || u8(binary_op) || u64le(left) || u64le(right)` |
| 10 | ordered comparison | `u64le(result) || u8(comparison_op) || u64le(left) || u64le(right) || u8(operand_type)` |
| 11 | `bool` constant | `u64le(result) || bool(value)` |
| 12 | `bool` not | `u64le(result) || u64le(operand)` |
| 13 | equality | `u64le(result) || u64le(left) || u64le(right) || u8(operand_type) || bool(negate)` |
| 14 | local store | `u64le(local_id) || u64le(value_id)` |
| 15 | local load | `u64le(result_value_id) || u64le(local_id)` |

Startup binary operation codes are add `1`, subtract `2`, multiply `3`, divide `4`, remainder `5`, shift-left `6`, arithmetic shift-right `7`, bitwise-and `8`, bitwise-xor `9`, and bitwise-or `10`. Unary codes are negate `1` and bitwise-not `2`. Ordered-comparison codes are less `1`, less-or-equal `2`, greater `3`, and greater-or-equal `4`.

Startup terminators are `u8(1) || u64le(exit_value_id)`, `u8(2) || u64le(jump_target_block_id)`, or `u8(3) || u64le(condition_value_id) || u64le(then_block_id) || u64le(else_block_id)`.

The system-body root is:

```text
u64le(1) || u8(2) || u64le(statement_count) || statement*
```

System statements and all recursively nested statement lists retain verified-Core order:

| Tag | Statement | Bytes after the tag |
|---:|---|---|
| 1 | query loop | `str(query_parameter) || u64le(binding_count) || binding* || u64le(body_count) || statement*`; each binding is `str(name) || id128(schema_id) || u8(access)` |
| 2 | expression | `expression` |
| 3 | `let` | `str(name) || u8(type) || bool(mutable) || expression` |
| 4 | assignment | `place || expression` |
| 5 | add-assignment | `place || expression` |
| 6 | lexical block | `u64le(statement_count) || statement*` |
| 7 | `if` | `condition_expression || u64le(then_count) || statement* || u64le(else_count) || statement*` |
| 8 | `while` | `condition_expression || u64le(body_count) || statement*` |

Places are recursively embedded without a length prefix:

| Tag | Place | Bytes after the tag |
|---:|---|---|
| 1 | local | `str(name) || u8(type) || bool(mutable)` |
| 2 | component field | `str(binding) || id128(component_schema_id) || str(field_name)` |
| 3 | resource field | `str(parameter) || id128(resource_schema_id) || str(field_name)` |

Expressions are likewise prefix-discriminated and recursive:

| Tag | Expression | Bytes after the tag |
|---:|---|---|
| 1 | `i32` constant | `u64le(zero_extend_u32(two_complement_bits))` |
| 2 | `f32` constant | `u64le(zero_extend_u32(binary32_bits))` |
| 3 | `bool` constant | `bool(value)` |
| 4 | local | `str(name) || u8(type)` |
| 5 | resource field | `str(parameter) || id128(resource_schema_id) || str(field_name)` |
| 6 | component field | `str(binding) || id128(component_schema_id) || str(field_name)` |
| 7 | boolean not | `operand_expression` |
| 8 | unary | `u8(unary_op) || operand_expression` |
| 9 | binary | `u8(binary_op) || left_expression || right_expression` |

System unary operation codes are `i32` negate `1`, `f32` negate `2`, `i32` bitwise-not `3`, and boolean-not `4`. System binary codes are:

| Codes | Operations |
|---|---|
| 1-10 | `i32` add, subtract, multiply, divide, remainder, shift-left, arithmetic shift-right, bitwise-and, bitwise-xor, bitwise-or |
| 11-14 | `f32` add, subtract, multiply, divide |
| 15-18 | `i32` less, less-or-equal, greater, greater-or-equal |
| 19-22 | `f32` less, less-or-equal, greater, greater-or-equal |
| 23-26 | equality, inequality, logical-and, logical-or |

Schema fields, ABI parameters and terms, startup instruction/component/field vectors, system statements, query bindings, and expression operands are hashed in the order stated above. Source spans, machine offsets, symbol names, and metadata dense indexes are not part of these ABI/body preimages. A system body hash does not repeat the system ID; the function link pairs it with the separately hashed ABI and declaration ID.

## 23.2 Dense IDs

Dense `u64` IDs are assigned after canonical stable-ID sorting during linking:

```text
Demo.Position -> 0
Demo.Velocity -> 1
Demo.Health   -> 2
Demo.Enemy    -> 3
```

Dense IDs are used for:

```text
array indexing
component column lookup
resource store indexing
query plans
hot runtime code
```

## 23.3 Linker role

The linker merges component metadata from object files and assigns dense IDs.

It must detect:

```text
same name, same schema: OK
same name, different schema: error
same stable ID, different declaration: error
unresolved component reference: error
```

Stable 128-bit IDs remain distinct from dense runtime indexes in every format and runtime API.

---

# 24. Arche Core

Arche Core is the canonical semantic representation of a compiled Arche program. Executable Core becomes usable only after verification and branding as `VerifiedExecutableCore`; unverified Core cannot enter runtime assembly, reference execution, metadata linking, or native lowering.

It is not a temporary high-level IR. It is a permanent contract between:

```text
compiler frontend
layout planner
query planner
schedule planner
backend
linker
debugger
profiler
runtime metadata
```

## 24.1 Arche Core goals

Arche Core should:

```text
preserve ECS semantics
represent component layout
represent query descriptors
represent system effects
represent schedules
represent startup code
be printable
be parseable
be verifiable
be suitable for tests
```

## 24.2 Core example

Surface:

```arche
system Move(
    time: read Time,
    q: query[mut Position, Velocity]
) {
    for (pos, vel) in q {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}
```

Core:

```text
system Demo.Move(world: *World, frame: *Frame, commands: *CommandBuffer)
effects {
    read_resource Demo.Time
    write_component Demo.Position
    read_component Demo.Velocity
}
queries {
    q0: query {
        write Demo.Position
        read Demo.Velocity
    }
}
body {
    %time = resource.ptr Demo.Time
    %delta = load.f32 %time + 0

    for_chunks q0 {
        %pos_col = chunk.column Demo.Position
        %vel_col = chunk.column Demo.Velocity
        %len = chunk.len

        for_rows %i in 0..%len {
            %pos = ptr.add %pos_col, mul %i, 8
            %vel = ptr.add %vel_col, mul %i, 8

            %vx = load.f32 %vel + 0
            %vy = load.f32 %vel + 4

            %old_x = load.f32 %pos + 0
            %old_y = load.f32 %pos + 4

            %new_x = fadd %old_x, fmul %vx, %delta
            %new_y = fadd %old_y, fmul %vy, %delta

            store.f32 %pos + 0, %new_x
            store.f32 %pos + 4, %new_y
        }
    }
}
```

## 24.3 Core verifier

The Core verifier checks:

```text
exactly one startup entry and one reachable final exit
typed SSA/values and boolean branch conditions
valid block targets, block terminators, and reachability
definite local initialization on every path
all symbols resolve
all fields exist
all loads/stores have valid types
system effects match actual access
queries do not conflict internally
query bindings preserve term order and zero-sized binding rules
component references do not escape
no post-startup structural mutation appears in the M26 subset
query loops only access declared query terms
resources are accessed with declared mutability
every scheduled resource access is initialized at that source-ordered run
payloads are exhaustive and schema/field references are valid
only the selected M26 feature set appears
```

The M26 Core directly represents startup operations, schedule dispatch, scalar operations, lexical blocks, branches, loops, non-nested query iteration, resource/component reads and writes, traps, and exit. Its independent interpreter is the generic semantic reference. Machine IR is derived only after verification.

---

# 25. Arche Object Format

## 25.0 M26 executable metadata package

M26 executable metadata is one little-endian `ARCHEECS` version 2 package embedded in a read-only ELF load segment. It is not ARCHECMP, ARCHEECS v1, an exact compiler-expected byte blob, or a compatibility envelope. Old artifacts fail before mutation with the rebuild diagnostics fixed in Section 6.4.

The 64-byte header and 64-byte directory rows describe canonical sections for strings, world, schemas, fields, systems, parameters, queries, terms, schedules, schedule items, startup operations, payloads, function links, and source spans. All offsets, lengths, counts, strides, layouts, slice references, dense indexes, and Core IDs are checked `u64`; stable IDs and hashes are raw 16-byte values.

The decoder validates the entire package, cross-references, startup/resource flow, and native links before world mutation. Coherent edits to resource payloads or startup/schedule ordering intentionally change both reference and native behavior. Invalid schema, query, schedule, or function links fail before mutation. This is structural validation, not authenticity.

The `.aco` design below is a later multi-file object interface. It must preserve the v2 authority split when implemented; it is not the M26 executable metadata format.

Arche object files use extension:

```text
.aco
```

An `.aco` file represents one compiled unit.

## 25.1 Object file contents

```text
header
section table
symbols
relocations
machine code
component descriptors
resource descriptors
tag descriptors
event descriptors
system descriptors
query descriptors
schedule descriptors
startup descriptor
debug metadata
source map metadata
profile metadata
```

## 25.2 Suggested sections

```text
.text                  native machine code
.rodata                strings and constants
.data                  initialized writable data
.bss                   zero-initialized data
.arche.components      component descriptors
.arche.resources       resource descriptors
.arche.tags            tag descriptors
.arche.events          event descriptors
.arche.systems         system descriptors
.arche.queries         query descriptors
.arche.schedules       schedule descriptors
.arche.startup         startup descriptors
.arche.debug           source/debug metadata
.arche.profile         profiling metadata
```

## 25.3 Why an Arche object format matters

`.aco` files allow:

```text
multi-file compilation
incremental builds
package linking
component metadata merging
query descriptor linking
schedule linking
source-level debugging
ECS-aware inspection
```

This is a permanent piece of the platform, not a detour.

---

# 26. Arche Executable Format Strategy

## 26.1 First executable format

Initial target:

```text
ELF64 static position-independent executable
ET_DYN
x86-64 Linux
no interpreter, dynamic relocations, or text relocations
```

The compiler or linker must emit:

```text
read-only header PT_LOAD
read-execute text PT_LOAD containing the entrypoint
read-write, non-executable data/BSS/world-state PT_LOAD
read-only ARCHEECS v2 metadata PT_LOAD
read-write, non-executable PT_GNU_STACK
page-aligned offset/vaddr-congruent segments
no writable-and-executable segment
```

Generated code establishes a RIP anchor and reaches metadata, functions, and data using 64-bit image-relative deltas and far-safe indirect calls/jumps. A far conditional branch inverts its condition over a local short skip and then uses the far sequence. No late `rel32` limitation is part of artifact production.

The writer streams and backpatches through `Write + Seek`. Sparse layout tests create holes by seeking, never by allocating or writing a zero-filled multi-gigabyte buffer.

## 26.2 Long-term executable support

Future targets:

```text
ELF64 for Linux/BSD
PE/COFF for Windows
Mach-O for macOS
WASM modules for web/embedded simulation
```

## 26.3 Debug sections

Arche should eventually support both:

```text
native platform debug metadata
Arche-specific ECS debug metadata
```

Arche-specific debug info should allow tools to inspect:

```text
systems
queries
components
entities
resources
schedules
archetype tables
command buffers
```

---

# 27. Compiler Architecture

The compiler should be organized around permanent stages:

```text
SourceManager
Diagnostics
Lexer
Parser
AST
NameResolver
TypeChecker
ECSAccessChecker
LayoutPlanner
CoreBuilder
CoreVerifier
QueryPlanner
SchedulePlanner
Backend
ObjectWriter
```

Suggested repository layout:

```text
compiler/
  basic/
    source
    diagnostics
    strings
    spans

  frontend/
    lexer
    parser
    ast

  sema/
    symbols
    name_resolution
    type_check
    ecs_access

  core/
    core_program
    core_builder
    core_verify
    core_print
    core_parse

  layout/
    type_layout
    component_layout
    resource_layout

  query/
    query_desc
    query_plan
    query_verify

  schedule/
    effects
    schedule_graph
    batch_planner

  backend/
    x86_64
    register_alloc
    frame_layout
    instruction_encode

  object/
    aco_writer
    elf64_writer

  linker/
    archeld
```

The compiler should keep source spans through all stages.

```text
AST node -> span
Core instruction -> span
machine code range -> source span
```

This enables diagnostics, debugging, and profiling.

For M26, every `SourceSpan` stores checked `u64` byte boundaries and the lexer-captured `u64` line and column at both endpoints. The compiler first copies the complete input into a private immutable `SourceSnapshot` spool, retains the original `SourceIdentity`, and parses only the spool. The lexer consumes `BufRead` incrementally, the parser holds two-token lookahead, diagnostics load only bounded snippets, and the spool is cleaned on success and failure. Interned identifiers keep AST/Core memory proportional to semantic content rather than raw source size.

Formatters and metadata encoders accept `Write`; metadata and ELF production accept `Write + Seek`. Values convert to `usize` only where a host allocation or slice requires it and report explicit overflow, address-space, filesystem, or allocation failures. No fixed product size cap substitutes for checked arithmetic.

---

# 28. Frontend

## 28.1 Lexer

The lexer converts source text into tokens.

Token categories:

```text
identifiers
keywords
integer literals
float literals
punctuation
operators
string literals later
comments
EOF
```

Keywords:

```text
world
component
resource
tag
event
relation
system
schedule
startup
run
flush
spawn
despawn
exit
query
read
mut
entity
for
in
if
else
while
let
true
false
```

## 28.2 Parser

The parser produces AST.

Initial declarations:

```text
world
component
resource
tag
system
schedule
startup
```

Initial statements:

```text
let
assignment
compound assignment
if
while
for query
spawn
initialize resource
run schedule
exit
expression statement
```

Initial expressions:

```text
literals
variables
field access
parentheses and unary operations
typed integer, float, and boolean binary operations
comparison and equality
short-circuit boolean logic
struct literal
```

M26 uses one expression parser and type checker for startup and systems. Its precedence is primary, unary, multiplicative, additive, shifts, relational, equality, bitwise `&`, `^`, `|`, logical `&&`, and logical `||`. Function calls remain later work.

## 28.3 AST should not be the final semantic form

The AST mirrors source shape. It should not carry final ECS semantics. That belongs in Arche Core.

---

# 29. Semantic Analysis

Semantic analysis resolves meaning.

## 29.1 Name resolution

Resolve:

```text
component names
resource names
tag names
system names
schedule names
field names
local variables
query variables
```

## 29.2 Type checking

Validate:

```text
field types
literal types
binary operators
assignment compatibility
field access
query variable access
resource mutability
exit expression type
definite initialization of every local on every path
exactly one startup and one reachable final exit
complete component/resource literals
source-ordered resource initialization before schedule use
```

## 29.3 ECS semantic checks

Validate:

```text
query terms refer to components or tags
mut query terms refer to components, not zero-sized tags, unless allowed
resources are accessed as read or mut
systems have valid parameter forms
schedules refer to existing systems
startup refers to existing schedules
duplicates are absent from every declaration and active lexical namespace
tags and other zero-sized query terms bind only `_`
query loops do not nest
```

---

# 30. ECS Access Checking

Every system gets an access set.

```text
read_components
write_components
excluded_components
read_resources
write_resources
events_read
events_written
structural_write
```

Example:

```arche
system Move(
    time: read Time,
    q: query[mut Position, Velocity, !Frozen]
)
```

Access set:

```text
read_resources: Time
write_components: Position
read_components: Velocity
excluded_components: Frozen
structural_write: false
```

Conflict detection:

```text
write(A) conflicts with read(A)
write(A) conflicts with write(A)
read(A) does not conflict with read(A)
```

Invalid:

```arche
system Bad(
    a: query[mut Position],
    b: query[Position]
) {
}
```

Diagnostic:

```text
error[ECS001]: conflicting access to component `Position`

  --> bad.arc:2:14
   |
2  |     a: query[mut Position],
   |              ------------ mutable access here
3  |     b: query[Position]
   |              -------- shared access here

A system cannot read and write the same component through separate queries.
Combine the queries or prove the queries are disjoint.
```

Future disjointness proof:

```arche
exclusive tags Player, Enemy, Projectile
```

Then:

```arche
system Valid(
    players: query[Player, mut Position],
    enemies: query[Enemy, Position]
) {
}
```

can be accepted because `Player` and `Enemy` are mutually exclusive.

---

# 31. Layout Planning

Layout planning computes binary representation.

For every component/resource/event:

```text
field offsets
size
alignment
stride
metadata
```

Initial algorithm:

```text
current_offset = 0
struct_align = 1
for field in fields:
    field_align = alignof(field.type)
    current_offset = align_up(current_offset, field_align)
    field.offset = current_offset
    current_offset += sizeof(field.type)
    struct_align = max(struct_align, field_align)
size = align_up(current_offset, struct_align)
```

Example:

```arche
component Transform {
    x: f32
    y: f32
    enabled: bool
}
```

Layout:

```text
x: offset 0
y: offset 4
enabled: offset 8
size: 12
align: 4
```

The layout planner must be deterministic. M26 performs alignment and size arithmetic as checked `u64`, including zero-sized alignment-one schemas, and converts to `usize` only at an actual host allocation or slice boundary.

---

# 32. Query Planning

Query planning maps query descriptors to runtime iteration plans.

Input:

```text
query[mut Position, Velocity, !Frozen]
```

Descriptor:

```text
required/write: Position
required/read: Velocity
excluded: Frozen
```

Planner determines:

```text
which archetype tables match
which column index holds Position in each table
which column index holds Velocity in each table
whether entity column is needed
```

Entity columns are post-M26. M26 preserves source term order for required bindings, omits exclusion bindings, represents tag/zero-sized membership without payload allocation, and iterates matching tables and rows in canonical order.

A query matches an archetype if:

```text
all required components/tags are present
no excluded components/tags are present
optional components may be present or absent
```

The first query planner can be runtime-based. Long term, common query descriptors should be cached and possibly precomputed after world archetype changes.

---

# 33. Schedule Planning

Schedule planning builds execution order from system effects and explicit schedule syntax.

Source:

```arche
schedule Main {
    run A
    run B
    run C
    flush
    run D
}
```

Planner tasks:

```text
resolve system names
load system access sets
build dependency/conflict graph
preserve explicit ordering where required
insert command flush barriers
create execution batches
```

Example:

```text
A reads Position
B reads Velocity
C writes Position
D writes Health
```

Possible batches:

```text
Batch 0: A, B, D
Batch 1: C
```

M26 executes schedule items sequentially and permits repeated system entries, repeated schedule runs, and arbitrary supported system/query shapes. Parallel batches, flush barriers, and command-buffer scheduling are later work.

---

# 34. Backend Architecture

The backend converts Arche Core into native machine code.

Initial target:

```text
x86-64 Linux
```

Backend phases:

```text
Core lowering
control-flow graph construction
virtual register assignment
instruction selection
frame layout
register allocation
machine instruction emission
relocation generation
object/executable writing
```

Backend constraints:

```text
Do not support every language feature first.
Do not optimize before correctness.
Emit simple but valid machine code.
Keep source span mapping.
Keep ECS hot loops recognizable.
Derive every machine body from VerifiedExecutableCore.
Use metadata links for function selection and dispatch.
```

Initial supported code:

```text
exit constant
integer locals
integer arithmetic
float arithmetic
boolean and bitwise operations
field loads/stores
while loops
if statements
multiple non-nested query loops
system calls into runtime kernel
source-span-aware integer traps
```

The backend must implement the M26 integer wrapping, masked-shift, division/remainder trap, boolean short-circuit, and deterministic `f32` rules exactly. Floating-point operations are not fused; arithmetic NaNs are canonicalized and process entry establishes the required floating-point control state.

---

# 35. x86-64 Backend

## 35.1 Initial instruction subset

Integer/control:

```text
mov
lea
add
sub
imul
cmp
test
jmp
jcc
call
ret
push
pop
syscall
```

Floating-point scalar:

```text
movss
addss
subss
mulss
divss
movsd
addsd
subsd
mulsd
divsd
```

Memory forms:

```text
[base]
[base + disp]
[base + index * scale]
[base + index * scale + disp]
```

This is enough for:

```text
field access
array/column indexing
query row loops
resource pointer access
```

## 35.2 Internal calling convention

For system functions:

```text
rdi = world pointer
rsi = frame pointer
rdx = command buffer pointer
```

For runtime calls, Arche can initially use its own internal convention. Later, FFI to C requires platform ABI support.

## 35.3 Register allocation

Initial allocator:

```text
simple linear scan with bounded stack temporaries
planned world/resource/table storage in writable data or BSS
```

World capacity and metadata reachability are not encoded as `u16` frame plans or signed-32 displacements.

Early correctness matters more than performance.

Long-term allocator:

```text
linear scan with live intervals
graph coloring or region-specialized allocation if needed
SIMD-aware allocation
```

## 35.4 Query loop codegen example

Conceptual x86-64 loop:

```asm
; xmm7 = time.delta
; r8 = pos_col
; r9 = vel_col
; rcx = len
; rax = i

row_loop:
    cmp rax, rcx
    jge row_done

    movss xmm0, [r9 + rax*8 + 0]
    mulss xmm0, xmm7
    addss xmm0, [r8 + rax*8 + 0]
    movss [r8 + rax*8 + 0], xmm0

    movss xmm1, [r9 + rax*8 + 4]
    mulss xmm1, xmm7
    addss xmm1, [r8 + rax*8 + 4]
    movss [r8 + rax*8 + 4], xmm1

    inc rax
    jmp row_loop

row_done:
```

---

# 36. ELF64 Writer

The M26 executable writer emits a segmented x86-64 Linux static PIE.

Required ELF pieces:

```text
ET_DYN ELF header and program headers
R-- header PT_LOAD
R-X text PT_LOAD with entrypoint
RW- data/BSS/world-state PT_LOAD
R-- ARCHEECS v2 metadata PT_LOAD
RW- non-executable PT_GNU_STACK
page alignment and file-offset/virtual-address congruence
no PT_INTERP, dynamic relocations, text relocations, or RWE segment
```

First executable:

```arche
world Main

startup {
    exit 42
}
```

Can emit:

```asm
mov rax, 60
mov rdi, 42
syscall
```

ELF responsibilities:

```text
stream and backpatch through Write + Seek
place the entrypoint in executable text
keep metadata read-only and world storage non-executable
seek across sparse holes rather than writing zero buffers
encode far-safe image-relative native transfers
publish with executable permissions through an atomic sibling temporary
```

The bootstrap may continue emitting complete executables directly. `.aco` remains a later multi-file interface and must not introduce a second semantic or metadata authority.

---

# 37. Arche Linker

`archeld` links Arche object files.

Responsibilities:

```text
resolve symbols
apply relocations
merge metadata sections
validate component schemas
assign dense IDs
build final query descriptors
build final system table
build final schedule table
emit native executable
```

Metadata merge example:

```text
player.aco references physics.Position
enemy.aco references physics.Position
physics.aco defines physics.Position

archeld resolves all references to one final component descriptor.
```

Conflict example:

```text
module A defines game.Position { x:f32, y:f32 }
module B defines game.Position { x:f32, y:f32, z:f32 }
```

Linker diagnostic:

```text
error[LINK_SCHEMA001]: conflicting component schema for `game.Position`
```

The linker should understand ECS metadata, not just symbols.

---

# 38. Startup and Boot

## 38.1 `_start`

The executable begins at `_start`.

Boot sequence:

```text
_start
  establish deterministic floating-point state
  decode and validate all ARCHEECS v2 metadata and links
  initialize allocator
  create world
  construct descriptors, resources, tables, queries, schedules, and function table
  run startup function
  emit and flush ARCHEOBS2 after source exit or semantic trap
  exit
```

Metadata decoding and linking completes before allocator/world mutation. Invalid metadata never produces a complete observation.

## 38.2 Startup block

Source:

```arche
startup {
    resource Time { delta: 0.016 }
    spawn { Position { x: 0.0, y: 0.0 } }
    run Main
    exit 0
}
```

Startup code can perform:

```text
resource initialization
entity spawning
schedule execution
shared scalar expressions, mutable locals, and direct assignment
exit
```

Startup remains straight-line and source ordered; `if`, `while`, and query loops are system-only in M26. Each resource initialization, spawn, and assignment validates/reserves its full operation before commit, so failure publishes no partial current operation while retaining earlier commits.

## 38.3 Exit

Initial `exit` implementation on x86-64 Linux:

```text
rax = 60
rdi = exit code
syscall
```

The source value is `i32`; the process status uses its low eight bits.

## 38.4 Observation and traps

The M26 runtime streams one canonical `ARCHEOBS2` snapshot after a source exit or semantic trap. It includes every resource state, zero-sized/tag membership, empty-archetype entities, zero-length columns, and committed spawn ordinal in stable ID/table/row order. It does not expose allocator capacity.

An integer trap leaves prior commits intact, suppresses the trapping write, flushes the observation, writes the exact source-span diagnostic `arche: trap[<KIND>] <basename>:<line>:<column> bytes <start>..<end>` to stderr, and exits `70`. Other runtime infrastructure failures exit `1` without claiming a complete observation.

---

# 39. Standard Library

The standard library should be small at first.

Initial modules:

```text
std.core
std.math
std.debug
std.platform
```

Initial capabilities:

```text
primitive types
basic math functions
assert
panic/abort later
raw memory helpers for runtime internals
platform exit
platform write for debug output
```

Delay:

```text
strings
collections
filesystem
networking
threads
async
reflection UI
serialization
```

Arche's runtime kernel may use lower-level internal modules that are not exposed as normal user APIs.

---

# 40. Package and Build System

The main tool should be `arche`.

Commands:

```bash
arche new NAME
arche check
arche build
arche run
arche test
arche clean
arche inspect
arche debug
arche profile
```

Build outputs:

```text
target/debug/
target/release/
target/objects/
target/metadata/
```

`Arche.toml`:

```toml
[package]
name = "demo"
version = "0.1.0"

[target]
default = "x86_64-linux"

[build]
opt-level = 0
debug = true
```

The package manager can come later. The project manifest should come earlier because the build system needs stable configuration.

---

# 41. Debugger

Arche needs an ECS-aware debugger.

Proposed tool:

```bash
arche debug target/debug/game
```

Commands:

```text
systems
schedule Main
break system Move
entities
entities with Position Velocity
entity 42
component 42 Position
resource Time
query [Enemy, Position]
watch component Health.hp
step system
next schedule
```

The debugger should understand:

```text
component metadata
entity location table
archetype tables
query descriptors
system descriptors
source spans
```

A generic debugger can inspect machine state, but `archedb` should inspect ECS state.

---

# 42. Profiler

Arche profiling should be ECS-aware.

Metrics:

```text
system execution time
schedule time
query iteration count
matched archetype count
entities iterated
command buffer size
flush time
archetype moves
spawn/despawn count
event count
```

Example output:

```text
Schedule Main: 2.38 ms

Systems:
  Move                  0.18 ms   20,481 entities   3 archetypes
  DetectCollisions      1.72 ms   4,102 entities    6 archetypes
  ApplyDamage           0.04 ms   93 events
  Render                0.39 ms   20,481 entities

Structural changes:
  spawns: 18
  despawns: 7
  component adds: 3
  component removes: 1
  flush time: 0.06 ms
```

Profiler hooks should be designed into system/schedule execution early, even if disabled by default.

---

# 43. Testing Strategy

Testing categories:

## 43.1 Lexer tests

```text
source -> token stream
```

## 43.2 Parser tests

```text
source -> AST dump
```

## 43.3 Semantic tests

```text
source -> accepted/rejected diagnostic
```

## 43.4 Core tests

```text
source -> Arche Core dump
```

## 43.5 Layout tests

```text
component/resource declarations -> size/align/offsets
```

## 43.6 Backend tests

```text
Core -> machine code bytes
Core -> ELF executable
seek-based ELF validation, including segments and >4-GiB offsets
```

## 43.7 Runtime tests

```text
entity allocation
entity generation
spawn
despawn
add component
remove component
resource initialization
query iteration
command flush
```

## 43.8 End-to-end tests

```text
.arc source -> executable -> expected exit code/output/state
decoded ARCHEECS v2 -> direct Core reference and native PIE -> byte-identical ARCHEOBS2/status
```

Example:

```bash
archec tests/e2e/exit_42.arc -o target/test_exit_42
./target/test_exit_42
echo $?
# 42
```

---

# 44. Diagnostics

Diagnostics should be source-level and specific.

Bad:

```text
invalid query
```

Good:

```text
error[ECS001]: conflicting access to component `Position`

  --> examples/bad.arc:12:14
   |
12 |     a: query[mut Position],
   |              ------------ mutable access here
13 |     b: query[Position]
   |              -------- shared access here

A system cannot read and write the same component through separate queries.
```

Diagnostic categories:

```text
LEX     lexical errors
PARSE   syntax errors
NAME    unresolved names
TYPE    type errors
FIELD   invalid field access
ECS     ECS access/query errors
LAYOUT  invalid layout
CORE    invalid Core generation
BACKEND backend/internal errors
LINK    linking/schema errors
RUNTIME runtime validation errors
```

Every diagnostic should include:

```text
error code
message
source location
primary span
secondary spans when useful
suggestion when obvious
```

---

# 45. Toolchain Commands

## 45.1 `archec0` bootstrap interface

```bash
archec0 file.arc
archec0 file.arc --check
archec0 file.arc --emit-ast
archec0 file.arc --inspect-components
archec0 file.arc --emit-core
archec0 file.arc --emit-machine
archec0 file.arc -o app
```

Bare invocation aliases `--check` and emits no file. `--emit-ast` is syntax-only; component inspection is declaration-only; check, Core, Machine, and output modes require executable verification. Success is status `0`, parse/semantic/build failure is `1`, and usage or unsafe output-target failure is `2`.

## 45.2 `arche`

```bash
arche new demo
arche check
arche build
arche run
arche test
arche clean
arche inspect target/debug/demo
```

## 45.3 `arche inspect`

```bash
arche inspect target/debug/demo
```

Output:

```text
Executable: demo
Target: x86_64-linux

Components:
  Demo.Position size=8 align=4
  Demo.Velocity size=8 align=4

Resources:
  Demo.Time size=4 align=4

Systems:
  Demo.Move
    reads resource Demo.Time
    writes component Demo.Position
    reads component Demo.Velocity

Schedules:
  Demo.Main
    Batch 0:
      Demo.Move
```

---

# 46. Language Surface

The M26 syntax is ECS-first and limited to the declarations and operations below. Broader examples elsewhere in this document are future design unless marked M26.

## 46.1 World

```arche
world Demo
```

## 46.2 Components

```arche
component Position {
    x: f32
    y: f32
    visible: bool
}

component Velocity {
    x: f32
    y: f32
}

component Empty {}
```

## 46.3 Tags

```arche
tag Player
tag Enemy
```

## 46.4 Resources

```arche
resource Time {
    delta: f32
}

resource Marker {}
```

## 46.5 Systems

```arche
system Move(
    time: read Time,
    marker: mut Marker,
    movers: query[mut Position, Velocity]
) {
    let enabled: bool = true
    for (pos, vel) in movers {
        if enabled {
            pos.x += vel.x * time.delta
            pos.y += vel.y * time.delta
        }
    }
}
```

Systems may use lexical blocks, direct assignment, `if`/`else`, `while`, and multiple non-nested query loops. Startup uses the same scalar expressions and assignment but remains straight-line. The complete scalar operator and trap rules are in Section 6.2.

## 46.6 Schedules

```arche
schedule Main {
    run Move
    run Move
}
```

M26 schedules are sequential. Command-buffer `flush` and parallel batches are post-M26.

## 46.7 Startup

```arche
startup {
    resource Time { delta: 0.016 }
    resource Marker {}

    spawn {
        Position { x: 0.0, y: 0.0, visible: true }
        Velocity { x: 1.0, y: 0.0 }
    }

    run Main
    exit 0
}
```

---

# 47. Example Programs

## 47.1 Exit code

```arche
world Main

startup {
    exit 42
}
```

## 47.2 Arithmetic

```arche
world Math

startup {
    let x: i32 = 40 + 2
    exit x
}
```

## 47.3 Basic ECS movement

```arche
world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}

schedule Main {
    run Move
}

startup {
    resource Time { delta: 1.0 }

    spawn {
        Position { x: 0.0, y: 0.0 }
        Velocity { x: 2.0, y: 3.0 }
    }

    run Main
    exit 0
}
```

## 47.4 Post-M26 health and despawn direction

This example deliberately uses deferred entity handles, command buffers, despawn, and flush syntax. It is not accepted by the M26 language.

```arche
world Combat

component Health {
    hp: i32
    max: i32
}

tag Enemy

system KillDead(
    cmd: commands,
    q: query[entity, Enemy, Health]
) {
    for (e, _, health) in q {
        if health.hp <= 0 {
            cmd.despawn(e)
        }
    }
}

schedule Main {
    run KillDead
    flush
}
```

---

# 48. Historical Bootstrap Roadmap (Superseded)

This 0.0.x sequence records the original bootstrap sketch. Section 0.7 supersedes it as the implementation roadmap; none of the entries below is a current acceptance gate.

## 48.1 Version 0.0.1 — Native executable seed

Goals:

```text
parse minimal world/startup/exit
emit x86-64 Linux ELF64 executable
support exit constant
```

Acceptance:

```arche
world Main
startup { exit 42 }
```

produces a native executable returning 42.

## 48.2 Version 0.0.2 — Primitive codegen

Goals:

```text
integer literals
integer arithmetic
locals
exit variable
basic if
basic while
```

Acceptance:

```arche
startup {
    let x: i32 = 40 + 2
    exit x
}
```

returns 42.

## 48.3 Version 0.0.3 — Layout and metadata

Goals:

```text
component declarations
resource declarations
field layout
metadata emission
metadata inspection
```

Acceptance:

```bash
archec demo.arc --emit=layout
```

shows component sizes, alignments, and field offsets.

## 48.4 Version 0.0.4 — Runtime kernel skeleton

Goals:

```text
allocator
world creation
entity allocation
archetype table creation
column allocation
spawn entity with components
```

Acceptance:

```arche
startup {
    spawn {
        Position { x: 1.0, y: 2.0 }
    }
    exit 0
}
```

stores one entity in one archetype table.

## 48.5 Version 0.0.5 — Resource support

Goals:

```text
resource descriptors
resource storage
initialize resource
read resource in system
```

## 48.6 Version 0.0.6 — Query support

Goals:

```text
query descriptors
query planning
chunk iteration
component column access
```

Acceptance:

```arche
query[mut Position, Velocity]
```

iterates matching archetype tables.

## 48.7 Version 0.0.7 — Compiled systems

Goals:

```text
system declarations
system ABI
system metadata
compiled system functions
query loops inside systems
```

Acceptance:

`Move` system mutates `Position` using `Velocity` and `Time`.

## 48.8 Version 0.0.8 — Schedules

Goals:

```text
schedule declarations
system lookup
sequential schedule execution
access set calculation
```

## 48.9 Version 0.0.9 — Commands

Goals:

```text
command buffer
cmd.despawn
cmd.spawn
flush
structural mutation barrier
```

## 48.10 Version 0.1.0 — First real Arche

Goals:

```text
native compiler
native runtime kernel
component metadata
resources
tags
systems
queries
schedules
commands
basic diagnostics
basic build command
basic inspect command
```

Arche 0.1.0 should be able to build small ECS simulations as native executables.

---

# 49. Bootstrap and Self-Hosting

Arche needs a seed compiler.

## 49.1 Seed compiler

`archec0` can be written in C, C++, Zig, Rust, or another systems language. Its job is limited:

```text
compile enough Arche to produce archec1
```

It should support:

```text
minimal parser
minimal semantic checks
Arche Core generation
x86-64 backend
ELF64 output
```

## 49.2 Self-hosting stages

```text
Stage 0: archec0 written externally.
Stage 1: archec0 compiles basic Arche runtime pieces.
Stage 2: archec0 compiles archec1 written partially in Arche.
Stage 3: archec1 compiles more of compiler/runtime.
Stage 4: archec2 compiles itself.
Stage 5: external seed becomes only a bootstrap artifact.
```

## 49.3 Features needed for self-hosting

Before full self-hosting, Arche needs:

```text
modules
arrays
strings
file IO
error handling
sum types or tagged unions
maps or hash tables
memory allocation APIs
pattern matching or equivalent control flow
```

Self-hosting should not block the first native ECS executable.

---

# 50. Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---:|---|
| Custom backend takes a long time | High | Start with tiny x86-64 subset and tiny ELF64 writer |
| Runtime kernel grows too complex | High | Begin with plain-data components and simple archetype tables |
| Query lowering becomes unsafe | High | Build Core verifier and strict access checking early |
| Object format design changes often | Medium | Start with explicit sections and versioned headers |
| Debugging compiler bugs is difficult | High | Add `--emit=ast`, `--emit=core`, `--emit=layout`, `--emit=machine` early |
| Component schema linking is tricky | Medium | Use stable IDs plus linker validation from the start |
| Parallelism introduces races | High | Delay parallel execution until access checking is proven |
| Self-hosting distracts from ECS runtime | Medium | Treat self-hosting as later, not required for 0.1 |
| Syntax bikeshedding consumes time | Medium | Keep syntax minimal until execution model works |
| Too many targets too early | High | Only target x86-64 Linux initially |

---

# 51. Historical Open Design Questions

These questions are retained to show the earlier decision frontier. Section 0 resolves or explicitly defers them for M27, M28, and Arche 0.1; they are not open implementation choices for those milestones.

These need future decisions:

1. Should Arche Core be serialized as text, binary, or both?
2. Should `.aco` contain machine code directly, Core, or both?
3. Should optional query terms return nullable references or option-like values?
4. Should Arche support immediate structural mutation outside query loops?
5. How should deterministic scheduling extend beyond M26's sequential order?
6. Should component schema evolution be versioned explicitly?
7. Should packages be globally named or content-addressed?
8. What is the first string model?
9. What error handling model should Arche use?
10. How much of the runtime kernel should be written in Arche once self-hosting begins?
11. Should relations be built into the core runtime or layered as indexed components?
12. Should events be stored as resources, special streams, or command-buffer-like append logs?
13. Should the first debugger attach to running processes or inspect paused snapshots?

---

# 52. Appendix A: M26 Grammar Boundary

This sketch names the M26 surface only. Semantic rules additionally require one world, one startup block, a reachable final startup exit, exhaustive literals, non-nested query loops, and the access/resource-flow invariants described above.

```text
program          := world_decl item*
world_decl       := "world" IDENT

item             := component_decl
                  | resource_decl
                  | tag_decl
                  | system_decl
                  | schedule_decl
                  | startup_decl

component_decl   := "component" IDENT "{" field* "}"
resource_decl    := "resource" IDENT "{" field* "}"
tag_decl         := "tag" IDENT
field            := IDENT ":" type
type             := "i32" | "f32" | "bool"

system_decl      := "system" IDENT "(" param_list? ")" system_block
param_list       := param ("," param)*
param            := IDENT ":" param_type
param_type       := "read" IDENT
                  | "mut" IDENT
                  | "query" "[" query_terms "]"
query_terms      := query_term ("," query_term)*
query_term       := IDENT | "mut" IDENT | "!" IDENT

schedule_decl    := "schedule" IDENT "{" ("run" IDENT)* "}"

startup_decl     := "startup" "{" startup_stmt* exit_stmt "}"
startup_stmt     := let_stmt
                  | assign_stmt
                  | add_assign_stmt
                  | resource_stmt
                  | spawn_stmt
                  | run_stmt
resource_stmt    := "resource" IDENT struct_literal_body
spawn_stmt       := "spawn" "{" component_init* "}"
component_init   := IDENT struct_literal_body
run_stmt         := "run" IDENT
exit_stmt        := "exit" expr

system_block     := "{" system_stmt* "}"
system_stmt      := let_stmt
                  | assign_stmt
                  | add_assign_stmt
                  | if_stmt
                  | while_stmt
                  | query_for_stmt
                  | system_block
if_stmt          := "if" expr system_block ("else" system_block)?
while_stmt       := "while" expr system_block
query_for_stmt   := "for" "(" binding_list? ")" "in" IDENT system_block
binding_list     := binding ("," binding)*
binding          := IDENT | "_"

let_stmt         := "let" "mut"? IDENT ":" type "=" expr
assign_stmt      := place "=" expr
add_assign_stmt  := place "+=" expr
place            := IDENT ("." IDENT)*

struct_literal   := IDENT struct_literal_body
struct_literal_body := "{" field_init_list? "}"
field_init_list  := field_init ("," field_init)*
field_init       := IDENT ":" expr

expr             := logical_or
logical_or       := logical_and ("||" logical_and)*
logical_and      := bitwise_or ("&&" bitwise_or)*
bitwise_or       := bitwise_xor ("|" bitwise_xor)*
bitwise_xor      := bitwise_and ("^" bitwise_and)*
bitwise_and      := equality ("&" equality)*
equality         := relational (("==" | "!=") relational)*
relational       := shift (("<" | "<=" | ">" | ">=") shift)*
shift            := additive (("<<" | ">>") additive)*
additive         := multiplicative (("+" | "-") multiplicative)*
multiplicative   := unary (("*" | "/" | "%") unary)*
unary            := ("-" | "~" | "!") unary | primary
primary          := atom ("." IDENT)*
atom             := INTEGER | FLOAT | "true" | "false"
                  | IDENT | "(" expr ")"
```

---

# 53. Appendix B: Initial Runtime Structs

```c
typedef uint64_t ArcheEntityBits;

typedef struct ArcheEntity {
    uint32_t index;
    uint32_t generation;
} ArcheEntity;

typedef struct ArcheEntityLocation {
    uint32_t generation;
    uint32_t alive;
    uint64_t archetype_index;
    uint64_t row;
} ArcheEntityLocation;

typedef struct ArcheEntityStore {
    ArcheEntityLocation* locations;
    uint64_t len;
    uint64_t cap;

    uint64_t* free_indices;
    uint64_t free_len;
    uint64_t free_cap;
} ArcheEntityStore;

typedef struct ArcheComponentColumn {
    uint64_t dense_id;
    uint64_t size;
    uint64_t align;
    uint64_t stride;
    void* data;
} ArcheComponentColumn;

typedef struct ArchetypeTable {
    uint64_t* component_ids;
    uint64_t component_count;

    ArcheEntity* entities;
    ArcheComponentColumn* columns;

    uint64_t len;
    uint64_t cap;
} ArchetypeTable;

typedef struct ArcheArchetypeStore {
    ArchetypeTable* tables;
    uint64_t len;
    uint64_t cap;
} ArcheArchetypeStore;

typedef struct ArcheResourceSlot {
    uint64_t dense_id;
    void* data;
    uint64_t size;
    uint64_t align;
    uint8_t initialized;
} ArcheResourceSlot;

typedef struct ArcheResourceStore {
    ArcheResourceSlot* slots;
    uint64_t len;
    uint64_t cap;
} ArcheResourceStore;

typedef struct ArcheWorld {
    ArcheEntityStore entities;
    ArcheArchetypeStore archetypes;
    ArcheResourceStore resources;
    ArcheAllocator allocator;
} ArcheWorld;
```

---

# 54. Appendix C: Initial Arche Core Example

Surface source:

```arche
world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}

schedule Main {
    run Move
}
```

Core dump:

```text
world Demo

component Demo.Position stable_id=... size=8 align=4 {
    field x: f32 offset=0
    field y: f32 offset=4
}

component Demo.Velocity stable_id=... size=8 align=4 {
    field x: f32 offset=0
    field y: f32 offset=4
}

resource Demo.Time stable_id=... size=4 align=4 {
    field delta: f32 offset=0
}

query Demo.Move.q0 {
    write Demo.Position
    read Demo.Velocity
}

system Demo.Move(world: *World, frame: *Frame, commands: *CommandBuffer)
effects {
    read_resource Demo.Time
    write_component Demo.Position
    read_component Demo.Velocity
}
body {
    %time = resource.ptr Demo.Time
    %delta = load.f32 %time + 0

    for_chunks Demo.Move.q0 {
        %pos_col = chunk.column Demo.Position
        %vel_col = chunk.column Demo.Velocity
        %len = chunk.len

        for_rows %i in 0..%len {
            %pos = ptr.add %pos_col, mul %i, 8
            %vel = ptr.add %vel_col, mul %i, 8

            %vx = load.f32 %vel + 0
            %vy = load.f32 %vel + 4

            %old_x = load.f32 %pos + 0
            %old_y = load.f32 %pos + 4

            %new_x = fadd %old_x, fmul %vx, %delta
            %new_y = fadd %old_y, fmul %vy, %delta

            store.f32 %pos + 0, %new_x
            store.f32 %pos + 4, %new_y
        }
    }
}

schedule Demo.Main {
    batch {
        run Demo.Move
    }
}
```

---

# 55. Appendix D: Milestone Acceptance Tests

## D.1 Exit constant

Source:

```arche
world Main
startup { exit 42 }
```

Expected:

```bash
./main
echo $?
# 42
```

## D.2 Arithmetic

Source:

```arche
world Main
startup {
    let x: i32 = 40 + 2
    exit x
}
```

Expected exit code:

```text
42
```

## D.3 Component layout

Source:

```arche
world Demo
component Position {
    x: f32
    y: f32
}
startup { exit 0 }
```

Expected layout dump:

```text
Demo.Position size=8 align=4
x offset=0
y offset=4
```

## D.4 Spawn entity

Source:

```arche
world Demo
component Position { x: f32 y: f32 }
startup {
    spawn { Position { x: 1.0, y: 2.0 } }
    exit 0
}
```

Expected runtime state:

```text
world has one archetype table
archetype signature includes Position
row count = 1
Position[0].x = 1.0
Position[0].y = 2.0
```

## D.5 Move system

Source:

```arche
world Demo

component Position { x: f32 y: f32 }
component Velocity { x: f32 y: f32 }
resource Time { delta: f32 }

system Move(time: read Time, q: query[mut Position, Velocity]) {
    for (pos, vel) in q {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}

schedule Main { run Move }

startup {
    resource Time { delta: 1.0 }
    spawn {
        Position { x: 0.0, y: 0.0 }
        Velocity { x: 2.0, y: 3.0 }
    }
    run Main
    exit 0
}
```

Expected runtime state after schedule:

```text
Position[0].x = 2.0
Position[0].y = 3.0
```

## D.6 Conflicting access rejection

Source:

```arche
world Bad
component Position { x: f32 y: f32 }

system BadSystem(
    a: query[mut Position],
    b: query[Position]
) {
}
```

Expected diagnostic:

```text
error[ECS001]: conflicting access to component `Position`
```

---

# Closing Position

Arche should be built from the bottom up around its permanent reality:

```text
native ECS memory
component metadata
compiled systems
query plans
schedule graphs
command buffers
Arche object files
Arche linker
Arche runtime kernel
```

The first usable language can be small. The foundation should not be temporary.

The first true Arche is not a polished package manager or a large standard library. It is this:

```text
A native executable that creates a world, stores entities in archetype tables, runs a compiled ECS system over component columns, and exits successfully.
```

Everything else grows from that.
