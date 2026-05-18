# Arche

Arche is an ECS-native software platform bootstrap. The current repository is focused on proving the permanent native execution substrate: a bootstrap compiler, a tiny ELF64 backend, Arche Core, ECS metadata, and the first runtime kernel pieces.

This is not a broad language implementation yet. Work advances through executable proofs tracked in `WORK_LOG.md`.

## Current Status

The proof chain currently demonstrates:

- A bootstrap compiler executable, `archec0`.
- Linux x86-64 ELF64 executable emission with no libc.
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

M8 resources are complete. The active board is currently on M9 systems. The next proof is parsing system query parameters.

## What This Is Not Yet

Arche is not yet a complete language, package manager, editor integration, debugger, profiler, production runtime, or general-purpose compiler. The current implementation is intentionally narrow and proof-driven.

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

- Rust and Cargo
- Windows PowerShell
- WSL for running generated Linux ELF64 executables

## Run The Proof Suite

From the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

The runner builds `archec0`, checks parser/Core/runtime proofs, emits ELF64 binaries, validates byte-level payloads, runs generated executables through WSL, and runs discovered e2e scripts.

## Useful Commands

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- --help
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-ast
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\time_delta.arc --emit-ast
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\move_system.arc --emit-ast
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --inspect-components
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc -o .\build\exit42
```

## Execution Model

Arche development is controlled by executable proofs, not by a top-to-bottom design checklist. Each issue must produce a runnable binary, a working compiler command, a passing test, observable runtime behavior, or a verifier that catches a real invalid program.

`WORK_LOG.md` is the operational source of truth for current board state, done evidence, and the next proof target.
