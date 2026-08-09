# Arche

Arche is an independent, native, general-purpose ECS language and software platform. ECS concepts—worlds, entities, components, resources, systems, queries, schedules, and structural commands—are part of the language and execution model rather than a library layered over another language.

Arche is for the full range of work that benefits from ECS: games, simulations, authoritative servers, tools, and deterministic environment workloads. Machine learning is an important application frontier, especially for running many reproducible simulation worlds, but it is one use of Arche rather than the project’s identity.

## Road to 0.1

- **M26 — closed:** established the generic verified-Core, metadata-authoritative reference/native execution substrate; ARCHEECS v2 and ARCHEOBS2; static x86-64 Linux PIE output; strict cross-platform CI; and the required greater-than-4-GiB source and artifact proofs.
- **M27 — platform foundation:** builds the general language, ownership and effect systems, reentrant ECS runtime, package/object formats, public `arche` toolchain, standard library, and source-package registry. M27 is one umbrella milestone executed through mandatory gates M27-A through M27-L. M27-A, M27-B, and M27-C0 are closed; M27-C is active in C1, the first of six implementation/closure slices consuming the frozen C0 language/Core contract.
- **M28 — Arche 0.1 release:** proves the completed platform with two materially different applications: an authoritative multiplayer arena server and a deterministic 1,024-world Grid Pursuit environment with a language-neutral trainer protocol.

After 0.1, game-platform and native-ML capabilities advance as equal application tracks over the same compiler and runtime. Reproducible self-hosting is a later objective before 1.0; the Rust `archec0` seed remains authoritative through 0.1.

## Project authority

- [`arche_comprehensive_design_document.md`](arche_comprehensive_design_document.md) defines the language, runtime, artifact, tooling, M27, and M28 contracts. Its M27/M28 authority section supersedes conflicting older roadmap sketches while retaining them as design history.
- [`WORK_LOG.md`](WORK_LOG.md) records the current promoted gate, implementation evidence, exact-head CI, and unresolved acceptance blockers. It is the operational source of truth for what has actually shipped.
- This README is an orientation page, not an implementation-completion claim.

Pre-1.0 contracts may make explicit versioned hard cuts. Unsupported source, manifest, object, metadata, observation, registry, or trainer-protocol versions must fail clearly; they are never silently reinterpreted.

## Target experience

The public toolchain is converging on one `arche` command:

```text
arche new example/demo
arche check
arche build
arche run
arche test
arche inspect
arche fmt
arche doc
arche lsp
arche debug
arche profile
arche add | remove | update | search | package
arche publish --dry-run
arche login | logout | whoami
arche scope | owner | trusted-publisher | yank | unyank
arche toolchain install | list | default | remove
```

M27-A established the public `arche` command shell and its shared contracts.
M27-B connected `arche check` to schema-1 manifests,
explicit workspaces and local path dependencies, deterministic resolution and
locking, streamed module loading, package-aware resolved HIR, and the M26
source-migration hard cut. A successful check publishes only canonical
`Arche.lock`; registry acquisition and the remaining public commands stay
explicitly unavailable until their assigned later gates. M27-C0 froze the
grammar, ownership/effect edge rules, CTFE accounting, diagnostic contract, and
verified generic Core schema. C1 is implementing complete retained syntax,
immutable source authority, and resolved symbolic HIR; C2-C6 remain sequenced
behind that slice. `archec0` remains the
authoritative executable compiler interface until the general language and AOT
pipeline are connected. Arche generates static x86-64 Linux PIE executables.
Windows remains a supported compiler and tooling host; generated programs run
through explicitly configured WSL until native Windows output becomes a later
target.

## Repository layout

```text
bootstrap/archec0/   Rust bootstrap compiler/runtime seed and Cargo workspace
  crates/arche-foundation/
                     Shared process, identity, and format foundations
  crates/arche-package/
                     Schema-1 manifests, workspaces, resolver, and lockfiles
  crates/arche-frontend/
                     Streaming M27 module parser and package-aware resolved HIR
  crates/arche/       Public command driver (`check` implemented since M27-B)
examples/            Arche source fixtures
tests/m27b/           Package/module/public-check proof fixtures
tests/e2e/           End-to-end executable proofs
tools/               Local proof runner
WORK_LOG.md          Promoted milestone state and acceptance evidence
arche_comprehensive_design_document.md
                     Normative design and roadmap contract
```

Generated files live under `build/`; Rust build output lives under `bootstrap/archec0/target/`. Both paths are ignored.

## Current proof suite

Requirements:

- Rust 1.95.0 and Cargo; `rust-toolchain.toml` selects the exact toolchain.
- PowerShell Core 7.6.4 (`pwsh`) as the preferred proof shell. Windows PowerShell 5.1 remains supported.
- WSL or native Linux to execute generated Linux ELF64 artifacts from a Windows compiler host.

Run the local proof inventory from the repository root:

```powershell
pwsh -NoLogo -NoProfile -File .\tools\test.ps1
```

Useful bootstrap commands:

```powershell
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -p arche -- --help
cargo test --locked --workspace --all-targets --manifest-path .\bootstrap\archec0\Cargo.toml
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- --help
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --check
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\math.arc --emit-core
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\move_system.arc --emit-machine
cargo run --locked --manifest-path .\bootstrap\archec0\Cargo.toml -- .\examples\exit42.arc -o .\build\exit42
```

Local success is not external acceptance. A milestone or gate closes only when the exact required hosted checks and evidence are recorded in `WORK_LOG.md`.

## License

First-party Arche code and templates are available under `MIT OR Apache-2.0`; see [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE). Arche makes no copyright claim over programs or other output produced by the toolchain.
