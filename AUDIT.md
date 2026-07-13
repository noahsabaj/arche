# Codebase Audit

Scope: `bootstrap/archec0`, source fixtures, and end-to-end test harnesses. Generated `build/` output is out of scope.

Severity definitions: **Critical** permits arbitrary code execution, data loss, or a total safety boundary failure; **High** reliably produces incorrect output or unsafe behavior for ordinary valid inputs; **Medium** causes a material correctness, robustness, or maintainability failure in a supported workflow; **Low** is a bounded defect or material test/diagnostic gap.

Each finding records verification status. No unverified findings are included.

## Axis 1 — Build, test, and delivery integrity

### Low

- **A1-L1 — Strict linting is not clean.** **Verified.** `cargo clippy --manifest-path .\\bootstrap\\archec0\\Cargo.toml -- -D warnings` fails on four unused native-storage constants, two eight-argument functions, one complex type, and one `filter_map(bool::then)` construct. Normal compilation and the project test runner continue despite the unused-constant warnings, so this is not a release blocker today, but it prevents adopting warnings-as-errors and obscures new diagnostics.

The functional checks are otherwise clean: `cargo test` passed **85/85** tests and `tools/test.ps1` passed the complete compiler, metadata-corruption, ELF/WSL execution, and discovered e2e suites.

## Axis 2 — Compiler correctness and input validation

### Medium

- **A2-M1 — The documented `--check` mode rejects supported ECS startup programs.** **Verified.** Running `cargo run --manifest-path .\\bootstrap\\archec0\\Cargo.toml -- .\\examples\\move_system.arc --check` exits 1 with `resource checking is not implemented yet`, while `--emit-core` and `-o` accept the same fixture and the full suite executes its output. The command dispatches `--check` to `checker::check_program`, whose `Statement::Resource`, `Spawn`, and `Run` arms unconditionally error; the output path instead uses the narrower `check_ecs_declarations`. This makes the advertised static-check command unusable for the codebase's main ECS vertical slice.

- **A2-M2 — Duplicate ECS declarations are compiled into a binary that deterministically fails at native startup instead of being rejected.** **Verified with a temporary fixture.** A `move_system` variant with a second `component Position` passed the `-o` compilation path and produced an ELF executable; it then exited **17** under WSL. `checker::check_ecs_declarations` validates only schedule targets, resource parameters, query components, and conflicting query access. The runtime assembly emits both component descriptors with the same stable ID, while the fixture-specific native decoder expects the canonical two-component descriptor layout. Add declaration/field/parameter uniqueness validation before assembly and have all compiler modes invoke the same semantic validator.

## Axis 3 — Native code generation and executable safety

Clean for the declared bounded `Demo` fixture. The complete proof suite verified ELF layout and WSL execution for arithmetic, one-row, and two-row programs, and exercised all 18 metadata-corruption cases. The native decoder also range-checks variable-length metadata it parses. No additional finding in this axis.

## Axis 4 — Runtime memory safety and ECS data integrity

### High

- **A4-H1 — Raw resource and component storage can expose uninitialized allocation as `&[u8]`.** **Verified by code trace; dynamic Miri verification unavailable (the installed toolchains have no Miri component).** `ResourceStorage::allocate` uses `std::alloc::alloc` without initialization, and `payload_bytes` immediately forms a byte slice across that allocation. A caller can reach this through the safe sequence `ArcheWorld::allocate_resource_storage` then `ArcheWorld::resource_payload`, before `store_resource_payload`. Component columns have the same issue: `ComponentColumn::allocate` uses `alloc`, while `row_count` is only a maximum written index, so writing row 1 first causes `row_bytes(0)` to return an uninitialized slice. Creating/reading these shared byte slices violates the unsafe contract and can leak allocator contents or produce undefined behavior. Allocate zeroed storage, or track initialization and refuse reads until a full payload/row is initialized.

### Medium

- **A4-M1 — Repeated source-runtime spawns into the same archetype fail and leave the world partially mutated.** **Verified by code trace against the existing two-spawn fixture.** `execute_startup_spawn_operation` sets `row_capacity` to `entity_count + 1`, but `ArchetypeTable::allocate_component_column` returns `Ok(false)` for an existing column instead of growing it. On the second `spawn` in `examples/move_system_two_rows.arc`, the existing columns retain capacity 1; the function then allocates and inserts a new entity before `copy_component_payload` rejects row 1 as out of capacity. `execute_runtime_program_assembly` propagates the error but does not roll back that entity/table row. The native two-row generator passes because it uses separate bounded stack-storage code, so the full proof suite does not exercise this Rust runtime path. Implement capacity growth before insertion and make spawn transactional (or reserve/validate every column, then commit the entity).

## Axis 5 — Metadata encoding and cross-layer compatibility

Clean for the current metadata format and fixed native decoder contract. The suite checks the encoded envelope and descriptor records host-side, then verifies generated code rejects mutations to every descriptor/startup-operation category. No additional finding in this axis.

## Axis 6 — Architecture, maintainability, and regression resilience

No additional finding beyond **A1-L1**, **R1**, and **R2**. The code generator is intentionally fixture-shaped and that limitation is explicitly documented in `README.md` and `WORK_LOG.md`; it is not reported as a separate defect.

## Root-cause consolidation

### R1 — Split and incomplete semantic validation

`main.rs` routes `--check` through `check_program` but routes ECS output through `check_ecs_declarations`; neither path establishes complete program invariants. A unified semantic phase that validates every supported startup statement and namespace before runtime assembly would clear **A2-M1** and **A2-M2**.

### R2 — Raw storage has no initialization/transaction model

The runtime allocates raw memory and exposes it as initialized bytes, while archetype insertion commits the entity before column capacity and payload writes are known to succeed. Establishing explicit storage initialization plus reserve/commit-or-rollback insertion semantics would clear **A4-H1** and **A4-M1**.
