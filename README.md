# Arche

Arche is an ECS-native software platform bootstrap. This repository is proving the permanent native execution substrate: the `archec0` bootstrap compiler, verified Arche Core, metadata-authoritative world execution, a direct reference executor, and an x86-64 Linux native backend.

This is not yet a broad language implementation. Work advances through executable proofs. [`WORK_LOG.md`](WORK_LOG.md) is the source of truth for implementation evidence, current milestone state, and any acceptance gates that remain open.

## M26 Execution Contract

M26 replaced the earlier fixture-shaped bridge with one generic execution contract. It closed after the reference and native paths agreed on the required structurally different programs, both required greater-than-4-GiB jobs passed, strict cross-platform CI was green at the exact implementation and merged commits, and the evidence was recorded in `WORK_LOG.md`.

The host decoder, verified-Core reference path, v2 linker, metadata-authoritative native storage and dispatch, segmented PIE writer, and canonical observation/trap paths are exercised together. Local Windows, WSL, PowerShell, readelf, physical-source, and sparse-executable proofs are green for the primary fixture, structurally different Arena fixture, and trap fixture. Strict branch protection requires the Linux, Windows, physical-source, and sparse-executable proofs; all four passed on the exact merged M26 implementation commit. The durable run and job identities are recorded in `WORK_LOG.md`.

### Compiler modes

- `archec0 SOURCE` is an alias for `archec0 SOURCE --check`. Both validate a buildable executable and emit no executable.
- `--check`, `--emit-core`, `--emit-machine`, and `-o` use the same executable checker. It requires exactly one startup block, a reachable final `exit i32`, and every invariant needed by Core verification, metadata encoding, and AOT lowering.
- `--emit-ast` is syntax-only.
- `--inspect-components` checks declarations and layouts without requiring startup.
- Compiler status is `0` for success, `1` for parse, semantic, or build failure, and `2` for usage errors or unsafe output targets.

`VerifiedExecutableCore` is the only executable-semantic authority. It can be constructed only by the executable verifier. Machine IR is an AOT lowering detail derived from verified Core, never an independent checker or runtime plan.

`--emit-ast` prints source structure and intentionally contains no derived stable IDs. `--emit-core` and `--emit-machine` print the canonical 32-uppercase-hex schema, system, query, and schedule IDs alongside their separate checked `u64` Core indexes; the Machine view also prints the ABI and Core-body link hashes used by AOT dispatch.

### M26 language boundary

The supported stored and local scalar types are deliberately small:

| Type | Size | Alignment | Representation |
|---|---:|---:|---|
| `i32` | 4 | 4 | little-endian two's-complement bits |
| `f32` | 4 | 4 | IEEE 754 binary32 bits |
| `bool` | 1 | 1 | only `0` or `1` |

Components and resources may have zero fields. `tag Name` declares a zero-sized, alignment-one schema whose v2 schema flags are exactly `TAG = 0x1`; components and resources have flags `0`. Tags, empty components, and other zero-sized schemas still participate in archetype signatures. M26 supports `{}` literals, initialized and uninitialized zero-sized resources, `Enemy {}` attachment, and `spawn {}` for an entity in the empty archetype.

Component and resource literals name every declared field exactly once. Unknown, duplicate, and omitted fields are errors. Fields may be written in any order: expressions evaluate in source order, while payload bytes use declaration order.

Startup and systems share one typed scalar-expression implementation:

- Mutable `let` locals, direct assignment, nested parentheses, and typed field initialization. Systems additionally use component and resource field reads and writes.
- `i32`: `+`, `-`, `*`, `/`, `%`, unary `-`, `&`, `|`, `^`, `~`, `<<`, `>>`, comparisons, and equality.
- `f32`: `+`, `-`, `*`, `/`, unary `-`, comparisons, and equality.
- `bool`: `true`, `false`, `!`, `&&`, `||`, and equality.
- No implicit numeric or boolean conversions. `&&` and `||` short-circuit.
- Precedence, from high to low, is primary; unary; multiplicative; additive; shifts; relational; equality; bitwise `&`, `^`, `|`; logical `&&`, `||`.

`i32` addition, subtraction, multiplication, unary negation, left shift, and bitwise operations wrap in two's complement. Right shift is arithmetic and shift counts use the low five RHS bits. Integer division or remainder by zero, and `i32::MIN / -1` or `i32::MIN % -1`, trap. `-2147483648` is a valid unary-negative literal; other integer literals must fit after unary interpretation.

At process entry the floating-point environment is round-to-nearest-even with exceptions masked and FTZ/DAZ disabled. Operations are not fused. Subnormals and signed zero are preserved. Every arithmetic NaN result is canonicalized to `0x7FC00000`. Comparisons with NaN are false except `!=`, which is true.

Startup remains straight-line and source ordered. It supports the shared scalar expressions and mutable-local assignments, including `+=`, plus `resource Name { ... }` initialization, spawn, schedule runs, and the final exit, but not `if` or `while`. Systems support lexical blocks, locals, assignment, `if`/`else`, `while`, and multiple query loops. Query loops may appear inside control flow but may not nest. Systems retain `+=` for mutable numeric locals, resource fields, and query component fields; no other compound assignment is part of M26.

System parameters support `read Resource`, `mut Resource`, and `query[...]`. Duplicate read-only resource aliases are permitted; any alias set containing mutable resource access is rejected conservatively. Query terms are required reads `T`, mutable terms `mut T`, and exclusions `!T`. Source term order determines bindings, exclusions do not bind, and a query containing only exclusions uses an empty binding list, `for ()`. A required zero-sized term can bind only `_`. `mut Tag`, including and excluding the same schema, mutable alias conflicts, nested query loops, entity bindings, optional terms, and change detection are invalid or deferred.

Before each startup schedule run, executable checking proves that every resource used by every dispatched system has already been initialized. The metadata decoder repeats this validation before mutating the world. Source `exit` returns the low eight bits of its `i32` value.

### Verified Core and execution authority

Verified Core represents startup resource initialization, spawn, schedule dispatch, blocks, branches, loops, query iteration, scalar operations, component and resource access, and exit. Verification proves typed values and conditions, valid CFG targets and terminators, definite local initialization on every path, exhaustive payloads, valid schema and field references, query alias and binding rules, resource-initialization order, exactly one startup entry, and one reachable final exit.

The direct reference executor interprets verified Core independently of native lowering. Both reference and native execution consume a package decoded and linked through the same ARCHEECS v2 contract:

- Metadata is authoritative for schemas, descriptors, startup operations, payloads, schedule order, query terms and bindings, function selection, and dispatch.
- AOT machine code is authoritative for compiled system-body instructions.
- ABI and Core-body hashes link metadata function records to the selected native bodies.

Production behavior must not depend on fixture names, declaration ordinals, fixed descriptor or term counts, fixed arithmetic lanes, whole-blob equality, source-derived host plans, or compiler-side recognition of an execution shape.

### Stable IDs and ARCHEECS v2

Schemas use 128-bit `SchemaId` values. Systems, queries, and schedules use domain-separated 128-bit declaration IDs. Native links use separate 128-bit ABI and verified-Core-body hashes. BLAKE3 is pinned to release `1.8.5` with its pure-Rust implementation.

Every stable-ID or link-hash preimage begins with its exact ASCII domain, including the trailing NUL, followed by fingerprint version `1` as a four-byte little-endian `u32`. The domains are `ARCHE-SCHEMA-ID\0`, `ARCHE-SYSTEM-ID\0`, `ARCHE-SCHEDULE-ID\0`, `ARCHE-QUERY-ID\0`, `ARCHE-ABI-HASH\0`, and `ARCHE-BODY-HASH\0`. Strings are a little-endian `u64` UTF-8 byte length followed by the bytes; integers are little-endian; embedded IDs are their raw 16 bytes.

Schema payloads contain the kind byte (`1` component, `2` resource, `3` tag), world and local-name strings, and ordered fields with primitive tags (`1` `i32`, `2` `f32`, `3` `bool`). System and schedule payloads contain world and local-name strings; query payloads contain the raw parent-system ID and parameter-name string. ABI and body payloads additionally begin with canonical-Core encoding version `1` as a little-endian `u64`; their complete startup/system discriminants and recursive encodings are normative in [Section 23.1 of the design document](arche_comprehensive_design_document.md#231-stable-ids). The first 16 BLAKE3 digest bytes are stored verbatim and displayed as 32 uppercase hexadecimal digits. Dense runtime indexes are separate checked `u64` values assigned after canonical ID sorting.

Executable metadata has one hard-cut little-endian format: `ARCHEECS` version 2. There is no production ARCHECMP or v1 compatibility path. Both legacy forms exit `1` before mutation with their exact rebuild diagnostics:

```text
arche: unsupported ARCHEECS version 1; rebuild with archec0
arche: unsupported ARCHECMP artifact; rebuild with archec0
```

The 64-byte header describes total length and a directory of 64-byte rows. Canonical sections hold strings, the world, schemas, fields, systems, parameters, queries, terms, schedules, schedule items, startup operations, payloads, function links, and source spans. IDs are raw 16-byte values; variable bytes live in string or payload sections. Offsets, counts, sizes, lengths, strides, slice references, layouts, and Core IDs are checked `u64`.

The decoder validates the complete directory and all cross-references before world initialization: checked arithmetic, bounds, alignment, overlap, record stride, UTF-8, canonical order, required and unknown sections, unique IDs, dense indexes, layouts and payloads, query access, schedule targets, startup/resource flow, source spans, function offsets, ABI hashes, and body hashes. ARCHEECS v2 provides structural validation, not signing or authenticity.

### Runtime observation and traps

After a source-directed exit or a semantic runtime trap, a successful runtime observation is one canonical uppercase ASCII `ARCHEOBS2` stream on stdout:

```text
ARCHEOBS2
RESOURCE <ID32> UNINITIALIZED
RESOURCE <ID32> INITIALIZED <LEN> <HEX-or-->
TABLE <KEY_COUNT> <IDS...> <ROW_COUNT>
ROW <ROW_INDEX> <SPAWN_ORDINAL> <COLUMN_COUNT>
COLUMN <ID32> <LEN> <HEX-or-->
END
```

Resources sort by schema ID. Tables sort lexicographically by their sorted schema-ID keys. Rows retain committed spawn order and columns follow key order. The stream includes uninitialized resources, initialized empty payloads, tags and other zero-sized membership, empty-archetype rows, and zero-length columns; `-` represents an empty payload. Capacity is not observable semantic state.

On an integer trap, all earlier committed effects remain and the trapping write is not applied. The runtime emits and flushes the complete snapshot, writes this form to stderr, and exits `70`:

```text
arche: trap[<KIND>] <basename>:<line>:<column> bytes <start>..<end>
```

Metadata, link, allocation, or observation-I/O failures exit `1` and do not claim a complete observation. A source `exit 70` has no trap diagnostic, so it remains distinguishable.

### Source, artifact, and publication boundaries

Source positions and all semantic/layout identifiers use checked `u64`. `SourceSpan` retains byte boundaries plus lexer-captured line and column endpoints. Compilation first copies the complete input to a private temporary `SourceSnapshot`; parsing uses only that immutable spool through an incremental buffered lexer and two-token-lookahead parser. Diagnostics retain the original source identity and read only bounded snippets. The spool is removed on every outcome.

AST and Core storage are proportional to semantic content and intern identifiers rather than copying source text. Encoders and formatters write to `Write`; metadata and ELF production use `Write + Seek`. Conversion to `usize` occurs only at actual host allocation or slice boundaries, with explicit overflow, address-space, filesystem, or allocation failures. The product imposes no fixed size cap.

The native target is an x86-64 Linux static PIE: `ET_DYN`, no interpreter, no dynamic relocations, and no text relocations. It has separate read-only headers, read-execute text, read-write non-executable data/BSS/world state, and read-only metadata `PT_LOAD` segments, plus a read-write non-executable `PT_GNU_STACK`. No segment is writable and executable. Page alignment and file-offset/virtual-address congruence are preserved.

Native code establishes a RIP anchor and uses 64-bit image-relative deltas for metadata, functions, and data. Calls and jumps use far-safe indirect sequences; conditional transfers invert over a local short skip before the far transfer. ELF output is streamed and backpatched, and sparse holes are created by seeking.

Executable publication uses a synchronized unique sibling temporary, preserves executable permissions, rechecks source/output alias safety, and atomically replaces the destination. The guarantee is atomic visibility only; it does not promise parent-directory synchronization or power-loss durability.

## Deliberate M26 limits

M26 emits x86-64 Linux executables; Windows is a compiler and test host. It does not add native Windows output, other architectures, dynamic linking, artifact signing, sandboxing, entity handles, system-time spawn/despawn, add/remove, command buffers, events, relations, optional query terms, change detection, nested query loops, parallel schedules, or parent-directory crash durability.

The greater-than-4-GiB source promise covers a semantically ordinary program with very large input bytes, not billions of declarations under a fixed memory ceiling. Host address-space, filesystem, allocation, and kernel limits remain explicit checked errors.

## Repository Layout

```text
bootstrap/archec0/   Rust bootstrap compiler crate
examples/            Minimal Arche source fixtures
tests/e2e/           End-to-end executable proof scripts
tools/               Local proof runner
WORK_LOG.md          Operational milestone state and evidence
arche_comprehensive_design_document.md
                     Durable design and M26 contract
```

Generated files live under `build/` and Rust build output lives under `bootstrap/archec0/target/`; both are ignored.

## Requirements

- Rust 1.95.0 and Cargo. The checked-in `rust-toolchain.toml` selects the exact toolchain.
- PowerShell Core 7.6.3 (`pwsh`) is the preferred proof shell. Windows PowerShell 5.1 remains supported.
- WSL or native Linux for executing generated Linux ELF64 artifacts on a Windows compiler host.

## Run the proof suite

From the repository root:

```powershell
pwsh -NoLogo -NoProfile -File .\tools\test.ps1
```

The local runner exercises the locked all-target Cargo inventory, CLI/Core/runtime proofs, artifact validation, native execution where the host permits it, and discovered end-to-end scripts. Hosted large-file gates and exact-head CI remain separate acceptance evidence; a green local run is not a claim that those external gates passed.

## Useful commands

```powershell
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- --help
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --check
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-ast
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\move_system.arc --emit-machine
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --inspect-components
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc -o .\build\exit42
```

Arche development is controlled by executable proofs, not by a top-to-bottom design checklist. Each issue must produce a runnable binary, a working compiler command, a passing test, observable runtime behavior, or a verifier that catches a real invalid program.
