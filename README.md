# Arche

Arche is an ECS-native software platform bootstrap. The current repository is focused on proving the permanent native execution substrate: a bootstrap compiler, a tiny ELF64 backend, Arche Core, ECS metadata, and the first runtime kernel pieces.

This is not a broad language implementation yet. Work advances through executable proofs tracked in `WORK_LOG.md`, which also keeps the current north star and integration debt visible.

## Current Status

The proof chain currently demonstrates:

- A bootstrap compiler executable, `archec0`.
- Linux x86-64 ELF64 executable emission with no libc.
- Identity-safe, sibling-temporary executable publication that refuses source/output aliases and never exposes a partially written artifact.
- Source-driven native exits, including `exit 42` and `exit 7`.
- Primitive `i32` arithmetic compiled into native code for addition, subtraction, and multiplication fixtures.
- Stable parser and AST output for the current minimal source forms.
- Arche Core data structures, AST-to-Core lowering, Core verification, and `--emit-core` output.
- Component declarations, primitive layouts, field offsets, component size/alignment, stable component IDs, and binary component metadata payloads.
- Runtime kernel skeleton pieces: entity handles, entity table, component descriptor table, archetype table, component columns, world create/destroy, and native startup/shutdown wrapper bytes.
- Spawn source parsing and spawn lowering into Core.
- Runtime archetype lookup/create for a normalized `Position` component set.
- Runtime insertion of an allocated entity row into the `Position` archetype table.
- Runtime copying and readback of `Position { x: 1.0, y: 2.0 }` payload bytes into the `Position` component column.
- Runtime debug inspection of a spawned-position world state.
- Runtime resource descriptors for singleton world data, starting with `Demo.Time`.
- Runtime aligned storage allocation for singleton resource payloads.
- Runtime storage of exact little-endian `Time.delta` payload bytes.
- Runtime retrieval and decoding of `Time.delta` as `1.0f32`.
- Runtime debug inspection of stored singleton resource state.
- Source-level parsing for a `Demo.Time` resource fixture with `Time { delta: 1.0 }`.
- Source-level parsing for named system declarations, starting with `system Move() {}`.
- Source-level parsing for system read-resource parameters, starting with `time: read Time`.
- Source-level parsing for system query parameters, starting with `movers: query[mut Position, Velocity]`.
- Core metadata lowering for system declarations and parameters.
- Runtime system descriptor registration for `Demo.Move` with deterministic resource/query metadata.
- Source-level parsing for non-executed system body field references, including `time.delta`, `Position`, and `Velocity` fields.
- Runtime query descriptor metadata for `Demo.Move.movers`.
- Runtime query descriptor matching against archetype component sets.
- Runtime query plan construction for matching `Position`/`Velocity` archetypes.
- Runtime query row iteration over matching archetypes.
- Runtime access to decoded `Time.delta` during query iteration.
- Runtime `Move` application over query rows, updating `Position` from `Velocity * Time.delta`.
- Source-level parsing for schedule declarations, starting with `schedule Main { run Move }`.
- Core metadata lowering for schedule declarations.
- Runtime schedule descriptor registration for `Demo.Main`.
- Runtime sequential schedule plan construction for `Demo.Main`.
- Runtime schedule plan execution for `Demo.Main`, invoking the bootstrap `Move` path.
- Source-level parsing for startup `run Main`.
- ECS semantic checking, starting with rejection of unknown schedule run targets.
- ECS semantic checking for unknown system resource parameters.
- ECS semantic checking for unknown query components.
- ECS semantic checking for conflicting query access.
- A non-executing runtime program assembly model for descriptor roots and startup operations.
- Source assembly for component and resource descriptors.
- Source assembly for system, query, and schedule descriptors.
- Source assembly for startup resource payload operations.
- Source assembly for startup spawn operations.
- Source assembly for startup run operations.
- Source-assembled descriptor registration into `ArcheWorld`.
- Source-assembled startup resource payload execution into `ArcheWorld`.
- Source-assembled startup spawn execution into `ArcheWorld`.
- Source-assembled startup run schedule execution into `ArcheWorld`.
- Source-driven runtime execution of the full `move_system.arc` vertical slice, updating `Position` through `Time`, `Velocity`, and `Move`.
- A deterministic `ARCHEECS` binary envelope for complete ECS metadata sections.
- Component and resource descriptor records encoded into the `ARCHEECS` metadata envelope.
- System, query, and schedule descriptor records encoded into the `ARCHEECS` metadata envelope.
- Startup-operation records encoded into the `ARCHEECS` metadata envelope.
- Generated native metadata-carrier binaries for `move_system.arc` with decoded full `ARCHEECS` payloads.
- Native startup validation for embedded `ARCHEECS` metadata envelopes, including a corrupted-metadata failure proof.
- Native startup descriptor-count registration from embedded `ARCHEECS` metadata.
- Native startup resource payload application from embedded `ARCHEECS` metadata.
- Native startup spawn-row application from embedded `ARCHEECS` metadata.
- Final observable native startup proof for the source-described `Demo.Time` and one `Demo.Position + Demo.Velocity` row.
- Verified Core system-body representation and query-loop lowering.
- Core lowering for query-loop skeletons, starting with `for (pos, vel) in movers`.
- Core lowering for query-loop field expressions and `f32` multiplication, starting with `vel * time.delta`.
- Core lowering for add-assign/update statements inside query loops, starting with `pos.x += vel.x * time.delta`.
- Core text emission for the lowered `Demo.Move` query loop in `move_system.arc --emit-core`.
- Generated-native query-loop execution for a supported verified-Core-derived shape.
- Capacity- and live-row-guarded native row scans across every matching planned archetype.
- Descriptor/Core-derived field loading, `f32` multiplication, and mutable component stores.
- Descriptor-identity schedule dispatch into the supported compiled system shape.
- A named native ECS execution-state layout for descriptor counts, startup state, query scan state, and compiled-system temporaries.
- Native descriptor record state materialization for component/resource/system/query/schedule section offsets and byte lengths.
- Native startup operation dispatch for source-order resource, spawn, and run-schedule operation kinds.
- Native query-planning state bound by stable component identity rather than declaration ordinal.
- Native compiled schedule state derived from verified schedule/system descriptors.
- Native component/resource descriptor-table decoding from embedded `ARCHEECS` records.
- Native system/query/schedule descriptor-table decoding from embedded `ARCHEECS` records.
- Native startup operation table materialization from embedded `ARCHEECS` records.
- Native query-plan construction from decoded descriptor records.
- Generated-native ECS execution through decoded descriptor, startup, schedule, and query-plan tables.
- A reusable native ECS table model that names the current descriptor, startup, compiled schedule, and query-plan rows without changing generated native bytes.
- Native descriptor name-reference decoding into table state, with generated startup validating exact descriptor name bytes from embedded `ARCHEECS`.
- Generic source-order startup operation table iteration for native resource, spawn, and run-schedule handlers.
- Native query-plan construction from reusable table rows and component-ID subset matching.
- Native compiled schedule execution through reusable descriptor-derived table rows.
- A native table iteration cursor model over current descriptor, startup operation, compiled schedule, and query-plan rows without changing generated native bytes.
- Count-driven native descriptor table row iteration for component, resource, system, query, and schedule descriptor records.
- Count-driven native startup operation table row iteration for resource, spawn, and run-schedule handlers.
- Native query-plan construction through iterated query-plan table rows.
- Canonical descriptor-generic `NativeWorldStoragePlan` values with multiple archetypes, descriptor-sized SoA columns, checked alignment, and compile-time geometric capacities.
- Typed `i32`/`f32` startup payload materialization for arbitrary component lists, with whole-spawn validation before row publication.
- Query subset matching and binding by stable component ID, including legal extra archetype columns and capacity/live-row guarded scans.
- One neutral `VerifiedCoreExecutionShape` shared by reference and native paths for both `move_system_two_rows.arc` and structurally unrelated `arena_recovery.arc`.
- Independent test-only `ARCHEOBS1` serialization of live post-schedule state, proving byte-identical reference/native results, memberships, row counts, and capacities.

M25 descriptor-generic world execution is complete. Demo and Arena execute because of verified Core and descriptors, not fixture names, hard-coded fixture IDs, declaration order, physical offsets, capacities, or expected row counts. Required native-Linux and Windows proof gates cover the two-program result. Physical payload offsets enter semantic execution only during catalog construction.

## What This Is Not Yet

Arche is not yet a complete language, package manager, editor integration, debugger, profiler, production runtime, or general-purpose compiler. The current implementation is intentionally narrow and proof-driven.

Arche still uses statically planned stack storage and metadata v1. The temporary execution adapter accepts one startup schedule, one system, one read resource, one two-term mut/read query, one loop, and exactly two `f32` multiply-add lanes with a direct source `exit 0`; M26 owns arbitrary supported Core bodies and sequential schedules. Runtime structural mutation, entity lifecycle, and command buffers remain M27 work.

## Repository Layout

```text
bootstrap/archec0/   Rust bootstrap compiler crate
examples/            Minimal Arche source fixtures
tests/e2e/           End-to-end executable proof scripts
tools/               Local proof runner
WORK_LOG.md          Living operational issue board and evidence log
arche_comprehensive_design_document.md
                     Source design constraint
```

Generated files live under `build/` and Rust build output lives under `bootstrap/archec0/target/`; both are ignored.

## Requirements

- Rust 1.95.0 and Cargo. The checked-in `rust-toolchain.toml` selects the exact verified toolchain; `rust-version = "1.95.0"` records the package requirement, not a historically tested lower MSRV.
- PowerShell Core 7.6.3 (`pwsh`) is the preferred and verified proof shell. Windows PowerShell 5.1 remains supported.
- WSL for running generated Linux ELF64 executables

## Run The Proof Suite

From the repository root:

```powershell
pwsh -NoLogo -NoProfile -File .\tools\test.ps1
```

The runner executes the complete Cargo test inventory once with `--locked --all-targets`, checks the CLI/Core/runtime proofs, emits ELF64 binaries, validates byte-level payloads, runs generated executables through WSL, and runs discovered e2e scripts under the same PowerShell host. The legacy `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` command remains supported on Windows PowerShell 5.1.

## Useful Commands

```powershell
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- --help
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-ast
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\time_delta.arc --emit-ast
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\move_system.arc --emit-ast
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\move_system.arc --emit-core
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --inspect-components
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc -o .\build\exit42
```

Executable output is assembled in memory, written to a unique sibling temporary file, synced, made executable on native Unix, and renamed into place. Existing exact-path, relative, symlink, and hard-link aliases of the input source are rejected. The repeated checks prevent ordinary alias mistakes; they do not claim to defeat a malicious process racing namespace replacement during publication.

## Execution Model

Arche development is controlled by executable proofs, not by a top-to-bottom design checklist. Each issue must produce a runnable binary, a working compiler command, a passing test, observable runtime behavior, or a verifier that catches a real invalid program.

`WORK_LOG.md` is the operational source of truth for current board state, done evidence, and the next proof target.
