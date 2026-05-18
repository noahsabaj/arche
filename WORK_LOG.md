# Arche Work Log

**Status:** Living operational work log  
**Source design constraint:** `arche_comprehensive_design_document.md`  
**Current focus:** M8 resources.

This file is not a second design document. It is the build map for proving that permanent pieces of Arche actually work.

## Repository Workflow

- Git repository initialized on `main`.
- Private remote: `https://github.com/noahsabaj/arche`.
- Generated proof/build artifacts are intentionally ignored via `.gitignore`, including `build/` and `bootstrap/archec0/target/`.
- Repository setup does not advance milestone issues; the current board is tracked below.
- `README.md` provides GitHub orientation for the repository; it does not advance the milestone board.

## Operating Model

The work structure is:

```text
Milestone
  -> Epic
    -> Issue
      -> Acceptance test
        -> Code
```

The design document constrains architecture. The issue board controls execution. Tests prove reality. Code is the source of truth.

Every session should work on one specific proof, not on "the language" in the abstract.

## Core Rule

Every issue must produce at least one of these:

```text
1. A binary that runs
2. A compiler command that works
3. A test that passes
4. A runtime behavior that can be observed
5. A verifier that catches a real invalid program
```

If an issue cannot produce one of those, it is too vague and must be split or rewritten.

## Board

Board columns:

```text
Backlog
Ready
Doing
Done
```

Board rules:

- Keep only one or two issues in `Doing`.
- Promote issues to `Ready` only when their dependencies are done.
- Do not expand the active board beyond the next one or two unblocked proofs.
- Do not use the design document as a top-to-bottom checklist.

### Ready

| Issue | Title | Done when |
|---|---|---|
| M8-001 | Define runtime resource descriptors | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml defines_time_delta_resource_descriptor` proves the runtime can represent `Demo.Time` as a singleton resource descriptor with `delta: f32`. |

### Doing

| Issue | Title | Notes |
|---|---|---|
| - | - | Empty. |

### Done

| Issue | Title | Evidence |
|---|---|---|
| M0-001 | Create monorepo structure | `Test-Path .\bootstrap\archec0`, `.\examples`, `.\tests\e2e`, and `.\tools` all returned `True`. No commit was made because this workspace is not currently a Git repository. |
| M0-002 | Add bootstrap compiler executable archec0 | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- --help` printed usage and exited `0`; `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- --version` printed `archec0 0.0.0` and exited `0`. |
| M0-003 | Add test runner for end-to-end executable tests | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; it ran `archec0 --help`, ran `archec0 --version`, reported `0 e2e tests discovered`, and exited `0`. |
| M0-004 | Add examples/exit42.arc | `Test-Path .\examples\exit42.arc` returned `True`; file content matches the minimal `world Main` / `startup { exit 42 }` source; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` still passed. |
| M0-005 | Add CI or local test script that builds and runs examples | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; it ran `archec0 --help`, ran `archec0 --version`, accepted `.\examples\exit42.arc`, reported `0 e2e tests discovered`, and exited `0`. |
| M1-001 | Implement ELF64 writer | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; it wrote `.\build\exit42` and verified ELF magic, ELF64 class, little-endian encoding, executable type, x86-64 machine, and at least one program header. Missing source with `-o` exited nonzero and did not create output. |
| M1-002 | Emit .text section | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; the generated ELF has an executable/readable load segment, length `121`, payload byte `0x90` at file offset `120`, and `p_filesz`/`p_memsz` covering the payload. |
| M1-003 | Emit _start symbol | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; the ELF entrypoint is `0x400078`, lies inside the executable load segment, and maps to the generated text payload byte `0x90`. |
| M1-004 | Encode x86-64 mov/syscall instructions | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; text bytes at the entrypoint are exactly `48 C7 C0 3C 00 00 00 48 C7 C7 2A 00 00 00 0F 05`, and `p_filesz`/`p_memsz` cover the full instruction sequence. |
| M1-005 | Generate executable that exits with hardcoded 42 | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; it ran `.\build\exit42` through WSL and verified exit code `42`. Direct `wsl /mnt/d/Code/arche/build/exit42` also returned `WSL_LASTEXITCODE=42`. |
| M1-006 | Add e2e test for exit code 42 | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; it discovered `1 e2e tests`, ran `tests\e2e\exit42.ps1`, built `.\build\e2e\exit42`, and verified exit code `42` through WSL. |
| M2-001 | Lexer for identifiers, numbers, braces, keywords | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-tokens` printed the exact token stream `Keyword(world)`, `Identifier(Main)`, `Keyword(startup)`, `LeftBrace`, `Keyword(exit)`, `Integer(42)`, `RightBrace`, `Eof`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the token assertion included. |
| M2-002 | Parser for world declaration | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-ast` printed exactly `Program` then `  world Main`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the AST assertion included. |
| M2-003 | Parser for startup block | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-ast` printed exactly `Program`, `  world Main`, `  startup`, `    statements 1`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the startup block AST assertion included. |
| M2-004 | Parser for exit statement | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-ast` printed exactly `Program`, `  world Main`, `  startup`, `    exit 42`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the exit statement AST assertion included. |
| M2-005 | Lower exit statement to backend instruction sequence | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed; it built `.\examples\exit42.arc` to `.\build\exit42`, verified the ELF `mov rdi` immediate was `42`, and verified WSL exit code `42`; it also built `.\examples\exit7.arc` to `.\build\exit7`, verified the immediate was `7`, and verified WSL exit code `7`. |
| M2-006 | Add source span diagnostics for syntax errors | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\tests\e2e\bad_syntax.arc -o .\build\bad` exits nonzero and now prints `.\tests\e2e\bad_syntax.arc:5:1: error[PARSE001]: expected expression after \`exit\`` after M3-005 widened `exit` to accept identifiers; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passes with the negative diagnostic assertion included. |
| M3-001 | Parse integer literals | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit007.arc --emit-ast` prints `integer 7` from source text `007`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with numeric AST assertions for `exit42` and `exit007`. This proof uses `exit007.arc` because `math.arc` depends on later `let` and binary-expression parsing. |
| M3-002 | Parse let statements | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\let40.arc --emit-ast` prints a `let x: i32` statement with `integer 40` initializer followed by `exit` with `integer 0`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the `let40` AST assertion included. This proof uses `let40.arc` because `math.arc` still depends on binary-expression parsing. |
| M3-003 | Parse binary expressions | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-ast` printed a `binary +` expression with `integer 40` and `integer 2` operands; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the `math` AST assertion included. M3-005 later updated `math.arc` from `exit 0` to `exit x`. |
| M3-004 | Type check i32 arithmetic | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --check` exits `0` with `archec0: check passed .\examples\math.arc`; `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\tests\e2e\bad_i32_arithmetic.arc --check` exits nonzero with `bad_i32_arithmetic.arc:4:12: error[CHECK001]: expected i32 binding for arithmetic expression`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with both checker assertions included. `--check` is semantic-only; executable arithmetic remains later M3 work. |
| M3-005 | Add local variable storage | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-machine` prints local slot `0` for `x`, stores the `40 + 2` result, loads slot `0`, and exits the loaded value; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the exact machine-plan assertion included. This is textual representation only; real executable arithmetic remains M3-006. |
| M3-006 | Emit add/sub/mul instructions | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc -o .\build\math` wrote a valid ELF64 executable whose text payload is exactly `48 83 EC 08 C7 04 24 28 00 00 00 81 04 24 02 00 00 00 8B 3C 24 B8 3C 00 00 00 0F 05`; `wsl /mnt/d/Code/arche/build/math` returned exit code `42`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the math payload and runtime proof included. This issue implemented the addition executable proof; subtraction and multiplication are deferred to M3-007. |
| M3-007 | Add e2e arithmetic tests | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\sub42.arc --emit-ast` prints `binary -`; `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\mul42.arc --emit-ast` prints `binary *`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed, discovered `2 e2e tests`, ran `tests\e2e\arithmetic.ps1`, built `math`, `sub42`, and `mul42` into `build\e2e`, verified their payload bytes, and observed exit code `42` for all three through WSL. M3 primitive computation is complete. |
| M4-001 | Define Core data structures | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml core_represents_math_startup` passed; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with the Core unit proof included before the existing lexer, parser, checker, machine, ELF, WSL, diagnostic, and e2e checks. The new `core` module is data-only and does not add AST lowering, `--emit-core`, printing, verification, or backend integration. |
| M4-002 | Lower AST to Core | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml lowers_math_ast_to_core` passed; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with both Core unit proofs included before the existing lexer, parser, checker, machine, ELF, WSL, diagnostic, and e2e checks. The new lowering module maps the parsed `examples\math.arc` AST into the exact Core startup shape proven by M4-001 without adding `--emit-core`, Core printing, verification, or backend integration. |
| M4-003 | Add Core verifier | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml core_verifier_accepts_lowered_math` passed; `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml core_verifier_rejects_invalid_value_reference` passed; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with the verifier proofs included before existing lexer, parser, checker, machine, ELF, WSL, diagnostic, and e2e checks. The verifier accepts lowered `math.arc` Core and rejects a real invalid Core program with an undefined `ValueId(99)`. No `--emit-core`, Core printing, ECS-specific verification, or backend changes were added. |
| M4-004 | Add --emit-core | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core` printed lowered and verified Core for `examples\math.arc`, including `local x: i32`, `%2 = i32.add %0, %1`, `local.store x, %2`, `%3 = local.load x`, and `exit %3`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with existing Core data, lowering, verifier, lexer, parser, checker, machine, ELF, WSL, diagnostic, and e2e proofs. Exact runner assertions for Core text remain M4-005. |
| M4-005 | Add tests for Core output | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with an exact `examples\math.arc --emit-core` assertion for `world Main`, `function startup`, `local x: i32`, `i32.const`, `i32.add`, `local.store`, `local.load`, and `exit`; `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core` also printed the expected Core text manually. M4 Arche Core is complete. No commit was made because this workspace is not currently a Git repository. |
| M5-001 | Parse component declarations | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --emit-ast` printed exactly `Program`, `world Demo`, `component Position`, fields `x: f32` and `y: f32`, and `startup { exit 0 }`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the `position.arc --emit-ast` assertion included. This was parser-only: no layout, metadata section, component IDs, Core changes, or `--inspect-components` were added. No commit was made because this workspace is not currently a Git repository. |
| M5-002 | Implement primitive type sizes and alignments | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml primitive_type_layouts` passed, proving `i32` and `f32` both have size `4` and alignment `4`, and unknown primitive names return `None`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with the targeted layout proof included before existing parser, Core, executable, diagnostic, and e2e checks. This was layout data only: no field offsets, component sizes, IDs, metadata sections, or `--inspect-components` were added. No commit was made because this workspace is not currently a Git repository. |
| M5-003 | Compute struct field offsets | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml computes_position_field_offsets` passed, proving parsed `Position` fields compute to `x: f32 @ 0` and `y: f32 @ 4`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with the targeted field-offset proof included before existing parser, layout, Core, executable, diagnostic, and e2e checks. This did not compute final component size/alignment, component IDs, metadata sections, or `--inspect-components`. No commit was made because this workspace is not currently a Git repository. |
| M5-004 | Compute component size and alignment | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml computes_position_component_layout` passed, proving parsed `Position` layout includes fields `x: f32 @ 0` and `y: f32 @ 4`, final size `8`, and alignment `4`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with the targeted component-layout proof included before existing parser, layout, Core, executable, diagnostic, and e2e checks. This did not add component IDs, metadata sections, CLI inspection, or runtime behavior. No commit was made because this workspace is not currently a Git repository. |
| M5-005 | Generate stable component IDs | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml stable_component_ids` passed, proving `Demo.Position` qualifies as `Demo.Position`, deterministically hashes to `ComponentId(0x002202c6aeb4f27b)`, stays stable across repeated calls, and differs from `Demo.Velocity`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` also passed with the targeted stable-ID proof included before existing parser, layout, Core, executable, diagnostic, and e2e checks. This did not add metadata sections, CLI inspection, or runtime behavior. No commit was made because this workspace is not currently a Git repository. |
| M5-006 | Emit .arche.components section | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc -o .\build\position` wrote an ELF64 executable with exit text at offset `120` and component metadata at offset `136`; `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml encodes_position_component_metadata` passed; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with a binary payload proof for magic `ARCHECMP`, version `1`, component count `1`, `Demo.Position` ID `0x002202c6aeb4f27b`, size `8`, align `4`, fields `x: f32 @ 0` and `y: f32 @ 4`. This did not add `--inspect-components`, real ELF section headers, runtime behavior, or M6 work. No commit was made because this workspace is not currently a Git repository. |
| M5-007 | Add --inspect-components | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --inspect-components` printed exactly `component Demo.Position`, size `8`, align `4`, and fields `x: f32 @ 0` and `y: f32 @ 4`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the exact inspector assertion included before existing parser, layout, Core, ELF metadata payload, WSL, diagnostic, and e2e checks. M5 component layout and metadata is complete. No commit was made because this workspace is not currently a Git repository. |
| M6-001 | Define ArcheEntity as u64 index/generation | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml arche_entity_packs_index_and_generation` passed, proving `ArcheEntity::new(0x89abcdef, 0x01234567).raw() == 0x0123456789abcdef`, unpacked index `0x89abcdef`, unpacked generation `0x01234567`, and the all-ones boundary `ArcheEntity::new(u32::MAX, u32::MAX).raw() == u64::MAX`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted entity-handle proof included before existing parser, layout, Core, executable, metadata, diagnostic, and e2e checks. This did not add entity allocation, liveness, free-list reuse, world creation, storage, or executable runtime linkage. No commit was made because this workspace is not currently a Git repository. |
| M6-002 | Implement entity table | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml entity_table_allocates_and_reuses_generation` passed, proving an empty `EntityTable` allocates index `0` generation `0`, marks the handle alive, frees it, rejects the stale handle, then reuses index `0` with generation `1`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted entity-table proof included before existing parser, layout, Core, executable, metadata, diagnostic, and e2e checks. This did not add world creation, component descriptors, archetype storage, or executable runtime linkage. No commit was made because this workspace is not currently a Git repository. |
| M6-003 | Implement component descriptor table | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml registers_position_component_descriptor` passed, proving `ComponentDescriptorTable` registers and retrieves the `Demo.Position` descriptor with ID `0x002202c6aeb4f27b`, size `8`, align `4`, fields `x: f32 @ 0` and `y: f32 @ 4`, and rejects duplicate registration without replacing the original descriptor; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted descriptor-table proof included before existing parser, layout, Core, executable, metadata, diagnostic, and e2e checks. This did not add archetype tables, component columns, world creation, parser coupling, binary decoding, or executable runtime linkage. No commit was made because this workspace is not currently a Git repository. |
| M6-004 | Implement archetype table structure | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml creates_archetype_table_for_position` passed, proving `ArchetypeKey` normalizes the `Position` component set by sorting and deduplicating `ComponentId`s, and `ArchetypeTable` stores that key with zero entity rows; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted archetype-table proof included before existing parser, layout, Core, executable, metadata, diagnostic, and e2e checks. This did not add component columns, row insertion, world creation, spawning, parser coupling, or executable runtime linkage. No commit was made because this workspace is not currently a Git repository. |
| M6-005 | Implement component column allocation | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml allocates_position_component_column` passed, proving `ArchetypeTable` can allocate a real aligned `Position` component column with component ID `0x002202c6aeb4f27b`, element size `8`, alignment `4`, capacity `1`, row count `0`, storage byte size `8`, and an aligned storage pointer; duplicate column allocation returns `false` without replacing the original column; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted component-column proof included before existing parser, layout, Core, executable, metadata, diagnostic, and e2e checks. This did not add row insertion, component writes, spawning, world creation, parser coupling, or executable runtime linkage. No commit was made because this workspace is not currently a Git repository. |
| M6-006 | Implement world_create/world_destroy | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml world_create_destroy_smoke` passed, proving `ArcheWorld::create()` builds an empty world root containing an `EntityTable`, `ComponentDescriptorTable`, and archetype storage root, and `destroy(self)` consumes the world for explicit teardown; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted world create/destroy proof included before existing parser, layout, Core, executable, metadata, diagnostic, and e2e checks. This did not link runtime into generated executables or add spawning, systems, parser coupling, or runtime behavior beyond root ownership. No commit was made because this workspace is not currently a Git repository. |
| M6-007 | Link runtime kernel into generated executable | `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed, proving generated `exit42`, `exit7`, `math`, `sub42`, and `mul42` ELF executables now run through a native runtime create/startup/destroy wrapper before the Linux `exit` syscall while still returning their expected WSL exit codes; byte assertions verify the exact runtime prefix `48 83 EC 18 31 C0 48 89 04 24 48 89 44 24 08 48 89 44 24 10` and destroy suffix around startup code, and `position.arc` metadata is still parsed after the longer wrapped text payload. This did not add spawning, component writes, systems, scheduler behavior, parser changes, heap allocation, or Rust object-file linking. M6 runtime kernel skeleton is complete. No commit was made because this workspace is not currently a Git repository. |
| M7-001 | Parse spawn blocks | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\spawn_position.arc --emit-ast` printed the exact spawn-block AST with `spawn`, `component Position`, and `fields unparsed`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the `spawn_position.arc --emit-ast` assertion included. This was parser/AST output only: no component field parsing, float literals, Core lowering, runtime insertion, or executable generation was added. No commit was made because this workspace is not currently a Git repository. |
| M7-002 | Parse component literals | `cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\spawn_position.arc --emit-ast` printed the exact parsed component-literal AST for `Position { x: 1.0, y: 2.0 }`, including `field x` / `float 1.0` and `field y` / `float 2.0`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the updated `spawn_position.arc --emit-ast` assertion included. This remained parser/AST output only: no Core lowering, runtime insertion, payload copying, executable generation, or world inspection was added. No commit was made because this workspace is not currently a Git repository. |
| M7-003 | Lower spawn to Core | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml lowers_spawn_position_to_core` passed, proving parsed `spawn_position.arc` lowers into Core with a `Spawn` instruction carrying `Demo.Position`, component ID `0x002202c6aeb4f27b`, `x` as `f32` bits `0x3f800000`, `y` as `f32` bits `0x40000000`, then `i32.const 0` and `exit %0`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted Core-lowering proof included. This was Core-only: no runtime archetype lookup, row insertion, payload copying, executable spawn behavior, or `--emit-core` CLI support for spawn was added. No commit was made because this workspace is not currently a Git repository. |
| M7-004 | Implement runtime archetype lookup/create | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml world_gets_or_creates_position_archetype` passed, proving `ArcheWorld` can look up an archetype by canonical `ArchetypeKey`, create the `Demo.Position` archetype table when absent, and reuse that same table when called again with duplicate component IDs normalized to the same key; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted runtime lookup/create proof included. This was runtime-only: no entity row insertion, component payload copying, spawn execution, parser/Core changes, generated ELF behavior, or descriptor auto-registration was added. No commit was made because this workspace is not currently a Git repository. |
| M7-005 | Insert entity into archetype table | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml inserts_entity_into_position_archetype` passed, proving `ArcheWorld` can allocate an `ArcheEntity`, get or create the `Demo.Position` archetype table, insert that entity as row `0`, read the row back, and keep the entity alive in the world entity table; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted row-insertion proof included. This was runtime-only: no component payload bytes, component column row writes, source-level spawn execution, parser/Core changes, or generated ELF behavior were added. Implementation commit: `586d149`. |
| M7-006 | Copy component payload into column | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml copies_position_payload_into_column` passed, proving the runtime can copy exact little-endian `Position { x: 1.0, y: 2.0 }` payload bytes into row `0` of the `Demo.Position` column, read them back, advance the column payload row count to `1`, preserve the inserted entity row, and keep the entity alive in the world entity table; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted payload-copy proof included. This was runtime-only: no parser/Core/source-level spawn execution, generated ELF behavior, or M8 work was added. Implementation commit: `4436203`. |
| M7-007 | Add runtime debug inspection for world state | `cargo test --manifest-path .\bootstrap\archec0\Cargo.toml debug_inspects_spawned_position_world` passed, proving runtime inspection reports a world with `1` entity, `1` archetype, row `0` entity index `0` generation `0`, component `Demo.Position`, and decoded fields `x: f32 = 1.0` and `y: f32 = 2.0`; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1` passed with the targeted debug-inspection proof included. This was runtime-only: no source-level spawn execution, generated ELF behavior, CLI inspection command, M8 resource storage, or broader debug tooling was added. M7 spawn entities is complete. Implementation commit: `9957c15`. |

### Backlog

Dependency ordered:

```text
M8-002 Allocate resource storage
M8-003 Store Time.delta payload
M8-004 Retrieve Time.delta payload
M8-005 Add runtime resource inspection
M8-006 Add resource source fixture
```

## Milestones

### M0: Repository and Executable Test Harness

Purpose:

```text
Create the permanent engineering loop.
```

Proof target:

```bash
./tools/test
```

can build something and report pass/fail.

### M1: Native Executable Emission

Purpose:

```text
Prove Arche can produce a standalone native binary.
```

First target:

```text
x86-64 Linux
ELF64
no libc
_start entrypoint
exit syscall
```

Proof target:

```bash
archec0 examples/exit42.arc -o exit42
./exit42
echo $?
# 42
```

At this stage, `archec0` may ignore the source and emit a hardcoded binary. The proof is that the tool can emit a runnable native executable.

### M2: Minimal Source Language to Native Code

Purpose:

```text
Stop hardcoding the executable and compile a tiny source program.
```

Proof target:

```arche
world Main

startup {
    exit 42
}
```

```bash
archec0 examples/exit42.arc -o exit42
./exit42
echo $?
# 42
```

Now the `42` must come from the source file.

### M3: Primitive Computation

Purpose:

```text
Prove the compiler can handle basic program logic needed by ECS systems.
```

Proof target:

```arche
world Main

startup {
    let x: i32 = 40 + 2
    exit x
}
```

```bash
archec0 examples/math.arc -o math
./math
echo $?
# 42
```

### M4: Arche Core

Purpose:

```text
Create the permanent internal representation that future Arche code lowers into.
```

Proof target:

```bash
archec0 examples/math.arc --emit-core
```

prints Core similar to:

```text
world Main

startup {
    %0 = i32.const 40
    %1 = i32.const 2
    %2 = i32.add %0, %1
    exit %2
}
```

### M5: Component Layout and Metadata

Purpose:

```text
Make ECS types real at the binary level.
```

Proof target:

```bash
archec0 examples/position.arc --inspect-components
```

prints:

```text
component Demo.Position
  size: 8
  align: 4
  fields:
    x: f32 @ 0
    y: f32 @ 4
```

### M6: Runtime Kernel Skeleton

Purpose:

```text
Create the ECS kernel that native Arche programs will use.
```

Proof target:

```text
An Arche executable can create and destroy an Arche world without crashing.
```

### M7: Spawn Entities

Purpose:

```text
Make entity/component storage work.
```

Proof target:

```text
world has 1 entity
entity has Position
Position.x == 1.0
Position.y == 2.0
```

### M8: Resources

Purpose:

```text
Support singleton world data.
```

Proof target:

```text
The runtime can store and retrieve Time.delta.
```

### M9: Systems

Purpose:

```text
Compile named ECS behavior.
```

Proof target:

```text
startup { run Main }
```

actually calls a compiled system function.

### M10: First Query Loop

Purpose:

```text
Deliver the core promise of Arche.
```

Proof target:

```text
Position.x: 0.0 -> 2.0
Position.y: 0.0 -> 3.0
```

after a compiled `Move` system scans `Position` and `Velocity` component columns using `Time.delta`.

M10 is the first true Arche milestone. Everything before it exists to make this possible.

## Detailed Issues M0-M8

### M0 Epic: Permanent Engineering Loop

#### M0-001: Create monorepo structure

Acceptance test:

```powershell
Test-Path .\bootstrap\archec0
Test-Path .\examples
Test-Path .\tests\e2e
Test-Path .\tools
```

Done when all expected root folders exist and no language implementation has been started outside that structure.

#### M0-002: Add bootstrap compiler executable archec0

Acceptance test:

```bash
archec0 --help
```

Done when the bootstrap compiler executable can be invoked and prints basic usage or command help.

#### M0-003: Add test runner for end-to-end executable tests

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the test runner executes, discovers e2e tests, and reports pass/fail.

#### M0-004: Add examples/exit42.arc

Acceptance test:

```powershell
Test-Path .\examples\exit42.arc
Get-Content .\examples\exit42.arc
```

Done when the example file exists and contains:

```arche
world Main

startup {
    exit 42
}
```

#### M0-005: Add CI or local test script that builds and runs examples

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when one command builds the current proof target and reports success or failure.

### M1 Epic: Native Binary Emission

#### M1-001: Implement ELF64 writer

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the output is recognized as an ELF64 executable for x86-64 Linux.

#### M1-002: Emit .text section

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the executable contains a `.text` section or equivalent executable load region containing code bytes.

#### M1-003: Emit _start symbol

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when `_start` exists as the executable entry symbol or the ELF entrypoint points to the generated start code.

#### M1-004: Encode x86-64 mov/syscall instructions

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when disassembly shows the generated exit path loads syscall `60`, loads exit code `42`, and executes `syscall`.

#### M1-005: Generate executable that exits with hardcoded 42

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the emitted binary exits with status code `42`. The source file may still be ignored in this issue.

#### M1-006: Add e2e test for exit code 42

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the test runner builds `examples/exit42.arc`, runs the emitted binary, and fails if the exit code is not `42`.

### M2 Epic: Source-Driven Exit

#### M2-001: Lexer for identifiers, numbers, braces, keywords

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-tokens
```

Done when token output is exactly:

```text
Keyword(world)
Identifier(Main)
Keyword(startup)
LeftBrace
Keyword(exit)
Integer(42)
RightBrace
Eof
```

#### M2-002: Parser for world declaration

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-ast
```

Done when AST output is exactly:

```text
Program
  world Main
```

#### M2-003: Parser for startup block

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-ast
```

Done when AST output is exactly:

```text
Program
  world Main
  startup
    statements 1
```

#### M2-004: Parser for exit statement

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc --emit-ast
```

Done when AST output is exactly:

```text
Program
  world Main
  startup
    exit 42
```

#### M2-005: Lower exit statement to backend instruction sequence

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when `examples/exit42.arc` emits a binary that exits `42`, `examples/exit7.arc` emits a binary that exits `7`, and the ELF byte checks prove the `mov rdi` immediate comes from the source.

#### M2-006: Add source span diagnostics for syntax errors

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\tests\e2e\bad_syntax.arc -o .\build\bad
```

Done when invalid syntax exits nonzero and prints:

```text
.\tests\e2e\bad_syntax.arc:5:1: error[PARSE001]: expected expression after `exit`
```

### M3 Epic: Primitive Computation

#### M3-001: Parse integer literals

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit007.arc --emit-ast
```

Done when AST output is exactly:

```text
Program
  world Main
  startup
    exit
      integer 7
```

This issue uses `exit007.arc` because `examples/math.arc` requires `let` and binary-expression parsing from M3-002 and M3-003.

#### M3-002: Parse let statements

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\let40.arc --emit-ast
```

Done when AST output is exactly:

```text
Program
  world Main
  startup
    let x: i32
      integer 40
    exit
      integer 0
```

This issue uses `let40.arc` because `examples/math.arc` requires binary-expression parsing from M3-003.

#### M3-003: Parse binary expressions

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-ast
```

Done when AST output is exactly:

```text
Program
  world Main
  startup
    let x: i32
      binary +
        integer 40
        integer 2
    exit
      identifier x
```

M3-005 changed `math.arc` to `exit x`; this issue's parser proof remains covered by the current `math.arc --emit-ast` assertion with an identifier exit.

#### M3-004: Type check i32 arithmetic

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --check
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\tests\e2e\bad_i32_arithmetic.arc --check
```

Done when valid `i32` arithmetic exits `0` and the invalid fixture exits nonzero with:

```text
.\tests\e2e\bad_i32_arithmetic.arc:4:12: error[CHECK001]: expected i32 binding for arithmetic expression
```

`--check` is semantic-only; it does not evaluate expressions or emit executable arithmetic.

#### M3-005: Add local variable storage

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-machine
```

Done when emitted machine output is exactly:

```text
function startup
  local x: i32 slot 0
  %0 = i32.const 40
  %1 = i32.const 2
  %2 = i32.add %0, %1
  store slot 0, %2
  %3 = load slot 0
  exit %3
```

This is a textual representation proof only, not real executable arithmetic.

#### M3-006: Emit add/sub/mul instructions

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc -o .\build\math
wsl /mnt/d/Code/arche/build/math
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when `build\math` is a valid ELF64 executable, its text payload is exactly:

```text
48 83 EC 08 C7 04 24 28 00 00 00 81 04 24 02 00 00 00 8B 3C 24 B8 3C 00 00 00 0F 05
```

and WSL observes exit code `42`. M3-006 narrows the issue title to the currently supported `+` syntax; subtraction and multiplication coverage are deferred to M3-007.

#### M3-007: Add e2e arithmetic tests

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the test runner covers at least one addition case, one subtraction case, one multiplication case, and one local-variable exit case.

### M4 Epic: Arche Core

M4 is complete. Arche Core now has data structures, AST lowering, verification, `--emit-core`, and runner-backed stable Core output.

#### M4-001: Define Core data structures

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml core_represents_math_startup
```

Done when permanent Core data structures can represent the current primitive-computation program shape:

```text
world Main
startup:
  i32.const 40
  i32.const 2
  i32.add
  local/store/load or equivalent Core value flow
  exit
```

This issue should define Core data structures only. AST-to-Core lowering and `--emit-core` belong to later M4 issues.

#### M4-002: Lower AST to Core

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml lowers_math_ast_to_core
```

Done when parsing the current `examples/math.arc` AST and lowering it produces the same Core startup shape proven by M4-001:

```text
world Main
startup:
  %0 = i32.const 40
  %1 = i32.const 2
  %2 = i32.add %0, %1
  store local x, %2
  %3 = load local x
  exit %3
```

This issue should add AST-to-Core lowering only. `--emit-core`, Core printing, and the Core verifier remain later M4 work.

#### M4-003: Add Core verifier

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml core_verifier_rejects_invalid_value_reference
```

Done when a Core verifier accepts valid Core and catches at least one real invalid Core program.

#### M4-004: Add --emit-core

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core
```

Done when `--emit-core` prints the lowered Core for `examples/math.arc`.

#### M4-005: Add tests for Core output

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when the local runner asserts stable Core output for at least `examples/math.arc`.

### M5 Epic: Component Layout and Metadata

M5 is complete. Component declarations can be parsed, laid out, assigned stable IDs, emitted into the binary metadata payload, and inspected from source.

#### M5-001: Parse component declarations

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --emit-ast
```

Done when the parser recognizes:

```arche
component Position {
    x: f32
    y: f32
}
```

and AST output includes a `component Position` node with fields `x: f32` and `y: f32`. This is parser-only; no layout, metadata section, or inspector is added yet.

#### M5-002: Implement primitive type sizes and alignments

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml primitive_type_layouts
```

Done when primitive layout data reports at least `f32` as size `4`, align `4`, and preserves existing `i32` behavior where needed.

#### M5-003: Compute struct field offsets

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml computes_position_field_offsets
```

Done when `Position { x: f32, y: f32 }` computes field offsets `x @ 0` and `y @ 4`.

#### M5-004: Compute component size and alignment

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml computes_position_component_layout
```

Done when `Position` computes component size `8` and alignment `4`.

#### M5-005: Generate stable component IDs

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml stable_component_ids
```

Done when the same world-qualified component name deterministically produces the same component ID across repeated runs.

#### M5-006: Emit .arche.components section

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when an emitted ELF contains an `.arche.components` section or equivalent metadata payload for declared components.

#### M5-007: Add --inspect-components

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\position.arc --inspect-components
```

Done when output is exactly:

```text
component Demo.Position
  size: 8
  align: 4
  fields:
    x: f32 @ 0
    y: f32 @ 4
```

### M6 Epic: Runtime Kernel Skeleton

M6 is complete. The runtime kernel skeleton now has entity handles, entity allocation, component descriptors, archetype tables, component columns, world create/destroy, and a generated native startup/shutdown wrapper.

#### M6-001: Define ArcheEntity as u64 index/generation

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml arche_entity_packs_index_and_generation
```

Done when the runtime has a `u64` entity handle representation with explicit index and generation packing/unpacking tests.

#### M6-002: Implement entity table

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml entity_table_allocates_and_reuses_generation
```

Done when the runtime can allocate and free entity handles while preserving generation-based stale-handle detection.

#### M6-003: Implement component descriptor table

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml registers_position_component_descriptor
```

Done when the runtime can store a descriptor for `Demo.Position` containing its stable ID, size, alignment, and field metadata.

#### M6-004: Implement archetype table structure

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml creates_archetype_table_for_position
```

Done when the runtime can create an archetype table keyed by a component set containing `Position`.

#### M6-005: Implement component column allocation

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml allocates_position_component_column
```

Done when an archetype table can allocate a component column sized and aligned for `Position`.

#### M6-006: Implement world_create/world_destroy

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml world_create_destroy_smoke
```

Done when the runtime can create and destroy a world containing entity, component descriptor, and archetype storage roots.

#### M6-007: Link runtime kernel into generated executable

Acceptance test:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
```

Done when a generated Arche executable creates and destroys an Arche world during startup/shutdown without crashing.

### M7 Epic: Spawn Entities

M7 is complete. M8 is expanded only through the controlled resource backlog below; do not expand M9-M10 before resource storage is proven.

#### M7-001: Parse spawn blocks

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\spawn_position.arc --emit-ast
```

Done when the parser recognizes a startup `spawn { ... }` block and reports one unparsed component literal, without parsing component fields yet.

This issue was completed before M7-002 advanced the same fixture from an unparsed component shell to parsed component literal fields.

#### M7-002: Parse component literals

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\spawn_position.arc --emit-ast
```

Done when AST output includes the parsed `Position { x: 1.0, y: 2.0 }` component literal fields.

#### M7-003: Lower spawn to Core

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml lowers_spawn_position_to_core
```

Done when Core contains a spawn operation for the parsed `Position` payload.

#### M7-004: Implement runtime archetype lookup/create

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml world_gets_or_creates_position_archetype
```

Done when the world runtime can find or create the `Position` archetype table from a component set.

#### M7-005: Insert entity into archetype table

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml inserts_entity_into_position_archetype
```

Done when an allocated entity row is inserted into the `Position` archetype table.

#### M7-006: Copy component payload into column

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml copies_position_payload_into_column
```

Done when `Position` payload bytes for `x = 1.0` and `y = 2.0` are copied into the `Position` column.

#### M7-007: Add runtime debug inspection for world state

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml debug_inspects_spawned_position_world
```

Done when runtime inspection proves the world has `1` entity, the entity has `Position`, `Position.x == 1.0`, and `Position.y == 2.0`.

### M8 Epic: Resources

Only M8-001 through M8-006 are expanded now. Do not expand M9-M10 into a large active board before singleton resource storage is proven.

#### M8-001: Define runtime resource descriptors

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml defines_time_delta_resource_descriptor
```

Done when the runtime can represent `Demo.Time` as a singleton resource descriptor with `delta: f32` layout metadata.

#### M8-002: Allocate resource storage

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml allocates_time_delta_resource_storage
```

Done when the runtime allocates aligned storage for one `Demo.Time` resource payload.

#### M8-003: Store Time.delta payload

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml stores_time_delta_resource_payload
```

Done when the runtime stores exact little-endian `f32` bytes for `Time.delta`.

#### M8-004: Retrieve Time.delta payload

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml retrieves_time_delta_resource_payload
```

Done when the runtime retrieves and decodes `Time.delta` as an observed `f32` value.

#### M8-005: Add runtime resource inspection

Acceptance test:

```powershell
cargo test --manifest-path .\bootstrap\archec0\Cargo.toml debug_inspects_time_delta_resource
```

Done when runtime debug inspection reports the stored `Demo.Time.delta` value.

#### M8-006: Add resource source fixture

Acceptance test:

```powershell
cargo run --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\time_delta.arc --emit-ast
```

Done when a source fixture represents `Time { delta: 1.0 }` without adding system execution or query behavior.

## Daily Workflow

Each work session:

```text
1. Pick one issue from Ready.
2. Read the relevant section of arche_comprehensive_design_document.md.
3. Write or update the acceptance test first.
4. Implement the smallest amount of code needed.
5. Run the test.
6. Commit.
7. Move the issue to Done with evidence.
8. Promote the next unblocked issue to Ready.
```

Examples of valid session goals:

```text
Today I am making ELF section headers work.
Today I am making exit 42 come from parsed source.
Today I am making component field offsets inspectable.
Today I am making one entity spawn into an archetype table.
Today I am making one query scan one component column.
```

Never spend a session just "working on the language."

## Planning Confidence

| Subproblem | Recommendation | Confidence |
|---|---|---:|
| Project control method | Milestone-driven issue board | 94/100 |
| First technical target | Native executable that exits 42 | 92/100 |
| Design document usage | Architecture constraint, not task list | 95/100 |
| Early issue granularity | Small testable implementation tasks | 93/100 |
| Avoiding wasted work | Build permanent substrate first | 88/100 |
| Overall approach | Executable proofs over abstract planning | 92/100 |

Weighted confidence: 92/100.

## Meta Check

Subproblem confidence:

| Subproblem | Confidence |
|---|---:|
| M7-007 stayed runtime debug inspection only | 99/100 |
| `debug_inspects_spawned_position_world` proves one entity, `Demo.Position`, and decoded `x = 1.0`, `y = 2.0` | 99/100 |
| Existing M0-M7 parser, runtime unit, layout, Core, executable, binary metadata, diagnostic, and e2e proofs remain passing | 98/100 |
| Board state reflects M7 complete and controlled M8 progress | 99/100 |
| Active inventory is limited to M8-001 ready plus M8-002 through M8-006 backlog | 97/100 |

Weighted confidence: 98/100.

Verification pass:

- The active board has only `M8-001` in `Ready`.
- `Doing` is empty.
- `Done` contains completed M0, completed M1, completed M2, completed M3, completed M4, completed M5, completed M6, and completed M7.
- Detailed active inventory includes M8-001 through M8-006 only.
- Later milestones remain proof targets only.
- M7 spawn entities is complete; singleton resource descriptor work starts with M8-001.
