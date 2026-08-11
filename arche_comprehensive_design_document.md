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

- `mod physics;` resolves only `physics.arc`; a child `mod collision;` declared there resolves `physics/collision.arc`.
- There is no `mod.arc`, wildcard discovery, path attribute, or duplicate module loading.
- `use`, `pub`, `pub(package)`, `pub(super)`, and `pub(in path)` define imports and visibility.
- Source identifiers use Unicode XID normalized to NFC. Filename aliases, case-fold collisions, and normalization collisions are errors.
- Public package scope and name segments are strict lowercase ASCII and use `scope/name`; official packages use `arche/*`.

The schema-1 toolchain pins its Unicode tables with the implementation: XID and
NFC use Unicode 17.0.0 through `unicode-ident` 1.0.24 and
`unicode-normalization` 0.1.25; full filename/path collision folding uses the
Unicode 9.0.0 table embedded by `unicode-casefold` 0.2.0. These deliberately
versioned tables are format/build inputs. A later Unicode upgrade is a reviewed
language/toolchain change, never an ambient host-library change.

M27-B freezes the structural module grammar used before M27-C owns complete
signatures and bodies. `mod name;` and `use path;` are the only module/import
forms. Imports bind their final segment; renames, groups, globs, inline modules,
and path attributes are unavailable. Paths begin with `package::`, `self::`, one
or more `super::` segments, or a declared dependency alias. The language does
not use Rust's `crate::` root. Module, type, and value namespaces are distinct.
M27-B resolves item headers and target links into session-local HIR identifiers;
it deliberately does not publish final `DefinitionId` values before M27-C can
encode complete generic, type, and effect shapes.

The graph-aware frontend assigns zero-based dense package nodes by canonical
package-name order. Within each package, the one manifest target order is:
library first when present, then `[[bin]]` entries in their array order, then
`[[environment]]` entries in their array order. Physical placement of the
`[lib]`, `bin`, or `environment` keys does not alter that order. Target IDs are
zero-based and dense in this order. A HIR
definition ID is the tuple `(package node, target ID, local definition
ordinal)`, so imports never collide across packages even though module/file
ordinals remain target-local. Workspace libraries are checked dependency-first.
Only direct normal dependency aliases enter a target scope; development and
transitive aliases do not. Cross-package traversal sees only public module/item
paths, a public re-export may expose a public item through a private internal
module, and no `pub use` may widen a package-private or module-private item.
One `use path;` imports every matching distinct namespace at the final segment.
Registry dependency exports require the package-cache/object adapter assigned
to later gates and therefore fail explicitly in the M27-B source-only driver.

M26 `startup`/final-`exit` source is not accepted by M27 target semantics. It receives an explicit migration diagnostic; no source compatibility shim remains.

## 0.3 General language contract

### 0.3.1 Values and numerics

The scalar set is `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, 64-bit `isize`/`usize`, `f32`, `f64`, `bool`, Unicode-scalar `char`, and 64-bit `entity`. The aggregate and owned-value set includes tuples, fixed arrays, slices, `str`, named structs, payload enums, generic `Option` and `Result`, `String`, `Vec<T>`, ordered `Map<K,V>`, `Box<T>`, `Rc/Weak`, `Arc/Weak`, and `Pin`. Arche has no garbage collector.

Integer arithmetic and shifts wrap in two's complement/modulo arithmetic. Shift counts are masked to the width; signed right shift is arithmetic. Unsigned division is the floor quotient and its remainder satisfies `a = q*b + r` with `0 <= r < b`. Signed division truncates toward zero; its remainder satisfies the same equation, has the dividend's sign when nonzero, and has magnitude less than the divisor's magnitude. For every integer width, division and remainder by zero trap as `IntegerDivideByZero`; for every signed width, both `MIN / -1` and `MIN % -1` trap as `IntegerSignedOverflow`. No implicit numeric or boolean conversion exists. `From` and `TryFrom` express safe conversions; `as` is reserved for unsafe pointer/address casts.

Floating-point operations use round-to-nearest-even, masked exceptions, FTZ/DAZ disabled, no contraction, preserved subnormals and signed zero, and canonical arithmetic NaNs: `f32` is exactly `0x7FC00000` and `f64` is exactly `0x7FF8000000000000`. Ordered comparisons with NaN are false except `!=`, which is true.
The compiler establishes that state before every semantic/CTFE worker executes
floating operations; the runtime establishes it at process entry and in every
spawned Arche thread before user code. A foreign host change is restored at the
next trusted Arche entry boundary. These rules are per thread, not inherited by
assumption.

### 0.3.2 Functions, generics, traits, and patterns

Arche supports direct and mutual recursion, real call frames, stack probes, and a noncatchable stack-overflow trap. Generics accept type, lifetime, and integer-const parameters.

Generic recursion must have a finite closed call-context graph before Core.
For every direct/trait/callable SCC, a recursive edge may only permute same-kind
SCC formals or replace a formal with an argument closed over that SCC. Across
each cycle, every still-carried type/lifetime/const formal therefore composes to
a permutation. Embedding an SCC type formal under a constructor, applying an
operator to an SCC const formal, duplicating a carried formal into an expanding
position, or otherwise producing a growing substitution is `CALL001` at the
recursive call. Thus `f<T> -> f<Option<T>>` and `f<const N> -> f<N+1>` are
rejected, while finite mutual permutations and closed replacements are legal.
The compiler enumerates the resulting finite closed context graph in canonical
call-site/substitution order; RootSlice closed views and M27-D instances must
equal that graph.

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

Checked exceptions use explicit canonical `throws {E...}` sets. Capability effects use separate canonical `requires {Capability...}` sets. Exported function/trait/callable signatures, schedules, and ABI hashes include their boundary sets; Generic Core bodies separately include their actual least-fixed-point sets. Recursive call graphs are solved to a fixed point. A schedule exposes the exact union of every dispatched system's boundary sets.

`throw`, propagation, and exhaustive `catch` implement recoverable exceptions. An exception escaping the entrypoint unwinds initialized values, discards the current unflushed structural-command epoch, preserves earlier committed effects, emits an enabled observation, writes the reserved diagnostic, and exits `71`. Panic and semantic traps are uncatchable and exit `70` after the same committed-state rule; a panic during unwind aborts `134`.

Capabilities are unforgeable, non-static, nonserializable driver-supplied values. General binaries may receive explicit capabilities for arguments, environment, standard I/O, files, subprocesses, wall/monotonic clocks, TCP/UDP, threads, atomics, and synchronization. Environment reset/step/self-play call graphs are statically checked and dynamically guarded against ambient/nondeterministic host effects, raw address observation, unsafe host calls, and threads.

Compile-time evaluation runs the full hermetic language subset, including recursion, allocation-backed values, traits, caught exceptions, closures/generators, and Drop. It cannot access ECS worlds, capabilities, threads, host I/O, FFI, or observable addresses. `include_bytes` and `include_str` are explicit hashed package inputs. Published manifests pin step, call-depth, and heap budgets; scaffold defaults are 10,000,000 steps, depth 1,024, and 64 MiB. Budget exhaustion is a compiler-resource error, not an Arche result.

### 0.3.5 M27-C surface grammar and evaluation order

M27-C freezes the complete 0.1 grammar before the implementation assigns AST
or Core goldens. The notation below is normative EBNF: postfix `?` means zero
or one, postfix `*` means repetition, and `|` separates alternatives. A line
break is ordinary whitespace. `let` and every non-final expression statement
whose outer form is not a block/control expression require `;`. A final
expression immediately followed by `}` is the block result and has no
semicolon. Block, `if`, `match`, `catch`, `loop`, `while`, and `for` expression
statements may omit it. Item, schedule, and world-initializer semicolons appear
exactly where required below; whitespace never acts as a hidden separator.

Source is exact UTF-8 without a BOM. Lexical whitespace is ASCII space, tab,
CR, and LF. `///` is recognized before `//` and produces one `DOC_COMMENT`
token containing the exact bytes after the third slash through before CR/LF,
with no normalization or implicit space removal. Other `//` comments end before
CR/LF; `/* ... */` comments nest with checked `u64` depth and must close. They
are whitespace and never occur inside a token. `//!`, `/**`, attributes, and
macro syntax do not exist in 0.1. Identifiers use the pinned Unicode XID/NFC
rules in Section 0.2.
`_` alone is the wildcard token. A lifetime is `'` followed by an XID identifier
without a closing quote; `'a'` is instead a character literal. The fixed keyword
set is:

```text
as break catch commands component const continue default else enum entity
false fn for gen if impl in init let loop match mod move mut package pub query
read ref requires resource resume return run schedule self Self spawn static str
struct super system tag throw throws trait true type unsafe use where while
world yield yields
i8 i16 i32 i64 u8 u16 u32 u64 isize usize f32 f64 bool char
```

Longest-match punctuation/operators are `::`, `->`, `=>`, `..=`, `..`, `<<`,
`>>`, `<=`, `>=`, `==`, `!=`, `&&`, `||`, and `+=`; remaining single-character
tokens are `{ } ( ) [ ] , ; : . @ | + - * / % & ^ ! ~ = < >`. `>>` is one shift
token normally and is split into two generic closers only by the generic-
argument parser, without whitespace dependence.

Digits inside a numeric component may contain single `_` separators only
between digits. The byte grammar is exact (`D`, `B`, `O`, and `H` mean one
ASCII decimal, binary, octal, or hexadecimal digit):

```text
digits10      := D ("_"? D)*
digits2       := B ("_"? B)*
digits8       := O ("_"? O)*
digits16      := H ("_"? H)*
int_suffix    := "i8" | "i16" | "i32" | "i64" | "isize"
               | "u8" | "u16" | "u32" | "u64" | "usize"
float_suffix  := "f32" | "f64"
integer       := (digits10 | "0b" digits2 | "0o" digits8 | "0x" digits16)
                 int_suffix?
dec_exponent  := ("e" | "E") ("+" | "-")? digits10
hex_exponent  := ("p" | "P") ("+" | "-")? digits10
decimal_float := (digits10 "." digits10? dec_exponent? | digits10 dec_exponent)
                 float_suffix?
hex_float     := "0x" digits16 "." digits16? hex_exponent float_suffix?
```

The lexer chooses `hex_float`, then `decimal_float`, then `integer` at one
starting byte and requires the following byte not to continue an identifier.
However, a decimal point immediately followed by another `.` never belongs to
a float token. Thus `1..2` and `1..=2` tokenize as INTEGER plus the range token
plus INTEGER, while `1.` is a float. Suffixes are contiguous ASCII, and
`1.foo` is an invalid numeric token rather than field access. A hexadecimal
float must contain at least one digit before
its point. Strings/chars support `\\`, `\"`, `\'`, `\n`, `\r`, `\t`,
`\0`, `\xNN`, and `\u{H...}` with one through six hex digits. `\xNN` must be at
most `0x7F`; a Unicode escape must be a scalar value. Unescaped control bytes,
CR/LF, and an unpaired quote are errors. A string contains zero or more decoded
scalars; a character contains exactly one. Raw/byte strings and implicit
adjacent-string concatenation do not exist.

```text
item          := DOC_COMMENT* (module_item | import_item | visible_item | impl_item)
module_item   := visibility? "mod" IDENT ";"
import_item   := visibility? "use" rooted_item_path ";"
visible_item  := visibility? (world | component | resource | tag | struct
                 | enum | type_alias | const_item | static_item | function
                 | generator | system | schedule | trait)
impl_item     := "impl" "default"? generics?
                 (trait_path "for")? type where_clause? "{"
                 impl_method* "}"
visibility    := "pub" | "pub" "(" ("package" | "super"
                 | "in" visibility_path) ")"
visibility_path := "package" ("::" IDENT)*
                 | "self" ("::" IDENT)*
                 | super_root ("::" IDENT)*
rooted_item_path := ("package" | "self" | super_root
                    | DEPENDENCY_ALIAS) "::" IDENT ("::" IDENT)*
super_root    := "super" ("::" "super")*
bound_path    := IDENT | "Self"
type_path     := bound_path | rooted_item_path
trait_path    := type_path generic_arguments?
value_path    := bound_path | value_path_root value_path_tail+
value_path_root := "package" | "self" | super_root | IDENT | "Self"
value_path_tail := "::" IDENT | "::" generic_arguments "::" IDENT
generics      := "<" generic_param ("," generic_param)* ","? ">"
generic_param := LIFETIME (":" LIFETIME)?
               | IDENT (":" type_bound ("+" type_bound)*)?
               | "const" IDENT ":" integer_type
where_clause  := "where" predicate ("," predicate)* ","?
predicate     := type ":" type_bound ("+" type_bound)*
               | LIFETIME ":" LIFETIME
type_bound    := trait_path | LIFETIME
effect_sets   := requires_set? throws_set?
requires_set  := "requires" "{" (type_path ("," type_path)* ","?)? "}"
throws_set    := "throws" "{" (type ("," type)* ","?)? "}"
function      := "unsafe"? "fn" IDENT generics? "(" parameters? ")"
                 effect_sets ("->" type)? where_clause? block
generator     := "unsafe"? "gen" "fn" IDENT generics? "(" parameters? ")"
                 "resume" type "yields" type effect_sets
                 ("->" type)? where_clause? block
trait         := "trait" IDENT generics? where_clause? "{"
                 trait_method* "}"
trait_method  := DOC_COMMENT* method_signature ";"
impl_method   := DOC_COMMENT* visibility? method_signature block
method_signature := "unsafe"? "fn" method_name generics?
                 "(" method_parameters? ")" effect_sets
                 ("->" type)? where_clause?
component     := "component" IDENT generics? where_clause? record_fields
resource      := "resource" IDENT generics? where_clause? record_fields
struct        := "struct" IDENT generics? where_clause?
                 (record_fields | tuple_fields ";" | ";")
enum          := "enum" IDENT generics? where_clause? "{"
                 (variant ("," variant)* ","?)? "}"
tag           := "tag" IDENT ";"
type_alias    := "type" IDENT generics? "=" type where_clause? ";"
const_item    := "const" IDENT ":" type "=" expression ";"
static_item   := "static" "mut"? IDENT ":" type "=" expression ";"
world         := "world" IDENT "{" "init" world_init_block "}"
system        := "system" IDENT system_generics? "(" system_parameters? ")"
                 effect_sets where_clause? block
schedule      := "schedule" IDENT "{" ("run" schedule_target ";")* "}"
system_generics := "<" system_generic_param
                   ("," system_generic_param)* ","? ">"
system_generic_param := IDENT (":" type_bound ("+" type_bound)*)?
                   | "const" IDENT ":" integer_type
schedule_target := rooted_or_bound_path
                   ("::" system_generic_arguments)?
system_generic_arguments := "<" system_generic_argument
                   ("," system_generic_argument)* ","? ">"
system_generic_argument := type | "const" const_expression
parameters    := parameter ("," parameter)* ","?
method_parameters := receiver ("," parameter)* ","? | parameters
parameter     := pattern ":" type
receiver      := "self" | "mut" "self" | "&" LIFETIME? "self"
               | "&" LIFETIME? "mut" "self"
system_parameters := system_parameter ("," system_parameter)* ","?
system_parameter  := IDENT ":" ("read" type | "mut" type
                     | "query" "[" query_terms? "]" | "commands" | type)
query_terms   := query_term ("," query_term)* ","?
query_term    := "mut"? type | "!" type
record_fields := "{" (record_field ("," record_field)* ","?)? "}"
record_field  := DOC_COMMENT* visibility? IDENT ":" type
tuple_fields  := "(" (tuple_field ("," tuple_field)* ","?)? ")"
tuple_field   := visibility? type
variant       := DOC_COMMENT* IDENT (enum_tuple_fields | enum_record_fields)?
enum_tuple_fields := "(" (type ("," type)* ","?)? ")"
enum_record_fields := "{" (IDENT ":" type ("," IDENT ":" type)* ","?)? "}"
world_init_block := "{" world_init* "}"
world_init    := "resource" type "=" expression ";"
               | "spawn" "{" (expression ("," expression)* ","?)? "}" ";"
rooted_or_bound_path := rooted_item_path | bound_path
```

`requires` always precedes `throws`. Source order is accepted for human
authorship, but duplicate resolved members are errors and semantic encodings
sort by canonical identity. Omitting a set on a named function/generator/system,
trait/impl method, or function-pointer type means the empty set. On a closure or
generator closure, each omitted set independently requests inference unless an
expected callable type supplies that set's boundary; each spelled set is an
exact declared boundary and does not change whether the other set is inferred.
An expected callable supplies both set boundaries, so either omitted source set
uses its expected boundary and neither is inference-requesting. A callable body
may use only effects covered by its declaration.
Trait methods spell exact sets, and implementations may narrow but never widen
them. Every resolved effect set is encoded into its semantic type and stable
declaration shape. The body also has a distinct least-fixed-point `actual`
summary. For named callables and declared/expected closure/generator boundaries,
actual sets need only be subsets of the boundary; unused declared members remain
part of the callable type and identity. Only an independently inference-
requesting closure/generator set takes that set's boundary from the actual
summary. Schedule boundaries are
their exact run union, and compiler-derived Drop requires remain their exact
transitive body summary.

A throwing call propagates automatically when its complete throws set is
covered by the enclosing declaration; Arche has no `?` propagation operator.
`throw expression` starts a checked exception of the expression's resolved
type. A thrown type must be a fully owned, sized, `'static`, non-capability
value; transitively, references, raw pointers, closures/generators,
capabilities, world/OS/thread handles, interior uninitialized storage, and types
whose Drop requires a capability cannot be thrown. This compiler-sealed
structural judgment is `UnwindPayload`. The internal exception carrier is a
sealed tagged union of the callable's canonical throws types, never a source
type or serializable value.

`catch operand { ... }` receives that heterogeneous set. An arm whose pattern
has a nominal constructor applies only to the matching thrown type; `_` can
catch all remaining types without binding their heterogeneous payload. Any
binding arm must have one statically unique exception type. Arms and guards use
ordinary pattern rules and must exhaust the operand's throws set. Bare `throw;`
is valid only inside an arm and rethrows its current typed payload. The caught
types are removed from the catch expression's escaping set; guard/arm effects
and explicit rethrows are added. Schedule declarations spell no effect sets:
their effects are the exact canonical union of run systems in listed order,
including repeats, and are exposed at every schedule dispatch.

Record fields use `name: Type`; enum variants are `Name`, `Name(Type, ...)`, or
`Name { field: Type, ... }`. A comma shown in the grammar is required whenever
another element follows; only the explicitly shown final comma is optional. A
trailing comma is part of a nonempty element group: an empty delimiter pair can
never contain a comma. Tuple type/expression/pattern syntax requires the first
comma to distinguish a singleton; with no second element that comma is the one
singleton comma, and a second consecutive comma is invalid. A generic argument
is a type, lifetime, or integer
constant matching the declared parameter kind. Empty tuples, arrays, records,
components, resources, tags, queries, effect sets, and world initializers are
valid.
Systems may declare type and integer-const parameters; their resource, query,
command, and ordinary borrow lifetimes are the compiler-owned schedule-dispatch
lifetime rather than user generic parameters. Every schedule run of a generic
system spells all declared arguments in order as `System::<Type, const N>` and
those arguments must be closed in the schedule's nongeneric context. Omitting
arguments from a generic target, supplying arguments to a nongeneric target, or
leaving inference variables is `TYPE001`. The run is represented as a system
definition plus its exact substitution, not merely as a definition ID.
The final bare-`type` system-parameter form is not an arbitrary injected value:
it must be `&Capability` or `&mut Capability` for one sealed capability type.
The dispatcher borrows that member from the Caps argument for the call, adds the
capability identity to the system's requires set, and applies ordinary alias
rules; any other ordinary system parameter is `TYPE001`. Resource/query/
command parameters keep their dedicated forms.
The restricted `const_expression` grammar is used only for array lengths,
array-repeat counts, and integer const-generic arguments and yields one exact
contextual integer. It may reference bound integer const parameters and const
items; calls, blocks, control flow, comparisons, aggregates, assignment, and
host/world operations are unavailable there. Const/static item initializers use
the full `expression` grammar and the complete hermetic evaluator.

A bare single `IDENT` is a lexical/generic/imported/module-scope binding, not a
discovering path. A multi-segment item path begins with `package`, `self`, one
or more `super` segments, or a declared direct dependency alias. A bare first
segment followed by `::` is accepted only when that segment is a declared
dependency alias or resolves in the type namespace and the final segment is an
associated function, static method, or enum variant of that type. The parser
retains one neutral segmented value-path AST. Resolution tests the permitted
module-item and type-associated partitions, accepts exactly one, and reports
`NAME002` if none or more than one is viable; it never uses fallback discovery.
Associated constants do not exist in 0.1. Module discovery remains exclusively
M27-B's explicit `mod` tree.
Explicit generic arguments on a call or record constructor use the turbofish
spelling `value::<...>`; type positions remain `Type<...>`. The contextual
keyword method names `read`, `resource`, `run`, and `spawn` are accepted only
in a method declaration or after `.`,
where the following delimiter distinguishes the ordinary `(...)` method from
the reserved Commands `.spawn { ... }` form. `resume` remains exclusively the
reserved generator postfix and is never ordinary method lookup.

The type grammar is:

```text
type          := scalar | "!" | "()" | "str" | "Self"
               | type_path generic_arguments?
               | "(" type "," (type ("," type)* ","?)? ")"
               | "[" type ";" const_expression "]" | "[" type "]"
               | "&" LIFETIME? "mut"? type
               | "*" ("const" | "mut") type
               | "unsafe"? "fn" "(" type_list? ")" effect_sets ("->" type)?
type_list     := type ("," type)* ","?
integer_type  := "i8" | "i16" | "i32" | "i64" | "u8" | "u16"
               | "u32" | "u64" | "isize" | "usize"
scalar        := integer_type | "f32" | "f64" | "bool" | "char" | "entity"
generic_arguments := "<" generic_argument ("," generic_argument)* ","? ">"
generic_argument  := type | LIFETIME | "const" const_expression
const_expression  := const_bit_or
const_bit_or      := const_bit_xor ("|" const_bit_xor)*
const_bit_xor     := const_bit_and ("^" const_bit_and)*
const_bit_and     := const_shift ("&" const_shift)*
const_shift       := const_additive (("<<" | ">>") const_additive)*
const_additive    := const_multiplicative
                     (("+" | "-") const_multiplicative)*
const_multiplicative := const_unary (("*" | "/" | "%") const_unary)*
const_unary       := ("-" | "~") const_unary | const_primary
const_primary     := INTEGER_LITERAL | rooted_or_bound_path
                   | "(" const_expression ")"

block         := "{" statement* tail_expression? "}"
statement     := let_statement | for_statement | assignment_statement
               | block_expression_statement | terminated_expression
let_statement := "let" pattern (":" type)? "=" expression
                 ("else" block)? ";"
for_statement := "for" pattern "in" expression block ";"?
assignment_statement := place_expression ("=" | "+=") expression ";"
place_expression := "*" place_expression | postfix_expression
block_expression_statement := block | if_expression | while_expression
               | loop_expression | match_expression | catch_expression
               | unsafe_expression
terminated_expression := expression ";"
tail_expression := expression

expression    := logical_or_expression
logical_or_expression := logical_and_expression ("||" logical_and_expression)*
logical_and_expression := bit_or_expression ("&&" bit_or_expression)*
bit_or_expression := bit_xor_expression ("|" bit_xor_expression)*
bit_xor_expression := bit_and_expression ("^" bit_and_expression)*
bit_and_expression := equality_expression ("&" equality_expression)*
equality_expression := relational_expression (("==" | "!=") relational_expression)?
relational_expression := shift_expression (("<" | "<=" | ">" | ">=") shift_expression)?
shift_expression := additive_expression (("<<" | ">>") additive_expression)*
additive_expression := multiplicative_expression (("+" | "-") multiplicative_expression)*
multiplicative_expression := cast_expression (("*" | "/" | "%") cast_expression)*
cast_expression := unary_expression ("as" type)*
unary_expression := ("-" | "!" | "~" | "*" | "&" | "&" "mut") unary_expression
                  | postfix_expression
postfix_expression := primary_expression postfix_part*
postfix_part  := "(" argument_list? ")" | "[" expression "]"
               | "." method_name ("::" generic_arguments)? "(" argument_list? ")"
               | "." (IDENT | INTEGER_LITERAL)
               | "." "spawn" command_spawn_payload
               | "." "resume" "(" expression ")"
               | "::" generic_arguments "(" argument_list? ")"
method_name   := IDENT | "read" | "resource" | "run" | "spawn"
argument_list := expression ("," expression)* ","?
command_spawn_payload := "{" (expression ("," expression)* ","?)? "}"
primary_expression := literal | value_path | self_expression
               | "(" ")" | "(" expression ")" | tuple_expression | array_expression
               | record_constructor record_expression
               | block | if_expression | while_expression | loop_expression
               | match_expression | catch_expression | unsafe_expression
               | closure_expression | generator_closure
               | "return" expression? | "break" expression? | "continue"
               | "throw" expression? | "yield" expression
tuple_expression := "(" expression "," (expression ("," expression)* ","?)? ")"
array_expression := "[" (expression ("," expression)* ","?
                  | expression ";" const_expression)? "]"
record_expression := "{" (IDENT ":" expression
                  ("," IDENT ":" expression)* ","?)? "}"
if_expression := "if" expression block ("else" (block | if_expression))?
               | "if" "let" pattern "=" expression block
                 ("else" (block | if_expression))?
while_expression := "while" expression block
                  | "while" "let" pattern "=" expression block
loop_expression := "loop" block
match_expression := "match" expression "{" match_arms? "}"
catch_expression := "catch" expression "{" catch_arms? "}"
match_arms    := match_arm ("," match_arm)* ","?
catch_arms    := catch_arm ("," catch_arm)* ","?
match_arm     := pattern ("if" expression)? "=>" expression
catch_arm     := catch_pattern ("if" expression)? "=>" expression
unsafe_expression := "unsafe" block
closure_expression := "move"? "|" closure_parameters? "|"
                  effect_sets ("->" type)? expression
generator_closure := "gen" "move"? "|" closure_parameters? "|"
                  "resume" type "yields" type effect_sets
                  ("->" type)? expression
closure_parameters := closure_parameter ("," closure_parameter)* ","?
closure_parameter := closure_at_pattern (":" type)?
closure_at_pattern := binding_pattern "@" closure_at_pattern
                    | closure_structural_pattern
closure_structural_pattern := "_" | "(" ")" | literal_pattern
                    | const_pattern | range_pattern | binding_pattern
                    | "&" "mut"? closure_at_pattern
                    | tuple_pattern | slice_pattern | constructor_pattern
self_expression := "self"
record_constructor := value_path ("::" generic_arguments)?

pattern       := or_pattern
or_pattern    := at_pattern ("|" at_pattern)*
at_pattern    := binding_pattern "@" at_pattern | structural_pattern
binding_pattern := "mut"? IDENT | "ref" "mut"? IDENT
structural_pattern := "_" | "(" ")" | literal_pattern | const_pattern | range_pattern
               | binding_pattern | "&" "mut"? pattern
               | tuple_pattern | slice_pattern | constructor_pattern
tuple_pattern := "(" pattern "," (pattern ("," pattern)* ","?)? ")"
slice_pattern := "[" (slice_pattern_part ("," slice_pattern_part)* ","?)? "]"
slice_pattern_part := pattern | ".."
constructor_pattern := value_path
               ("(" (pattern ("," pattern)* ","?)? ")"
               | "{" (IDENT ":" pattern ("," IDENT ":" pattern)* ","?)? "}")?
literal_pattern := signed_integer_pattern | CHAR_LITERAL | STRING_LITERAL
                 | "true" | "false"
const_pattern := value_path
range_pattern := range_endpoint (".." | "..=") range_endpoint
range_endpoint := signed_integer_pattern | CHAR_LITERAL | const_pattern
signed_integer_pattern := "-"? INTEGER_LITERAL
catch_pattern := pattern
literal       := INTEGER_LITERAL | FLOAT_LITERAL | CHAR_LITERAL | STRING_LITERAL
               | "true" | "false"
```

Local items and labels do not exist in 0.1. A query value may be iterated only
by `for`; query loops may occur inside control flow but may not nest, and no
query/component/resource borrow may survive the loop or a structural-command
epoch. World initialization contains only the grammar's resource/spawn forms;
neither may call a schedule or perform host effects.
The type after `resource` must resolve to one closed resource instantiation and
the expression must produce that exact type. Each spawn payload expression must
produce one closed component or tag instantiation; generic constructor
arguments may be explicit or fully inferred from the expression context.

`return` targets the current function/generator and its optional expression must
match the declared result. `yield` is valid only in a generator. `break` and
`continue` target the nearest enclosing loop because labels do not exist. Only
`loop` accepts `break expression`; every reachable valued break and the loop's
never-fallthrough paths must unify to one result type. `while` and `for` accept
only bare `break` and have type `()`. There is no language-level iteration or
recursion cap; CTFE resource budgets and external runtime cancellation are
separate infrastructure controls.

The scalar keywords are `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`,
`isize`, `usize`, `f32`, `f64`, `bool`, `char`, and `entity`. `isize` and
`usize` are exactly 64 bits for every 0.1 target. `()` is the unit type and `!`
is the uninhabited never type. Bare dynamically sized `str` and `[T]` may occur
only behind a reference or an approved owning/metadata-bearing intrinsic.

Expressions use this precedence from highest to lowest: paths/literals/grouping
and aggregate construction; calls, indexing, field access, and method calls;
unary `- ! ~ * & &mut`; unsafe raw-pointer/address `as`; `* / %`; `+ -`;
`<< >>`; relational `< <= > >=`;
equality `== !=`; bitwise `&`, `^`, `|`; then logical `&&`, `||`.
Control expressions are primary forms. Assignment is not an expression and therefore has no
precedence or associativity. Binary operators of the same row associate
left. Except for assignment's explicitly
RHS-first replacement rule below, calls, arguments, fields, indexes, operands,
guards, and aggregate elements evaluate in source order. `&&` and `||`
short-circuit. `return`, `break`, `continue`, `throw`, `yield`, `if`, `if let`,
`while`, `while let`, `loop`, `match`, `catch`, `unsafe`, blocks, closures, and
generator closures are expressions. A block's final unterminated expression is
its value; otherwise it has type `()`.
Range values are not expressions in 0.1; `..` and `..=` exist only in the
pattern grammar.
At the outermost operand of `if`, `if let`, `while`, `while let`, `for`,
`match`, and `catch`, the construct's required `{` is its delimiter and cannot
begin an unparenthesized record constructor. A record-valued condition,
iterator, scrutinee, or catch operand must be parenthesized. This makes
`if Name { ... }` unambiguously a path condition followed by a block, while
`if (Name {}) { ... }` tests a constructed value.
After `.name`, an immediately following `::<...>` or `(` is parsed as one
method-call postfix before the field-plus-call alternative. Method generic
arguments always use that turbofish, so `object.field < rhs` is relational and
never begins a generic method call. Calling a callable value stored in a field
therefore requires `(object.field)(...)`. A cast consumes the longest
type after `as`; when that type reaches a path, a following `<` commits to its
type-generic list and the parser does not backtrack to reinterpret it as a
relational operator. Comparing a cast result with `<` therefore requires
parentheses, as in `(value as T) < rhs`.
`place_expression` admits direct dereference recursively, so `*pointer = value`
is syntactically a destination; place typing and the unsafe-region rules still
decide whether that destination is writable.

Closures use `move`? `|pattern: Type, ...| effect_sets (-> Type)? expression`.
Generator closures use `gen` `move`? `|pattern: Type, ...| resume Type yields
Type effect_sets (-> Type)? expression`. Parameter types may be inferred only for a
closure whose expected callable type is known at the expression site. Named
functions and generators require parameter types. For named functions and
generators an omitted return type is `()`. For a closure/generator closure it is
inferred from expression completion and explicit returns unless an expected
callable type supplies it; incompatible sites are `TYPE002`, not an implicit
unit fallback.

A closure or generator-closure parameter accepts one `closure_at_pattern`, not
a top-level or-pattern, because the same `|` token closes the parameter list.
The restriction propagates through undelimited `&` and `@` operands.
Or-patterns remain available after a tuple, slice, or constructor payload has
opened its own explicit delimiter. Consequently
`|a| a | b` is one parameter followed by a bit-or body. Because whitespace is
nonsemantic, the token-equivalent spelling `|a | b| expr` likewise means one
`a` parameter followed by the body `b | expr`; it can never request a top-level
parameter or-pattern. An enclosing structural form such as the singleton tuple
pattern `(a | b,)` is required when an or-pattern is needed inside a parameter.

A named generator item and a generator-closure expression denote generator
factories, not an already-running body. Calling either factory evaluates the
factory then the initial arguments in source order and returns one unpinned
generator state-machine value. The declaration's parameter list is the factory
input; its `resume` type is the later input to each resume; `yields` is the
suspension output; and its `->` type is the eventual completion value rather
than the factory call result. A generator-closure expression first evaluates to
an anonymous, compiler-typed factory value that owns/borrows its captures. Its
factory call implements the inferred `Fn`/`FnMut`/`FnOnce` class, has exact
`requires {}` and `throws {}`, and stores the ordered captures and initial
arguments into the new state. Construction neither enters the generator body
nor contributes that body's effects; only resume does so.
`unsafe gen fn` makes its named factory unsafe: constructing its state requires
a verified unsafe region even though construction is total and deferred. The
unsafe contract is discharged at construction; the resulting state resumes
through the ordinary safe Pin API. Generator closures are always safe factories,
and no coercion or descriptor substitution may erase a named factory's safety
bit.

Patterns are `_`; literal or const paths; bindings with optional `mut`, `ref`,
or `ref mut`; tuple, record, enum, reference, and slice patterns; inclusive
`a..=b` and exclusive `a..b` integer/character ranges; or-patterns with `|`;
and `name @ pattern`. A slice pattern contains at most one `..`. A match arm is
`pattern (if expression)? => expression`. `let pattern = expression else block`
requires the `else` block to diverge. The scrutinee is evaluated once. Guards
run only after structural matching, in source order, borrow their bindings for
the guard, and do not contribute to exhaustiveness. Owned non-`Copy` bindings
move unless declared `ref`/`ref mut` or reached through a borrowed scrutinee.
Every alternative of an or-pattern binds exactly the same names, types, and
binding modes. Float patterns and structural `Map` patterns are errors.
Integer/char `a..b` covers `a <= value < b`; `a..=b` covers
`a <= value <= b`. Endpoints are same-typed CTFE constants, descending ranges
are errors, and an equal exclusive range is empty/unreachable. String/str
literal and Vec/slice patterns compare borrowed logical contents without calling
user trait code. A const pattern requires a finite structural canonical value
and likewise invokes no user Eq/Ord implementation.
Unary-negative integer pattern tokens use the same post-negation type-fit rule
as expression literals, including every signed minimum. A qualified pattern path
always resolves as a const or constructor. For a bare `IDENT`, `mut IDENT`,
`ref IDENT`, and `ref mut IDENT` are bindings; an unadorned bare `IDENT` is a
const/unit-variant pattern only when ordinary value-namespace lookup at that
site resolves one unique visible const or unit variant, otherwise it introduces
a binding. An ambiguous value lookup is `PATTERN001`, never a binding fallback.

Function/system/generator parameters, closure parameters, `for` bindings, and a
`let` without `else` require an irrefutable pattern. `let ... else`, `if let`,
`while let`, `match`, and `catch` admit refutable patterns; the `let ... else`
else block must diverge. Exhaustiveness is computed over the structural pattern
space after guards are erased; a guarded arm never covers values for later arms.

Match ergonomics use one explicit default binding mode. It starts as `Move`.
When a non-reference structural pattern meets `&T`, matching inserts one shared
deref and changes `Move`/`RefMut` to `Ref`; when it meets `&mut T`, it inserts
one mutable deref and changes `Move` to `RefMut` while preserving an existing
`Ref`. This repeats until the structural forms agree. An explicit `& pattern` or
`&mut pattern` consumes exactly one matching reference layer and resets the
nested default to `Move`. An unadorned binding uses the current default; `ref`
forces `Ref`, `ref mut` requires a mutable path and forces `RefMut`, and `mut`
changes only the local binding's assignability. `Move` of non-`Copy` input owns
the matched subvalue; Ref/RefMut borrow it. These modes and inserted derefs are
stored in the decision tree and are not re-inferred during MIR lowering.

Integer literals may use decimal, `0b`, `0o`, or `0x` spelling and `_`
separators, followed optionally by an integer suffix. An unsuffixed integer is
an exact mathematical integer constrained by its use and otherwise defaults to
`i32`; it must fit the selected type after unary-negative interpretation. Thus
every signed minimum value is valid only as unary `-` applied to its positive
magnitude. A value not fitting any contextual type is an error, never wrapped
during parsing. Float literals accept decimal or hexadecimal binary-exponent
syntax with optional `f32`/`f64` suffix and otherwise default to `f64`. Decimal
digits and exponent denote one exact rational value; hexadecimal digits and
binary exponent denote one exact dyadic value. Conversion rounds that value
once to the selected IEEE binary format using round-to-nearest-even. A finite
spelling that would round to infinity is `TYPE003`; underflow may produce a
subnormal or signed zero. Unary `-` is applied after this positive-literal
conversion, so `-0.0` has the negative-zero bit pattern. Arithmetic results,
not literal parsing, apply the pinned wrapping/NaN rules.
Character literals decode exactly one Unicode scalar; surrogate values are
invalid. Boolean literals have type `bool`; there is no null literal. String
literals are exact UTF-8 after escapes, have type `&'static str`, and perform no
Unicode normalization. Conversion to `String` is explicit. There is no source
NaN literal; CTFE/runtime operations can produce canonical NaNs.

There are no implicit numeric, boolean, string, owning-pointer, or user-defined
conversions. The only coercions are never-to-any, lifetime shortening, mutable
reborrow to shared borrow, `&[T; N]` to `&[T]`, `&mut [T; N]` to `&mut [T]`,
and a noncapturing closure or function item to a function pointer with
contravariantly compatible parameters, covariant result, and effect subsets.
Safety is part of the pointer type: a safe function item may coerce to a safe or
unsafe function pointer, but an unsafe item only to an unsafe pointer; no cast
can erase that bit. Calling an unsafe function pointer requires an unsafe region.
A direct call whose callee syntax resolves to a named function path remains a
DirectCall and never materializes a callable value. In every other first-class
value context, a named function item is normalized immediately to the exact
FunctionRef/function-pointer value selected by that coercion; Arche has no
separate semantic function-item type. Passing that value through a generic Fn
bound therefore later selects FunctionPointerCall.
`From` and `TryFrom` are ordinary statically selected trait calls. `as` is
accepted only in an `unsafe` context for raw-pointer/address exposure and
reconstruction; it is never a numeric conversion.

Component, resource, and struct record/tuple fields may spell visibility and
default to the declaring module. Cross-package construction, field access, and
destructuring require public fields. Enum variants inherit the enum item's
effective visibility and variant payload fields cannot spell independent
visibility. Field names, variants, generic parameters, methods within one
trait/impl, and active lexical bindings are unique in their namespace. Arche
does not overload functions/methods by signature and does not permit shadowing
an enclosing live lexical binding.

A struct/component/resource or record-variant literal names every declared
field exactly once. Unknown, duplicate, or omitted fields are `TYPE002` at the
offending name or closing brace. Named fields may appear in any order;
expressions evaluate exactly in source order, while the constructed logical
value and later layout use declaration order. Tuple construction requires exact
arity and evaluates by index order. Unit/zero-field records use `{}`; tags use
their unit constructor. Construction never copies an omitted default because
field defaults do not exist in 0.1.

Every user-declared component/resource/struct/enum is `Sized`; direct recursive
storage cycles and bare slice/str fields are errors. Recursion may pass through
references, raw pointers, Box, Rc/Arc, Vec, or other approved sized indirection,
but a recursive re-entry of the same nominal declaration must use the identical
normalized generic substitution. A transformed re-entry such as
`Nest<T> -> Box<Nest<Option<T>>>` is a nonregular recursive type and `TYPE001`
at the inner nominal path; 0.1 does not attempt an infinite type/region family.
Type aliases are transparent, nonnominal, and may not form a cycle. User generic
type parameters are implicitly `Sized`; 0.1 has no `?Sized` source escape.

Const and static initializers require hermetic CTFE. A const denotes its
canonical logical value at each use. A static has process lifetime, is
initialized before entry, and is never automatically dropped. Immutable static
type `T` requires `T: Sync`; mutable static type `T` requires `T: Send` and every
access is unsafe. Capabilities, live world values, references to nonstatic
storage, closures/generators, and OS/thread handles cannot be static.

The exact binary entry is a public nongeneric safe
`fn main(app: &mut App<RootWorld>, caps: Caps<Declared>) requires {Declared}
throws {Errors} -> i32`, where `RootWorld` is the manifest world and `Declared`
is exactly the manifest capability set in canonical order. `Caps` is the sole
source-spellable compiler-special variadic nominal: it spells `Caps<()>` for the
empty set and
otherwise one capability type argument per canonical member, with no lifetime/
const arguments or duplicates. No other parameter,
receiver, or overload is accepted. Library targets contain no world or main.
The two parameter binding names are not semantic and may be any distinct
irrefutable identifier patterns; their order and exact types are semantic.
Every schedule dispatch uses the reserved
`app.run(schedule, caps)` form, including `Caps<()>`. The sealed operation takes
the Caps place without moving it and borrows exactly the
schedule's required capability members for the dispatch and fails statically if
the supplied Caps type omits one or a member was moved out. Root code may also
borrow an initialized resource through reserved
`app.resource<T>() -> &T` and `app.resource_mut<T>() -> &mut T`; NLL prevents a
live resource borrow from overlapping a schedule run or another conflicting
world access.
A schedule declaration binds its name in the value namespace only as a
non-first-class direct schedule operand. The first `app.run` argument must be a
value_path resolving through HIR `Res::Item` to that Schedule DefinitionId; it
cannot be stored, returned, captured, passed to another function, constructed,
or used outside this reserved operand position. HIR notation `ScheduleRef<S>`
means that resolved definition plus its verified ordered SystemRun rows and is
not a source/Core value or virtual nominal type. Lowering places the DefinitionId directly in
Intrinsic 200; the verifier rederives its descriptor, substitutions, and effects.
Environment targets contain no main; each manifest schedule path resolves to a
schedule for the root world, has inferred `requires {}`, and may expose only
deterministic owned checked exceptions. The complete reachable graph must obey
the environment effect prohibition independently of its declared surface.

Resolved HIR keeps session identities separate from stable identities.
`PackageNodeId` and `TargetId` are distinct checked-`u64` session newtypes.
PackageNodeId is globally dense in the canonical resolved-package order;
TargetId is dense only within its PackageNodeId in the fixed manifest target
order above. Both
start at zero, never wrap, and report status-1 `IDENTITY001` at the originating
package/target manifest span when the next value cannot be represented. This is
a compiler-resource failure, not an Arche-language result. `HirModuleId` is
`(PackageNodeId, TargetId, local u64)`; workspace `FileId`,
`HirItemId`, and `HirBodyId` are checked globally unique `u64` arena IDs.
For a selected registry release, the verified registry inclusion metadata
carries the exact source `Arche.toml` `[package]` header span. The internal
registry-snapshot commitment version `2` includes that span, and PackageNodeId
exhaustion uses its canonical virtual registry-manifest label rather than substituting a
workspace or dependency span. Source acquisition later revalidates the span
against the included manifest bytes through the assigned package-cache adapter
before any semantic row can be branded.
Modules, types, values, lifetimes, methods, fields/variants, generics,
and lexical locals use distinct scope kinds. Struct/enum constructors and enum
variants bind the value namespace; their nominal declaration binds type.
Resolved paths carry `Res::{Module, Item, Generic, Local, Builtin}` and no later stage
re-resolves a string. The public interface is a graph of module bindings and
explicit re-export edges, not a flattened name list; traversal is cycle-safe
and preserves M27-B visibility. Stable identity construction occurs only after
the required semantic shape is complete and never replaces an arena index.

### 0.3.6 Lifetimes, ownership, cleanup, and unsafe operations

Every reference carries a lifetime even when source elision supplies it. Each
elided input reference receives a fresh lifetime binder. An elided output
reference is accepted only when exactly one input lifetime exists, or for a
method when a borrowed receiver supplies the lifetime; otherwise the output
lifetime must be explicit. Shared references are covariant in lifetime and
referent; mutable references are covariant in lifetime and invariant in
referent; `*const T` is covariant and `*mut T` invariant in `T`; function
parameters are contravariant and results covariant. Nominal generic variance is
inferred structurally from fields and then frozen in the public type interface.
No user variance annotation exists.

Stable semantic type identity and body-local region inference are deliberately
separate. A TypeId lifetime node is `'static`, a declaration binder by de
Bruijn depth/index, or the one payloadless `erased-local` shape marker. The
marker is permitted only in body-local Core types/generic arguments and is
forbidden in declaration signatures, public interfaces, ABI inputs, static
types, and promoted logical values. It carries no body/session ordinal. An
inferred NLL region or LoanId never enters a TypeId, DefinitionId, ABI hash, or
interface hash. Generic Core carries
canonical `RegionFact` proof rows that bind each active reference family of an SSA
value or place-at-program-point to `'static`, a bound lifetime, body-local
LoanId, or verified generator-frame/self origins. MIR lowering constructs the candidate rows, but
`verify_generic_core` rederives them solely from the Core CFG, types,
continuation arguments, move paths, loans, and encoded outlives constraints;
the brand never trusts or reopens MIR. This permits two values with the same structural type to
have different local borrow regions without inventing session-dependent stable
types.

For a named generator, every distinct lifetime free in an initial-parameter
type—including each fresh elided input lifetime—is also a compiler-synthesized
generator-state lifetime binder. These hidden binders follow declared lifetime
parameters in deterministic first-occurrence order, enter the generator
declaration/type template, and cannot be named or supplied by source generic
syntax. A factory call maps them to its fully typed inputs' local region facts.
The produced value keeps the stable structural state TypeId while its result
`RegionFact` records the exact static/bound/local-loan origin substitution. NLL proves
every stored reference outlives that frame. Two calls may therefore share one
structural TypeId while retaining distinct, noninterchangeable borrow-region
substitutions.

Borrow checking uses place-sensitive nonlexical lifetimes over typed MIR. A
loan begins when its reference is created and ends after its final possible use,
including uses through copies, reborrows, calls, unwind edges, and generator
suspension. A mutable loan excludes every overlapping live loan; a shared loan
excludes overlapping mutation. Moving a place deinitializes that move path and
all descendants; moving a field is forbidden when an ancestor has custom
`Drop`. Reassignment may reinitialize a moved path. Every read and drop requires
definite initialization on that incoming edge.

Passing, returning, assigning from, capturing by value, and by-value pattern
binding move a non-`Copy` value unless the operation explicitly borrows or
clones it. `Copy` is an implicit value duplication, is structurally legal only
for compiler-approved fields, and cannot coexist with custom `Drop`. `Clone` is
an explicit ordinary trait call with exact empty effects; an infallible
allocation performed by its implementation still follows the status-1 rule.

Locals, parameters, and temporaries are registered in successful initialization
order. At a lexical or temporary-scope boundary they drop in reverse order.
Function parameters initialize left to right before the body. A custom
`Drop::drop` runs first, followed by initialized fields in declaration order,
the active enum payload in declaration order, tuple elements in index order,
and array/slice elements in increasing index order. Only initialized, unmoved
paths drop. Temporaries drop at the end of their full expression in reverse
creation order; a match scrutinee lives through the selected arm and an `if` or
`while` condition lives through that condition's decision. A temporary directly
borrowed by a reference-binding initializer is extended to that binding's
scope. A suspended generator drops exactly the captures and locals live in its
current state.

Assignment evaluates and fully owns the right-hand side first, then resolves
the destination place, validates that it is writable, drops the old initialized
value, and moves the new value into the destination. If right-hand evaluation
throws or panics, the old destination is unchanged. If old-value destruction
panics, the new value is destroyed during cleanup and is not published.

`Drop::drop` has exact empty `throws {}` and may declare a canonical `requires`
set. It may call a throwing operation only inside a local catch that exhaustively
handles its complete throws set. A type's drop requirement is the canonical
union of its custom method and every transitively droppable initialized field.
Every scope/callable that can implicitly destroy that type must possess and
expose the requirement on all normal and cleanup paths; missing capability is a
compile-time `EFFECT002`, not a skipped destructor. A checked exception never
crosses a drop boundary. Panic begins cleanup; panic during exception or panic
cleanup aborts with status `134`. CTFE and EcsValue require an empty transitive
drop-requires set.

Safe references are nonnull, aligned, dereferenceable for their lifetime, and
point to a valid value. Raw pointers retain allocation provenance. Arithmetic
may produce only an in-allocation or one-past pointer, and dereference requires
a live, aligned, in-bounds allocation holding a valid value. `MaybeUninit<T>` is
the sole safe representation of possibly uninitialized storage. Raw-pointer
dereference/arithmetic, union-style `MaybeUninit::assume_init`, mutable static
access, address exposure/reconstruction, calling any user or trait method whose
signature is `unsafe`, constructing a state through an `unsafe gen fn` factory,
unchecked Pin construction/projection, and calls to
unsafe trusted host intrinsics require a lexically enclosing `unsafe` block.
Declaring the containing function `unsafe fn` changes its call boundary but does
not make its body an implicit unsafe block. `Box::pin` is the sole safe creation
of a pinned `!Unpin` value; safe `Pin::new` additionally requires `T: Unpin`.
Projecting a pinned field is safe only through a compiler-verified structural
projection that cannot move that field. Unsafe code does not relax the type,
lifetime, effect, or cleanup rules. CTFE rejects address exposure entirely.
Unsafe raw-pointer-to-integer `as` exposes address bits and discards provenance;
integer-to-pointer `as` creates a provenance-less pointer that may be compared or
converted back to an integer but never dereferenced or turned into a reference.
The explicit unsafe `original.with_address(address)` operation instead
retains only `original`'s live allocation provenance and is valid only for an
address in that allocation or one-past; it cannot recover a freed or unrelated
allocation. Pointer offset preserves the same provenance.
Raw-pointer equality compares addresses; ordering is unavailable. Merely
forming an invalid raw pointer is not UB, but invalid dereference, reference
construction, use-after-free, invalid-value reads, alias violations, and moving
a pinned `!Unpin` value are UB in unsafe code and impossible in safe code.

### 0.3.7 Traits, effects, closures, generators, and thread judgments

Trait selection is static. A trait contains required methods only. `impl
default<T> Trait<...> for Type ...` marks the entire implementation as the sole
specializable parent; individual default methods do not exist. An impl is legal
only when its package owns the trait or the outermost nominal target type.
Two impls may overlap only when one is marked `default`, the other match set is
a provable strict subset under generic unification and where-predicate
entailment, and no third incomparable impl matches. Identical match sets,
constraint-only guesses the solver cannot prove, and incomparable children are
errors. Inherent impls require ownership of the nominal type and never use
`default`. An inherent impl's canonical head is its target type, alpha-normalized
generic-parameter declarations, and predicates sorted by their complete
canonical bytes. Across every inherent-impl block with a byte-identical head, a
method name may be declared at most once; splitting that head across source
blocks does not create another method namespace or identity. The duplicate is
rejected before stable IDs or interface rows are constructed.
The source marker is stored as the exact `is_default` boolean on the impl's
semantic inventory, DefinitionSignature, DefinitionId shape, ImplRow, and public
coherence row. It is not inferred from overlap or from whether a child currently
exists. Flipping it therefore changes the impl and every owned method identity
and the affected InterfaceHash even when the trait, target, predicates, and
method signatures are unchanged.

The solver is deterministic and terminating for the selected 0.1 fragment. It
canonicalizes an obligation as trait identity, self/input/output type trees,
generic arguments, and a predicate set sorted by each predicate's complete
encoded bytes. A package graph contributes candidate impls in raw
`DefinitionId` order. The worklist is a `BTreeSet` ordered by the obligation's
complete canonical bytes; the least obligation is always processed first.
Unification may introduce only declared generic variables, equality constraints,
and already-canonical child obligations. A candidate is viable only when every
predicate is proven from the environment or recursively solved; cycles succeed
only for compiler-declared structural coinductive traits (`Send`, `Sync`, and
`Unpin`) and otherwise remain unsatisfied. Selection chooses the unique viable
non-default impl, or the unique deepest strictly-more-specific descendant of one
default chain. Zero candidates is unsatisfied and multiple incomparable maxima
are ambiguous. Results are memoized by canonical obligation bytes; source,
filesystem, hash iteration, and discovery order cannot affect the result.

Termination is verified when an impl is declared. Build the directed graph of
trait predicates appearing in impl requirements and condense it into SCCs. An
edge leaving an SCC follows the finite condensation DAG. For every requirement
whose trait remains in the same SCC, rigidly substitute the impl head and prove
that the complete child input tuple is a strict proper structural subterm of the
parent input tuple: at least one nominal/tuple/array/reference/owner constructor
is removed and no child position adds a constructor or larger const expression.
Mutual recursion uses the same tuple measure after canonical trait-order
rotation. An impl failing that universal symbolic check is `TRAIT001`, so
`P<T> -> P<Vec<T>>` is rejected while `P<Vec<T>> -> P<T>` terminates. The sealed
coinductive Send/Sync/Unpin solver instead visits the finite concrete type graph
once. Memoization plus these well-founded rules make an infinite sequence of
distinct obligations impossible.

Predicate entailment is deliberately closed. A lifetime predicate follows from
the reflexive/transitive closure of declared outlives edges plus `'static`
outliving every lifetime. A trait predicate follows from an identical
environment member after substitution or from recursively selecting an impl and
proving all of that impl's substituted predicates. The sole additional sealed
rule is that an indexed canonical environment member `K: EcsKey` entails the
compiler-owned `Eq<K,K>` and `Ord<K,K>` predicates for that same substituted
`K` and furnishes their sealed selections; it entails no other predicate and
never selects a user ImplRow. `T: 'a` follows from an
identical substituted TypeOutlives environment member or, for a concrete type,
when every reference lifetime reachable in its semantic type tree outlives
`'a`; an unconstrained bound type never gains it structurally. There are no supertraits,
associated-type equality, const inequalities, user axioms, or other implication
rules in 0.1 beyond that sealed EcsKey comparison rule. To prove a
specialization child is contained by a parent, replace
the child's parameters with rigid skolems, first-order unify the parent's head
against that rigid head, and require the child environment to entail every
substituted parent predicate. Strictness means the reverse containment proof,
using rigid parent skolems and the same entailment algorithm, fails. A child may
therefore be proper because its head is more concrete, its provable predicates
are stronger, or both; mere syntactic difference without one-way entailment is
not enough. This calculus is the only meaning of “provable strict subset.”

`Copy`, `Clone`, `Drop`, `Fn`, `FnMut`, `FnOnce`, `Send`, `Sync`, `Unpin`,
`From`, `TryFrom`, `Eq`, `Ord`, every operator trait, `EcsValue`, `EcsKey`,
`IntoIterator`, `Iterator`, and `UnwindPayload` are
compiler-known identities rather than name comparisons. Positive user impls
remain subject to coherence; user negative impls are rejected. Compiler-owned
intrinsics provide structural negative facts such as `Rc: !Send + !Sync` and
`App<World>: !Send + !Sync`. Operator traits use explicit input and output type
parameters and have an exact empty `throws` set. Any declared `requires` set is
visible at the operator expression and an implementation may only narrow the
trait method's set.

The auto-trait rules are closed by this table. `all(X)` means every listed
transitive child proves `X`; an omitted condition is unconditional. `!X` is a
sealed negative fact, not a user-overridable implementation.

| Type family | `Send` | `Sync` | `Unpin` |
|---|---|---|---|
| scalar, unit, never, function pointer | yes | yes | yes |
| array, tuple, record, enum, Option, Result, GeneratorState | `all(Send)` | `all(Sync)` | `all(Unpin)` |
| `str` | yes | yes | yes |
| `[T]` | `T: Send` | `T: Sync` | `T: Unpin` |
| `&T` | `T: Sync` | `T: Sync` | yes |
| `&mut T` | `T: Send` | `T: Sync` | yes |
| raw pointer | no | no | yes |
| `String` | yes | yes | yes |
| `Box<T>`, `Vec<T>` | `T: Send` | `T: Sync` | `T: Unpin` |
| `Map<K,V>` | `K: Send, V: Send` | `K: Sync, V: Sync` | `K: Unpin, V: Unpin` |
| `MaybeUninit<T>` | `T: Send` | `T: Sync` | `T: Unpin` |
| `Rc<T>`, `RcWeak<T>` | no | no | yes |
| `Arc<T>`, `ArcWeak<T>` | `T: Send + Sync` | `T: Send + Sync` | yes |
| `Pin<P>` | `P: Send` | `P: Sync` | `P: Unpin` |
| `Atomic<T: AtomicScalar>`, `Ordering`, `AtomicRmw`, `Condvar` | yes | yes | yes |
| `Mutex<T>`, `RwLock<T>` | `T: Send` | `T: Send` | `T: Unpin` |
| `MutexGuard<'a,T>`, `ReadGuard<'a,T>`, `WriteGuard<'a,T>` | no | `T: Sync` | yes |
| `Sender<T>`, `Receiver<T>` | `T: Send` | `T: Send` | yes |
| `JoinHandle<T,E...>` | `T: Send, all(E: Send)` | no | yes |
| `File`, `TcpListener`, `TcpStream`, `UdpSocket` | yes | no | yes |
| `MapIter<'a,K,V>` | `Map<K,V>: Sync` | `Map<K,V>: Sync` | yes |
| closure or generator factory | `all(captures: Send)` | `all(captures: Sync)` | `all(captures: Unpin)` |
| generator state | `all(live frame values: Send)` | `all(live frame values: Sync)` | `all(live frame values: Unpin)` unless self-referential, then no |
| `Caps<C...>` | no | no | yes |
| `App<W>`, `Query<Q>`, `QueryCursor<Q>`, `Commands`, live world/resource/component access | no | no | yes |

Args, Environment, WallClock, MonotonicClock, Threads, Atomics, and
Synchronization capability handles are `Send + Sync + Unpin`. Stdio, Files,
Subprocess, Tcp, and Udp capability handles are `Send + Unpin` and `!Sync`.
All other transparent value records in the virtual interface use the aggregate
row; every other opaque type has no positive auto-trait fact. Guards are never
Send, channels require `T: Send` because queued ownership may cross threads,
and a JoinHandle's exception types are its complete canonical set. User
positive or negative auto-trait impls remain forbidden.

General `Map<K,V>` requires the ordinary statically selected `K: Eq + Ord`.
Those comparison methods have exact empty effect sets and define one total key
order used by insertion, lookup, iteration, and CTFE. ECS-stored maps impose the
stronger sealed `EcsKey` judgment and use compiler-defined structural ordering;
an arbitrary user `Eq` or `Ord` implementation can never establish `EcsKey`.
For a structurally eligible concrete `EcsKey` type or a bound `K: EcsKey`, the
compiler supplies the one sealed structural Eq/Ord selection. The bound case
uses only the indexed sealed entailment above; any overlapping user selection
makes that use ineligible. When matching EcsKey evidence exists, the sealed
selection is mandatory even if an identical Eq/Ord predicate is also present in
the environment; that redundant predicate never creates an alternate
BoundWitness or ambiguity. Thus
every `Map<K,V>` eligible for `EcsValue` was constructed and operated with the
same comparator later used by Canonical Value; comparator identity is never
changed when the map enters ECS storage.
`MapIter<'a,K,V>` holds one shared borrow of its map for `'a` and visits each
live entry exactly once in that
same key order; mutation is rejected while it lives. No insertion history, tree
shape, hash bucket, or capacity affects iteration or CTFE.

Map insertion retains the resident key when `compare` returns zero. Receiver,
incoming key, and incoming value evaluation has completed before search. A
missing-key insertion stages the complete new logical map, then atomically moves
the incoming key/value into it and returns `None`. For an equal key, staging of
the replacement map/value completes while the old map remains unchanged; the
incoming key is then moved into a compiler-owned temporary and destroyed by its
ordinary Drop glue before publication. A panic from that Drop leaves the map
unchanged and cleanup destroys the still-owned incoming value. After successful
key destruction, one total nonallocating commit transfers the resident value to
`Some(old_value)` and installs the incoming value. Thus the resident key's
logical bytes remain observable and the incoming key is dropped exactly once.

Removal searches without mutation. On a hit, one total commit removes the
resident entry, moves its value into the prospective `Some` result, moves its
key into a compiler-owned Drop temporary, shifts the suffix, and publishes the
new logical map. It then destroys that removed key. If its Drop panics, removal
remains committed and cleanup destroys the not-yet-returned value exactly once;
otherwise `Some(value)` is returned. A miss returns `None`. `map.insert` and
`map.remove` conservatively contribute the complete compiler-derived
Drop-requires set of `K` to their caller even when the runtime path performs no
key destruction; comparison and allocation/staging failure before the stated
commit leave the original map unchanged.

M27-C embeds one versioned virtual `arche/core` library interface under the
official registry/package identity. M27-H publishes source packages matching
that byte-exact interface; source resolution never substitutes name strings for
lang-item IDs. Its nominal types are `Option`, `Result`, `String`, `Vec`, `Map`, `MapIter`,
`Box`, `Rc`, `RcWeak`, `Arc`, `ArcWeak`, `Pin`, `MaybeUninit`,
`GeneratorState`, `App`, `Caps`, `Query`, `Commands`, and `AllocError`. Its
compiler-known trait policy is:

Every target receives one non-configurable virtual prelude containing those
nominal names, the trait names in the table below, `panic`, and the scalar/unit/
never keywords. Prelude bindings have the lowest lookup priority: one explicit
same-module declaration or `use` shadows a prelude spelling, but compiler-known
operations still resolve only the embedded identity, never the shadowing name.
No other module, method, macro, host function, or package is imported implicitly.
The embedded interface version and digest enter the toolchain/build identity.

The compiler constructs this table only through the sealed
`VerifiedEmbeddedCoreAuthority`. It owns the fixed `arche/core` PackageId,
interface version and digest, every virtual definition/type/trait/method row,
and the compiler-owned `panic` Generic Core body. It is verified byte-for-byte
against the release-manifest table before semantic inventory or Generic Core
branding; a source package, lock row, test fixture, unsafe caller, or matching
name cannot construct or replace it. The virtual package has no workspace path,
source tree, lock dependency, registry archive, or user target. Its sole
`PackageSource::EmbeddedCore` row carries the same version/digest, has an empty
dependency vector, fixed zero CTFE-root budgets, no initializer root of its own,
and is legal only inside this branded authority. Calls into its bodies execute
under the calling CTFE root's package budget exactly like any dependency body.
The 32-byte `interface_digest` is SHA-256 over
`ARCHE-EMBEDDED-CORE\0 || u32le(1) || u32le(interface_version) ||
u64le(package_bytes_length) || package_bytes`, where `package_bytes` is the
canonical GenericCorePackage printer encoding of the full authority package
with only `PackageSource::EmbeddedCore.interface_digest` replaced by 32 zero
bytes. The stored source field and release-manifest value then equal that digest.
No digest field hashes itself and any other zeroing/omission is invalid.
The authority also installs one immutable compiler-owned synthetic source
snapshot before the shared SourceDatabase is sealed. Its fixed package path,
bytes, digest, root-library ModuleRow, DefinitionRow spans, and public
interface hash are part of the interface digest; it has no host path and cannot
be replaced by user bytes. It alone uses reserved FileId
`0xFFFF_FFFF_FFFF_FFFF`; ordinary source/include acquisition never assigns that
value and reports checked exhaustion before reaching it, so existing canonical
workspace FileIds neither shift nor collide. The embedded GenericCorePackage
and every scope projection retain exactly this non-source-tree SourceFile row;
it sorts after ordinary files and every embedded span must lie within its bytes.
The full embedded package has no TargetRow and uses
`Final{hash}` computed from its fixed public rows. A RootSlice projection uses
`PendingSkeleton`, filters only unreferenced virtual rows, and retains every
ancestor/binding/type/body needed by a referenced row. The synthetic file is
diagnostic authority, not a package source-tree or includable input.

The prelude also exposes `include_bytes` and `include_str`, every capability
type named in a manifest, and every opaque input/result/guard/iterator/OS type
named by the intrinsic registry. Source-to-sealed-operation resolution is closed:

| Source surface | Exact lowering |
|---|---|
| `String::{new,from_str}`; `.len/.push_str/.as_str` | `1..5` |
| `Vec::new`; `.len/.push/.pop/.get` | `10..14` |
| `Map::new`; `.len/.insert/.get/.remove/.iter`; `MapIter.next` | `20..26` |
| `Box::{new,try_new}`; `.as_ref/.as_mut` | `30..33` |
| `Rc::new`; `.clone/.downgrade/.upgrade/.as_ref` | `40..44` |
| `Arc::new`; `.clone/.downgrade/.upgrade/.as_ref` | `50..54` |
| `Box::pin`; `Pin.as_ref/.as_mut` | `60..62` |
| `Pin::new`; unsafe `Pin::new_unchecked` | `pin.make` with checked/unchecked mode |
| `MaybeUninit::{uninit,new}`; unsafe `.assume_init` | `maybe-uninit.make/assume-init` |
| raw-pointer `.offset/.with_address`; raw-pointer/address `as` | `raw.offset/with-address/expose-address` and `pointer.cast` |
| reserved `Caps.take<Capability>()` | `caps.take` |
| prelude `include_bytes/include_str` | `70/71` |
| methods on `Args`, `Environment`, `Stdio`, `Files`, `File`, `Subprocess`, clock, TCP, and UDP handles using the stable method suffix in the registry | `100..118` |
| methods on `Threads`, `JoinHandle`, `Atomics`, `Atomic`, `Synchronization`, Mutex/RwLock/Condvar/channel handles using the stable suffix | `120..136, 140..142` |
| reserved `App.run/resource/resource_mut`; compiler-generated query iteration; reserved Commands postfixes; compiler-generated world init | `200..211` |

Associated calls require the named type identity; method calls require the
listed receiver identity and the exact signature row. The registry's stable
hyphenated name is an internal identity, while the source method replaces `-`
with `_` (`try-new` is `try_new`, `wall-now` is `wall_now`, and so on).
Resource/query/world rows have no forgeable source function: they arise only
from verified system parameters, `for` query lowering, reserved Commands syntax,
or a world init body. `panic` resolves to the compiler-owned verified function
body described below, not an intrinsic. Shadowing a prelude spelling produces
an ordinary user binding and therefore cannot accidentally select these IDs.

The non-IntrinsicId rows above are equally sealed compiler operations. In
semantic-signature notation, `Pin::new<T:Unpin>(&mut T) -> Pin<&mut T>` emits
checked `pin.make`, while `unsafe Pin::new_unchecked<T>(&mut T) -> Pin<&mut T>`
emits its unchecked form. Explicit source arguments use the grammar's
`Pin::new::<T>(...)` and `Pin::new_unchecked::<T>(...)` spellings.
Likewise, signature-notation `MaybeUninit::uninit<T>()` and
`MaybeUninit::new<T>(T)` are called as `MaybeUninit::uninit::<T>()` and
`MaybeUninit::new::<T>(value)` and emit an absent/present
`maybe-uninit.make`, while `unsafe value.assume_init()` consumes the wrapper.
For `*const T`/`*mut T`, `unsafe pointer.offset(delta)` preserves provenance and
`unsafe pointer.with_address(address)` preserves only the receiver's provenance;
the latter is the source spelling for `RawWithAddress`. The `as` forms remain
restricted exactly as specified above. None of these operations is ordinary
user-overloadable method lookup.

For host/thread rows the source receiver and spelling are exact; the receiver
is lowered as the registry signature's first argument:

| IDs | Source spelling |
|---:|---|
| `100..101` | `args.all()`; `environment.get(name)` |
| `102..104` | `stdio.read(buffer)`; `stdio.write_out(bytes)`; `stdio.write_error(bytes)` |
| `105..107` | `files.open(path, options)`; `files.read(file, buffer)`; `files.write(file, bytes)` |
| `108` | `subprocess.run(spec)` |
| `109..110` | `wall_clock.now()`; `monotonic_clock.now()` |
| `111..115` | `tcp.bind(address)`; `tcp.connect(address)`; `tcp.accept(listener)`; `tcp.read(stream, buffer)`; `tcp.write(stream, bytes)` |
| `116..118` | `udp.bind(address)`; `udp.receive(socket, buffer)`; `udp.send(socket, bytes, address)` |
| `120..122` | `threads.spawn(closure)`; `threads.scope(closure)`; `threads.join(handle)` |
| `123..127` | `atomics.new(value)`; `atomics.load(atomic, order)`; `atomics.store(atomic, value, order)`; `atomics.rmw(atomic, operation, value, order)`; `atomics.compare_exchange(atomic, expected, value, success, failure)` |
| `128..136` | `sync.mutex_new(value)`; `sync.mutex_lock(mutex)`; `sync.rwlock_new(value)`; `sync.rwlock_read(lock)`; `sync.rwlock_write(lock)`; `sync.condvar_new()`; `sync.condvar_wait(condvar, guard)`; `sync.condvar_notify_one(condvar)`; `sync.condvar_notify_all(condvar)` |
| `140..142` | `sync.channel_new<T>()`; `sync.channel_send(sender, value)`; `sync.channel_receive(receiver)` |
| `200..202` | reserved `app.run(schedule, caps)`; `app.resource<T>()`; `app.resource_mut<T>()` |
| `203..205` | compiler-generated query open/next/close only |
| `206..209` | reserved Commands `.spawn {}`, `.despawn(...)`, `.add(...)`, `.remove<T>(...)` |
| `210..211` | compiler-generated world resource/spawn initialization only |

`args`, `environment`, and so on are ordinary local names in this table; the
receiver's sealed type selects the operation. Mutability/borrowing is exactly
the registry signature and cannot be relaxed by source spelling. The virtual
core's constructible data definitions are byte-exact in semantic shape:

```text
Option<T> = None | Some(T)
Result<T,E> = Ok(T) | Err(E)
GeneratorState<Y,R> = Yielded(Y) | Complete(R)
AllocError = OutOfMemory
Ordering = Relaxed | Acquire | Release | AcqRel | SeqCst
AtomicRmw = Add | Sub | And | Or | Xor | Exchange | Min | Max
SocketAddress = V4 { octets:[u8;4], port:u16 }
              | V6 { octets:[u8;16], port:u16, flow_info:u32, scope_id:u32 }
OpenOptions { read:bool, write:bool, append:bool, truncate:bool,
              create:bool, create_new:bool }
ProcessSpec { program:String, arguments:Vec<String>,
              environment:Map<String,String>,
              current_directory:Option<String>, stdin:Vec<u8> }
ProcessOutput { status:i32, stdout:Vec<u8>, stderr:Vec<u8> }
IoError { code:i32, message:String }
ProcessError { code:i32, message:String }
ThreadError { code:i32, message:String }
ChannelClosed = Unit
```

Every listed field/variant is public and ordered as shown. The remaining File,
socket/listener/stream, atomic, guard, sender/receiver, and capability values are
opaque sealed handles with no record constructor. Subprocess execution invokes
the program directly without a shell. Wall-clock `now` returns nanoseconds from
the Unix epoch as `u64`; monotonic `now` returns nanoseconds from the driver's
opaque monotonic origin as `u64`, with checked saturation forbidden.

| Trait family | Required semantic method/effects | User positive impl |
|---|---|---|
| `Copy` | no method; all fields structurally Copy and no Drop | allowed and validated |
| `Clone` | `clone(&self) -> Self requires {} throws {}` | allowed |
| `Drop` | `drop(&mut self) -> ()` with declared requires and exact `throws {}` | allowed; one impl maximum |
| `Fn`/`FnMut`/`FnOnce` | `call` with callable's exact signature/effects | compiler-derived only |
| `Send`/`Sync`/`Unpin` | no methods; structural judgment | compiler-derived only |
| `From<Source,Target>` | `from(Source) -> Target requires {} throws {}` | allowed |
| `TryFrom<Source,Target,Error>` | `try_from(Source) -> Result<Target,Error> requires {} throws {}` | allowed |
| `Eq<Lhs,Rhs>` | `eq(&Lhs,&Rhs) -> bool requires {} throws {}` | allowed |
| `Ord<Lhs,Rhs>` | `compare(&Lhs,&Rhs) -> i32 requires {} throws {}`; negative/zero/positive means less/equal/greater | allowed |
| `IntoIterator<Source,Iter>` | `into_iter(Source) -> Iter requires {} throws {}` | allowed |
| `Iterator<Iter,Item>` | `next(&mut Iter) -> Option<Item> requires {} throws {}` | allowed |
| operator traits | one fixed method below; `throws {}` and declared requires | allowed |
| `EcsValue`/`EcsKey` | no method; structural sealed evidence | forbidden |
| `UnwindPayload` | no method; fully owned, sized, `'static`, and transitively reference/raw-pointer/closure/generator/capability/handle/uninitialized-state free; every reachable Drop has `requires {}` | forbidden |

`Ord::compare` result magnitude has no meaning. Its zero result is equivalent to
the selected `Eq::eq` returning true; negative and positive results satisfy
antisymmetry and transitivity and induce one total order. Map search invokes
only `compare` and treats zero as equality, so an inconsistent user Eq/Ord pair
violates the trait's semantic contract rather than selecting a second Map
equality rule. The compiler-sealed EcsKey selection satisfies these laws by
construction.

Operator mapping is closed: unary `-` selects `Neg<Input,Output>::neg`, unary
`!` selects nonoverloadable bool-not or `LogicalNot<Input,Output>::logical_not`,
`~` selects `BitNot<Input,Output>::bit_not`; binary `+ - * / % << >> & ^ |`
select respectively `Add`, `Sub`, `Mul`, `Div`, `Rem`, `ShiftLeft`,
`ShiftRight`, `BitAnd`, `BitXor`, and `BitOr`, each parameterized as
`Trait<Lhs,Rhs,Output>` with the lowercase operation name. `==`/`!=` use `Eq`;
ordering operators use `Ord`; `&&`/`||` are nonoverloadable bool-only CFG
operations. `+=` performs one sequence: evaluate and own the RHS, resolve and
read the destination once, invoke `Add<PlaceType,Rhs,PlaceType>`, fully own its
result, drop the old value, then install the result. RHS/Add failure leaves the
destination unchanged. No other compound assignment exists.

`Self` is a type only inside a trait or impl. A receiver is permitted only as
the first method parameter. Trait-impl methods cannot spell visibility;
inherent methods may. Method lookup considers the nominal type's inherent
methods and explicitly in-scope traits, then applies at most one compiler-added
shared/mutable borrow or reborrow. It performs no user-defined dereference or
numeric conversion. Built-in Box/Rc/Arc/Pin projection is available only through
their declared methods. Multiple viable candidates are an error, never resolved
by declaration order or return type. `for` desugars through the resolved
`IntoIterator` and `Iterator` lang items; the query form uses sealed query
iteration evidence and retains the nonnesting rule.
Lowercase `self` without `::` is a dedicated value expression legal only in a
method body with a receiver and resolves directly to that receiver LocalId;
`self::name` remains a module-rooted item path. Static methods have `Self` type
but no lowercase `self` value.

A callable value with effects `(requires R, throws T)` is a subtype of an
otherwise identical callable type `(requires R2, throws T2)` exactly when
`R` is a subset of `R2` and `T` is a subset of `T2`. Callers must possess every
required capability and must catch or declare every possible thrown type.
Effects are canonical sorted sets of resolved identities. Recursive strongly
connected call components are solved to the least fixed point. Catch removes
only the exhaustively handled thrown types; rethrow adds its payload type back.
Panics and semantic traps are not members of `throws` and are never catchable.

Actual-effect inference first builds the reachable statically identified call graph
with nodes sorted by pre-identity `SemanticBodyKey`, finds SCCs with successors
visited in that order, and processes the condensation graph dependency-first.
Within one SCC it initializes every actual set to the body's explicit
primitive effects, then repeatedly visits members in SemanticBodyKey order and
unions canonical callee actual sets until one complete pass changes nothing. Declared
boundaries are checked only after the fixed point. This monotone algorithm,
rather than source traversal, a stable ID containing the result, or hash-table
iteration order, defines the inferred result.

`SemanticBodyKey` is a session-only value containing PackageId; target kind
(`1=library`, `2=binary`, `3=environment`) and absent/present exact target name;
module segments; declaration-kind tag and absent/present declaration name; the
declaration SourceSpan; body-kind tag (`1=declaration`, `2=closure`,
`3=generator`, `4=world-initializer`, `5=array-length`, `6=repeat-count`,
`7=integer-generic-argument`); checked `u64` ordinal (`0` for declaration/world,
otherwise the canonical expression ordinal); and body SourceSpan. Its byte
encoding is raw PackageId, the `u8`/option/string/list fields under the identity
framing rules, then each SourceSpan as `file_id`, `start_byte`, `end_byte`,
`start_line`, `start_column`, `end_line`, `end_column` in `u64le` order, the
body-kind byte, `u64le` ordinal, and the second encoded span. It contains no
requires/throws set, declaration shape, CTFE result, DefinitionId, or TypeId.
The complete tuple is unique; a collision is `IDENTITY001`.

Before const results exist, each effect member is a session-only
`SymbolicEffectAtom`. Its canonical bytes are effect-kind byte `1=requires` or
`2=throws`, followed by the length-prefixed pre-identity semantic type tree. The
tree uses the exact semantic type tags and field order defined below, except
that every nominal uses tag 29's canonical declaration path and every
const-definition node contains only that canonical path, omitting the pending
result digest and bits. Bound parameters retain their de Bruijn coordinates.
No source spelling, provisional ID, result placeholder, or traversal ordinal
enters an atom. C4 sorts and unions these complete byte strings; an identical
member repeated explicitly is `EFFECT001`.

C4 solves each body's actual least fixed point over SymbolicEffectAtoms, stores
that summary separately from the declared/inferred boundary in typed MIR, and
finalizes only const-independent identities. The CTFE
dependency DAG includes every const result needed by an atom. At each
dependency-ready C5 frontier, the successful receipts replace affected symbolic
const paths with their result digest/bits. A requires atom must then resolve to
one capability DefinitionId; a throws atom finalizes its complete semantic type
to one TypeId. Every affected call SCC is solved again in SemanticBodyKey order
over those kind-correct raw identities. Inferred unions collapse equal IDs of
the same kind. Distinct explicitly declared atoms that finalize to one raw
identity are instead a post-CTFE duplicate and produce `EFFECT001` at the
declaration's second source member. Subset, catch-remainder, and declared-
boundary checks involving a pending atom are final only in this raw-ID pass; no
callable identity or root slice depending on that set is published earlier.

After the affected raw-ID fixed points stabilize, C5 finalizes callable
identities from their declared or inference-selected boundary and continues the
canonical dependency-ready root order. Generic Core then sorts its final records
by BodyOwner as specified below and independently re-solves the complete raw-ID
least fixed point in that order. It must obtain the identical canonical
`actual_requires`/`actual_throws` stored in each body. Those actual sets must be
subsets of the owning named or declared/expected descriptor boundary; each set
is byte-identical only when its corresponding closure/generator discriminator
is `Inferred`, or for a schedule's derived run union or compiler-derived Drop
requires. An internal actual-summary
change inside an unchanged declared superset does not change DefinitionId.
Ordering can change iteration count but never the least-fixed-point value; any
summary/boundary mismatch is `CORE004`, not a reason to rewrite an ID.
Compiler-inserted Drop on every normal/unwind edge contributes its transitive
requires set to the owning body before SCC solving; cleanup is never an effect-
invisible call.
Direct function/method calls, concrete trait selections, statically typed
closure calls, and generator resumes add edges to their known bodies. Generator
factory creation/construction adds no body edge. A FunctionPointerCall and any
BoundWitness trait call have no guessed body edge: the complete requires/throws
sets of their verified callable/trait signature are added directly to the
caller's primitive effects. A SealedEcsKeyComparison has no callable body edge
and contributes its compiler-fixed empty requires/throws sets. Intrinsics
likewise contribute their registry row;
M27-D may replace a witness/known function reference with a concrete edge only
while preserving that declared upper-bound effect result.

Capabilities are compiler-created affine handles. They cannot be constructed,
cloned, placed in `static`, encoded, used as `EcsValue`, captured by a longer
lived value than the driver lease, or obtained except from a driver-supplied
`Caps<...>` projection.
Environment roots reject the complete reachable effect closure containing any
host I/O, clock, entropy, subprocess, networking, thread, raw-address, or
unsafe-host operation, including calls hidden behind traits, closures, or
generators.

Capability effects alone are not the authority for that rejection. Every
GenericCoreBody carries one verifier-derived `environment_forbidden` set of the
forbidden operations occurring directly in that body, excluding calls:
`NondeterministicHostEffect`, `RawAddressObservation`, `UnsafeHostCall`, and
`Thread`. The first includes every args/environment/I/O/subprocess/clock/network/
atomic/synchronization capability operation; `Thread` additionally marks thread
creation, scoped spawn, join, or other thread-control operation. Raw address exposure/reconstruction
marks `RawAddressObservation`, and any trusted-runtime entry classified as an
unsafe host call marks `UnsafeHostCall`. The vector is unique in the listed tag
order and is recomputed from instructions, intrinsics, unsafe operations, and
their sealed registry rows. It is an implementation safety summary only: it
does not enter source `requires`/`throws`, DefinitionId, TypeId, InterfaceHash,
callable ABI/effect variance, or the effect SCC.

Environment validation starts from each manifest-selected reset/step/self-play
schedule and computes the exact closed call graph using direct selections,
closure/generator descriptors, Drop edges, and the finite function-pointer
points-to proof defined below. It unions the direct summaries of every reachable
body and rejects any nonempty result as `CAPABILITY001`. An indirect target that
cannot be proven finite is also `CAPABILITY001`; a permissive function-pointer
signature never hides a forbidden implementation body.

Manifest capability keys and virtual-core identities are closed:

| Manifest key | Capability type/effect identity |
|---|---|
| `args` | `Args` |
| `environment` | `Environment` |
| `stdio` | `Stdio` |
| `files` | `Files` |
| `subprocess` | `Subprocess` |
| `wall-clock` | `WallClock` |
| `monotonic-clock` | `MonotonicClock` |
| `tcp` | `Tcp` |
| `udp` | `Udp` |
| `threads` | `Threads` |
| `atomics` | `Atomics` |
| `synchronization` | `Synchronization` |

Manifest keys are exact lowercase ASCII and appear once in lexical byte order.
The compiler maps them to the embedded DefinitionIds, sorts the semantic set by
raw identity, and constructs exactly that `Caps<...>` type and main `requires`
set. The only projection is the reserved expression
`caps.take<Capability>() -> Capability`. Its type argument must be one declared
member; it performs a sealed `caps.take` move of that member, so a second take is
`MOVE001`. Caps has no fields, constructor, Clone/Copy implementation, or user-
callable projection method. The source variable may have any name; resolution
is by its sealed Caps type.

`Caps<...>` itself is `!Send + !Sync`; crossing a thread boundary requires an
explicit projection and move/borrow of the narrow handle. Args, Environment,
WallClock, MonotonicClock, Threads, Atomics, and Synchronization handles are
compiler-marked `Send + Sync`. Stdio, Files, Subprocess, Tcp, and Udp handles
are `Send` but not `Sync`; one may be moved into a network/I/O thread, while
shared access requires an explicit synchronization owner. These sealed facts
cannot be changed by user impls and do not make capability values static or
serializable. A moved narrow handle contains no borrowed source value and may
satisfy the `'static` closure bound while its driver lease is live. Every
unscoped `JoinHandle` is affine and must be joined on every path before the
entrypoint returns or observation begins; detach is unavailable. This proves a
network thread cannot outlive or retain its driver capability.

Closure capture inference selects the narrowest mode required by all uses:
shared borrow, mutable borrow, or move. `move` forces ownership capture without
changing later use classification. A closure implements `Fn` when no capture is
mutated or moved, `FnMut` when captures may be mutated but not consumed, and
only `FnOnce` when a capture may be consumed. Captures initialize by first
source use and drop in reverse initialization order. Each capture receives a
checked one-based ordinal by the first capturing-use expression's resolved-body
preorder, then projection evaluation order, then the captured binding's lexical
declaration order. The stable closure/generator type encodes only this dense
ordinal, mode, and semantic type; it never encodes Core Place, LocalId, source
span, or a host address. The Core descriptor separately retains the resolved
Place for execution/body hashing and must use the same dense ordinal.

Generator-factory capture transfer uses the same classes. A shared-borrow
capture is copied into each returned frame and permits `Fn`; a mutable-borrow
capture is reborrowed into one live returned frame and requires `FnMut`; an
owned non-Copy capture moved into a returned frame requires `FnOnce`. Copying an
owned `Copy` capture does not by itself force `FnOnce`. The returned generator's
lifetime carries every capture borrow, so the factory cannot move or drop while
a frame borrows it and a mutable factory cannot construct a conflicting second
frame. The frame owns the transferred initial arguments. Their irrefutable
parameter patterns are installed before first resume and their live fields use
the ordinary reverse-initialization/drop rules.

A generator is a stackless affine state machine. The notation
`Generator<Resume, Yield, Return> requires R throws T` in this section is
schematic and non-source-spellable; semantic tag 28 below is the sole concrete
state-type authority. `resume` requires
`Pin<&mut G>` and returns `GeneratorState<Yield, Return>` or propagates a checked
exception in `T`. The first resume value is consumed but is not observable by
the body and is dropped before body entry; its transitive drop requirement is
part of the generator's requires set. After a suspension, the `yield` expression evaluates to the next
resume value. Completion or uncaught throw makes the generator terminal;
resuming a terminal generator is a noncatchable `GENERATOR_COMPLETE` trap.
A borrow may cross `yield` only when its owner outlives the generator and the
generated state remains valid while pinned. Self-referential generator states
are `!Unpin`. Query, component, resource, command-buffer, capability, and live
world borrows may never cross `yield`.

The reserved postfix `pinned.resume(value)` is the only source-level resume
operation. Its receiver must be exactly `Pin<&mut G>`, its argument must match
`G::Resume`, its result is `GeneratorState<G::Yield,G::Return>`, and its exact
requires/throws sets are those of G. It lowers only to `GeneratorResume`; it is
not ordinary name-based method lookup and cannot be shadowed. Auto-borrowing or
auto-pinning an unpinned generator is forbidden.

`Send` and `Sync` are structural compiler judgments repeated after
monomorphization. Spawning an unscoped thread requires a `Send + 'static`
closure, result, and every checked-exception payload. Intrinsic 121 is the sole
scoped-thread operation: it immediately spawns its FnOnce argument and joins
that same child before returning, exposes no handle or detach point, and may
capture non-`'static` borrows only when each is proven to outlive the complete
intrinsic call. The closure/captured transfers are Send; a returned reference
may name only storage borrowed from outside the child frame with a proven
remaining lifetime. The child result or checked exception is moved back before
the call returns; creation/join failure throws ThreadError, while child panic or
trap is process-fatal. Sharing
`&T` requires `T: Sync`, moving `T` requires `T: Send`, and no live `App`, world,
query, component/resource borrow, or non-`Send` capability handle may cross a
thread boundary. Atomic orderings are exactly Relaxed, Acquire, Release, AcqRel, and
SeqCst; loads reject Release/AcqRel, stores reject Acquire/AcqRel, failed CAS
rejects Release/AcqRel and cannot be stronger than success, and Consume is a
syntax/type error. These are static contracts only; OS execution belongs to
M27-G.

### 0.3.8 Deterministic compile-time evaluation

Required closed CTFE roots are exactly every const initializer, every static
initializer, and every fixed-array length, array-repeat count, or integer
const-generic argument whose canonical expression contains no bound generic
parameter.
An `include_bytes`/`include_str` call is part of its enclosing root, not a second
root. Referenced consts form dependency edges; a cycle is `CTFE006`. Roots are
ordered dependency-first, then by canonical package/target/module/declaration
path and source span. Each root receives a fresh copy of the declaring package's
sealed effective manifest step, depth, and heap budgets from its semantic
inventory row. A dependency is evaluated and metered
exactly once as its own canonical root. A consuming root reads the sealed,
promoted predecessor value and does not replay or recharge the predecessor's
evaluation; it pays only the ordinary current-body instruction, copy/borrow, and
owned-value construction charges caused by that use. Thus every root has one
fresh independent budget and one receipt, while a dependency fan-out cannot
make the dependency itself pass for one consumer and fail for another. Arche
0.1 provides no persistent CTFE replay cache, and a receipt can never substitute
for evaluating its own root key. Resource failures and language failures are
never negative-cached. Root order and cache warmth therefore cannot change
acceptance or diagnostics.

C4 assigns each root a session-stable `CtfeRootKey`, never a provisional stable
ID. Its exact fields are package ID; target kind (`1=library`, `2=binary`, or
`3=environment`) plus an absent name for library or the exact target name for
binary/environment; module segments; root kind (`1=const-initializer`,
`2=static-initializer`, `3=array-length`, `4=repeat-count`, or
`5=integer-generic-argument`); one-based same-kind expression ordinal; and
SourceSpan. The package/target/module/span/ordinal tuple is the complete session
identity, including for an unnamed impl owner, and does not contain a
declaration shape, current result, or provisional DefinitionId. The typed-MIR
dependency DAG includes every referenced const and every const needed to
finalize any row or identity in the exact transitive slice, including reachable
callees/drop bodies, non-invoked FunctionRef/DataRef types, and every trait/impl
candidate in its complete coherence universe. A root is ready only after all predecessor results exist; a cycle is
`CTFE006` before any member executes.

The one CtfeRootKey byte encoding used for ordering, receipt matching, and trace
preimages is, in field order: raw 16-byte PackageId; one `u8` target-kind tag;
for binary/environment only, `u64le(target_name_byte_length)` and exact UTF-8
target-name bytes; `u64le(module_count)` followed by each module segment as
`u64le(byte_length)` and exact UTF-8 in path order; one `u8` root-kind tag;
`u64le(ordinal)`; then SourceSpan as FileId, start byte, end byte, start line,
start column, end line, and end column, each `u64le`. Library encodes no
target-name length or bytes.
All lengths/counts are checked, target/root tags are exactly those above, the
ordinal is nonzero, and an invalid UTF-8 string, trailing byte, or conditional
field is rejected rather than normalized.

C5 lowers one ready root plus its complete reachable callable/drop/trait body
closure to `VerifiedGenericCore<RootSlice>`. Every stable ID present in that
slice is already final from predecessor results. The current root body is keyed
by its `CtfeRootKey`, so a containing declaration whose identity depends on that
root's result is not assigned or smuggled in as a provisional DefinitionId.
The slice also carries the complete coherence/selection universe needed by its
trait calls and the immutable predecessor result rows it reads. The ordinary
Generic Core verifier proves the slice before evaluation. A successful result
extends the dependency environment; only after every root succeeds are all
remaining identities finalized and the complete workspace Core lowered and
verified. This dependency-ready slicing is the only permitted solution to the
identity/evaluation ordering; neither HIR/MIR interpretation nor placeholder
hashing is allowed.

For every executable call whose type/lifetime substitution is closed, RootSlice
verification also constructs one sealed `ClosedRegionView`. A
`ClosedBodyContext` contains the raw PackageId/body plus its complete closed type/lifetime/
integer-const generic arguments and an exact present callable signature;
initializer/CtfeRoot contexts require it absent. Views sort by the
complete caller-context bytes, canonical caller ProgramPoint, then the complete
target tag/payload. Body targets include their complete context; Intrinsic
targets include every generic/const argument and result type; GeneratorConstruct
targets include descriptor, generic arguments, call trait, and state type. The
view set is exactly the reachable closed region-affecting site/target graph; a
function-pointer site has one Body view for every retained closed target.
Duplicates, omitted contexts, or a view for a parameterized unexecuted template
are `CORE003`. The ProgramPoint belongs to caller.body.

A view expands bound bundles to concrete families without flattening body-local
origin namespaces. `ClosedRegionFact::Caller` subjects are resolved only in the
caller context; `Target` subjects are legal only for a Body target and resolve
only in that target context. Intrinsic and GeneratorConstruct views contain
Caller facts only. `LocalLoan` names Caller or Target explicitly and its
package/body must equal that context. `Incoming` remains a symbolic source/path/family token
that evaluator invocation binds to the current dynamic caller frame; recursion
therefore reuses one structural view with fresh per-frame origins even when both
frames contain LoanId zero. GeneratorSelf is qualified the same way. Facts and
origins use their schema variant order and reject any cross-body ID collision.
The view creates no stable identity, cloned body, dynamic frame ID, machine
instance, or layout. CTFE body frames and sealed intrinsic/construction
operations accept only the matching view. A ParameterizedTemplate may retain
bundles but cannot execute. M27-D performs the identical expansion and
verification when it later creates real instances.

`evaluate_ctfe_root` is the sole constructor of the compiler-sealed,
nonserializable `VerifiedCtfeResult`. It returns a receipt only after executing
the exact `Arc<VerifiedGenericCore<RootSlice>>`, from the exact retained
`Arc<SourceDatabase>`, under the declaring package inventory row's exact sealed
effective budgets; completing
all cleanup; passing promotion; and computing the logical value/digest. The
receipt owns those two branded authorities plus the root key, package source-
tree digest vector, budgets, result TypeId/value/digest, charged-step count,
maximum depth, peak/final logical heap, and a 32-byte commitment to the complete
accounting transition trace. It also owns the exact validated DataRef-provenance
side table for its logical value. Failure produces no receipt.

Every receipt `source_trees` vector is unique and sorted by raw PackageId bytes;
each row is `(PackageId, SHA-256 source-tree digest)` and covers exactly the
ordinary packages whose source/include rows occur in the retained RootSlice;
the embedded-core synthetic diagnostic file is excluded. The current root
package is present. Missing, duplicate, unordered, unrelated, or digest-
mismatched rows prevent receipt construction.

The trace commitment is unkeyed BLAKE3 over `ARCHE-CTFE-TRACE\0 || u32le(1) ||
CtfeRootKey bytes || u64le(event_count)` and the ordered events. Each event is a
one-byte `u8` tag. A step-charge event is tag `1`, followed by one `u8` charge class
(`1=instruction`, `2=terminator`, `3=body-entry`, `4=collection-element`,
`5=unicode-scalar`, `6=byte`, `7=predecessor-value-node`),
`u64le(context_length) || context_bytes`, the canonical ProgramPoint encoding,
the seven-u64 SourceSpan encoding used by CtfeRootKey, and a `u64le` visited
ordinal (`0` for a noniterated charge);
tags `2=depth-enter` and `3=depth-exit` are followed by that dynamic frame's
same length-prefixed context encoding and `u64le(resulting_depth)`; tags `4=heap-stage`,
`5=heap-publish`, `6=heap-release`, and `7=heap-transfer` are followed by
`u64le(allocation_id)`, `u64le(charged_bytes)`, and
`u64le(resulting_live_bytes)`. `context_bytes` is the exact
ClosedBodyContext encoding: raw 16-byte PackageId, `u64le(CoreBodyId)`,
`u64le(generic_argument_count)`, then for each argument in declared order
`u64le(argument_length) || argument_bytes` under the identity framing above,
followed by one `u8` signature option tag `0`, or tag `1` plus raw 16-byte TypeId.
ProgramPoint's one-byte tags are exactly `1=BlockEntry`,
`2=BeforeInstruction`, `3=AfterInstruction`, `4=BeforeTerminator`, and
`5=Edge`; its BlockId, instruction index, and successor ordinal payload fields
follow schema order as `u64le`.
Every step-charge site is fixed. Body-entry uses the entered context,
`BlockEntry{entry}`, that GenericCoreBody's span, and visited ordinal zero. An
instruction uses the current context, `BeforeInstruction{block,index}`, its
Instruction span, and ordinal zero; a terminator uses the current context,
`BeforeTerminator{block}`, its Terminator span, and ordinal zero. Collection-
element, Unicode-scalar, and byte visits use the context, ProgramPoint, and span
of the instruction or terminator whose operation initiated the hidden traversal.
A predecessor-value-node visit uses its containing CtfeResultRef Const
instruction's context, BeforeInstruction point, and span. For each initiating
operation and each iterated charge class separately, visited units are numbered
one-based in their mandated encounter order; nested helper/comparator traversal
and repeated internal comparisons continue that operation's counter rather than
resetting it. No other context, point, span, or zero-based iterated ordinal is
valid. The initiating class-1 or class-2 event always precedes all of that
operation's hidden class-4 through class-7 events.
These events are exactly the state transitions specified below; no host timing/
allocation event enters them.

RootSlice predecessor inputs and CompleteWorkspace result inputs are private
references to these receipts, never caller-supplied value rows. The Core
verifier projects and recomputes each receipt's row, verifies that its retained
slice/root/source/budgets match the current dependency graph and exact declaring
package inventory row, and rejects a raw,
stale, mismatched, or duplicated receipt. In-process sharing preserves the same
immutable receipt object; reconstitution from a digest, text, cache file, test
fixture, or unsafe candidate is unavailable in 0.1.

CompleteWorkspace verification additionally runs the sole canonical
`project_root_slice(workspace, receipt)` operation for every receipt. It locates
the final root by the exact package/target/module/kind/ordinal/span key, follows
the same executable call/drop/trait-selection closure and coherence universe,
selects the same Normal-edge package closure, and scope-projects each retained
PackageProvenance dependency vector by the RootSlice rule above. It retains the
identical sealed inventory and embedded-core authority Arcs, package version/
source provenance, and package CTFE budgets, filters
files, modules/bindings, data, types, definitions, traits/impls, closure/
generator/world/query/type-const/schedule descriptors, bodies, static bindings,
and predecessor receipts to exactly the rows required by that closure, and
drops every TargetRow because RootSlice target authority is symbolic inventory.
It independently projects the embedded-core rows through the branded exception
above; they never acquire a source-tree or dependency row. It also drops the
CompleteWorkspace-only `environment_points_to` proof vector after independently
verifying it, because RootSlice has no environment-target proof field.
Module ancestors and every binding/re-export row needed to resolve a retained
definition remain; no unrelated module/binding row remains. It then reassigns
all dense DataId, ClosureId, GeneratorId, CoreBodyId, and cross-references in the
slice's canonical order. Body-local BlockId/ValueId/LocalId/MovePathId/LoanId
orders are already canonical and must compare directly.

Projection applies exactly these normalization differences after filtering. For
every retained package, CompleteWorkspace verification first independently
recomputes and matches `Final{hash}` against the sealed inventory/public graph,
then rewrites it to `PendingSkeleton`; it never copies, guesses, or ignores the
hash. The final root's Definition or TypeConst BodyOwner is rewritten to the
receipt's CtfeRoot owner; the current result receipt/result_key is removed from
the projected RootSlice wrapper; and the one final enclosing DefinitionRow/
TypeConstDescriptor that could not exist before the result-dependent ID is
dropped only when the retained slice lacks it. That dropped row's kind, semantic
path, source span, expression, body link, and final ID are independently
rederived from the same root key/HIR/receipt before removal. The projection
retains the exact predecessor receipt objects, static bindings, and receipt
DataRef-provenance side tables. Other than the defined package/dependency/target/
interface/environment-proof filtering and these owner/result rewrites, no opcode, operand, CFG
edge, body/effect set, selected impl/evidence, DataRef/FunctionRef/CtfeResultRef,
data bytes/type, source span, provenance field, or descriptor field may change.
After normalization, every retained record is structurally equal in field order
to the receipt's branded RootSlice; only unrelated workspace rows may remain
outside the projection. A mismatch prevents CompleteWorkspace branding, final
interface construction, and lock publication.

A type-level const expression containing a bound integer-const parameter is a
verified parameterized template, not an executable C root. C5 verifies its
restricted typed Core, dependencies, effects, and budget-independent language
semantics while retaining its bound nodes. M27-D substitutes a closed concrete
argument, assigns the declaring package inventory row's same sealed effective
budgets, evaluates it
through a dependency-ready `VerifiedGenericCore<RootSlice>` under this identical
CTFE authority before final instance/layout verification, and
rejects a failing instance without publishing an object. A template is never
guessed, host-evaluated, or branded as an evaluated value in Generic Core.

CTFE interprets only `VerifiedGenericCore<RootSlice>` with explicit heap-managed
frames; it never uses host recursion or native pointer identity. Each root starts
with the exact accounting state `charged_steps=0`, `depth=0`, `live_heap=0`,
`peak_heap=0`, `event_count=0`, and allocation-ID state
`{ next=1, exhausted=false }`; allocation ID zero is invalid. After checking that
depth one is permitted, the evaluator's first event is `depth-enter` for the
root body with resulting depth one, followed by its body-entry step charge
before the first Core instruction or terminator. A call, closure invocation,
generator resume, or drop invocation applies the same order at the next depth
and consumes that depth until it returns, yields, raises, or unwinds; a suspended
generator retains no call depth. A frame first completes its required cleanup
and then emits `depth-exit` with the resulting caller depth. On a successful
root Return, its terminator and cleanup therefore precede `depth-exit(..., 0)`,
and promotion, heap-transfer events, the zero `final_heap` sample, and receipt
sealing occur afterward in that order. A checked compiler-resource or host
failure emits no transition that did not complete and produces no receipt.
Before any transition that would emit an event, the evaluator checks
`event_count < u64::MAX`. Failure is `CTFE004` before the transition, action,
event, or other mutation. Otherwise the completed transition appends exactly
one event and increments event_count with checked addition; reaching
`u64::MAX` is valid, while the next prospective event fails without wrapping.
Generator factory creation and construction are ordinary total instructions:
each charges its one instruction step, adds no call depth, and adds no logical-
heap allocation beyond separately owned child values already charged by their
own operations. One step is charged before
every Core instruction, terminator, callable/drop body entry, logical collection element
visited by an intrinsic, Unicode scalar decoded by a string intrinsic, and byte
compared/copied/transformed by a byte intrinsic. Block-parameter binding and
edge transfer add no step beyond the terminator. Statically known charges are
reserved before the operation publishes an effect; dynamic work charges before
each element. A budget is checked before the charged action, so a limit of zero
executes nothing and exhaustion publishes no partial current operation.
Optimization may not change accounting because CTFE consumes canonical
pre-optimization Core.

A Const instruction materializing CtfeResultRef first reserves the complete
fresh logical-heap footprint of its receipt value, then charges its ordinary
instruction step, one predecessor-value-node step for every logical value node
in preorder, and one byte step for every String or DataRef payload byte. All
charges occur before publishing the result; failure leaves no partial value or
allocation. This is the consumer's value-use work and never recharges execution
of the predecessor root.

For that materialization, owned allocating nodes are exactly String, Vec, Map,
and Box nodes and are enumerated by CTFE Logical Value preorder (Map key then
value); nonallocating nodes receive no allocation ID. After the complete checked
footprint and all step charges succeed, consecutive monotonic allocation IDs
are reserved in that preorder. `heap-stage` events occur in preorder;
`heap-publish` occurs in DFS postorder so children become owned before
their parent and the root publishes last. A host failure during staging releases
already staged nodes in exact reverse stage order and publishes no Core value;
budget/step failure occurs before any ID or heap event. Later ordinary Drop uses
the value's normal semantic child/drop order rather than this construction
order. No host traversal or allocator order can affect the trace.

The logical heap uses monotonic checked `u64` allocation IDs. Reserving zero IDs
leaves its allocation-ID state unchanged. Reserving `N>0` while not exhausted
first checks `N - 1 <= u64::MAX - next`; failure is `CTFE004` before an ID, heap
event, or mutation. Success assigns the consecutive range `next..=last`; if
`last == u64::MAX` it sets `exhausted=true`, otherwise it sets `next=last+1`.
Any nonzero reservation while exhausted is the same pre-mutation `CTFE004` and
the counter never wraps. Its heap budget is the maximum simultaneously live
logical bytes. Every owned String, Vec, Map, Box,
Rc control block, and Arc control block creates exactly one logical allocation,
including an empty String/Vec/Map. Each allocation charges a fixed 16-byte
accounting header plus its logical payload. Inline width is exact: integer/float
width is its language width, `bool` is one, `char` is four, entity/reference/raw
pointer/owned-handle/function-pointer is eight, and a zero-sized inline value is
zero. Structural widths never depend on the current logical value: tuple,
record, closure, and generator-factory widths sum their ordered field/capture
types without padding; an array multiplies element width by length with checked
u64; an enum is an eight-byte tag plus the maximum variant-payload width; and a
generator is the maximum sum of descriptor captures, initial arguments, and
frame locals live in state zero or any suspension state. `MaybeUninit<T>` has
exactly T's structural width whether initialized or not. Pin delegates to the
pinned owner. Owned descendant allocations exist only for active/initialized
children, but their containing allocation's structural payload width is fixed,
so enum changes, generator resumes, and in-place replacement require no hidden
re-accounting. These abstract widths apply recursively inside
Box/Vec/Map/Rc/Arc, stop at eight-byte owner/reference handles, and never use
native frame/layout size.

String payload is exact UTF-8 byte length. Vec payload is the sum over elements
of `max(1, inline_width(element))`; separately owned children keep their own
allocations. Map payload is the sorted-entry sum of
`max(1,inline_width(key)) + max(1,inline_width(value))`; key comparisons charge
only the initiating Core terminator plus the exact Map/sealed-comparator
traversal fixed below and add no hidden heap bytes. Box
payload is `inline_width(T)` plus separately owned descendants. For Rc/Arc the
allocation's one 16-byte accounting header is also its logical control header;
there is no second header. Its payload is the pointee structural width, and
cloning adds no allocation.
Pin adds no charge beyond its owner. Growth allocates a new header plus the new
logical payload before releasing the old header/payload; moved descendants are
not duplicated. Pop/remove/drop release an owned descendant only when ownership
actually ends. Rc/Arc pointee payload drops at strong-count zero while the
16-byte control header remains charged until total weak-count zero; leaked strong
cycles retain their complete charge. All sums and the live total use checked
`u64`; overflow is `CTFE004` before mutation.
Neither native layout nor allocator capacity affects the result. Checked
logical-size overflow, allocation-ID or event-count exhaustion, and heap-budget
exhaustion are `CTFE004`; step and depth exhaustion remain `CTFE002` and
`CTFE003`. All are compiler-resource errors and are not catchable or memoized
as language failures. A host
allocation/address-space failure is instead an infrastructure failure with
status `1`, not a CTFE heap-budget result.

The abstract String, Vec, and Map allocation always has exactly its current
logical payload size; it has no capacity. An operation that increases payload
stages one replacement allocation (header plus complete new payload) while the
old allocation remains live, publishes the new owner, then releases the old
allocation. An operation that shrinks payload transfers any returned owned
value first and then reduces the existing logical charge without a replacement
allocation. Failed staging leaves the old value and charge unchanged. Map is an
abstract sorted vector. `get`, `insert`, and `remove` perform lower-bound binary
search with `lo=0`, `hi=len`, `mid=lo+(hi-lo)/2` rounded down, exactly one Ord
comparison per visited midpoint, and the ordinary equality result of that same
comparison. Insertion/removal then shift the logical suffix in key order and
follow the resident-key, Drop-temporary, publication, and panic rules above;
CTFE does not substitute a host collection or silently replace an equal key.
Iteration visits indices `0..len` and performs no hidden comparisons. These
rules, rather than a host collection or growth strategy, define exact step and
peak-heap budget boundaries.

Per listed H/I intrinsic, hidden iteration charges are closed. String new/len/
as_str add none; from_str and push_str visit each output UTF-8 byte once in
order. Vec new/len add none; push visits each existing element then the staged
element, pop visits the removed element when present, and get visits the one
selected element only when in bounds. Map new/len/iter add none beyond the
search/iteration rules above; each lower-bound midpoint visits its resident
entry once, insert/remove additionally visit each shifted suffix entry once,
get adds only those midpoint-entry and comparator visits, and next visits one
entry when present. Box/Rc/Arc/Pin creation visits the owned input once;
reference-count changes and borrow projections add no hidden element visit.
include_bytes visits every
input byte once; include_str visits every input byte and every decoded Unicode
scalar once. User calls, comparisons, allocations, moves, and drops still pay
their ordinary Core charges in addition to these intrinsic visits.

The compiler-sealed EcsKey comparator has one exact additional CTFE traversal.
An explicit `TraitCall` whose selection is `SealedEcsKeyComparison` uses that
Invoke terminator's ClosedBodyContext, `BeforeTerminator` ProgramPoint, and span
as the parent of every hidden charge. A comparator invoked internally by a Map
intrinsic instead uses the Map Invoke terminator's context, point, and span; all
binary-search comparisons and any later suffix visits in that one Map operation
share its continuing per-charge-class ordinal streams. Immediately before each
lower-bound resident key is inspected, that Map operation emits its one class-4
midpoint-entry visit; the nested comparator's structural and String visits then
continue the same class streams. Scalar, unit, variant-
ordinal, and sequence-length decisions add no hidden charge. String comparison
charges one class-6 byte visit immediately before each paired UTF-8 byte is read
and compared in lexicographic order. It stops after the first differing pair;
an equal prefix is followed by the length decision with no further visit.
Array, tuple, record, equal-variant enum payload, Box, and Vec comparison charges
one class-4 collection-element visit immediately before descending into each
paired child in comparator order and stops after the first nonzero child; an
equal sequence prefix is followed by its length decision without another
visit. Map comparison charges one class-4 collection-element visit immediately
before each paired entry, then compares its key and, only when that is equal,
its value recursively; it does not add separate wrapper visits for the key or
value, and entry count decides an equal prefix without another visit. Nested
children and String bytes add their own class-4/class-6 visits under the same
parent and continuing one-based class ordinal. Structural Eq performs this
identical traversal and tests whether its result is zero. A step/event-count
failure occurs before the affected child, entry, or byte is read, before a
comparison result exists, and before any Map mutation or other publication.
These metering rules apply identically to explicit sealed comparisons and every
Map-internal comparison; runtime comparator semantics remain those fixed above.

Rc and Arc share one logical count machine with unsigned 64-bit counters. `new`
starts at `strong=1`, `explicit_weak=0`, plus one implicit weak while strong is
nonzero. `clone` increments strong; `downgrade` increments explicit_weak;
`upgrade` returns None without mutation at strong zero and otherwise increments
strong and returns an owner. An increment whose resulting counter would exceed
`u64::MAX` produces `ReferenceCountOverflow` before mutation. While the implicit
weak exists, downgrade also traps before an explicit count that would make the
total weak count overflow. Dropping an owner decrements strong first. When it
becomes zero, a cleanup guard drops the pointee exactly once, then removes the
implicit weak; the control header is released immediately if explicit_weak is
also zero. If pointee Drop panics, the guard still performs that one implicit-
weak transition during unwind. Dropping a Weak decrements explicit_weak and
releases the header only when both counts are zero. Clone/downgrade/upgrade/Drop
never allocate or change the logical payload charge; Rc uses ordinary sequential
counts and Arc uses the runtime atomic ordering fixed by its sealed implementation,
but CTFE observes this same sequential logical transition order.

CTFE may temporarily use abstract Box/String/Vec/Map/Rc/Arc/Pin values, closures,
and generators, but a CTFE result eligible for later M27-D promotion must be a
finite canonical value tree with no reference, raw pointer, capability,
allocator identity, shared-owner graph, closure, generator factory/frame, thread/I/O
handle, or mutable interior state. The sole reference exceptions are immutable
`'static` references to compiler-owned decoded source-literal DataRows and
included-input DataRows; M27-D encodes each through a checked relocation rather
than an ambient address. Thus `const S: &'static str = "x"` is promotable.
An uncaught checked exception, panic, or semantic trap is a const-evaluation
diagnostic. Drop runs exactly as at runtime.

Promotion is part of C5 required evaluation, not a later best-effort step. If a
root's declared result type is structurally incapable of promotion, `CTFE007`
is emitted before executing its initializer. Otherwise evaluation completes,
then a deterministic depth-first logical-value walk validates the active value
graph and permitted DataRefs before the result is cached or branded. Dynamic
failure records a pending `CTFE007` candidate and first drops the complete
initialized result in ordinary reverse cleanup order. If that cleanup panics,
traps, aborts, exhausts a compiler budget, or encounters a host/infrastructure
failure, the cleanup failure takes precedence and suppresses CTFE007. Only
successful cleanup emits CTFE007 at the root with the first failing child path
as a note.
No CTFE result digest, InterfaceHash, lock update, object input, or successful
target is produced for that root.

On successful promotion, the verified Return path has already dropped every
nonresult live local. The promotion walk constructs the immutable address-free
logical tree while the moved root result remains owned by the evaluator. Its
reachable logical allocations then transfer to the sealed receipt in CTFE
Logical Value preorder: each emits one `heap-transfer` event and leaves the
evaluator live-byte total without invoking Arche Drop. The receipt owns logical
bytes, not allocation IDs, so a later CtfeResultRef creates fresh allocations.
After all transfers, `final_heap` is sampled exactly as the remaining live
logical bytes and must equal zero for every successful root. The selected 0.1
CTFE surface has no `forget`, interior-mutation primitive, or other operation
that can construct an unreachable strong Rc/Arc cycle; Weak edges do not retain
storage. Every nonresult owner is therefore released by verified cleanup, and
every result-owned allocation has transferred to the address-free receipt. A
nonzero sample prevents receipt construction and a fabricated receipt with
nonzero `final_heap` fails verification. `peak_heap`, zero `final_heap`, event
count, and trace digest are sealed only after this sequence. Interpreter arena
teardown then releases host memory nonsemantically, emits no event, and invokes
no Arche Drop. Promoted String/Vec results are the normative nonzero-transfer/
zero-final-heap case.

`include_bytes("path")` and `include_str("path")` accept only a literal portable
path relative to the declaring package root. For exact input length `N`, their
types are respectively `&'static [u8; N]` and `&'static str`; neither exposes an
address. Resolution uses the same no-follow,
containment, exact-NFC/case, immutable-snapshot procedure as source modules.
The included file's path, checked `u64` length, and digest enter that package's
source-tree commitment. Included paths are deduplicated by canonical portable
path; two paths resolving to one physical identity are an error. `include_str`
additionally requires exact UTF-8. No
environment variable, current directory, absolute path, parent escape, network,
clock, random input, ECS state, thread, FFI, or ambient filesystem API is
available to CTFE. Reading an immutable static is an ordinary dependency;
reading or writing a mutable static during CTFE is `CTFE001`, so root order and
cache warmth cannot create shared mutable compile-time state. A failed include
or const evaluation preserves an existing
lock and leaves no snapshot or sibling temporary.
The include acquisition opens once without following links, binds the opened
identity and contained canonical destination before and after spooling, and
evaluates only that retained spool. Concurrent replacement or identity change
is `CTFE005`; the original path is never reopened. Every included spool is owned
by `SourceDatabase` and cleaned with it on all outcomes.

## 0.4 ECS and world-lifecycle contract

### 0.4.1 Entities and structural commands

`entity` packs a nonzero 32-bit index in the low half and a nonzero 32-bit generation in the high half. Fresh entities start at generation one. Despawn increments the generation before deterministic LIFO slot reuse; overflow permanently retires the slot. Zero is invalid. `Option<entity>` expresses absence—there is no sentinel entity.

Systems request structural access with an explicit `cmd: commands` parameter:

```arche
let e: entity = cmd.spawn { Position { x: 0.0, y: 0.0 } };
cmd.despawn(e);
cmd.add(e, Velocity { x: 1.0, y: 0.0 });
cmd.remove<Velocity>(e);
```

The `.spawn { ... }` postfix is reserved to a `Commands<W>` receiver and each
payload expression must produce one closed component/tag value; duplicates are
an error and the empty payload is valid. `despawn` takes exactly one entity,
`add` one entity plus one closed EcsValue component, and `remove<T>` exactly one
closed component type argument plus one entity. Their spelling maps uniquely to
IntrinsicIds `206..209`; user methods cannot shadow it on Commands.

`cmd.spawn` immediately reserves and returns a handle. That handle may be stored or targeted by later queued commands, but the entity is not query-visible until flush. Structural commands flush exactly once at the implicit end of a schedule; no public intermediate flush exists.

Command emission order is schedule order, system order, query table/row order, then statement order. Each command is atomic. Earlier valid commands remain committed if the first stale, duplicate, conflicting, or allocation-failing command stops the flush. The failing command publishes no partial effect, later commands remain unapplied, and every owned queued payload is dropped exactly once.

### 0.4.2 Tables, queries, and isolated worlds

Queries visit materialized tables in table-creation order and live physical row slots in row order. Spawn and archetype transition append to the destination table. Removal performs deterministic swap-remove and repairs the moved entity's location. Materialized empty tables remain observable because their catalog position affects future iteration.

Required query terms preserve source binding order; exclusions do not bind; tags and other zero-sized required terms bind only `_`; mutable tags remain invalid. M27/M28 do not add optional terms, change detection, nested query loops, events, relations, or parallel schedules.
The same schema may not be both required and excluded or bound mutably more
than once in one query. Duplicate read-only resource parameters are permitted;
any whole-system alias set containing mutable access to that resource is
rejected conservatively. Parameter/local bindings remain distinct even when
their read-only resource values alias. Every schedule dispatch is checked
against the root-world initializer so each reachable resource read/write is
initialized before the first run. Schedule items and schedule dispatches may
repeat and execute in listed/source order.

Every `WorldContext` owns an independent allocator, resources, table catalog, rows/columns, entity locations and generations, free list, retired slots, command buffer, and allocation ledger. No mutable heap allocation crosses world instances. One immutable linked world template and shared code image may create many reentrant instances.

### 0.4.3 ECS-storable values

`EcsValue` is a compiler-sealed eligibility judgment revalidated after
monomorphization. Integer and floating scalars, bool, char, entity, unit,
eligible arrays/tuples/records/enums/Option/Result, `Box`, `String`, `Vec`, and
ordered `Map` are the complete eligible forms. Every transitive child must be
owned, sized, `'static`, and canonically encodable; `Box<T>` and `Vec<T>` require
`T:EcsValue`, and `Map<K,V>` requires `K:EcsKey` plus `V:EcsValue`. A nominal
type is eligible only when every field of every variant satisfies the same
recursive judgment.

“Safely droppable” means the compiler-derived Drop-requires union for the
complete reachable value tree is exactly `requires {}`. Drop already has exact
`throws {}`; a Drop panic remains an ordinary semantic panic and follows the
normal cleanup/double-panic rules rather than making the type ineligible. This
empty-requires proof covers custom Drop and every container child and is repeated
after monomorphization. References, raw pointers, `Rc`/`Arc`/Weak, `Pin`,
closures, generator factories/states, synchronization/interior-mutable values,
capabilities, and operating-system/runtime handles are transitively ineligible.

`EcsKey` is the following closed subset of `EcsValue`: unit; bool; every signed
or unsigned integer including `isize`/`usize`; char; entity; String; and an
array, tuple, record, enum/Option/Result, Box, Vec, or Map whose every logical
child is itself `EcsKey`. Floating-point values are ineligible at any depth.
User `Eq`/`Ord` behavior is never evidence. The compiler-sealed comparator is
total and compares bool as `false < true`; integers by their typed mathematical
value; char by Unicode scalar value; entity by its packed unsigned 64-bit value;
and String by lexicographic unsigned UTF-8 bytes. Arrays, tuples, records, and
Vec compare children lexicographically in logical/declaration order, with the
shorter equal-prefix sequence first. Enums compare declaration-order variant
ordinal and then payload fields; Box compares its pointee; Map compares its
already sealed-order entries lexicographically as key then value and then by
entry count. Unit and zero-field forms compare equal. Structural Eq is true
exactly when this comparator returns zero. These rules are the sole `EcsKey`
Eq/Ord selection used by Map, CTFE, Canonical Value, and observation.

Reference and native execution consume the same decoded/linked metadata but remain independent semantic implementations. The reference executor interprets verified instantiated Core; native execution runs AOT bodies. Exact parity is required for deterministic programs without ambient host effects. Live networking and thread scheduling use separate behavioral conformance tests rather than byte-identical execution claims.

## 0.5 Compiler, artifacts, native runtime, and observation

### 0.5.1 Semantic pipeline and identities

The compiler pipeline is:

```text
streamed source snapshots
  -> AST
  -> resolved HIR
  -> typed generic MIR (move paths, NLL, patterns, calls, effects, cleanup/unwind edges)
  -> dependency-ready VerifiedGenericCore<RootSlice> -> CTFE results/final IDs
  -> VerifiedGenericCore<CompleteWorkspace>
  -> deterministic link-time instance graph and monomorphization
  -> VerifiedInstanceCore
  -> direct reference execution or Machine IR/AOT
```

The M27 frontend returns one `FrontendOutput` that owns both the resolved HIR
and the immutable `SourceDatabase` consumed to create it. File IDs are globally
unique within that workspace output and are assigned in canonical package,
target, then module traversal order for module sources. Included inputs follow
all module sources, sorted by canonical package name then portable include path;
reuse of the same exact path/identity retains the first ID. Every literal and token retains all bytes
needed for meaning; diagnostics read only bounded snippets from the retained
snapshot. Semantic checking, identity construction, include acquisition, Core
lowering, CTFE, and source-tree hashing consume that authority and never reopen
an original source path. The source database is destroyed on every success or
failure after lock inputs have been finalized.

The Rust seed keeps replaceable typed MIR inside a private `arche-semantics`
crate. A target-independent `arche-core` crate, depending only on shared
foundation contracts, owns generic Core, its verifier, brand, and deterministic
printer. A hermetic `arche-ctfe` crate consumes only the verified RootSlice
scope and snapshotted include inputs. `arche-semantics` is the sole HIR-to-Core lowering
path and orchestrates CTFE; the public `arche` driver orders package resolution,
frontend, semantics/Core/CTFE, and only then lock publication. No M27 crate
depends on the historical root `archec0` implementation crate.

`SourceDatabase` owns the private spool handle, original physical identity,
canonical portable path, checked length/digest, and bounded-reader factory for
every source/include. The same exact path/identity used by multiple targets is
snapshotted once and reuses one global FileId; different paths aliasing one
physical identity are errors. AST/HIR nodes own or intern decoded literal and
identifier payloads, so their meaning never depends on a later snippet read.
Included inputs join this table before package source digests, CTFE, or lock
construction. Every spool and sibling temporary is cleaned on all normal/error
outcomes.

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
ARCHE-CTFE-RESULT\0
ARCHE-CTFE-TRACE\0
```

Every M27 canonical 128-bit identity preimage begins with the selected identity
domain followed by fingerprint encoding version `2` as `u32le(2)` before the
domain-specific fields. `ARCHE-CTFE-RESULT\0` and `ARCHE-CTFE-TRACE\0` are
instead 32-byte evaluated-value/accounting commitment domains and are followed
by `u32le(1)` as defined below. No
domain may be reused for another identity kind. M27 retains the exact M26
ABI/body domain strings but changes their prefix from the historical M26
`u32le(1)` to `u32le(2)` and uses the new domain-specific preimage contract;
Section 23.1's M26 version-1 preimages and vectors remain frozen historical M26
contracts rather than being reinterpreted.

Every M27 identity/hash constructor is the unkeyed pinned pure-Rust
`blake3 = { version = "=1.8.5", features = ["pure"] }` operation over the
complete preimage just specified. A 128-bit result stores the first 16 digest
bytes verbatim, without integer reinterpretation, and displays those wire-order
bytes as 32 uppercase hexadecimal digits. Both CTFE commitments retain all 32
digest bytes and display 64 uppercase hexadecimal digits. The M27-A
empty/domain vectors remain the byte-exact preimage-prefix goldens; C1/C5 add
the domain-specific 128-bit and CTFE-result digest vectors. No historical M26
constructor text is imported by reference.

M27-C completes the definition and generic-type identity preimages. All counts
and byte lengths below are checked `u64le`; strings are exact UTF-8; sequences
retain their stated order:

- `DefinitionId` encodes the official registry origin, scoped package name, a
  semantic target-module path, declaration-kind byte, declaration name (empty
  only for an implementation), and alpha-normalized declaration shape. The
  target-module path begins with `1` for the library, `2` plus binary target
  name, or `3` plus environment target name, followed by the declared module
  segments. This makes M27-B target-local roots globally unambiguous without
  adding package version or host path identity.
- A declaration shape includes generic parameter kinds in source order,
  canonical where predicates, field/variant/method names and order, parameter
  and result types, and canonical `requires`/`throws` sets. It excludes visibility,
  implementation body, const initializer value, source span, package version,
  source digest, and host path. References to declarations inside a shape use
  their canonical registry/package/target/module/kind/name path rather than a
  recursively computed hash, so recursive nominal declarations have no hash
  cycle.
- Declaration and member visibility are excluded from DefinitionId but are not
  discarded. The sealed semantic inventory records every declaration's
  resolved declared visibility and every field, variant, variant payload field,
  trait method, inherent method, and trait-impl method visibility row. Generic
  Core reproduces those rows and adds verifier-derived effective visibility;
  InterfaceHash consumes the exact public projection described below.
- `TypeId` encodes a closed or generic semantic type tree. Nominal leaves use
  `DefinitionId`; bound type/lifetime/const parameters use alpha-normalized
  de Bruijn coordinates; tuples/arguments retain source order; effect sets sort
  by raw identity. Inference variables and unresolved paths have no `TypeId`.
- `InterfaceHash` encodes the package identity and every externally visible
  binding in canonical public path/namespace order, including its
  `DefinitionId`, effective visibility, re-export origin, declaration shape,
  trait/impl coherence data, and effects. Bodies and unrelated private
  declarations are excluded, but every impl that can affect downstream
  coherence or selection for an externally nameable trait or outer nominal
  target is included even though an impl is not itself a public binding. M27-D
  repeats cross-object coherence before linking.

Changing a package version alone preserves these identities. Changing a
declaration's type, generic/effect shape, target root, or semantic module path
changes its definition identity. Concrete `InstanceId`, layout, ABI, and
serialized body/object identities remain M27-D responsibilities.
If a declaration/type shape contains a const-definition path, its stable ID is
explicitly pending until C5 supplies that path's successful CTFE value and
normalizes it as defined below; the identity constructor rejects an unresolved
placeholder rather than hashing the const declaration name or receipt digest.

The byte encoding is closed. Booleans are one byte `0` or `1`; options are tag
`0` for absent or tag `1` followed by the value; variants use the exact `u8`
tags below; lists are `u64le(count)` followed by self-delimiting elements;
strings are `u64le(byte_length)` plus UTF-8; stable IDs are raw 16 bytes.
Unknown tags, nonzero reserved values, invalid UTF-8, duplicate canonical set
members, or trailing bytes are invalid identity inputs.

Every semantic set is encoded by first encoding each member independently,
sorting the complete member byte strings lexicographically as unsigned bytes,
rejecting adjacent duplicates, then emitting `u64le(count)` and each
`u64le(member_length) || member`. This rule governs predicates, `requires`,
`throws`, and coherence sets. Ordered source constructs use their stated source
order and never this set rule.

| Category | Exact tags |
|---|---|
| Target root | `1=library`, `2=binary`, `3=environment`; binary/environment append target-name string |
| Declaration | `1=world`, `2=component`, `3=resource`, `4=tag`, `5=system`, `6=schedule`, `7=function`, `8=generator`, `9=struct`, `10=enum`, `11=trait`, `12=impl`, `13=type-alias`, `14=const`, `15=static`, `16=query` |
| Namespace | `1=module`, `2=type`, `3=value` |
| Visibility | `1=declaring-module`, `2=ancestor-module` plus target-relative module-segment list, `3=package`, `4=public` |
| Generic parameter/argument | `1=type`, `2=lifetime`, `3=integer-const` |
| Predicate | `1=trait-bound`, `2=lifetime-outlives`, `3=type-outlives` |
| Record/member form | `1=unit`, `2=tuple`, `3=record` |
| Reference/pointer mutability | `1=shared/const`, `2=mutable` |
| Binding origin | `1=declaration`, `2=re-export` |
| Definition owner | `1=trait`, `2=inherent-impl`, `3=trait-impl`, `4=system-query` |

Trait and impl methods use Declaration tag `7=function`. Their complete
Definition-owner chain and `DefinitionSignature::Callable.kind` distinguish
`TraitMethod` from `ImplMethod`; no unlisted method declaration tag exists.

Semantic type-tree tags are exact: `1=i8`, `2=i16`, `3=i32`, `4=i64`,
`5=u8`, `6=u16`, `7=u32`, `8=u64`, `9=isize`, `10=usize`, `11=f32`,
`12=f64`, `13=bool`, `14=char`, `15=entity`, `16=unit`, `17=never`,
`18=str`, `19=slice`, `20=array`, `21=tuple`, `22=reference`,
`23=raw-pointer`, `24=nominal`, `25=function-pointer`, `26=bound-type`,
`27=closure`, `28=generator`, `30=join-handle`, and
`31=generator-factory`. Slice appends element type; array appends
element type and canonical integer-const tree; tuple appends ordered type list;
reference appends mutability, lifetime tree, then pointee; raw pointer appends
mutability then pointee; nominal appends `DefinitionId` and ordered generic
arguments; function pointer appends unsafe bit, ordered parameter types, result,
requires, and throws; bound type appends de Bruijn depth/index. A closure appends its
owning `DefinitionId`, semantic-expression ordinal, ordered capture
ordinal/mode/type rows, callable parameter/result/effect shape, and generic
arguments. Every generator state uses tag 28 followed by target tag `1=named`
plus its generator DefinitionId, complete explicit generic arguments, and the
declaration's ordered hidden lifetime-binder positions,
or target tag `2=anonymous` plus its owning DefinitionId, nonzero expression
ordinal, and generic arguments. Both forms then append ordered transferred
capture ordinal/mode/type rows (zero for named), initial-parameter types with
their stable/static/bound lifetime trees,
factory_unsafe byte (`00=safe`, `01=unsafe`; anonymous is always `00`),
resume/yield/return types, and
requires/throws sets. A generator-factory uses target tag `1=named` followed by
its generator DefinitionId and generic arguments, or target tag `2=anonymous`
followed by owner DefinitionId, nonzero expression ordinal, and generic
arguments. It then appends ordered capture ordinal/mode/type rows (zero for named), factory call
trait, ordered initial-parameter types with stable/static/bound lifetime trees,
factory_unsafe byte with the same
encoding, and raw produced-generator TypeId. It is a sealed,
non-source-spellable callable type distinct from the produced state.
Join-handle appends one result semantic type followed by the child checked-
exception types as a canonical raw-TypeId-sorted semantic set. It is the sealed,
non-source-spellable representation of `JoinHandle<T,E...>` produced by thread
spawn and consumed by join; it is not nominal tag 24 and cannot be forged with
ordinary generic syntax. Tag 29 remains the identity-only nominal-path form
defined below and never appears as a runtime/Core type. Body-local region-origin
substitutions for tags 27, 28, and 31 live only in RegionFact rows and are not
appended to these preimages.
The ordinal is the checked one-based `u64` preorder index among closure and generator
expressions in the owner's resolved body after transparent parentheses are
removed; tag-28 named states use target tag 1 and no expression ordinal. Thus two anonymous
expressions in one owner cannot collide. A declaration-shape type
tree uses tag `29=nominal-path` plus canonical registry/package/target/module/
kind/name fields instead of tag 24, preventing recursive identity cycles.

Lifetime-tree tags are `1=static`, `2=bound` followed by de Bruijn depth/index,
and payloadless `3=erased-local`. The third form is legal only in the private
body-local contexts fixed above; it deliberately gives identical structural
TypeIds to equal local type shapes while RegionFact holds their distinct
origins. Integer-const-tree tags are `1=literal` followed by integer-type
tag and fixed-width little-endian bits, `2=bound` plus depth/index, and
`3=expression` plus a length-prefixed canonical hermetic const-expression tree.
Required const expressions are fully typed and contain only operator tags and
resolved declaration paths; no source spelling or host value enters.
Const-expression node tags are exact: `1=integer-literal`, `2=bound-const`,
`3=const-definition-path`, `4=wrapping-neg`, `5=bit-not`, `6=wrapping-mul`,
`7=integer-divide`, `8=integer-remainder`, `9=wrapping-add`,
`10=wrapping-sub`, `11=masked-shift-left`, `12=masked-shift-right`,
`13=bit-and`, `14=bit-xor`, and `15=bit-or`. Literal and bound payloads use the
forms above. A definition path uses canonical origin/package/target/module/kind/
name rather than DefinitionId and exists only in the pre-result dependency
tree. C5 replaces each closed definition-path node with its contextual integer-
literal node after validating the sealed receipt; the path and 32-byte result
digest remain receipt/TypeConst provenance and never enter a finalized TypeId or
DefinitionId. If no bound parameter remains, C5 evaluates the complete checked
expression and the final integer-const tree is tag 1 plus contextual type and
fixed-width bits. If bound parameters remain, tag 3 retains only operators,
bound nodes, and already-normalized literal nodes until M27-D substitutes them,
evaluates the closed expression through the same verified RootSlice authority,
and emits tag 1. Consequently two paths or expressions producing equal
contextual bits have the same finalized type identity; changing an initializer
changes dependent identities if and only if those bits change. The const
declaration's own DefinitionId remains stable.
Unary appends one length-prefixed child and
binary appends left then right. Every node appends its contextual integer-type
tag before children. No other full-expression node enters a type identity.

After `ARCHE-DEF-ID\0 || u32le(2)`, a definition preimage is exactly origin
string, package-name string, target-root encoding, module-segment list, owner
chain, declaration tag, declaration-name string, then `u64le(shape_length)` and shape.
Top-level items have an empty owner chain. A trait method's owner is tag 1 plus
the canonical trait path/shape; an inherent method uses tag 2 plus its enclosing
inherent impl's target type, alpha-normalized generic-parameter declarations,
and canonical sorted predicates; a trait-impl method uses tag 3 plus trait path,
target type, generic parameters, predicates, and the impl's one-byte
`is_default` value. Owner entries use length-prefixed declaration-shape trees, not
already-hashed IDs, so same-named methods in different traits/impls cannot
collide and recursive ownership creates no hash cycle.
A query uses owner tag 4 plus its system's canonical path/shape and has the
source parameter name as its declaration name; its shape is the ordered
read/write/exclude term list. Renaming an ordinary callable parameter does not
change identity, while renaming a query parameter deliberately changes Query
DefinitionId.
Shape begins with generic-parameter list, canonical predicate set, and a
declaration-tag-specific payload: records encode ordered name/type fields;
enums ordered variant name/form/field payloads; callables ordered parameter
type/mode (ordinary parameter names are excluded), result, unsafe bit, generator resume/yield types when applicable,
and both effects; traits ordered required method entries; impls optional trait
path plus target type, the one-byte `is_default` value, and ordered method
entries; aliases target type;
const/static declared type plus static-mutability bit; world/tag have no
payload; schedules encode effects but not run-body order. Components, resources,
and systems use their record/callable payload plus system-access tags
`1=capability-shared`, `2=capability-mutable`, `3=resource-read`,
`4=resource-write`, `5=query`, `6=commands`;
query terms use `1=read`, `2=write`, `3=exclude` in source order.
Each trait/impl method entry is exactly the length-prefixed UTF-8 method name,
then `u64le(callable_shape_length)`, then that method's callable shape. The same
entry bytes and source order are used when reconstructing the trait/impl owner
chain and public interface; the method DefinitionRow name and callable shape
must match its enclosing entry exactly. Ordinary parameter names remain excluded.
Within a system DefinitionId shape, query mode expands only that ordered term
shape, not the query parameter name or its later Query DefinitionId. The query
DefinitionId is then derived from the completed noncyclic parent-system path/
shape plus its own parameter name and terms. Core's ParameterSignature may carry
the resulting ID, but identity reconstruction must apply this expansion rule.

After `ARCHE-TYPE-ID\0 || u32le(2)`, a TypeId preimage is
`u64le(type_tree_length)` plus exactly one semantic type tree. After
`ARCHE-INTERFACE-HASH\0 || u32le(2)`, an interface preimage is origin string,
package-name string, then public binding rows sorted by public binding path,
namespace, and complete binding-target encoding. A public binding path is
exactly target-root encoding followed by its module/name segment list; the root
therefore distinguishes identical library, binary, and environment paths. A
target is tag `1=module` followed by raw target PackageId, target-root encoding,
and module-segment list, or tag `2=declaration` followed by raw DefinitionId;
module namespace rows require tag 1 and type/value rows tag 2.

Every row first encodes that complete public binding path, namespace, binding
target, effective visibility tag 4, and binding origin. A re-export origin also
encodes raw source PackageId, source target-root/path/namespace binding path,
and source binding target. It then appends a payload tag: `1=module` or
`2=declaration`. A module payload is empty and is legal only with a module
target; it never fabricates a declaration shape, member tree, coherence row,
query, or result digest. A declaration payload is legal only with a declaration
target and encodes, in order: the declaration's declared visibility;
`u64le(shape_length)` plus its declaration shape; `u64le(member_count)` and the
member rows; `u64le(impl_count)` and coherence rows; `u64le(query_count)` and
query rows; then an option-tagged CTFE result digest. No trailing field is
implicit.

A member row is sorted by and begins with its complete path: tag `1=field` plus
field ordinal, `2=variant` plus variant ordinal, `3=variant-field` plus variant
and field ordinals, or `4=method` plus method ordinal. It then encodes declared
and effective Visibility values. The row set contains every immediate member in
source order through those ordinals; inherited/fixed visibility is encoded as
the derived declared value rather than omitted. A coherence row is sorted by
raw Impl DefinitionId and encodes that ID, its exact one-byte `is_default`, the
option-tagged immediate specialization-parent DefinitionId derived by the
solver, and `u64le(impl_shape_length)` plus the canonical trait/target/generic/
predicate/method-signature/effect shape. It is required whenever its trait or
outermost nominal target is externally nameable, regardless of the impl's or
method's binding visibility; no private body is included. A query row is the
parameter name, raw Query DefinitionId, and ordered term shape; only a public
system declaration has nonzero query rows, in source parameter order, and query
declarations are not separate module bindings.

The result option is present exactly for an exported const or externally
observable static and contains the 32-byte digest of its canonical evaluated
logical result; it is absent for every other declaration. That digest is BLAKE3
over `ARCHE-CTFE-RESULT\0 || u32le(1) || raw TypeId ||
u64le(value_encoding_length) || canonical CTFE Logical Value v1 bytes`. C1 can
prove a declaration-only interface skeleton, but final `InterfaceHash` vectors
and acceptance wait for C5 CTFE; changing an exported value must change the
interface even though it preserves `DefinitionId`.

CTFE Logical Value v1 is a hash-preimage encoding, not Canonical Value v1 or a
public artifact. Every node begins with its `u8` tag and raw 16-byte semantic
TypeId. Counts and byte lengths are checked `u64le`; integer bits use the exact
little-endian language width; child nodes are concatenated in the order below:

| Tag | Logical value payload |
|---:|---|
| `1` | unit; no payload |
| `2` | bool; one byte exactly `0` or `1` |
| `3` | char; scalar as `u32le` |
| `4` | integer; integer-type tag followed by exact fixed-width two's-complement/raw bits |
| `5` | f32; canonical semantic bits as `u32le` |
| `6` | f64; canonical semantic bits as `u64le` |
| `7` | entity; packed bits as `u64le` |
| `8` | tuple; element count and child nodes in tuple order |
| `9` | array; element count and child nodes in index order |
| `10` | record/component/resource; raw nominal DefinitionId, field count, and child nodes in declaration order |
| `11` | enum/Option/Result; raw nominal DefinitionId, `u64le` variant ordinal, field count, and active payload children in declaration order |
| `12` | String; UTF-8 byte length and exact bytes |
| `13` | Vec; element count and child nodes in logical index order |
| `14` | Map; entry count and alternating key/value child nodes in the Map's one logical order established by its statically selected `Ord`; an EcsValue map necessarily uses the sealed EcsKey selection |
| `15` | Box; one pointee child node |
| `16` | DataRef; kind byte `1=source-string`, `2=included-bytes`, or `3=included-string`, byte length, and exact logical bytes |
| `17` | function pointer; target tag `1=named` followed by raw DefinitionId and canonical generic arguments, or `2=zero-capture-closure` followed by raw owning DefinitionId, nonzero `u64le` expression ordinal, and canonical owner/closure generic arguments; then raw target function-signature TypeId |

No other tag is valid. Zero-sized aggregates still carry their tag, TypeId, and
zero count. A DataRef excludes dense DataId, source path/span, and allocation
identity; a FunctionRef excludes a machine address. Ordinary references, raw
pointers, MaybeUninit, Rc/Arc/Weak, Pin, capabilities/handles, closures, and
generators have no node and fail promotion with `CTFE007`. The root TypeId in
the digest prefix must equal the root node TypeId; every child TypeId and
nominal/variant/field arity is revalidated against its parent semantic type.
Thus unrelated private source/data rows and allocator choices cannot perturb an
exported value digest. The debug `ARCHE-CTFE-TEXT` printer is never hashed.

Because tag 16 intentionally omits source identity, each sealed receipt carries
one CtfeDataRefProvenance row for every tag-16 node and no other row. The root
path is empty; otherwise `value_path` is the checked-u64 child ordinal at each
logical node, with tuple/array/record/enum payload children in encoded order,
Vec elements in index order, Map children as key `2*i` then value `2*i+1`, and
Box pointee zero. Rows are unique and sorted lexicographically by their numeric
`u64` ordinal sequence, comparing each ordinal as an unsigned integer and
placing a shorter equal-prefix path first. Each row
must select one exact DataRow in the retained producer package; kind, CoreType,
DataSource, byte digest, and tag-16 kind/bytes/type all match. The side table is
excluded from CTFE Logical Value bytes and result/InterfaceHash digests, so two
equal immutable values remain equal even when sourced from different literals.
It is nevertheless sealed receipt provenance: CtfeResultRef materialization
imports/re-densifies the exact referenced DataRows into the consumer RootSlice,
and M27-D uses the same rows for checked relocations. Missing, extra, ambiguous,
stale, or remapped provenance prevents receipt, RootSlice, or object branding.

The CTFE evaluator's function-pointer logical value uses exactly tag 17's target
union. A CallableToFunctionPointer result from a zero-capture closure retains
that closure's owner/ordinal/substitution; it is not collapsed to the owner
DefinitionId. M27-D uses the identical union as its relocation/link target and
must resolve it to the matching named instance or ClosureDescriptor body before
publishing an object. Two closure ordinals with identical signatures remain
distinct targets and digests.

#### Verified generic Core authority

M27-C introduces one in-memory, target-independent brand family named
`VerifiedGenericCore<Scope>`, where `Scope` is the compiler-sealed marker
`RootSlice` or `CompleteWorkspace`. Only `verify_generic_core` may construct
either scope. Its payload is private, immutable, owns every proof table it
references, and is exposed only through read-only accessors. Parsers, HIR/MIR
builders, decoders, tests, and unsafe code construct an unverified candidate
instead. A verifier failure is a status-1 compiler failure, never a panic. CTFE
accepts only RootSlice; M27-D linking, object production, and every non-CTFE
consumer accept only CompleteWorkspace.

M27-C also introduces the compiler-sealed `VerifiedSemanticInventory`. Only
`verify_semantic_inventory` constructs it, from the same immutable
`FrontendOutput` source database, the validated M27-B workspace/resolved graph/
decoded lock authority, and the fully checked symbolic HIR/typed-MIR workspace;
tests, unsafe code, and Core builders cannot construct or mutate it.
It owns every workspace root, package/source-tree commitment, manifest target,
module/declaration/re-export row, canonical symbolic declaration shape, and
SemanticBodyKey for every accepted body, including otherwise unreferenced
private items and empty targets. Its `SemanticDefinitionKey.owner_path` is the
exact pre-identity owner-chain byte encoding above with canonical declaration
paths instead of hashes; `symbolic_shape` is the complete pre-result semantic
shape tree used by C4, including symbolic const/effect atoms. Both are
independent of final DefinitionId/TypeId values. A Generic Core candidate carries
the same immutable inventory Arc, and `verify_generic_core` must match its
scope projection against that authority rather than trusting a producer-supplied
inventory hash. M27-D must retain an equivalently verifiable inventory/commitment
in any later serialized Core authority; it cannot synthesize completeness from
raw IDs.

The inventory additionally owns the exact
`Arc<VerifiedEmbeddedCoreAuthority>` selected by the toolchain. That authority
is not a resolved M27-B package and is excluded from `source_trees`,
`workspace_roots`, the ordinary `packages` vector, lock closure, and source-file
completeness. The semantic verifier nevertheless resolves every compiler-known
virtual binding through its branded rows and includes referenced virtual
definition/body keys in the applicable inventory projection. A candidate whose
embedded authority Arc, version, digest, PackageId, or row bytes differ is
invalid even when all raw stable IDs are self-consistently remapped.

Inventory `source_trees`, `workspace_roots`, and `packages` are unique and sorted
by raw PackageId bytes, matching their corresponding SourceDatabase/package rows
exactly. Within one semantic package, target manifest ordinals use the fixed
library/binary/environment order above, are zero-based, dense, and unique, and
the vector is ascending by ordinal; each target/root-module
pair has the same package/TargetRoot and an empty module path. Definitions are
unique and sorted by complete SemanticDefinitionKey bytes, and bodies are unique
and sorted by the exact SemanticBodyKey bytes defined above. A
SemanticDefinitionKey encodes ModuleRef (raw PackageId, TargetRoot, segments),
length-prefixed owner_path, declaration-kind tag, name string, then SourceSpan
in the already-fixed field order. Semantic module rows and bindings use the
same module/binding order specified below. Each GenericCorePackage `files`
vector is unique and sorted by global FileId, and every row matches one immutable
SourceDatabase entry, package-relative path, length, and digest for that package;
no host path or unlisted source file is admitted.

Each ordinary semantic package also seals its effective `CtfeBudgets`, after
manifest defaults have been applied and checked. These three checked `u64`
limits are package authority, not evaluator input. The corresponding Generic
Core package row must reproduce them byte-for-byte; a CtfeRoot always uses the
budgets of its declaring ordinary package inventory row. Neither a dependency,
receipt, Core producer, command-line option, nor cache may override them.

The inventory's toolchain version is the canonical selected SemVer string and
its release-manifest, workspace-source, registry-snapshot, package-source,
archive, provenance-record, and inclusion-record digests are the raw 32-byte
values decoded and revalidated from `Arche.lock`. `registry_identity` is exactly
`registry+https://packages.arche-lang.org`. Every one of those fields, every
resolved instance version/source row, and every dependency requirement/kind is
byte-for-byte the validated M27-B authority; the semantic verifier neither
re-resolves nor permits a Core producer to substitute an equivalent-looking
graph. Package versions use canonical SemVer without build metadata. A Workspace
source stores the canonical `/`-separated workspace-member path (`.` only for
the root member) and that member's source-tree digest. A Registry source stores
the exact archive/source/provenance/inclusion digests committed by the lock.

Each semantic target contract variant exactly matches its TargetRoot. Library
has no root-world link, entrypoint, capabilities, environment profile, or
driver schedule links.
A Binary contract's root_world resolves to one World definition and its main to
one public Callable definition in that same package and target; the world link
and exact main signature/Caps boundary are the contracts above. Its
`ManifestCapability` vector is unique and sorted by exact manifest-key ASCII
bytes, using the zero-based tags in the schema's listed order. An Environment
contract similarly resolves its root_world in the same package/target, has no
main or capability vector, and its canonical NFC profile identifier resolves
exactly the reset, step, and self-play Schedule definitions selected by the
manifest profile. Those three rows obey the environment schedule/signature and
ambient-effect prohibitions above. Every semantic key is present in the sealed
definition inventory; cross-target or wrong-kind links fail inventory
verification.

CompleteWorkspace contains one TargetRow for every semantic target, in the same
dense manifest-ordinal order. Its variant and profile are identical, semantic
world/main/schedule keys map to the recomputed final DefinitionIds, and Binary
capability keys map to the embedded virtual-core capability DefinitionIds then
sort by raw DefinitionId bytes as required for semantic effect sets. No row may
add, omit, reorder, or substitute a target contract. RootSlice TargetRow vectors
are empty—the sealed semantic target inventory and CtfeRootKey remain the target
authority while final target-linked IDs may still be pending. M27-D consumes the
CompleteWorkspace TargetRows as the sole branded target configuration for
target-specific object and link production.

Generic Core contains no concrete instance graph, target layout, field offset,
calling convention, ABI hash, relocation, object section, allocator address,
machine register, stack offset, or native instruction. Canonical-Core v2 byte
serialization and all post-substitution instance claims belong to M27-D.
Every dense identifier is a checked `u64`; conversion to `usize` occurs only at
an actual host allocation/slice boundary and returns an explicit failure.

One generic Core package row owns its scope-projected package provenance,
semantic dependency edges, source-file/module/binding rows, canonical data/type
tables, definition/trait/impl declarations, world/query/schedule descriptors,
body kinds, closure/generator descriptors, and interface state.
`registry_origin` is exactly `registry+https://packages.arche-lang.org`; the
scoped name is its canonical package name. The verifier rebuilds PackageId from
those two strings before trusting any package-keyed row. Its selected `version`
and `source` must match the sealed inventory row. Dependency aliases are NFC
source identifiers; each row matches the inventory/lock alias, raw target
PackageId, canonical SemVer requirement, and Normal/Development kind. Aliases
are unique under full case fold across both kinds for one package. Rows are unique and sorted by alias
UTF-8 bytes, raw target PackageId, requirement UTF-8 bytes, then kind tag
(`0=Normal`, `1=Development`), and every target package is present in the
corresponding scope. An ordinary package source kind is exactly Workspace or
Registry; cycles and registry-to-workspace edges have already failed in M27-B
and are independently rejected here. `EmbeddedCore` is accepted only for the
separately carried branded virtual projection, never in the ordinary package or
dependency vectors.

Only direct Normal aliases enter library, binary, or environment source scopes
and only Normal edges participate in a RootSlice package closure. Development
edges remain sealed in the full inventory and CompleteWorkspace graph so they
cannot disappear or change the lock commitment, but they are not source-visible
in M27-C; M27-H test orchestration may consume them only through its separately
verified compiler-generated test target. `CompleteWorkspace.workspace_roots` is
byte-for-byte the inventory roots, unique/raw-PackageId-sorted, and closure over
both retained dependency kinds is exactly `packages`. For RootSlice, the root
key's package supplies the sole projected graph root and Normal-edge closure
selects package rows from the full inventory; each selected package provenance
row drops Development edges and Normal edges leaving that closure. The
`packages` vector is unique and sorted by verified raw PackageId bytes and
contains each selected package exactly once.

The sealed inventory retains the complete target/module/declaration/public-
re-export graph. CompleteWorkspace Core rows match its full finalized projection;
RootSlice rows are the exact dependency-ready definition/module/body/coherence
projection required by that root, while all omitted inventory rows remain
visible to the verifier and cannot be mistaken for source absence. Module rows
sort by target-root encoding then module path; their bindings sort by NFC UTF-8
name, namespace tag, then target encoding, with no duplicate `(name, namespace)`.
Every inventory target has one empty-path root module with public declared/
effective visibility. Every nonroot module row names one same-package parent
module binding and one exact source file; every ModuleRef's package is present,
a ModuleRow's ModuleRef package equals its owning package, and every module-owned DefinitionRow
span uses that module's file. A DefinitionRow's module, declaration kind, name,
owner chain, and semantic signature reconstruct the exact DefinitionId preimage.
References inside a shape are expanded through the referenced row's canonical
package/target/module/kind/name path rather than accepted as opaque raw IDs.
The verifier computes definitions before types and rejects a self-consistent raw
ID remap just as it rejects a single wrong ID.

Every SemanticDefinitionInventory row carries the declaration's explicit-or-
defaulted declared visibility and one complete MemberVisibilityPath row for each
immediate field, variant, variant payload field, or method. Paths use the closed
tag/ordinal grammar in the interface contract and are unique in encoded-path
order. Record field and inherent-method rows carry their source-spelled or
defaulted visibility; enum variants/payload fields, trait methods, and trait-
impl methods carry the visibility fixed or inherited by their owning contract.
A definition with no such member has an empty vector. Generic Core repeats the
declared rows byte-for-byte, adds the one recomputed effective visibility per
member, and rejects an omission even when the member is private. Definition
declared visibility also matches DefinitionProvenance and, when present, its
declaration binding; effective values are rederived from the module/re-export
graph. A Query definition inherits its owning system's declaration/effective
visibility and has no binding of its own. An unnamed impl has fixed
DeclaringModule visibility and reaches an interface only through its separately
derived coherence row.

Module rows retain every module/declaration binding and every public re-export
needed to reconstruct the package interface; ordinary private `use` aliases that
have no binding/export authority are already resolved away. A declaration
binding's target/provenance/declared visibility must equal its DefinitionRow.
Each possibly cross-package re-export source resolves through the same graph to
the exact target and namespace and
cannot widen its source audience. Effective visibility is recomputed from the
declared visibility, module ancestors, and re-export source rather than trusted.
The public rows derived from that graph use the exact interface encoding above,
including member visibility, coherence-relevant impls, queries, and sealed CTFE
result digests. RootSlice requires `PendingSkeleton` and verifies the complete
inventory skeleton plus its exact ready Core projection but publishes no
InterfaceHash. CompleteWorkspace
requires `Final`, recomputes its 128-bit hash from the owned graph and sealed
results, and rejects any mismatch or public/re-export-only mutation.

Literal Data rows sort by source span, kind, then bytes and are not deduplicated
across spans. Included views sort after literals by retained input portable
path, then kind. Acquisition/FileId is deduplicated by input path/identity, but
data views are keyed by `(input FileId, IncludedBytes|IncludedString, exact
CoreType)`: using one UTF-8 file through both include functions therefore creates
two views over the same immutable bytes. Repeated uses of one exact view reuse
its row. `bytes` are decoded literal bytes or exact included bytes and `digest`
is SHA-256 of those bytes.
type rows sort by raw
`TypeId`; definition, trait, and impl rows each sort by raw `DefinitionId`;
generic parameters retain source order; effects are unique and raw-identity
sorted. Every TraitRow and ImplRow has exactly one correctly kinded DefinitionRow.
A TraitRow method vector is byte-for-byte its `DefinitionSignature::Trait`
method vector. An ImplRow's trait, target, `is_default` bit, generics,
predicates, and method vector are byte-for-byte the corresponding DefinitionRow
fields and `DefinitionSignature::Impl` payload. Every listed trait/impl method
has exactly one Callable DefinitionRow of the matching method kind whose owner
is that trait/impl; its name/callable shape reconstructs the enclosing method
entry exactly, and no owned method is omitted or listed twice. A body owns its
tagged owner and source span, ordered generic parameters and obligations,
parameter/result types, exact effects, locals, move paths, loans, unsafe regions,
CFG blocks, and linear unwind-token state. Closure descriptors sort by raw
owning `DefinitionId` then nonzero expression ordinal. Generator descriptors
sort by raw owning `DefinitionId` then expression ordinal, where zero is
reserved for a named generator item and nonzero identifies an anonymous
generator expression. Dense `ClosureId`/`GeneratorId` values are assigned only
after those sorts.
World, query, and schedule descriptors each sort by raw definition ID and have
exactly one matching correctly kinded DefinitionRow. World resource TypeIds are
unique and raw-byte sorted and equal exactly the resource types named by that
world initializer's `world.init-resource` operations: one operation per listed
type and no unlisted or uninitialized resource. The initializer body retains
source operation order; the descriptor list is only its canonical set view.
Query terms and schedule runs retain source order;
their semantic type/access/run payloads equal the corresponding Query/Schedule
DefinitionSignature fields, while descriptor-only source indices, binding
ordinals, and spans are independently derived. TypeConst descriptors sort by owner raw ID, purpose
tag, expression ordinal, then mode and are the complete set of type-level const
use sites retained in HIR.

A CompleteWorkspace contains every package/target declaration and descriptor
required by the resolved workspace. A RootSlice instead contains exactly one
CtfeRoot body plus the transitive callable/drop/data/type/definition closure it
can execute, and the complete trait/impl coherence universe that can affect any
selection in that closure; unrelated worlds, schedules, and bodies are absent.
All rows that are present obey the same canonical ordering and reciprocal rules.
Each scope also carries an embedded-core projection from the same branded
authority Arc. CompleteWorkspace retains the full byte-exact virtual package;
RootSlice retains exactly the virtual rows/bodies reachable from its root plus
the rows required to verify their types, traits, and effects. This is the sole
permitted exception to the ordinary resolved-package closure. It has no
ordinary SourceFile and exactly the authority's reserved synthetic SourceFile,
but no source-tree receipt, lock, workspace-root, TargetRow, or dependency
projection; `project_root_slice` compares it against the authority rather
than inventing such provenance.
`predecessor_results` and complete-workspace `results` are unique and sorted by
the byte encoding of CtfeRootKey. A RootSlice predecessor set is the exact union
of every CtfeResultRef key in its reachable bodies, every closed const-
definition path named by its retained TypeConstDescriptor pre-result trees, and
every immutable-static initializer key named by its `static_bindings`.
Final
type/effect rows contain only normalized bits and are cross-checked against
those descriptors/receipts rather than treated as provenance. The verifier
derives that union from the candidate rows and rejects
any missing or extra receipt. A CompleteWorkspace result set contains exactly
one receipt for every Const or Static initializer DefinitionRow and every
ClosedRoot TypeConstDescriptor, matched by package/target/module, root kind,
ordinal, and source span; ParameterizedTemplate rows have none. No other root is
retained.
That key encoding is raw PackageId; target-kind `u8`; no target-name bytes for
Library, otherwise `u64le` target-name length and its UTF-8 bytes for Binary or
Environment; `u64le` module count and length-prefixed UTF-8 segments; root-kind
`u8`; `u64le` ordinal; then SourceSpan's `file_id`, `start_byte`, `end_byte`,
`start_line`, `start_column`, `end_line`, and `end_column` as `u64le` in that
order. Library requires an absent target name; binary and environment require
one present exact name. No other bytes or alternate form
are accepted.
`CtfeLogicalValue` is the private immutable node tree in one-to-one
correspondence with the CTFE Logical Value v1 tag/payload table above. Its digest
and TypeId are always recomputed from that tree rather than trusted.

RootSlice construction also computes one sealed finite CTFE function-pointer
points-to closure. Its target key is either a named DefinitionId plus complete
generic substitution and signature TypeId, or a zero-capture closure's raw
owner DefinitionId, nonzero expression ordinal, complete owner/closure generic
substitution, and signature TypeId. The byte encoding is exactly Logical Value
tag 17's target payload: target tag 1/2 followed by those fields in its stated
order. Dense ClosureId/CoreBodyId, span, and descriptor storage never enter the
key; the descriptor is resolved only after key sorting. Seeds are every
FunctionRef constant, every CallableToFunctionPointer result, and every tag-17
node in a predecessor receipt. A flow-insensitive abstract interpretation of
the same Core propagates target sets through SSA/block parameters, place
copy/move/init/replace, aggregate/enum/array/owned-container construction and
projection, arguments, returns, and the exact value-preserving semantics of
sealed CTFE intrinsics; joins are set union. Each possible
FunctionPointerCall target adds its body/descriptor/data/type rows and that
body's transitive direct, drop, trait, and indirect closure, and construction
and analysis repeat to the least fixed point. A RootSlice has no ambient
function-pointer input, and an intrinsic without a closed propagation summary
cannot execute in CTFE. Target sets and retained rows sort by target-key bytes.
The verifier recomputes this closure from Core plus the sealed predecessor
logical values and rejects a missing or extra target/body row. This retention
analysis does not add a call-graph effect edge: FunctionPointerCall effects
remain the declared signature contribution fixed above.

CompleteWorkspace construction performs a second, independently branded use of
that analysis for every Environment TargetRow. Each reset, step, and self-play
ScheduleDescriptor is expanded in source run order, including every repeated
SystemRun; the run's system DefinitionRow and exact closed generic/const
substitution must resolve to one system body, and that ClosedBodyContext is an
initial reachable-body seed. Schedules themselves have no Core body. Additional
seeds are FunctionRef constants, CallableToFunctionPointer results, tag-17 nodes
in CtfeResultRef receipts, and tag-17 nodes mounted through immutable-static
receipts reachable from the seeded systems. The same value/container/call
propagation runs context-sensitively from those system bodies. A
function-pointer parameter is finite only when every reachable call-site input
has a finite set; environment drivers, capabilities, ECS values, and world state
cannot inject a function pointer. Every possible indirect target adds its closed
body context, descriptors, Drop edges, direct and further indirect callees until
the least fixed point. A missing propagation summary, unknown input, unsafe
forged target, or compatible-signature target outside that set rejects the
environment before execution.

The candidate carries one `EnvironmentPointsToProof` per Environment target,
sorted by raw PackageId then target name. Within it, sites sort by complete
caller context and ProgramPoint; targets sort by the exact shared
FunctionPointerTargetKey bytes; reachable body contexts sort by their complete
encoding. `verify_generic_core` recomputes every set from Core, sealed receipts,
target contracts, and descriptors, then unions each reachable body's exact
`environment_forbidden` summary. Missing/extra sites, targets, bodies, summaries,
or an unknown target are `CORE004`; a nonempty final forbidden union is
`CAPABILITY001`. These proof rows are not stable identity, effect, interface, or
artifact inputs, and RootSlice has none.

Core bodies sort by the complete tagged owner encoding: tag then definition raw
ID; closure/generator descriptor's owning raw DefinitionId then expression
ordinal; world raw ID; or TypeConst owner raw ID, purpose tag, then one-based
semantic-expression ordinal. The in-memory descriptor ID is dereferenced for
this ordering; TypeConst appends mode (`1=closed`, `2=template`). No dense
descriptor ID becomes the stable key. Bodies receive dense
`CoreBodyId`s in that order. Each executable/initializer BodyOwner has exactly
one matching body. Each owning DefinitionRow, closure/anonymous-generator,
world, and TypeConst descriptor references that one body. A named generator's
DefinitionRow and required ordinal-zero GeneratorDescriptor are two reciprocal
references to the same Definition-owned body, not two owners. No body is
unreferenced or has two distinct BodyOwners. The effect graph uses this same owner key, with edges from a
call/intrinsic site to the body it can actually invoke, rather than assuming
every node has a DefinitionId. Merely constructing or lexically containing a
closure/generator does not execute its body and creates no effect edge; capture
expressions and capture-drop requirements remain operations of the containing
body, while closure/generator calls and resumes add the invoked body's edge.
For RootSlice only, the single `CtfeRoot` owner sorts first and is followed by
the ordinary reachable owners in the same order. CompleteWorkspace never
contains that owner variant.

The in-memory schema is closed by the following Rust-like records; field order
is also canonical printer order. `Vec` means the semantic order defined here
and `Option` uses absent/present. `FileId`, `DataId`, `ClosureId`, `GeneratorId`,
`CoreBodyId`, `BlockId`, `ValueId`, `LocalId`, `MovePathId`, `LoanId`,
`UnsafeRegionId`, and `BoundRegionBundleId` are distinct checked-`u64` dense or
session newtypes. `PackageId`, `DefinitionId`, and `TypeId` are distinct raw
16-byte stable identities. `IntrinsicId` is the separately defined checked
`u16`; structured keys/references retain their stated fields. An unlisted type
does not acquire a representation merely because its name ends in `Id`.
FileId is a session newtype; `u64::MAX` is reserved exclusively for the branded
embedded-core synthetic snapshot and is never returned by ordinary acquisition.

```text
UnverifiedGenericCore = RootSlice {
  inventory:Arc<VerifiedSemanticInventory>,
  embedded_core:GenericCorePackage,
  root:CtfeRootKey, predecessor_results:Vec<VerifiedCtfeResult>,
  static_bindings:Vec<CtfeStaticBinding>,
  closed_region_views:Vec<ClosedRegionView>,
  packages:Vec<GenericCorePackage>
} | CompleteWorkspace {
  inventory:Arc<VerifiedSemanticInventory>,
  embedded_core:GenericCorePackage,
  workspace_roots:Vec<PackageId>,
  results:Vec<VerifiedCtfeResult>,
  environment_points_to:Vec<EnvironmentPointsToProof>,
  packages:Vec<GenericCorePackage>
}
CtfeRootKey {
  package:PackageId, target_kind:Library|Binary|Environment,
  target_name:Option<String>, modules:Vec<String>,
  kind:ConstInitializer|StaticInitializer
    | ArrayLength|RepeatCount|IntegerGenericArgument,
  ordinal:u64, span:SourceSpan
}
VerifiedCtfeResult = SealedReceipt {
  slice:Arc<VerifiedGenericCore<RootSlice>>,
  sources:Arc<SourceDatabase>, source_trees:Vec<(PackageId,[u8;32])>,
  budgets:CtfeBudgets, accounting:CtfeAccounting, row:CtfeResultRow,
  data_refs:Vec<CtfeDataRefProvenance>
}
VerifiedSemanticInventory = SealedInventory {
  sources:Arc<SourceDatabase>, source_trees:Vec<(PackageId,[u8;32])>,
  embedded_core:Arc<VerifiedEmbeddedCoreAuthority>,
  toolchain_version:String, release_manifest_digest:[u8;32],
  workspace_source_digest:[u8;32], registry_identity:String,
  registry_snapshot_digest:[u8;32],
  workspace_roots:Vec<PackageId>, packages:Vec<SemanticPackageInventory>
}
VerifiedEmbeddedCoreAuthority = SealedEmbeddedCore {
  interface_version:u32, interface_digest:[u8;32],
  package:GenericCorePackage
}
SemanticPackageInventory {
  package:PackageId, provenance:PackageProvenance, ctfe_budgets:CtfeBudgets,
  targets:Vec<SemanticTargetInventory>,
  modules:Vec<SemanticModuleInventory>,
  definitions:Vec<SemanticDefinitionInventory>,
  bodies:Vec<SemanticBodyKey>
}
SemanticTargetInventory {
  manifest_ordinal:u64, target:TargetRoot, root_module:ModuleRef,
  contract:SemanticTargetContract
}
SemanticTargetContract = Library
  | Binary{root_world:SemanticDefinitionKey,main:SemanticDefinitionKey,
           capabilities:Vec<ManifestCapability>}
  | Environment{root_world:SemanticDefinitionKey,profile:String,
                reset:SemanticDefinitionKey,step:SemanticDefinitionKey,
                self_play:SemanticDefinitionKey}
ManifestCapability = Args|Atomics|Environment|Files|MonotonicClock|Stdio
  | Subprocess|Synchronization|Tcp|Threads|Udp|WallClock
SemanticDefinitionKey {
  module:ModuleRef, owner_path:Vec<u8>, kind:DeclarationKind,
  name:String, span:SourceSpan
}
SemanticDefinitionInventory {
  key:SemanticDefinitionKey, declared_visibility:Visibility,
  member_visibilities:Vec<SemanticMemberVisibility>, symbolic_shape:Vec<u8>
}
SemanticMemberVisibility {
  path:MemberVisibilityPath, declared_visibility:Visibility
}
MemberVisibilityPath = Field{ordinal:u64} | Variant{ordinal:u64}
  | VariantField{variant_ordinal:u64,field_ordinal:u64}
  | Method{ordinal:u64}
SemanticModuleInventory {
  module:ModuleRef, file:FileId, declared_visibility:Visibility,
  bindings:Vec<SemanticBindingInventory>
}
SemanticBindingInventory {
  name:String, namespace:Namespace, target:SemanticBindingTarget,
  declared_visibility:Visibility, origin:SemanticBindingOrigin
}
SemanticBindingTarget = Module(ModuleRef) | Definition(SemanticDefinitionKey)
SemanticBindingOrigin = Declaration
  | ReExport{source:SemanticBindingPath,target:SemanticBindingTarget}
SemanticBindingPath {
  module:ModuleRef, name:String, namespace:Namespace
}
CtfeBudgets { step_limit:u64, depth_limit:u64, heap_limit:u64 }
CtfeAccounting {
  charged_steps:u64, maximum_depth:u64, peak_heap:u64, final_heap:u64,
  event_count:u64, trace_digest:[u8;32]
}
CtfeResultRow {
  key:CtfeRootKey, ty:TypeId, digest:[u8;32], value:CtfeLogicalValue
}
CtfeDataRefProvenance {
  value_path:Vec<u64>, package:PackageId, data:DataId,
  kind:SourceString|IncludedBytes|IncludedString,
  ty:CoreType, source:DataSource, digest:[u8;32]
}
CtfeStaticBinding {
  definition:DefinitionId, key:CtfeRootKey, ty:TypeId, digest:[u8;32]
}
GenericCorePackage {
  package:PackageId, provenance:PackageProvenance, ctfe_budgets:CtfeBudgets,
  interface:PendingSkeleton|Final{hash:[u8;16]},
  targets:Vec<TargetRow>, files:Vec<SourceFile>, modules:Vec<ModuleRow>,
  data:Vec<DataRow>,
  types: Vec<TypeRow>,
  definitions: Vec<DefinitionRow>, traits: Vec<TraitRow>, impls: Vec<ImplRow>,
  closures: Vec<ClosureDescriptor>, generators: Vec<GeneratorDescriptor>,
  worlds: Vec<WorldDescriptor>, queries: Vec<QueryDescriptor>,
  type_consts: Vec<TypeConstDescriptor>, bodies: Vec<GenericCoreBody>,
  schedules: Vec<ScheduleDescriptor>
}
PackageProvenance {
  registry_origin:String, scoped_name:String, version:String,
  source:PackageSource,
  dependencies:Vec<PackageDependency>
}
PackageSource = Workspace{path:String,source_digest:[u8;32]}
  | Registry{archive_digest:[u8;32],source_digest:[u8;32],
             provenance_record_digest:[u8;32],
             inclusion_record_digest:[u8;32]}
  | EmbeddedCore{interface_version:u32,interface_digest:[u8;32]}
PackageDependency {
  alias:String, package:PackageId, requirement:String,
  kind:Normal|Development
}
TargetRoot = Library | Binary{name:String} | Environment{name:String}
ModuleRef { package:PackageId, target:TargetRoot, path:Vec<String> }
BindingPath { module:ModuleRef, name:String, namespace:Namespace }
DefinitionProvenance {
  module:ModuleRef, declared_visibility:Visibility
}
Visibility = DeclaringModule | AncestorModule{path:Vec<String>}
           | Package | Public
Namespace = Module | Type | Value
ModuleRow {
  module:ModuleRef, file:FileId, declared_visibility:Visibility,
  effective_visibility:Visibility, bindings:Vec<ModuleBindingRow>
}
ModuleBindingRow {
  name:String, namespace:Namespace, target:BindingTarget,
  declared_visibility:Visibility, effective_visibility:Visibility,
  origin:BindingOrigin
}
BindingTarget = Module(ModuleRef) | Definition(DefinitionId)
BindingOrigin = Declaration
  | ReExport{source:BindingPath,source_target:BindingTarget}
TargetRow {
  manifest_ordinal:u64, target:TargetRoot, contract:TargetContract
}
TargetContract = Library
  | Binary{root_world:DefinitionId,main:DefinitionId,
           capabilities:Vec<DefinitionId>}
  | Environment{root_world:DefinitionId,profile:String,
                reset:DefinitionId,step:DefinitionId,self_play:DefinitionId}
EnvironmentPointsToProof {
  package:PackageId, target:TargetRoot,
  sites:Vec<EnvironmentFunctionPointerSite>,
  reachable_bodies:Vec<ClosedBodyContext>
}
EnvironmentFunctionPointerSite {
  caller:ClosedBodyContext, point:ProgramPoint,
  targets:Vec<FunctionPointerTargetKey>
}
FunctionPointerTargetKey = Named {
  definition:DefinitionId, generic_arguments:Vec<GenericArgument>,
  signature:TypeId
} | ZeroCaptureClosure {
  owner:DefinitionId, expression_ordinal:u64,
  owner_arguments:Vec<GenericArgument>,
  closure_arguments:Vec<GenericArgument>, signature:TypeId
}
SourceFile { id: FileId, package_path: String, length: u64, digest: [u8;32] }
DataRow { id:DataId, kind:SourceString|IncludedBytes|IncludedString,
          bytes:Vec<u8>, ty:CoreType, source:DataSource, digest:[u8;32] }
DataSource = Literal{span:SourceSpan} | Included{file:FileId}
TypeRow { id: TypeId, tree: SemanticType }
DefinitionRow {
  id: DefinitionId, owner: Option<DefinitionId>, kind: DeclarationKind,
  semantic_key:SemanticDefinitionKey, name: String,
  generics: Vec<GenericParameter>, predicates: Vec<Predicate>,
  signature: DefinitionSignature, provenance:DefinitionProvenance,
  member_visibilities:Vec<MemberVisibilityRow>,
  body: Option<CoreBodyId>, span: SourceSpan
}
MemberVisibilityRow {
  path:MemberVisibilityPath, declared_visibility:Visibility,
  effective_visibility:Visibility
}
TraitRow { definition: DefinitionId, methods: Vec<DefinitionId> }
ImplRow {
  definition: DefinitionId, trait: Option<DefinitionId>, target: CoreType,
  is_default: bool, generics: Vec<GenericParameter>,
  predicates: Vec<Predicate>, methods: Vec<DefinitionId>
}
GenericCoreBody {
  id: CoreBodyId, owner: BodyOwner, span: SourceSpan,
  generics: Vec<GenericParameter>, predicates: Vec<Predicate>,
  parameters: Vec<LocalId>, result: CoreType,
  actual_requires: Vec<DefinitionId>, actual_throws: Vec<TypeId>,
  environment_forbidden:Vec<EnvironmentForbiddenOperation>,
  locals: Vec<Local>,
  move_paths: Vec<MovePath>, loans: Vec<Loan>,
  bound_region_bundles:Vec<BoundRegionBundle>,
  region_facts:Vec<RegionFact>,
  shared_storage_aliases:Vec<SharedStorageAliasFact>,
  shared_storage_facts:Vec<SharedStorageRegionFact>,
  unsafe_regions: Vec<UnsafeRegion>,
  entry: BlockId, auxiliary_drop_entries: Vec<GeneratorDropEntry>,
  blocks: Vec<Block>, ctfe_eligible: bool
}
EnvironmentForbiddenOperation = NondeterministicHostEffect
  | RawAddressObservation | UnsafeHostCall | Thread
Local { id: LocalId, name: Option<String>, ty: CoreType, mutable: bool,
        storage: Parameter|User|Temporary|Return|Capture{ordinal:u64},
        span: SourceSpan }
MovePath { id: MovePathId, parent: Option<MovePathId>, local: LocalId,
           projection: Option<Projection> }
Loan { id: LoanId, kind: Shared|Mutable, place: Place,
       issued_at: ProgramPoint, live_points: Vec<ProgramPoint> }
GenericKey { depth:u64, index:u64 }
FamilyPathStep = TupleField(u64) | ArrayElement
  | NominalField{definition:DefinitionId,variant:Option<u64>,field:u64}
  | OwnerPointee{kind:Box|Rc|Arc|Weak|Pin|Vec|Mutex|RwLock|Channel}
  | MapKey | MapValue | MapIterBorrow | JoinResult
  | MaybeUninitValue | Capture(u64)
  | GeneratorSlot{state:u64,slot:u64}
  | QueryBinding{query:DefinitionId,ordinal:u64} | Referent
BoundRegionBundle {
  id:BoundRegionBundleId, type_parameter:GenericKey,
  prefix:Vec<FamilyPathStep>
}
RegionSubject = Value(ValueId)|PlaceAt{place:Place,point:ProgramPoint}
RegionFact {
  subject:RegionSubject,
  bindings:Vec<RegionBinding>
}
RegionBinding = Family{family:u64,origins:Vec<RegionOrigin>}
              | Bundle{bundle:BoundRegionBundleId,
                       sources:Vec<BundleSource>}
GeneratorFrameToken { package:PackageId, body:CoreBodyId,
                      construct_at:ProgramPoint }
GeneratorDescriptorRef { package:PackageId, descriptor:GeneratorId }
RegionOrigin = Static | Bound{depth:u64,index:u64} | Loan(LoanId)
             | GeneratorSelf{token:GeneratorFrameToken,
                             descriptor:GeneratorDescriptorRef,
                             state:u64,family:u64}
TypeRegionSource = Argument(u64) | CallableReceiver | Capture(u64)
BundleSource = Incoming{source:TypeRegionSource,bundle:BoundRegionBundleId}
             | Static | Bound{depth:u64,index:u64} | Loan(LoanId)
             | GeneratorSelf{token:GeneratorFrameToken,
                             descriptor:GeneratorDescriptorRef,
                             state:u64,family:u64}
RegionSubstitution {
  formal_depth:u64, formal_index:u64, origins:Vec<RegionOrigin>
}
TypeRegionSubstitution {
  formal:GenericKey, concrete:CoreType, inputs:Vec<TypeRegionInput>
}
TypeRegionInput {
  source:TypeRegionSource,
  occurrence_path:Vec<FamilyPathStep>,
  binding:Family{concrete_family:u64,origins:Vec<RegionOrigin>}
        | Bundle{bundle:BoundRegionBundleId,sources:Vec<BundleSource>}
}
ClosedBodyContext {
  package:PackageId, body:CoreBodyId,
  generic_arguments:Vec<GenericArgument>,
  signature:Option<TypeId>
}
ClosedRegionView {
  caller:ClosedBodyContext, call_site:ProgramPoint, target:RegionViewTarget,
  lifetime_substitutions:Vec<RegionSubstitution>,
  type_substitutions:Vec<TypeRegionSubstitution>,
  expanded_facts:Vec<ClosedRegionFact>,
  expanded_shared_aliases:Vec<ClosedSharedStorageAliasFact>,
  expanded_shared_facts:Vec<ClosedSharedStorageRegionFact>
}
RegionViewTarget = Body(ClosedBodyContext)
  | Intrinsic{intrinsic:IntrinsicId,
              generic_arguments:Vec<GenericArgument>,
              const_arguments:Vec<ConstArgument>,result:CoreType}
  | GeneratorConstruct{descriptor:GeneratorDescriptorRef,
                       generic_arguments:Vec<GenericArgument>,
                       call_trait:Fn|FnMut|FnOnce,state_type:CoreType}
ClosedRegionFact {
  namespace:Caller|Target, subject:RegionSubject,
  bindings:Vec<ClosedRegionBinding>
}
ClosedRegionBinding { family:u64, origins:Vec<ClosedRegionOrigin> }
ClosedRegionOrigin = Static | Bound{depth:u64,index:u64}
  | Incoming{source:TypeRegionSource,path:Vec<FamilyPathStep>,family:u64}
  | LocalLoan{namespace:Caller|Target,package:PackageId,
              body:CoreBodyId,loan:LoanId}
  | GeneratorSelf{namespace:Caller|Target,token:GeneratorFrameToken,
                  descriptor:GeneratorDescriptorRef,state:u64,family:u64}
SharedStorageToken {
  package:PackageId, body:CoreBodyId, create_at:ProgramPoint,
  kind:Rc|Arc|Mutex|RwLock|Channel
}
SharedStorageRef = Token(SharedStorageToken)
                 | Incoming{source:TypeRegionSource,path:Vec<FamilyPathStep>}
SharedStorageAlias { path:Vec<FamilyPathStep>, storage:SharedStorageRef }
SharedStorageAliasFact { subject:RegionSubject,
                         aliases:Vec<SharedStorageAlias> }
SharedStorageRegionFact { storage:SharedStorageRef, point:ProgramPoint,
                          bindings:Vec<RegionBinding>,
                          aliases:Vec<SharedStorageAlias> }
ClosedSharedStorageRef = Token(SharedStorageToken)
  | Incoming{source:TypeRegionSource,path:Vec<FamilyPathStep>}
ClosedSharedStorageAlias {
  path:Vec<FamilyPathStep>, storage:ClosedSharedStorageRef
}
ClosedSharedStorageAliasFact {
  namespace:Caller|Target, subject:RegionSubject,
  aliases:Vec<ClosedSharedStorageAlias>
}
ClosedSharedStorageRegionFact {
  namespace:Caller|Target, storage:ClosedSharedStorageRef,
  point:ProgramPoint, bindings:Vec<ClosedRegionBinding>,
  aliases:Vec<ClosedSharedStorageAlias>
}
UnsafeRegion { id: UnsafeRegionId, parent: Option<UnsafeRegionId>, span: SourceSpan }
Block { id: BlockId, parameters: Vec<BlockParameter>,
         instructions: Vec<Instruction>, terminator: Terminator }
BlockParameter { value: ValueId, ty: CoreType, role: Ordinary|Result|UnwindToken }
Instruction { result: Option<ValueId>, result_type: Option<CoreType>,
              kind: InstructionKind, unsafe_region: Option<UnsafeRegionId>,
              span: SourceSpan }
Terminator { kind: TerminatorKind, unsafe_region: Option<UnsafeRegionId>,
             span: SourceSpan }
Place { root: Local(LocalId)|Static(DefinitionId), projections: Vec<Projection> }
Operand = Constant(CoreConstant) | Value(ValueId) | Copy(Place) | Move(Place)
CallableReceiver = Shared{place:Place,loan:LoanId}
  | Mutable{place:Place,loan:LoanId} | Once{operand:Operand}
CaptureInit = Shared{ordinal:u64,source:Place,loan:LoanId}
  | Mutable{ordinal:u64,source:Place,loan:LoanId}
  | Transfer{ordinal:u64,source:Place,value:Operand}
Continuation { target: BlockId, arguments: Vec<Operand> }
SourceSpan { file_id: FileId, start_byte: u64, end_byte: u64,
             start_line: u64, start_column: u64,
             end_line: u64, end_column: u64 }
```

Source bytes are zero-based and every span is the half-open UTF-8 byte interval
`[start_byte,end_byte)` in its immutable SourceFile; endpoints must lie on
Unicode-scalar boundaries and `start_byte <= end_byte <= SourceFile.length`.
Lines and columns are one-based positions at those same endpoints. LF advances
the line and resets the column to one; CR advances the byte offset but neither
line nor column, so CRLF is one newline and a bare CR is zero display columns.
Every other Unicode scalar, including a tab, advances the column by exactly
one regardless of its UTF-8 width or display width. The end line/column is the
exclusive position after the final scalar. EOF is the unique zero-width span at
the source length and final line/column; missing-token and missing-delimiter
diagnostics use that captured EOF position. Checked overflow or an invalid UTF-8
snapshot is a status-1 source failure rather than a wrapped span.

The referenced record/enums are closed as follows:

```text
CoreType = Type(TypeId) | UnwindToken | ExceptionTag
BodyOwner = Definition{definition:DefinitionId}
  | Closure{closure:ClosureId}
  | Generator{generator:GeneratorId}
  | WorldInitializer{world:DefinitionId}
  | CtfeRoot{key:CtfeRootKey}
  | TypeConst{owner:DefinitionId,ordinal:u64,
              purpose:ArrayLength|RepeatCount|IntegerGenericArgument,
              mode:ClosedRoot|ParameterizedTemplate}
GenericParameter {
  index: u64, name: String,
  kind: Type | Lifetime | IntegerConst{integer_type: IntegerType}
}
GenericArgument = Type(CoreType) | Lifetime(LifetimeValue)
                | IntegerConst(IntegerConstValue)
LifetimeValue = Static | Bound{depth:u64,index:u64} | ErasedLocal
IntegerConstValue = Literal{integer_type:IntegerType,bits:FixedWidthBits}
                  | Bound{depth:u64,index:u64}
                  | Expression{canonical_tree:Vec<u8>}
FixedWidthBits = LittleEndian{bytes:Vec<u8>}
Predicate = Trait {
              trait_definition: DefinitionId, self_type: CoreType,
              arguments: Vec<GenericArgument>
            }
          | LifetimeOutlives{longer:LifetimeValue,shorter:LifetimeValue}
          | TypeOutlives{ty:CoreType,lifetime:LifetimeValue}
ParameterSignature {
  name: Option<String>, ty: CoreType,
  mode: Value|Shared|Mutable|CapabilityShared|CapabilityMutable
      | ResourceRead|ResourceWrite
      | Query{query:DefinitionId}|Commands
}
FieldSignature { name:String, visibility:Visibility, ty:CoreType }
VariantSignature { name:String, form:Unit|Tuple|Record,
                   fields:Vec<FieldSignature> }
DefinitionSignature = World
  | Record{form:Unit|Tuple|Record,fields:Vec<FieldSignature>}
  | Enum{variants:Vec<VariantSignature>}
  | Tag
  | Callable {
      kind:Function|Generator|System|TraitMethod|ImplMethod,
      unsafe_:bool, parameters:Vec<ParameterSignature>, result:CoreType,
      resume:Option<CoreType>, yields:Option<CoreType>,
      requires:Vec<DefinitionId>, throws:Vec<TypeId>
    }
  | Trait{methods:Vec<DefinitionId>}
  | Impl{trait_definition:Option<DefinitionId>,target:CoreType,
         methods:Vec<DefinitionId>,is_default:bool}
  | Alias{target:CoreType}
  | Const{ty:CoreType}
  | Static{ty:CoreType,mutable:bool}
  | Query{owner_system:DefinitionId,parameter_name:String,
          terms:Vec<QuerySignatureTerm>}
  | Schedule{runs:Vec<SystemRun>,
             requires:Vec<DefinitionId>,throws:Vec<TypeId>}
Capture { ordinal:u64, frame_place:Place, mode:Shared|Mutable|Move, ty:CoreType }
ClosureDescriptor {
  id:ClosureId, owner:DefinitionId, expression_ordinal:u64,
  captures:Vec<Capture>, call_trait:Fn|FnMut|FnOnce,
  parameters:Vec<CoreType>, result:CoreType,
  region_flows:Vec<CallableRegionFlow>,
  requires_boundary:Declared|Inferred,
  throws_boundary:Declared|Inferred,
  requires:Vec<DefinitionId>, throws:Vec<TypeId>,
  creation_body:CoreBodyId, body:CoreBodyId,
  span:SourceSpan
}
CallableRegionFlow {
  target:Result{family:u64}|Yield{family:u64}
       | Capture{ordinal:u64,family:u64}|Frame{state:u64,family:u64},
  sources:Vec<CallableRegionSource>
}
CallableRegionSource = Static
  | Parameter{ordinal:u64,path:Vec<FamilyPathStep>}
  | Resume{path:Vec<FamilyPathStep>}
  | Capture{ordinal:u64,path:Vec<FamilyPathStep>}
  | ReceiverCapture{ordinal:u64,path:Vec<FamilyPathStep>}
  | Frame{state:u64,path:Vec<FamilyPathStep>}
GeneratorDescriptor {
  id:GeneratorId, owner:DefinitionId, expression_ordinal:u64,
  captures:Vec<Capture>, factory_unsafe:bool,
  factory_call_trait:Fn|FnMut|FnOnce,
  parameters:Vec<CoreType>, state_type:CoreType,
  resume:CoreType, yields:CoreType, result:CoreType,
  region_flows:Vec<CallableRegionFlow>,
  requires_boundary:Declared|Inferred,
  throws_boundary:Declared|Inferred,
  requires:Vec<DefinitionId>, throws:Vec<TypeId>, state_count:u64,
  creation_body:Option<CoreBodyId>,
  body:CoreBodyId, drop_entries:Vec<GeneratorDropEntry>, span:SourceSpan
}
GeneratorDropEntry { state:u64, block:BlockId }
WorldDescriptor {
  definition:DefinitionId, initializer:CoreBodyId,
  resources:Vec<TypeId>, span:SourceSpan
}
QueryDescriptor {
  definition:DefinitionId, owner_system:DefinitionId,
  parameter_index:u64, parameter_name:String, terms:Vec<QueryTerm>,
  span:SourceSpan
}
QueryTerm { source_index:u64, ty:CoreType, access:Read|Write|Exclude,
            binding_ordinal:Option<u64> }
QuerySignatureTerm { ty:CoreType, access:Read|Write|Exclude }
ScheduleDescriptor {
  definition:DefinitionId, runs:Vec<SystemRun>,
  requires:Vec<DefinitionId>, throws:Vec<TypeId>, span:SourceSpan
}
SystemRun { system:DefinitionId, arguments:Vec<SystemGenericArgument> }
SystemGenericArgument = Type(CoreType) | IntegerConst(IntegerConstValue)
TypeConstDescriptor {
  owner:DefinitionId, ordinal:u64,
  purpose:ArrayLength|RepeatCount|IntegerGenericArgument,
  mode:ClosedRoot|ParameterizedTemplate, integer_type:IntegerType,
  expression:IntegerConstValue, body:CoreBodyId,
  result_key:Option<CtfeRootKey>, span:SourceSpan
}
ProgramPoint = BlockEntry{block:BlockId}
  | BeforeInstruction{block:BlockId,index:u64}
  | AfterInstruction{block:BlockId,index:u64}
  | BeforeTerminator{block:BlockId}
  | Edge{block:BlockId,successor_ordinal:u64}
ConstArgument = Integer(IntegerConstValue) | Data(DataId)
              | Definition(DefinitionId)
SwitchCase = Scalar(SwitchConstant) | ExceptionOrdinal{ordinal:u64,ty:TypeId}
SwitchConstant = Bool{byte:u8} | Char{scalar:u32} | Entity{bits:u64}
               | Integer{integer_type:IntegerType,bits:FixedWidthBits}
```

`FixedWidthBits.bytes` has exactly `IntegerType.width / 8` bytes, with
`isize`/`usize` fixed at 64 bits. The least-significant byte appears first;
unsigned values use their ordinary binary representation and signed values use
the same-width two's-complement representation. A shortened, extended, or
nonmatching payload is invalid rather than sign-extended, truncated, or
reinterpreted through a host integer.

`SemanticType`, `IntegerType`, `Visibility`, and declaration/member kinds are
the exact tag trees already fixed by the identity tables above. Definition kind
and signature variant must match; a non-generator body has no auxiliary drop
entry, and named generator descriptors use expression ordinal zero while
anonymous expressions use their nonzero body preorder ordinal. Program-point
instruction indices are zero-based; successor ordinals use the canonical edge
order below. No open payload map, implementation-defined enum, or Rust host type
is permitted in these rows.

A named generator item's executable body has `BodyOwner::Definition`; its
ordinal-zero GeneratorDescriptor is a reciprocal state-machine description of
that same body. It has empty captures, factory call trait `Fn`, parameters
byte-for-byte equal to its Callable signature, and a target-tag-1 state_type
containing the complete explicit substitution and synthesized bound-lifetime
slots; call-local origins are carried by RegionFact rather than that TypeId. Its
factory_unsafe bit equals that Callable signature's unsafe bit and
`creation_body` is absent; both boundary discriminators are Declared and its
effect vectors equal that Callable boundary. An anonymous
descriptor has a nonzero ordinal and factory_unsafe false,
parameters equal to its factory signature, the inferred factory call trait, and
tag-28 state_type, with `creation_body` naming its unique lexical Make owner.
Each ClosureDescriptor likewise names its unique lexical creation body. In both
anonymous descriptor kinds, `requires_boundary` and `throws_boundary` are
independently Inferred exactly when source omitted that set and no expected
callable supplied its boundary; otherwise that discriminator is Declared. The
descriptor vectors always store the resulting callable boundaries. Each body's
`actual_requires` must equal `requires` only for an Inferred requires boundary
and otherwise be a subset; `actual_throws` follows the corresponding independent
throws rule.
In both
generator forms the body parameter locals correspond one-for-
one to the descriptor parameter types and are frame-initialized by construction
before ordinary entry; they are not fresh resume-block operands. Anonymous
closure/generator bodies use their respective
descriptor owners. Const/static/function/method/system bodies use Definition,
world init uses WorldInitializer, and every array length, repeat count, or
integer const-generic expression not itself a definition uses TypeConst. The
matching TypeConstDescriptor repeats the owner/purpose/ordinal/mode, contextual
integer type, canonical expression tree, span, and body ID. In
CompleteWorkspace a ClosedRoot has exactly one result_key naming its successful
row, while a ParameterizedTemplate has none and awaits D-time substitution; the verifier rejects
any disagreement or any other owner/descriptor pairing.

`BodyOwner::CtfeRoot` is legal only for the one ordinary entry body of a
RootSlice, must equal that scope's root key, and has no TypeConstDescriptor or
DefinitionRow requiring the not-yet-final containing ID. All other slice bodies
use their final ordinary owners. CompleteWorkspace forbids CtfeRoot and replaces
each successful root with its final Definition/TypeConst owner plus the matching
CtfeResultRow commitment.

`QueryTerm.binding_ordinal` is absent for exclusions and zero-sized/tag terms;
otherwise it is the dense zero-based binding position obtained by filtering the
source term list in order. A query loop's pattern creates its own LocalIds from
those positions, so a reusable QueryDescriptor never stores loop-local IDs.
Every `SystemRun` targets a system, supplies exactly its type/const generic
parameters in declaration order, and is closed; the dispatch lifetime is
implicit. Schedule descriptors are world-independent package items. M27-D links
each use to an `App<W>`/root world and revalidates queries, resources, initializer
flow, and every run against W rather than baking a binary world into generic
package Core.

`CoreType` is exactly a semantic type referenced through the package type table or one of
the verifier-private pseudo-types `UnwindToken` and `ExceptionTag`. Neither
pseudo-type may appear in a declaration, local, aggregate, place, return, or
serialized value. `ExceptionTag` is a checked `u64` ordinal into the body's raw-
TypeId-sorted throws set and is legal only as the result of `catch.type` and the
discriminant of its immediately dominated catch `switch`. That switch uses only
ExceptionOrdinal cases; each case's ordinal and TypeId must name the same exact
throws-set row, cases are strictly ascending, and default handles only the
still-unmatched canonical remainder. Scalar cases are forbidden for this
pseudo-type, and ExceptionOrdinal is forbidden for every other discriminant.
An ordinary switch discriminant is exactly bool, char, entity, or one concrete
integer type; every Scalar case has that identical type, cases are unique, and
sort by the unsigned lexicographic bytes of `(case tag, fixed-width payload)`.
Float, string, DataRef, FunctionRef, unit, and aggregate decisions lower through
typed comparisons and branches instead of switch cases.

The reference families of a `CoreType` are numbered densely from zero by one
canonical, descriptor-aware logical-storage graph traversal, not by scanning
only the top-level TypeId preimage. The traversal expands aliases; visits
tuple positions, one unioned array-element shape, and substituted nominal
fields in declaration order;
enumerates every enum variant in variant/field order with that variant in the
family key; and visits the element/key/value/pointee type once for each approved
owning wrapper or collection. On every reference it emits a family whose key is
its complete tuple/field/variant/owner path and stable lifetime shape and stops
at that reference; referent projections create separate facts as fixed below.
Repeated uses of one lifetime
binder at distinct paths remain distinct families. A dynamic collection's
element family represents the canonical union of origins for all its elements.

Traversal maintains a stack of normalized nominal `(DefinitionId,
substitution)` anchors. Re-entering the identical key through an owning
indirection emits a checked back-edge to that anchor: every dynamic recursive
occurrence contributes to the anchor's already-defined region families rather
than being omitted or expanded again. Re-entering the same nominal DefinitionId
with a different substitution is a nonregular recursive region shape and is
`TYPE001` before Core; inline recursion remains rejected by the ordinary sized
rule. Families are assigned by first DFS emission with variants and fields in
the order above; back-edges never allocate another family. Alias spelling
creates no family. The verifier has every referenced descriptor/back-edge input
in the branded package closure and independently recomputes this finite graph.

The storage traversal is exhaustive by semantic type form:

| Type form | Region-family storage rule |
|---|---|
| integers, floats, bool, char, entity, unit, never, str, slice | no embedded family |
| reference | emit exactly its own lifetime family and stop; referent fields are not stored in the reference value |
| raw pointer or function pointer | no embedded family; pointed-to/signature lifetimes are not stored values |
| array | traverse the element shape once and union origins across all indices |
| tuple | traverse fields in index order |
| ordinary record/enum nominal | traverse substituted fields in declaration order; enum families include variant identity |
| String and payload-free capability/OS/thread handles | no embedded family |
| Box, Vec | traverse the owned pointee/element shape once; dynamic occurrences union |
| Rc/Arc/Weak | the handle embeds no payload family; its alias fact selects shared storage whose region fact traverses T once |
| Pin<P> | traverse the stored pointer-owner P by P's own rule and never implicitly traverse P's referent |
| Map | traverse key then value shape once; dynamic entries union per side |
| MapIter<'a,K,V> | emit the shared map-borrow family; K/V are reached only through iterator projections and the referent summary |
| JoinHandle<T,E...> | traverse the possibly pending/completed result T under JoinResult and track its nested shared aliases; all reference origins are Static by the unscoped spawn bound, exception payloads are reference-free, and scoped threads expose no handle |
| MaybeUninit | traverse T with activation conditional on initialization |
| bound type | emit its canonical opaque bound-region bundle regardless of predicates; TypeOutlives constrains the bundle but never creates it |
| Mutex/RwLock/channel endpoints and queues | the handle embeds no payload family; its alias fact selects shared storage whose region fact traverses/union-stores T |
| mutex/RwLock guards | emit the guard's lock-borrow family and preserve the selected shared-storage alias; accesses to T use that storage fact plus the referent summary |
| QueryBindings/Q cursor/resource guards | traverse descriptor-selected binding families and emit the cursor/guard world-borrow family; App, Commands, and payload-free query markers add none |
| closure or generator factory | traverse descriptor captures in ordinal order only; callable signatures/produced types are not stored fields |
| generator state | traverse descriptor captures, initial arguments, and frame locals separately for state zero and each suspension; state identity qualifies every family |

`FamilyPathStep` tag order is exactly the schema order: `1=TupleField`,
`2=ArrayElement`, `3=NominalField`, `4=OwnerPointee` (owner subtags follow the
listed owner order), `5=MapKey`, `6=MapValue`, `7=MapIterBorrow`,
`8=JoinResult`, `9=MaybeUninitValue`, `10=Capture`, `11=GeneratorSlot`,
`12=QueryBinding`, and `13=Referent`; numeric
payloads are `u64le`, DefinitionIds are raw 16-byte values, and Option uses
`00=absent`/`01=present`. These are proof-key bytes, not stable public IDs.

Every occurrence of a bound type in a region-bearing storage shape creates one
`BoundRegionBundle`, even when that parameter has no TypeOutlives predicate.
Rows sort by `(type_parameter.depth, type_parameter.index, prefix bytes)` and
receive dense IDs in that order. A bundle is a universally quantified,
pointwise family pack: substituting a concrete type expands it to every family
of that type at the recorded prefix. TypeOutlives constrains every expanded
member but never creates or removes the bundle. At a generic body entry, each
active bundle is sourced from the corresponding argument, callable receiver,
or capture's `Incoming{source,bundle}` token. Argument ordinals are zero-based;
every `TypeRegionSource::Capture`, `FamilyPathStep::Capture`,
`CallableRegionSource::Capture`, `CallableRegionFlow` capture target,
`CallableRegionSource::ReceiverCapture`,
`Capture`, `CaptureInit`, and capture Local uses the same one-based descriptor
ordinal and reserves zero. Move/copy/reborrow preserves the
pack, projection uses its path, and joins union `BundleSource` sets pointwise;
there is no all-families scalar union. This lets `id<T>` preserve every family
and lets a selection between two T values union the two incoming maps per
concrete family without swapping or collapsing fields.

`RegionBinding::Family` is used for a concrete family; `Bundle` is used only
for a bound pack in a generic template. Bindings sort by variant tag then
family or bundle ID. Bundle sources sort as `Incoming`, `Static`, `Bound`,
`Loan`, `GeneratorSelf`, with the corresponding canonical payload order; the
vector is nonempty and unique whenever the bundle is active.
Every generic call carries one `TypeRegionSubstitution` for each referenced type
parameter, sorted by GenericKey. Its CoreType equals the ordinary generic
argument. Inputs sort by source tag (`Argument`, `CallableReceiver`, `Capture`),
source ordinal, occurrence-path bytes, binding tag (`Family` before `Bundle`),
then concrete family or bundle ID. A generic-to-generic template edge may use a
Bundle binding with its exact symbolic sources. Every executable closed edge
must instead contain only Family bindings covering exactly every active
concrete family of every actual argument, receiver, or capture occurrence;
origins are redundant proof data rederived from those actual facts. Expansion
replaces each Incoming bundle source with the matching per-family origin row,
while Static/Bound/Loan/GeneratorSelf sources are evaluated pointwise at the
same implicit family member. Parameterized edges compose callee bundles
pointwise onto caller bundles. ClosedRegionView construction transitively
composes that finite graph from the concrete call-site facts; recursive SCCs
take the least pointwise fixed point seeded by those concrete inputs and reuse
it per frame. A component with no concrete anchor or any surviving Bundle arm
on an executable view is unresolved and `CORE003`. Missing, extra, lossy,
reordered, or cross-family
substitution is `CORE003`.

Shared interior storage has a separate verifier-only provenance. Each Rc/Arc,
Mutex/RwLock, and channel construction site has one structural
`SharedStorageToken{package,body,create_at,kind}`; both channel endpoints share the one
Channel token. Every dynamic evaluation mints a hidden storage instance under
that token. Clone, downgrade/upgrade, endpoint projection, and guard creation
preserve the instance; branch joins union possible references. Like generator
frame instances, dynamic storage identity is never serialized or hashed, and
the structural token alone cannot substitute one live instance for another.
For loops/joins the static proof conservatively unions all possible instances
from one token while the evaluator/runtime value retains its exact hidden
instance.

`SharedStorageAliasFact` maps each live Rc/Arc/Weak/lock/guard/channel value or
nested handle path in an aggregate to its possible Token/Incoming storage
references; a direct handle uses the empty path. A `SharedStorageRegionFact`
holds the initialized payload families and nested shared-storage aliases at a
mutation ProgramPoint. Alias rows sort by path bytes then reference; Token
sorts before Incoming and token payload is `(package,body,create_at,kind)`.
Facts sort by RegionSubject or storage reference/ProgramPoint and use the
ordinary canonical RegionBinding/alias rules. Creation publishes the initial
payload fact; Rc/Arc/Weak operations,
lock guards, and the two channel endpoints never fork the storage authority.
Mutation through any alias—including an `&self` sender, lock, or guard—publishes
one new fact for every possible reference according to the operation's normal/
checked/unwind publication rule. Reads, receives, upgrades, and guard
projections use the unique reaching fact for each possible reference and union
their families. A borrow origin remains live while any reachable storage fact
may contain it; removal/drop may conservatively retain it but may never forget
it early.

Generic bodies use Incoming shared-storage references and closed views qualify
Caller versus Target alias/storage facts exactly like ordinary region facts.
Body calls and sealed intrinsics propagate storage summaries to the least fixed
point, so a write through one endpoint/alias is visible through every other
alias. Intrinsic/GeneratorConstruct views contain Caller shared facts only; a
Body view may contain both namespaces. The verifier independently derives
tokens, alias preservation, mutation/publication, reaching facts, and closed-
view expansion. Missing, extra, cross-instance, stale, or weakened storage
facts are `CORE003`.

Option/Result and other sealed value enums use the ordinary enum row. A
reference projection that reads a field of its referent creates a new Value or
PlaceAt fact for that projected value; it never retroactively adds pointee
families to the reference itself. No other implicit type form or traversal rule
exists.

A `RegionFact` has one binding for exactly each initialized, possibly active
concrete reference family or generic bound bundle at that subject/point and none for a definitely inactive enum
variant, uninitialized MaybeUninit field, or moved path. Because the variant is
part of the family key, mutually exclusive variants never collide. Joins union
both possible family activation and origins; enum refinements and move paths
filter them. `MaybeUninit(Some)` propagates origins, while AssumeInit without
valid propagated origins remains unsafe UB and deterministic `CTFE008`.
Bindings use the canonical variant/family-or-bundle order above and are unique.
`Value` subjects are the defining SSA result or block parameter.
`PlaceAt` subjects are the parameter/initialization/replacement definition point
or one reference-family-mutating instruction/Invoke edge of a memory-held value,
and later uses obtain the unique reaching fact. Facts
sort first by subject tag (`Value` before `PlaceAt`), then ValueId or canonical
Place and ProgramPoint encoding. Each `Family` binding's origin vector is nonempty,
unique, and sorted by tag (`Static`, `Bound`, `Loan`, `GeneratorSelf`) then
depth/index, LoanId, or canonical `(token package/body/site, descriptor
package/id, state, family)` bytes.
Control-flow joins take exact set union, allowing one reference value to select
static, bound, multiple local-loan, or verified generator-self origins on
different incoming edges. A
`Loan` origin names one exact Loan row whose issue point, place, kind, live
points, and outlives solution match the use; `Static` and `Bound` match the
stable type binder or satisfy its verified outlives coercion. No LoanId is
serialized into or hashed as a TypeId; an `ErasedLocal` family accepts only the
independently derived origin set in its RegionFact. Missing,
extra, duplicate, nonreaching, or dead-loan facts are `CORE003`.

Each GeneratorConstruct site has one structural
`GeneratorFrameToken{package,body,construct_at}`. Every dynamic evaluation mints a
distinct hidden affine frame instance under that token; the instance is
evaluator/runtime provenance, never a Core/stable ID. It travels with the
generator value through Pin<Box> owner moves, Pin reborrows, helper calls, and
returns, and destruction invalidates it. Clone/Copy/detach is impossible. A
structurally identical token at two dynamic evaluations does not authorize
cross-instance substitution; ordinary SSA/place/alias dataflow and the hidden
affine instance must match.

`GeneratorFrameToken.construct_at` is exactly the GeneratorConstruct
instruction's `ProgramPoint::BeforeInstruction{block,index}`. Rc/Arc new,
mutex/rwlock new, and channel new are Invoke intrinsics, so
`SharedStorageToken.create_at` is exactly that Invoke's
`ProgramPoint::BeforeTerminator{block}`; the two channel result endpoints use
the same token. No Edge/After/other point variant is legal for either token.

`GeneratorSelf` is the sole cross-body self origin. A yielded edge converts each
internal generator-body Loan whose place is stored within that pinned frame and
remains live in the suspension into
`GeneratorSelf{token,descriptor,state,family}` in the caller's fact for the
exact pinned generator instance. It is live only while that same instance has
that descriptor and suspension state. Resume maps it back to the uniquely
corresponding internal frame-loan/family proof, and the next yield rebuilds the
external set; completion, escaping checked throw, panic, trap, or drop consumes
it with the frame. The verifier derives both directions from the descriptor,
Yield state, internal Loan rows, frame-token dataflow, and resume receiver, and
rejects an origin for another body, instance, state, or family. Helper-call
views carry it as qualified Incoming provenance rather than rebasing it to a
syntactic Place. A body-local LoanId never crosses a Core
body boundary.

Every direct, trait, function-pointer, closure, generator-resume, sealed-
intrinsic call, and GeneratorConstruct carries the exact canonical lifetime
`RegionSubstitution` and `TypeRegionSubstitution` tables for its callable
signature. Lifetime rows are unique and sorted by formal `(depth,index)` and
cover every bound lifetime instantiated at that site;
their nonempty origin vectors use RegionOrigin order above. The verifier derives
them from the formal parameter/receiver family paths, actual operand and
callable-capture RegionFacts, declared outlives constraints, and the lexical
generic-argument positions. `ErasedLocal` may remain in the structural
GenericArgument list, but never substitutes for this binder-keyed map.
Function-pointer alternatives share one alpha-normalized signature map.

Each closure and generator descriptor also carries the complete verifier-
derived `CallableRegionFlow` summary for origins not expressible by its public
signature binders. Targets sort `Result`, `Yield`, `Capture`, `Frame` then by
their numeric fields. Sources sort `Static`, `Parameter`, `Resume`, `Capture`,
`ReceiverCapture`, `Frame` then ordinal/state and path bytes. Closure rows use
only Result/Capture targets and Static/Parameter/Capture/ReceiverCapture
sources. Generator rows may additionally
map yielded/completed results and state-qualified frame families from initial
parameters, resume input, captures, or prior frame state. The standalone
verifier recomputes the exact least dataflow summary from the descriptor body;
a producer cannot widen or omit it. ClosureCall and GeneratorResume combine
these rows with the lifetime and type-region substitutions and the actual
callable/frame fact. Thus two closure instances borrowing different locals
produce different result origins, and a generator may yield or return a
capture-derived reference without hashing that local region into its TypeId.
`ReceiverCapture` means a borrow of owned capture storage, not a reference
already stored in that capture. It is instantiated from the exact Shared or
Mutable CallableReceiver loan/reborrow and is legal only for Fn/FnMut; its
result lifetime is no longer than that receiver loan. On a normal ClosureCall
edge, every ReceiverCapture-derived result family takes a checked transfer of
that receiver reborrow; the reborrow remains live until the last derived value
or projection dies. A Mutable transfer keeps the callable exclusively borrowed,
so the callable cannot be called, mutated, moved, or dropped during that
interval. When no result family derives from ReceiverCapture, the reborrow ends
at normal completion. An unwind edge produces no result and ends the reborrow
only after its cleanup. An FnOnce/Once receiver cannot return or store such a
borrow because it consumes the callable. The verifier derives the capture
ordinal/path and exact transfer from the body projection and rejects a dangling,
stronger, cross-instance, prematurely ended, or duplicated receiver source.
`GeneratorSelf` remains reserved for an internal self-reference into the pinned
frame and is not a substitute for these capture/argument flow rows.

For a formal lifetime nested below a reference or slice, derivation uses one
canonical referent-projection summary without treating the referent as embedded
storage. Starting at the actual outer reference family, `Loan` follows its
exact Loan.place to the unique reaching PlaceAt fact and then follows the
formal projection path; `Static` and `Bound` follow the corresponding stable
type path; `GeneratorSelf` follows the checked descriptor/state family. A
visited set keyed by `(origin, formal projection path)` terminates cycles and a
re-entry unions the already-derived origins. The resulting map must cover every
formal nested lifetime exactly once. Direct, trait, closure, intrinsic, and
function-pointer calls use this same signature-driven algorithm; an indirect
call never requires inspecting a target body. These summaries are verifier-
derived proof values, are not stored as reference-value families, and cannot be
asserted by a producer.

On a normal edge, result families replace each formal bound lifetime with its
mapped origins, expand every bound bundle pointwise through the type-region
table, and apply any descriptor flow; static families remain static. A checked exception carrier has
no region transfer because thrown types are transitively reference/raw-pointer-
free. Every place that may change reference-bearing storage has a canonical
`PlaceAt` fact at the mutating instruction or outgoing Invoke `Edge` ProgramPoint,
not only at parameter/init/replace. For a general user/closure/function-pointer/
trait call, each mutable receiver/argument's outgoing possible-origin set is
the pre-call set union every mapped origin permitted by that referent family's
lifetime shape, plus canonical `Static` for every possibly written static
family. It activates every enum/owner family that the complete mutable referent
shape may write, including a previously inactive family. That conservative
committed-write update applies on normal,
checked-exception, and panic/trap-unwind edges; safe code may retain a loan
longer but can never forget a possibly stored reference. Read-only arguments do
not update.

Sealed intrinsics use their fixed transactional semantics: a successful
Vec/Map/owner insertion or replacement adds exactly the input families selected
by its signature; an allocation/error edge that promises no publication keeps
the pre-call fact; removal may conservatively retain prior possible origins;
and read-only operations never update. GeneratorConstruct derives the result
fact from captures/arguments. GeneratorResume replaces the generator referent's
fact with the descriptor-state-qualified union on a yielded normal edge and
with the empty terminal-frame fact on completion, escaping checked throw,
panic, or trap after cleanup. The verifier recomputes all substitutions and
edge facts from Core/signatures/descriptors/intrinsic rows, checks reaching
facts independently of MIR, and rejects any missing, extra, or weakened update.

`CoreConstant` is exactly `Unit`; `Bool{byte}`; `Char{scalar:u32}`;
`Entity{bits:u64}`; `Integer{integer_type,bits:FixedWidthBits}`;
`Float32{bits:u32}`; `Float64{bits:u64}`; `DataRef{data:DataId}`; or
`FunctionRef{definition:DefinitionId,generic_arguments:Vec<GenericArgument>,
signature:TypeId}`; or
`CtfeResultRef{key:CtfeRootKey,ty:TypeId,digest:[u8;32]}`. DataRef obtains its exact reference/array/slice type from
DataRow. FunctionRef is legal only for a fully resolved function item whose
signature is a legal safety/variance/effect subtype of the named target
function-pointer TypeId; the target signature is the constant's result type and
the referenced definition/substitution remain its invocation identity.
Aggregate and owned constants are constructed through instructions/intrinsics
so their evaluation and allocation order is explicit, except for the sealed
CtfeResultRef materialization rule below. `Projection` is exactly
`Field{owner,index,type}`, `Tuple{index,type}`, `Variant{owner,variant,index,type}`,
`ArrayConstant{index,type}`, `Index{value,type}`, `DerefShared{type}`,
`DerefMutable{type}`, or `DerefRaw{type,mutability}`. Every embedded type is the
projected result and is rechecked rather than trusted.

Operand ownership is closed. `Constant` produces the constant's typed value;
`Copy(place)` is legal only for an initialized `Copy` path and leaves it
initialized; `Move(place)` transfers ownership and deinitializes the exact move
path. For `Value(v)`, the verifier derives Copy from `v`'s CoreType. A Copy SSA
value may be used any number of times. Every non-Copy SSA definition or block
parameter is affine: on each reachable dynamic path it is consumed exactly once
by one ownership-taking operand position, transferred once through the selected
continuation into one block parameter, or first installed into one Place. It
may be named on mutually exclusive successor edges, but the selected edge alone
consumes it. It may not be copied, silently abandoned, or consumed twice.

A non-Copy SSA value live across a block edge must be transferred as an explicit
continuation argument and rebound to a block parameter; a later block cannot
use its predecessor ValueId directly. Every InvokeOperation and instruction
signature fixes each operand position as borrow/copy/consume from its semantic
type, and verification rederives that classification. Callee/owner-consuming
arguments transfer before body entry; a reborrow preserves the owner and has
the exact LoanId/RegionFact. Because Drop terminators accept Places, lowering
must install any otherwise-unconsumed owned SSA temporary into a compiler Place
before its cleanup point and route that Place through ordinary Drop. An owned
result with no such consumption or cleanup route is invalid Core.

By-value aggregate inspection never hides destruction. `TupleGet` and
`EnumPayload` accept an aggregate Operand only when the complete aggregate is
Copy and therefore return a Copy field; `EnumDiscriminant` similarly accepts a
Copy enum value or a shared-reference operand. Access to a non-Copy tuple,
record, array, or enum payload uses a refined Place projection followed by
PlaceCopy, PlaceMove, or a borrow, so remaining fields retain their move paths
and explicit Drop obligations. A non-Copy SSA aggregate must therefore be
installed into a Place before projection. These restrictions also apply to
CtfeResultRef materialization and Invoke/block-parameter results; no instruction
may consume an aggregate, return one field, and leak or implicitly destroy the
remainder.

CtfeResultRef may appear only as the value of a `Const` instruction, and its key
kind is never `StaticInitializer`. In a
RootSlice, the containing body must be CTFE-eligible and its key must select one
exact predecessor receipt in that scope; the current root or a non-predecessor
key is `CORE001` and can never turn a CTFE cycle into a value. RootSlice
evaluation is the only Generic Core execution that materializes the reference:
each occurrence creates a fresh logical value from the receipt tree and
preserves no predecessor allocation or pointer identity.

CompleteWorkspace may retain CtfeResultRef in any accepted body, including a
nonhermetic runtime body that uses a computed owned const before performing host
I/O. Its key must select the one exact sealed result receipt in the workspace;
`ty` and `digest` must equal the
receipt's recomputed result row; and the instruction result type must equal
`ty`. It remains a declarative promoted-value reference, not an executable Core
allocation in that scope. `project_root_slice` retains an occurrence only when
the selected CTFE root's executable closure contains it and then requires the
same receipt as a predecessor. M27-D replaces every occurrence in every body
with its validated promoted-value construction and DataRef/FunctionRef
relocations before VerifiedInstanceCore. It is never source-constructible, a
runtime address, or a substitute receipt, and neither the reference executor
nor native execution observes CtfeResultRef.

Every block is reachable from the ordinary body entry or an explicitly listed
generator-drop auxiliary entry. The ordinary entry is the first traversal root;
auxiliary entries follow by ascending generator-state number. All other
unreachable blocks are invalid. Blocks receive dense IDs in deterministic
reverse postorder over those roots. For a
branch, `then` precedes `else`; switch cases use ascending canonical scalar
value then default; an invoke visits normal, checked-exception, then panic/trap
cleanup; all other successor lists retain opcode field order. Values are dense
in block order, block-parameter order, then instruction order. Locals retain
source declaration order followed by compiler temporaries in lowering order.
Move paths use local-root preorder and declaration/index/variant projection
order. No input enumeration, hash table, or host path may affect these orders.

A place root is one local or one static DefinitionId followed by typed field, tuple element, active enum-payload
field, constant array element, dynamic index, shared dereference, mutable
dereference, or raw-pointer dereference projections. A Place containing a
dynamic index cannot be the operand of PlaceMove, PlaceInit, or Drop because
dynamic partial initialization state has no MovePath representation. This
remains illegal even when every dynamic projection has a valid
IndexAuthorization. Raw-pointer dereference and exposed-address operations name
a verified enclosing unsafe region.
Moving out of a static is forbidden. Reading an immutable static is shared;
borrowing/writing a mutable static requires an unsafe region. Its DefinitionId,
declared type, initializer body, and process-lifetime storage are revalidated.
In RootSlice CTFE, every reachable immutable `Place::Static` has exactly one
raw-DefinitionId-sorted `CtfeStaticBinding`. Its key selects the matching sealed
StaticInitializer receipt, and its type/digest equal the receipt and Static
DefinitionRow. Before root entry the evaluator mounts that logical tree once as
an immutable external slot. RootSlice construction imports/re-densifies exactly
the mounted value's sealed DataRef provenance rows into the consumer package
closure, just as CtfeResultRef materialization does, and the evaluator retains
that mapping. Mounting neither
replays the initializer nor charges the consuming root's steps/logical heap;
ordinary current-body borrows/copies still pay their normal charges. The slot
cannot be moved, is never automatically dropped (including evaluator teardown),
and interpreter backing storage is discarded nonsemantically after the root.
Mutable static access is forbidden in CTFE. Missing, extra, aliased, or
mismatched bindings are `CORE001`.

Core instructions have no Arche control edge and cannot throw, panic, trap,
destroy a value, or yield. Every instruction except the RootSlice-only
`Const{CtfeResultRef}` materialization is total and cannot allocate. That sealed
materialization may stage fresh logical CTFE allocations and stop evaluation
with a compiler budget/resource/infrastructure failure before publication; this
is not an Arche result or control edge, CompleteWorkspace/reference/native
execution never runs it, and the no-partial rule above is mandatory. The closed
M27-C instruction families are:

```text
const
tuple.make                  tuple.get
array.make                  struct.make
enum.make                   enum.discriminant
enum.payload                slice.make
slice.len                   place.copy
place.move                  place.init
borrow.shared               borrow.mutable
raw.address                 raw.offset
raw.expose-address          raw.with-address
maybe-uninit.make           maybe-uninit.assume-init
primitive.unary             primitive.binary-total
primitive.compare           pointer.cast
closure.make                pin.make
generator.factory-make      generator.construct
caps.take                   catch.type
catch.consume               callable.to-function-pointer
```

Their exact fields are:

```text
Const { value }
TupleMake|ArrayMake { elements: Vec<Operand> }
TupleGet { tuple: Operand, index: u64 }
StructMake { definition, fields: Vec<Operand> }
EnumMake { definition, variant: u64, fields: Vec<Operand> }
EnumDiscriminant { value: Operand }
EnumPayload { value: Operand, variant: u64, field: u64 }
SliceMake { data: Operand }
SliceLen { slice: Operand }
PlaceCopy|PlaceMove { place: Place }
PlaceInit { place: Place, value: Operand }
BorrowShared|BorrowMutable { place: Place, loan: LoanId }
RawAddress { place: Place, mutability }
RawOffset { pointer: Operand, element_delta: Operand }
RawExposeAddress { pointer: Operand }
RawWithAddress { provenance_pointer: Operand, address: Operand,
                 pointee: CoreType, mutability }
MaybeUninitMake { value: Option<Operand>, contained: CoreType }
MaybeUninitAssumeInit { value: Operand }
PrimitiveUnary { op: UnaryOp, operand: Operand, scalar: CoreType }
PrimitiveBinaryTotal { op: TotalBinaryOp, left: Operand, right: Operand,
                       scalar: CoreType }
PrimitiveCompare { op: CompareOp, left: Operand, right: Operand,
                   scalar: CoreType }
PointerCast { pointer: Operand, target: CoreType }
PinMake { reference: Operand, unchecked: bool }
CapsTake { caps: Place, capability: DefinitionId }
ClosureMake { descriptor: ClosureId, captures: Vec<CaptureInit> }
GeneratorFactoryMake { descriptor: GeneratorDescriptorRef,
                       generic_arguments:Vec<GenericArgument>,
                       captures: Vec<CaptureInit> }
GeneratorConstruct {
  target: Named{definition:DefinitionId,generic_arguments:Vec<GenericArgument>}
        | Factory{factory:CallableReceiver,descriptor:GeneratorDescriptorRef,
                  call_trait:Fn|FnMut|FnOnce},
  arguments: Vec<Operand>, region_substitutions:Vec<RegionSubstitution>,
  type_region_substitutions:Vec<TypeRegionSubstitution>
}
CatchType { token: Operand }
CatchConsume { token: Operand, exception_type: TypeId }
CallableToFunctionPointer { callable:Operand, target_signature:TypeId }
```

`UnaryOp` is exactly `WrappingNeg`, `FloatNeg`, `BoolNot`, or `BitNot`.
`TotalBinaryOp` is exactly `WrappingAdd`, `WrappingSub`, `WrappingMul`,
`MaskedShiftLeft`, `MaskedShiftRight`, `BitAnd`, `BitXor`, `BitOr`,
`FloatAdd`, `FloatSub`, `FloatMul`, or `FloatDiv`. Logical boolean operations
are CFG branches. `CompareOp` is exactly `Equal`, `NotEqual`, `Less`,
`LessEqual`, `Greater`, or `GreaterEqual`. The scalar field determines width/
signedness, is part of verification, and cannot request an unsupported pair.

Instructions introduce no source-visible control edge. For verified safe
operands they are total except for the already-specified external CTFE
materialization failure, which publishes no Core value. Raw-pointer and assume-init instructions additionally
carry unsafe preconditions: violation is language UB in runtime execution and a
deterministic `CTFE008` diagnostic in CTFE, never an implicit trap or
checked exception.
`PinMake{unchecked:false}` requires a mutable-reference operand whose pointee is
proven `Unpin`; `unchecked:true` instead requires a verified unsafe region.
Both return `Pin<&mut T>` without moving T, and no other opcode constructs a
borrowed Pin.
`SliceMake` is only the total full-slice coercion from a shared/mutable reference
to `[T; N]` (including an included-bytes DataRef array) to the corresponding
`[T]` reference. Its implicit start is zero, its length is exactly N, and it
preserves the input region and mutability. Dynamic subslicing is not selected
in 0.1 and cannot use this instruction. `EnumPayload` is legal only when a dominating discriminant switch
or constructor refinement proves the exact operand has the named active variant
on every incoming path with no intervening mutation; the verifier maintains and
intersects these path refinements. `PlaceMove` already deinitializes its move
path. Destruction occurs only through the Drop terminator, so there is no
independent PlaceDeinit instruction that could bypass drop glue.
`CapsTake` accepts only a driver parameter whose sealed Caps member set contains
the named capability, moves exactly that affine member, and marks its dedicated
move path uninitialized. It cannot target an aggregate assembled by source or a
non-Caps place.
CallableReceiver is shared by ClosureCall and factory-target GeneratorConstruct.
`Shared` is one compiler-inserted `&F` borrow/reborrow and may select only `Fn`;
`Mutable` is one exclusive `&mut F` reborrow and may select `FnMut`; `Once`
moves an F place or consumes an already-owned SSA value and selects `FnOnce`,
deinitializing any move path. A descriptor-derived `Fn` callable supports all
three selections, `FnMut` supports FnMut/FnOnce, and `FnOnce` supports only
FnOnce; selecting the narrower supported interface determines the required
receiver form. The callable expression/borrow is established before arguments,
then arguments evaluate in source order. A closure-call loan lasts through
unwind cleanup and through normal completion unless a ReceiverCapture-derived
result takes the transfer defined above; a transferred Shared/Mutable reborrow
then remains live until every derived result dies. A generator Shared/Mutable
loan is transferred into the returned frame and remains live until that frame completes or drops;
its lifetime appears in state_type. The verifier rederives the exact loan,
exclusivity, move-path update, and descriptor compatibility, so repeated Fn
calls remain usable, conflicting FnMut frames are rejected, and FnOnce cannot
be used again.

A descriptor `Capture.frame_place` is the dedicated capture local in the
closure/generator body, never the enclosing source place. Its root is the one
Local with `storage=Capture{ordinal}` and any projections are the canonical
stored-value projections for that capture. Capture locals use dense ordinals
`1..=capture_count` and body reads use those frame places. A
`CaptureInit.source` is instead a place in the one enclosing body and lexical
Make instruction that creates that anonymous-expression descriptor;
re-executing that site
in a loop creates another value but does not create another descriptor site.
Every ClosureDescriptor has exactly one ClosureMake site, and every
GeneratorDescriptor with nonzero `expression_ordinal` has exactly one
GeneratorFactoryMake site, in its
exact `creation_body`; that body must lie in the lexical body subtree of the
descriptor's stable top-level Definition owner. A ClosureDescriptor always has
that body, an anonymous GeneratorDescriptor has it present, and an ordinal-zero
named GeneratorDescriptor requires it absent because named factory references
may occur at multiple sites. Each Make row has the same dense ordinals and exact stored
types as the descriptor. Shared/Mutable rows establish the named loan at that
instruction, require the same source place and borrow mode inferred in HIR, and
initialize the corresponding frame place with that reference. A Transfer row
has descriptor mode Move and its value is exactly `Move(source)`, or
`Copy(source)` only when the captured type is Copy; the matching move path is
updated. Initializers execute in ordinal/source-evaluation order. Region facts
carry every borrowed capture origin into the produced callable/frame, and Drop
uses reverse initialized-ordinal order. A wrong site, source, ordinal, mode,
type, loan, transfer operand, or frame destination is `CORE003` even when the
substituted value has the same CoreType.

`GeneratorFactoryMake` resolves its descriptor reference in the named package
before applying any dense `GeneratorId`; a reference to the same dense ID in a
different package is a different descriptor and cannot be substituted. It
accepts an ordinal-zero named descriptor only with empty
captures, or a nonzero anonymous descriptor with the exact CaptureInit rows
just defined; in both cases its generic arguments are complete and its result
is the descriptor's exact tag-31 factory type. `GeneratorConstruct` accepts
either a named generator definition and complete generic substitution or a
CallableReceiver whose selected referent/owned callable has tag 31
and the exact package-qualified descriptor/call trait. Its
arguments match the descriptor parameter vector in source order; its result is
the descriptor's unpinned state_type after substituting explicit type/const and
stable bound-lifetime arguments. Synthesized local lifetime binders are not
TypeId arguments; the result Value's RegionFact binds them to the typed
arguments' exact static/bound/local-loan origin sets. The verifier/NLL result
proves each stored reference outlives that state. It moves or reborrows factory captures
according to the verified factory call trait, initializes state zero and every
parameter local, and is total: it performs no allocation, body entry, yield,
panic/trap edge, checked throw, or generator-body effect edge. Named
construction requires the instruction's verified unsafe_region exactly
when the selected descriptor's factory_unsafe is true. Anonymous descriptors
are always false; first-class named factories retain their named descriptor's
bit. The produced state's factory_unsafe byte must match.
A direct source call resolved to a generator item or generator factory lowers only to this
instruction, never DirectCall or ClosureCall. A closed C5 RootSlice and an M27-D
instance whose selected generic Fn-trait callable is a generator factory must
both rewrite that call to this same operation before Core verification; an
evaluator never performs an ad hoc rewrite. Only GeneratorResume invokes the descriptor body and adds
its requires/throws edge.
`CallableToFunctionPointer` accepts only a statically known zero-capture closure
and yields the target function-pointer type. Named function items form
FunctionRef directly at their contextual coercion and never enter this
instruction. The
source callable must satisfy the same parameter contravariance, result
covariance, safety bit, and requires/throws subset rules as FunctionRef; a
capturing closure or an unsafe-to-safe conversion is `CORE002`. The resulting
pointer retains the closure descriptor or function DefinitionId/substitution so
FunctionPointerCall invokes the correct body without adding capture storage.

An opaque linear unwind token has one of `checked-exception`, `panic`, or
`trap` context plus its sealed carrier/kind and origin span. A checked carrier
contains exactly one fully owned value tagged by its ordinal in the callable's
canonical throws set; a panic carrier contains its optional UnwindPayload; a
trap has no primary carrier. Every carrier stores its verified TypeId and
nonthrowing, capability-free drop glue beside the owned value. UnwindPayload is
transitively reference/raw-pointer-free, so a token never carries a body-local
region family and needs no RegionFact. The carrier/token representation is not
a source type and cannot be serialized. The
token is supplied only
by `raise`, a checked-exception invoke edge, `panic`, or `trap`; cannot be copied,
stored, returned, or constructed by an instruction; and must be consumed by
exactly one `catch.consume`, `resume-unwind`, or terminal outcome. Cleanup block
parameters carry the same token through every destruction edge. `catch.type`
observes but does not consume a checked token and yields its canonical throws-set
ordinal. `catch.consume` is valid only on the matching TypeId arm after required
cleanup, consumes the token, and always yields the owned exception value into a
compiler temporary for ordinary source-ordered pattern/guard CFG. A wildcard
arm does not make the instruction destroy the payload: it immediately routes
that temporary through the ordinary Drop terminator with its complete normal/
unwind/effect contract. Thus custom Drop, panic, and active-unwind behavior are
never hidden inside catch.consume. A wildcard covering multiple remaining
exception types lowers one explicit type case and matching CatchConsume/Drop
block per canonical throws-set row, then joins the source wildcard arm body;
the switch default is verifier-derived unreachable rather than a heterogeneous
payload operation.

Token propagation preserves an ordered carrier vector. A checked/panic token
starts with its primary carrier when present. CatchConsume transfers and removes
the matching checked carrier. After ordinary frame cleanup, an uncaught checked
or panic terminal destroys its remaining carrier vector in order before the
diagnostic/observation; a plain trap has an empty vector. Each carrier is moved
and its storage deinitialized before ordinary Drop glue begins, so a failing
Drop is never retried. If a trap supersedes a live checked/panic/trap token, the
replacement trap adopts that token's still-owned carrier vector and continues
cleanup; a further trap while destroying one carrier replaces the diagnostic,
retains only the not-yet-started carriers, and continues. A panic arising while
any unwind token is live aborts immediately with `134`; both tokens and all
remaining carriers are deliberately abandoned to process termination, the sole
exception to exact-once cleanup. Drop cannot raise a checked exception. The
Core verifier's linear token dataflow rederives these carrier ownership states
from Raise/Panic/Trap/Invoke origins and every continuation, without exposing
the carrier as a Core value.

`primitive.binary-total` contains wrapping integer addition/subtraction/
multiplication, masked shifts, bitwise operations, and nontrapping floating
operations. Integer division/remainder, bounds checks, calls, replacement/drop,
generator resume, and allocation-owning sealed intrinsics are control-flow
operations. No generic Allocate operation exists: each owning allocation is
performed only by the closed intrinsic whose signature, accounting, publication,
failure, and region-transfer semantics are independently verified. Every block
ends in exactly one of:

```text
goto            branch             switch
return          raise              invoke
drop            yield              generator.return
panic           trap               resume-unwind
unreachable
```

Terminator fields are closed:

```text
Goto { next: Continuation }
Branch { condition: Operand, then: Continuation, otherwise: Continuation }
Switch { discriminant: Operand,
         cases: Vec<(SwitchCase,Continuation)>, default: Continuation }
Return { value: Operand }
Raise { exception: Operand, cleanup: Continuation }
Invoke { operation: InvokeOperation, normal: Continuation,
         checked_exception: Option<Continuation>, unwind: Option<Continuation>,
         active_unwind: Option<Operand> }
Drop { place: Place, normal: Continuation, unwind: Option<Continuation>,
       active_unwind: Option<Operand> }
Yield { state: u64, value: Operand,
        resume: Continuation, suspended_drop: BlockId }
GeneratorReturn { value: Operand }
Panic { kind: PanicKind, payload: Option<Operand>, cleanup: Option<Continuation>,
        active_unwind: Option<Operand> }
Trap { kind: TrapKind, cleanup: Option<Continuation>,
       active_unwind: Option<Operand> }
ResumeUnwind { token: Operand }
Unreachable { never_value:Option<Operand> }
```

The normal invoke continuation declares the operation result as its first block
parameter when nonunit. A Yield resume continuation always declares the next
resume value as its first `Result` block parameter with the descriptor's exact
resume type; that generated value is the result of the source `yield`
expression and is omitted from `resume.arguments`. A checked-exception
continuation, a panic/trap cleanup,
and every cleanup reached by `Raise` declare the unwind token as their first
parameter. The checked exception value remains owned by that token until
`catch.consume`; it is never a second edge value. Generated values are not
duplicated in the explicit continuation argument list. Every other block
parameter is supplied one-for-one by `arguments`. `active_unwind` is absent in
ordinary control flow and contains exactly the already-live linear token when
an Invoke or Drop executes during cleanup. A successful normal edge, or a
checked-exception edge that will catch its new exception, threads that older
token explicitly as a non-generated continuation argument; it is never copied.

Generator state zero is the constructed-before-entry state. Each reachable
source `yield` expression receives a checked one-based `yield_ordinal` in
resolved-body preorder after transparent parentheses are removed, before Core
blocks exist. Its lowered Yield terminator has `state == yield_ordinal`; every
ordinal appears exactly once even when CFG/RPO order differs, and a loop
revisiting one Yield reuses that state. Auxiliary drop roots are traversed in
ascending state order only after this assignment, so state numbering never
depends on BlockId. `state_count` is
exactly one plus the number of reachable Yield terminators and is never zero.
GeneratorDescriptor.drop_entries and its body's auxiliary_drop_entries are
byte-for-byte identical, strictly state-sorted, and contain exactly one entry
for every state `0..state_count-1`. State zero's entry drops constructed
captures/arguments; each Yield's entry block equals its suspended_drop field and
drops exactly the values live at that suspension. Resume targets remain in the
same body and satisfy the generated-parameter rule above. Every terminal body
exit—GeneratorReturn, an escaping checked throw, panic, or trap—enters an
internal terminal flag outside this state range after that exit's verified
frame cleanup and before its result/token propagates. The terminal flag has no
drop entry, so caller cleanup may drop the generator handle without redropping
frame values; a later resume traps. On the first
state-zero resume, the resume operand is consumed and dropped through verified
cleanup before ordinary entry and is never bound in source. Duplicate, missing,
out-of-range, mismatched-target, or noncanonical state/drop rows are `CORE003`.

`InvokeOperation` is exactly:

```text
DirectCall { function: DefinitionId, generic_arguments,
             region_substitutions, type_region_substitutions, arguments }
FunctionPointerCall { function: Operand, signature: TypeId,
                      region_substitutions, type_region_substitutions, arguments }
TraitCall { trait_method: DefinitionId, selection: TraitSelection,
            generic_arguments, region_substitutions,
            type_region_substitutions, arguments }
ClosureCall { closure: CallableReceiver, call_trait: Fn|FnMut|FnOnce,
              region_substitutions, type_region_substitutions, arguments }
GeneratorResume { generator: Operand, resume_value: Operand,
                  region_substitutions, type_region_substitutions }
Intrinsic { intrinsic: IntrinsicId, generic_arguments, region_substitutions,
            type_region_substitutions,
            const_arguments, arguments }
IntegerDivide { left: Operand, right: Operand, scalar: CoreType }
IntegerRemainder { left: Operand, right: Operand, scalar: CoreType }
CheckedIndex { place: Place, projection_ordinal:u64 }
Replace { destination: Place, staged_value: Operand }
```

`CheckedIndex` evaluates no source expression: lowering has already evaluated
the base and index once in source order and stored the dynamic index as the
ValueId named by one `Projection::Index` in `place`. `projection_ordinal` is the
zero-based ordinal in that complete Place projection vector and must select
exactly that Index projection. The verifier recomputes the selected prefix's
built-in bounds-bearing sequence type, logical length, index `usize` type, and
projected element type; otherwise non-indexable storage is invalid. On the
normal edge the operation returns unit and creates a verifier-
only `IndexAuthorization` for that exact place-prefix, index ValueId, projection
ordinal, and reaching storage version. Bounds failure raises `TrapKind::Bounds`
through the ordinary cleanup edge and performs no access or mutation.

Except for CheckedIndex validating its selected projection, every instruction,
terminator, Operand, CallableReceiver, or CaptureInit that consumes a Place with
a dynamic Index projection must be dominated by the matching normal-edge
authorization for that projection. This includes PlaceCopy, shared/mutable
borrow, RawAddress, discriminant/payload access, Replace, and any nested Place
carried by another closed record. IndexAuthorization proves bounds only and
never overrides move, initialization, or Drop legality; PlaceMove, PlaceInit,
and Drop remain invalid for every Place containing a dynamic Index. CheckedIndex
itself requires authorizations for every earlier dynamic projection in its
prefix. A move/deinitialization/replacement of the base, or any call/intrinsic/alias write
that may change that sequence's storage or logical length, invalidates it;
immutable fixed-array storage does not. Nested dynamic projections require one
authorization apiece in outer-to-inner order. Authorizations are proof facts,
not Core values or continuation arguments. The incoming set at a join is the
intersection of predecessor authorizations with byte-identical place/index/
version keys; no union or producer annotation is permitted. A mismatched
ordinal/type/value, use before check, use after invalidation,
or producer-asserted authorization is `CORE003`.

`TraitSelection` is closed:

```text
TraitSelection = ConcreteImpl {
  implementation:DefinitionId,
  implementation_arguments:Vec<GenericArgument>,
  evidence:Vec<Predicate>
} | BoundWitness { obligation:Predicate, environment_index:u64 }
  | SealedEcsKeyComparison {
      obligation:Predicate, key_type:CoreType,
      bound_evidence:Vec<EcsKeyBoundEvidence>
    }
EcsKeyBoundEvidence { structural_path:Vec<u64>, environment_index:u64 }
```

A ConcreteImpl is used only when generic selection already has one unique
implementation and records its complete substitution/evidence. A BoundWitness
must be byte-identical to the indexed canonical body predicate and is used when
selection depends on a bound type. C verifies that witness without guessing an
implementation. A SealedEcsKeyComparison is legal only for an exact embedded-
core `Eq<K,K>` or `Ord<K,K>` obligation whose `K` equals key_type. The verifier
constructs one finite canonical structural-EcsKey proof graph from key_type.
Each node's child vector is the comparator order fixed above: an array, Box, or
Vec has element/pointee child zero; a tuple or record uses element/field order;
an enum uses variant order then payload-field order; and Map uses key child zero
then value child one. Nominal children are expanded after applying their exact
generic substitution. A repeated normalized nominal/container instantiation is
a back-edge to its first canonical visit and is not expanded again.

Every encountered bound-type leaf emits exactly one EcsKeyBoundEvidence row.
`structural_path` is the sequence of child ordinals from the root proof node to
that leaf (empty for a bound root), and `environment_index` names the exact body
predicate `LeafType: EcsKey`. Rows are unique and sorted lexicographically by
their unsigned-u64 path; the same environment index may appear at distinct
paths. Missing, extra, duplicate, reordered, nonleaf, or wrong-index rows are
invalid. Zero rows are legal only when the traversal encounters no bound-type
leaf and independently proves every structural node; every fully closed key
therefore has zero rows. An identical explicit Eq or Ord predicate is never
alternate evidence. The verifier rejects any overlapping user Eq/Ord selection,
and the selection invokes the compiler-sealed structural comparator rather than
an ImplRow method.

M27-D substitutes each instance and reruns the same deterministic selection,
specialization, and sealed-EcsKey rules. It replaces every BoundWitness with one
ConcreteImpl before `VerifiedInstanceCore`, but preserves the
SealedEcsKeyComparison variant, substitutes its obligation/key type, and
independently rebuilds its exact path-qualified bound evidence from the
instantiated type and predicate environment. A fully closed instance has zero
rows. The variant never becomes a user or synthetic ConcreteImpl and lowers only
to the sealed comparator operation. Compiler-
derived Fn/FnMut/FnOnce
implementations have no forgeable ImplRow or ConcreteImpl. Once a callable
substitution is closed, RootSlice construction and M27-D use one exact rewrite
table before verification: a syntactic direct named-function call was already
lowered to DirectCall; every first-class named function is already FunctionRef
and becomes FunctionPointerCall; a direct named-generator item becomes named
GeneratorConstruct; an ordinary closure becomes
ClosureCall with its verified CallableReceiver; an anonymous generator factory
or first-class named generator factory becomes factory-target GeneratorConstruct
with its verified CallableReceiver; and a
function pointer becomes FunctionPointerCall. Effects/call-graph edges are
rederived after rewriting. RootSlice construction performs the rewrite for
every closed C5 substitution; a genuinely parameterized, unexecuted template
may retain BoundWitness. A fabricated callable ConcreteImpl, unresolved witness
in executable closed Core, or closed callable TraitCall left unreduced is
`CORE004` and can never be branded or executed.

Generic arguments retain declared kinds/order and selected-trait evidence is a
canonical predicate list independently rederived by the verifier. `PanicKind`
is `User`, `Assertion`, or `DropDuringCleanup`. `TrapKind` is exactly
`IntegerDivideByZero`, `IntegerSignedOverflow`, `Bounds`, `GeneratorComplete`,
`StackOverflow`, `ReferenceCountOverflow`, or `SealedIntrinsic{IntrinsicId}`.
Unknown kinds are invalid. `Unreachable.never_value` when present must be a
dominating never-typed operand. When absent, the verifier's own closed
discriminant/constructor/range path-refinement analysis must derive an empty
incoming value set for that block from the Core CFG; there is no producer-
asserted exhaustive-decision witness. A reachable missing-pattern path therefore
cannot be erased by changing this tag.

`invoke` contains exactly one direct call, function-pointer call, selected
trait-method call, closure call, generator resume, compiler-sealed intrinsic,
integer divide/remainder, checked index, or place replacement. It names normal,
checked-exception, and panic/trap-cleanup
continuation classes. Each present continuation names a block and a complete
typed block-argument list; a normal result or unwind token is the first declared
parameter of its respective successor. A missing checked-exception
continuation is valid only for an empty throws set. A potentially panicking or
trapping operation has a cleanup continuation unless already inside cleanup,
where it ends in `resume-unwind` or the double-panic abort path.

DirectCall rejects a generator DefinitionId and ClosureCall rejects a receiver
selecting a generator-factory callable; those total constructions use
GeneratorConstruct. A resume
requires the exact descriptor state_type behind `Pin<&mut G>`, consumes the
resume operand, and is the sole call-graph edge to that generator body.

The may-unwind classification is conservative and closed: every
InvokeOperation, every Drop terminator, and every explicit Panic/Trap may unwind,
independent of a callee body or package boundary. Its unwind continuation is
required when the liveness/drop tables contain one initialized value/temporary
requiring cleanup **or** an active unwind token is live. With neither, absence
means direct terminal propagation, not “cannot panic.” With an active token,
even a body having no remaining droppable value routes a new panic/trap through
the closed nested-unwind rule. Invoke/Drop `active_unwind` must name that exact
token: a new panic consumes both tokens and terminates with status `134`; a new
trap consumes the old token, adopts its remaining carrier vector, and passes
only the replacement trap token to the unwind successor. A newly raised checked exception may enter its declared catch
continuation while the old token is threaded separately and linearly; if it
escapes instead, the Core is invalid. Total Core
instructions never acquire unwind behavior. Consequently an imported body may
add a panic without changing its interface and callers remain sound; later
optimization may remove a proven-dead edge only after preserving identical
cleanup/outcome semantics.

`drop` has normal and panic-cleanup continuations and never a checked-exception
continuation. `raise` escapes one value whose type is in the body's throws set
by creating a checked-exception token; catch consumes that token before its
ordinary decision blocks. `panic` and `trap` are noncatchable and enter cleanup.
`resume-unwind` continues the supplied token. A panic created while any unwind
token is live selects abort status `134`; a trap supersedes the prior token and
continues cleanup as a trap, so it still exits `70` with its trap diagnostic.
`yield` is
valid only in a generator body and records the yielded value, resume block,
resume-value type, and suspended-drop cleanup. `generator.return` records the
terminal return value.

`IntrinsicId` is a checked `u16`. The registry below is closed for 0.1; every
unlisted value and every listed value with a different arity, generic kind,
operand/result type, effect, or eligibility class is `CORE004`. `H` means
hermetic CTFE, `I` means include acquisition only, and `N` means forbidden in
CTFE. `R{X}` and `T{E}` are the exact requires/throws sets; `R{schedule}` and
`T{schedule}` mean the independently verified canonical sets of the selected
schedule. `R{Drop(T)}` is the complete compiler-derived Drop-requires union for
`T`; it is conservatively present whenever the operation may destroy such a
value. Empty cells mean the empty set. Lifetimes in results are tied to the
corresponding input borrow. Infallible allocation failure is status `1`; only a
signature explicitly returning `AllocError` exposes allocation failure as an
ordinary value.

| ID | Stable name and exact semantic signature | Requires | Throws | CTFE |
|---:|---|---|---|:---:|
| `1` | `string.new() -> String` | | | H |
| `2` | `string.from-str(&str) -> String` | | | H |
| `3` | `string.len(&String) -> usize` | | | H |
| `4` | `string.push-str(&mut String, &str) -> ()` | | | H |
| `5` | `string.as-str(&String) -> &str` | | | H |
| `10` | `vec.new<T>() -> Vec<T>` | | | H |
| `11` | `vec.len<T>(&Vec<T>) -> usize` | | | H |
| `12` | `vec.push<T>(&mut Vec<T>, T) -> ()` | | | H |
| `13` | `vec.pop<T>(&mut Vec<T>) -> Option<T>` | | | H |
| `14` | `vec.get<T>(&Vec<T>, usize) -> Option<&T>` | | | H |
| `20` | `map.new<K:Eq+Ord,V>() -> Map<K,V>` | | | H |
| `21` | `map.len<K,V>(&Map<K,V>) -> usize` | | | H |
| `22` | `map.insert<K:Eq+Ord,V>(&mut Map<K,V>, K, V) -> Option<V>` | `R{Drop(K)}` | | H |
| `23` | `map.get<K:Eq+Ord,V>(&Map<K,V>, &K) -> Option<&V>` | | | H |
| `24` | `map.remove<K:Eq+Ord,V>(&mut Map<K,V>, &K) -> Option<V>` | `R{Drop(K)}` | | H |
| `25` | `map.iter<'a,K:Eq+Ord,V>(&'a Map<K,V>) -> MapIter<'a,K,V>` | | | H |
| `26` | `map.next<'a,'b,K,V>(&'b mut MapIter<'a,K,V>) -> Option<(&'a K,&'a V)>` | | | H |
| `30` | `box.new<T>(T) -> Box<T>` | | | H |
| `31` | `box.try-new<T>(T) -> Result<Box<T>,AllocError>` | | | H |
| `32` | `box.as-ref<T>(&Box<T>) -> &T` | | | H |
| `33` | `box.as-mut<T>(&mut Box<T>) -> &mut T` | | | H |
| `40` | `rc.new<T>(T) -> Rc<T>` | | | H |
| `41` | `rc.clone<T>(&Rc<T>) -> Rc<T>` | | | H |
| `42` | `rc.downgrade<T>(&Rc<T>) -> RcWeak<T>` | | | H |
| `43` | `rc.upgrade<T>(&RcWeak<T>) -> Option<Rc<T>>` | | | H |
| `44` | `rc.as-ref<T>(&Rc<T>) -> &T` | | | H |
| `50` | `arc.new<T>(T) -> Arc<T>` | | | H |
| `51` | `arc.clone<T>(&Arc<T>) -> Arc<T>` | | | H |
| `52` | `arc.downgrade<T>(&Arc<T>) -> ArcWeak<T>` | | | H |
| `53` | `arc.upgrade<T>(&ArcWeak<T>) -> Option<Arc<T>>` | | | H |
| `54` | `arc.as-ref<T>(&Arc<T>) -> &T` | | | H |
| `60` | `box.pin<T>(T) -> Pin<Box<T>>` | | | H |
| `61` | `pin.as-ref<T>(&Pin<Box<T>>) -> Pin<&T>` | | | H |
| `62` | `pin.as-mut<T>(&mut Pin<Box<T>>) -> Pin<&mut T>` | | | H |
| `70` | `include.bytes<const N:usize>(literal-path) -> &'static [u8;N]` | | | I |
| `71` | `include.str(literal-path) -> &'static str` | | | I |
| `100` | `args.all(&Args) -> Vec<String>` | `R{Args}` | | N |
| `101` | `environment.get(&Environment, &str) -> Option<String>` | `R{Environment}` | | N |
| `102` | `stdio.read(&mut Stdio, &mut [u8]) -> usize` | `R{Stdio}` | `T{IoError}` | N |
| `103` | `stdio.write-out(&mut Stdio, &[u8]) -> ()` | `R{Stdio}` | `T{IoError}` | N |
| `104` | `stdio.write-error(&mut Stdio, &[u8]) -> ()` | `R{Stdio}` | `T{IoError}` | N |
| `105` | `files.open(&Files, &str, OpenOptions) -> File` | `R{Files}` | `T{IoError}` | N |
| `106` | `files.read(&Files, &mut File, &mut [u8]) -> usize` | `R{Files}` | `T{IoError}` | N |
| `107` | `files.write(&Files, &mut File, &[u8]) -> usize` | `R{Files}` | `T{IoError}` | N |
| `108` | `subprocess.run(&Subprocess, ProcessSpec) -> ProcessOutput` | `R{Subprocess}` | `T{ProcessError}` | N |
| `109` | `clock.wall-now(&WallClock) -> u64` | `R{WallClock}` | | N |
| `110` | `clock.monotonic-now(&MonotonicClock) -> u64` | `R{MonotonicClock}` | | N |
| `111` | `tcp.bind(&Tcp, SocketAddress) -> TcpListener` | `R{Tcp}` | `T{IoError}` | N |
| `112` | `tcp.connect(&Tcp, SocketAddress) -> TcpStream` | `R{Tcp}` | `T{IoError}` | N |
| `113` | `tcp.accept(&Tcp, &mut TcpListener) -> (TcpStream,SocketAddress)` | `R{Tcp}` | `T{IoError}` | N |
| `114` | `tcp.read(&Tcp, &mut TcpStream, &mut [u8]) -> usize` | `R{Tcp}` | `T{IoError}` | N |
| `115` | `tcp.write(&Tcp, &mut TcpStream, &[u8]) -> usize` | `R{Tcp}` | `T{IoError}` | N |
| `116` | `udp.bind(&Udp, SocketAddress) -> UdpSocket` | `R{Udp}` | `T{IoError}` | N |
| `117` | `udp.receive(&Udp, &mut UdpSocket, &mut [u8]) -> (usize,SocketAddress)` | `R{Udp}` | `T{IoError}` | N |
| `118` | `udp.send(&Udp, &mut UdpSocket, &[u8], SocketAddress) -> usize` | `R{Udp}` | `T{IoError}` | N |
| `120` | `thread.spawn<F:FnOnce,T>(&Threads, F) -> JoinHandle<T,F.throws>` | `R{Threads}` plus `F.requires` | `T{ThreadError}` | N |
| `121` | `thread.scope<F:FnOnce,T>(&Threads, F) -> T` | `R{Threads}` plus `F.requires` | `T{F.throws,ThreadError}` | N |
| `122` | `thread.join<T,E...>(&Threads, JoinHandle<T,E...>) -> T` | `R{Threads}` | `T{E...,ThreadError}` | N |
| `123` | `atomic.new<T:AtomicScalar>(&Atomics, T) -> Atomic<T>` | `R{Atomics}` | | N |
| `124` | `atomic.load<T>(&Atomics, &Atomic<T>, Ordering) -> T` | `R{Atomics}` | | N |
| `125` | `atomic.store<T>(&Atomics, &Atomic<T>, T, Ordering) -> ()` | `R{Atomics}` | | N |
| `126` | `atomic.rmw<T>(&Atomics, &Atomic<T>, AtomicRmw, T, Ordering) -> T` | `R{Atomics}` | | N |
| `127` | `atomic.compare-exchange<T>(&Atomics, &Atomic<T>, T, T, Ordering, Ordering) -> Result<T,T>` | `R{Atomics}` | | N |
| `128` | `mutex.new<T>(&Synchronization, T) -> Mutex<T>` | `R{Synchronization}` | | N |
| `129` | `mutex.lock<T>(&Synchronization, &Mutex<T>) -> MutexGuard<T>` | `R{Synchronization}` | | N |
| `130` | `rwlock.new<T>(&Synchronization, T) -> RwLock<T>` | `R{Synchronization}` | | N |
| `131` | `rwlock.read<T>(&Synchronization, &RwLock<T>) -> ReadGuard<T>` | `R{Synchronization}` | | N |
| `132` | `rwlock.write<T>(&Synchronization, &RwLock<T>) -> WriteGuard<T>` | `R{Synchronization}` | | N |
| `133` | `condvar.new(&Synchronization) -> Condvar` | `R{Synchronization}` | | N |
| `134` | `condvar.wait<T>(&Synchronization, &Condvar, MutexGuard<T>) -> MutexGuard<T>` | `R{Synchronization}` | | N |
| `135` | `condvar.notify-one(&Synchronization, &Condvar) -> ()` | `R{Synchronization}` | | N |
| `136` | `condvar.notify-all(&Synchronization, &Condvar) -> ()` | `R{Synchronization}` | | N |
| `140` | `channel.new<T>(&Synchronization) -> (Sender<T>,Receiver<T>)` | `R{Synchronization}` | | N |
| `141` | `channel.send<T>(&Synchronization, &Sender<T>, T) -> Result<(),ChannelClosed>` | `R{Synchronization}` | | N |
| `142` | `channel.receive<T>(&Synchronization, &Receiver<T>) -> Result<T,ChannelClosed>` | `R{Synchronization}` | | N |
| `200` | `app.run<W,C>(&mut App<W>, &mut Caps<C>; schedule:DefinitionId) -> ()` | `R{schedule}` | `T{schedule}` | N |
| `201` | `resource.read<W,T>(&App<W>) -> &T` | | | N |
| `202` | `resource.write<W,T>(&mut App<W>) -> &mut T` | | | N |
| `203` | `query.open<W,Q>(&mut App<W>) -> QueryCursor<Q>` | | | N |
| `204` | `query.next<Q>(&mut QueryCursor<Q>) -> Option<QueryBindings<Q>>` | | | N |
| `205` | `query.close<Q>(QueryCursor<Q>) -> ()` | | | N |
| `206` | `commands.spawn<W,B>(&mut Commands<W>, B) -> entity` | | | N |
| `207` | `commands.despawn<W>(&mut Commands<W>, entity) -> ()` | | | N |
| `208` | `commands.add<W,T:EcsValue>(&mut Commands<W>, entity, T) -> ()` | | | N |
| `209` | `commands.remove<W,T:EcsValue>(&mut Commands<W>, entity) -> ()` | | | N |
| `210` | `world.init-resource<W,T:EcsValue>(&mut App<W>, T) -> ()` | | | N |
| `211` | `world.init-spawn<W,B>(&mut App<W>, B) -> entity` | | | N |

The capability names and the opaque helper/result types appearing here are
compiler-known virtual `arche/core` definitions, except that JoinHandle uses the
sealed semantic type-tree tag 30 rather than a forgeable nominal DefinitionId.
`AtomicScalar` is exactly the
integer widths, `isize`/`usize`, bool, and raw-pointer forms permitted by the
atomic operation; `AtomicRmw` is exactly Add, Sub, And, Or, Xor, Exchange, Min,
or Max with a type-legal operand. `Ordering` is checked by the static rules in
Section 0.3.7. `ProcessError`, `IoError`, and `ThreadError` are owned checked-
exception types; `ChannelClosed` is an ordinary enum result. Thread child
exceptions are carried by the opaque `JoinHandle<T,E...>` canonical throws set
and re-enter the ordinary checked-exception mechanism at `scope`/`join`;
`ThreadError` additionally covers thread creation, join, or runtime failure.
Intrinsic 120 throws ThreadError only before publishing a child handle; an
exception from F is owned by the handle and cannot escape spawn directly.
Intrinsic 121 implements exactly the immediate spawn-and-join scope described
above and has no hidden handle, second child, or lifetime extension.
Schedule effects are not
caller-selected generic arguments. Query/resource guard types and every OS,
thread, synchronization, capability, and ECS cursor/handle type are sealed,
nonserializable, and ineligible for `EcsValue`.
For intrinsic 200 the source schedule marker is not an ordinary operand: it is
the sole `ConstArgument::Definition`, must name the selected ScheduleDescriptor,
and is omitted from the ordinary argument vector. Lowering auto-reborrows the
App receiver and Caps place, and ordinary NLL records loans for exactly the Caps
members named by that schedule. Definition const arguments are invalid for every
other 0.1 intrinsic.

Only the listed H/I rows within IDs `1..71` can execute in M27-C CTFE, and listed
IDs `70..71` additionally require the `SourceDatabase` include authority.
M27-C verifies but does not execute the listed N rows within `100..211`; every
hole remains invalid. Their runtime implementations belong to M27-G/F/H as assigned by
the roadmap. Public standard-library functions may wrap these rows but cannot
mint a new intrinsic identity or weaken the row's effects. The virtual core
defines safe `fn panic<T: UnwindPayload>(T) requires {} throws {} -> !`; its compiler-owned
verified body ends in `PanicKind::User`, so it needs no additional intrinsic ID
and becomes `CTFE006` during required evaluation.

Before branding, the verifier independently proves:

- the semantic-inventory Arc carries the unforgeable inventory brand from the
  same immutable SourceDatabase; its source-tree commitments, workspace roots,
  targets, modules, declarations, re-exports, symbolic shapes, and body keys are
  complete, and the candidate is the exact RootSlice-ready projection or full
  CompleteWorkspace finalization of those rows;
- the inventory and candidate carry the same branded embedded-core authority;
  its version/digest, PackageId, synthetic source/module/spans, complete virtual
  definition/type/trait/method/interface rows, panic body, and scope projection
  match the release-manifest table byte-for-byte, while no ordinary package,
  lock, dependency, target, or source-tree row claims that virtual provenance;
- every PackageId recomputes from its official registry origin/scoped name;
  workspace roots and source-kind/dependency edges form the exact canonical
  acyclic closure, with no registry-to-workspace edge or absent/extra package;
  every ordinary package's effective CTFE budgets match its sealed semantic
  inventory row and every receipt uses the declaring package's exact row;
- every module/file/definition provenance row and declaration/re-export binding
  is complete, canonical, audience-safe, and graph-consistent; each DefinitionId
  recomputes from owned package/target/module/owner/name/shape rows before any
  TypeId or downstream reference is accepted, so a global raw-ID remap fails;
  declaration/member declared-visibility rows exactly match semantic inventory,
  effective member rows are rederived, and module/declaration InterfaceHash
  payload variants obey their closed conditional grammar;
  RootSlice has only a verified pending interface skeleton, while every
  CompleteWorkspace package's public binding/coherence/query/CTFE-result rows
  recompute its exact claimed InterfaceHash;
- every remaining table, dense/stable ID, generic substitution, source file, and span is
  canonical, unique, in range, correctly kinded, and obligation-complete;
- RootSlice has one matching CtfeRoot entry, exactly its executable dependency
  closure and complete coherence universe relative to the sealed inventory,
  exact predecessor sealed receipts,
  and
  no unresolved/provisional stable ID; CompleteWorkspace contains the complete
  resolved semantic package/module graph from its explicit workspace roots, all
  and only successful sealed receipts, no CtfeRoot owner,
  and only final identities; every receipt comes from `evaluate_ctfe_root` over
  its retained exact branded slice/source/budgets, and every result TypeId,
  value/digest, accounting summary, source-tree vector, and trace commitment is
  recomputed or matched to that provenance, with `final_heap == 0` for every
  successful receipt; canonical project_root_slice
  reconstruction/re-densification differs only by the permitted root/result
  rebase and is structurally equal to the evaluated receipt slice;
- every DataRef/FunctionRef/CtfeResultRef/static root and ConstArgument has an
  exact row/receipt, type, source/provenance authority, safety/effect signature,
  and permitted use; RootSlice executes CtfeResultRef only in its CTFE closure,
  CompleteWorkspace permits it in any body only against an exact result receipt,
  and M27-D replacement is total over all bodies; receipt DataRef side tables, immutable-static bindings,
  and the finite CTFE function-pointer points-to/retained-body closure are
  complete, canonical, and independently rederived;
- every definition/closure/generator/world/TypeConst body has one canonical
  BodyOwner and reciprocal descriptor link; no body or auxiliary root is lost,
  duplicated, or attached to the wrong declaration kind;
- world initializers contain only permitted init operations; query descriptors
  preserve parameter/source-term order, read/write/exclude and binding rules,
  and agree with every Query parameter/intrinsic use; every schedule run names
  a system with one exact closed type/const substitution, and schedule effects
  equal the substituted run union while remaining independent of any root world;
- world/query/schedule descriptors are in raw-definition order, match exactly
  one correctly kinded DefinitionRow/signature, and use the canonical resource,
  term, run, and effect ordering; every TypeConst descriptor exactly matches its
  canonical expression use site and BodyOwner;
- trait selection is unique and satisfies orphan, overlap, and strict
  specialization rules; trait/impl rows are raw-ID ordered exact projections of
  their one correctly kinded DefinitionRow and have complete reciprocal method-
  owner links; every impl's source `is_default` bit agrees across semantic shape,
  DefinitionId/owned-method identity, DefinitionSignature, ImplRow, and public
  coherence rows, while the optional immediate parent is independently derived;
  ConcreteImpl substitutions/evidence are exact, while a
  BoundWitness names one canonical environment predicate and is never executed
  before closed substitution/selection; every SealedEcsKeyComparison has the
  exact Eq/Ord obligation and key type, reproduces the complete canonical
  structural proof graph and its path-qualified bound-leaf environment rows (or
  zero rows when no bound leaf exists), has no competing user selection, and
  never resolves through an ImplRow;
- every block is reachable from an enumerated root, has one terminator, and contains no
  value use before its one dominating definition;
- every edge targets an existing block with the exact block-argument arity and
  types, and every opcode/place projection has exact legal operand/result types;
- every dynamic Index projection has one matching dominating CheckedIndex
  authorization for its exact projection ordinal/index/storage version, no use
  survives an invalidating mutation, and no generic Allocate operation exists;
- Copy SSA values may be reused, while every non-Copy SSA value is consumed
  exactly once on each path, crosses a block edge only through one explicit
  argument/parameter transfer, and is installed into a cleanup Place before it
  could otherwise die unused; Copy/Move place operands update initialization
  exactly as specified;
- aggregate inspection never extracts from a non-Copy SSA owner: such values
  are first installed into Places and use refined PlaceCopy/PlaceMove/borrow
  projections, so all unselected fields retain explicit move/drop state;
- every raw/address/assume-init/unchecked-pin operation, unsafe direct, trait,
  function-pointer, or named-generator construction, mutable-static access, and
  unsafe host intrinsic names a lexically
  containing verified unsafe region; safe `Box::pin` and `Pin::new<T:Unpin>` are
  the only construction exceptions;
- definite initialization, move paths, partial moves, replacement, NLL loan
  liveness/exclusivity/outlives, exhaustive variant/state/recursive region-family
  shapes, exact origin joins/facts, binder-keyed call substitutions, mutating-
  edge PlaceAt updates, and verified unsafe regions hold on every path;
- right-hand ownership completes before destination destruction, and every
  initialized owned path drops exactly once on return, raise, panic, trap, and
  each generator state's destruction path, except the explicitly terminal
  status-134 double-panic abandonment;
- unwind tokens are linear, context-correct on every cleanup edge, consumed only
  by matching catch/resume/terminal operations, and cannot lose the distinction
  between checked-exception, panic, and trap cleanup; each cleanup Invoke/Drop
  names its exact active token, every carrier is UnwindPayload with exact type/
  drop ownership, and nested checked/panic/trap outcomes obey the closed
  carrier-adoption/destruction and two-token consumption rules;
- `Drop` has empty throws and its exact transitive requires set; calls and
  selected methods use their exact signature boundaries; every body's independently
  recomputed actual summary equals its stored `actual_*` vectors and is a subset
  of its declared/expected boundary, with per-set equality for each Inferred
  descriptor discriminator, schedules, and compiler-derived Drop requires;
  schedule effects equal their
  dispatched-system boundary union; operator calls cannot throw;
- closure/generator capture ordinals are dense and their TypeId mode/type rows
  equal descriptor rows while actual Places remain body semantics; every
  ClosureDescriptor and nonzero-expression-ordinal GeneratorDescriptor has one
  exact lexical creation body/Make site and every
  CaptureInit proves source-to-frame-place mode, loan, and transfer;
  CallableReceiver
  loans/moves and `Fn` class, generator
  factory safety/state/parameter substitution, resume/yield/return/pin/
  suspended-drop rows and borrow rules, and structural `Send`/`Sync` proofs
  agree with their descriptors;
- every tag-30 JoinHandle records exactly its spawned closure's result and
  canonical throws set, is consumed once by matching join, and cannot escape
  the entrypoint; scoped-thread calls synchronously join, preserve every proven
  capture/result lifetime, and expose no child-local reference or handle;
- capability, `EcsValue`, `EcsKey`, and `UnwindPayload` evidence is compiler-sealed and
  structurally derivable rather than asserted by a user or corrupted producer;
- every body's direct environment-forbidden summary and every Environment
  target's finite function-pointer sites/targets/reachable-body closure are
  independently rederived; the union over reset/step/self-play is empty and no
  unknown indirect target reaches the graph;
- a CTFE-eligible body cannot reach world, capability, host-I/O, FFI, thread,
  synchronization, address-observation, or otherwise nonhermetic operations;
- no unsupported or later-gate construct is silently represented.

Constructed invalid-Core tests cover every rule above. Byte-level Core/object
corruption belongs to M27-D. M26 `VerifiedExecutableCore` is a distinct
historical brand and cannot enter any M27 API.

#### M27-C diagnostic contract

M27-C reserves these exact deterministic codes:

| Code | Meaning |
|---|---|
| `TYPE001` | Invalid type formation, generic arity/kind, or sizedness |
| `TYPE002` | Expression, assignment, argument, return, or conversion mismatch |
| `TYPE003` | Invalid literal or primitive operation |
| `CALL001` | Invalid direct, indirect, trait, closure, or generator call |
| `TRAIT001` | Invalid trait or implementation declaration |
| `TRAIT002` | Unsatisfied or ambiguous trait selection |
| `COHERENCE001` | Orphan-rule violation |
| `COHERENCE002` | Illegal overlap or specialization |
| `PATTERN001` | Invalid pattern, range, binding, or binding mode |
| `PATTERN002` | Nonexhaustive match or catch |
| `MOVE001` | Use of a moved or uninitialized value |
| `MOVE002` | Illegal move or partial move |
| `BORROW001` | Conflicting shared or mutable borrow |
| `BORROW002` | Escaping reference or unsatisfied lifetime |
| `DROP001` | Invalid `Copy`, `Clone`, or `Drop` contract |
| `UNSAFE001` | Unsafe operation outside a verified unsafe region |
| `EFFECT001` | Duplicate, malformed, or noncanonical effect declaration |
| `EFFECT002` | Missing or incompatible throws/requires effect |
| `EXCEPTION001` | Invalid throw, catch, propagation, or unwind contract |
| `CAPABILITY001` | Forged, static, serialized, or forbidden capability use |
| `CLOSURE001` | Invalid capture or `Fn` category use |
| `GENERATOR001` | Invalid yield, resume, pin, or suspended borrow |
| `THREAD001` | Invalid `Send`/`Sync`/`Unpin`, scope, or atomic ordering |
| `ECSVALUE001` | Type is not eligible for sealed `EcsValue` |
| `ECSKEY001` | Type is not eligible for sealed `EcsKey` |
| `CTFE001` | Operation is forbidden in hermetic CTFE |
| `CTFE002` | CTFE step budget exhausted |
| `CTFE003` | CTFE call-depth budget exhausted |
| `CTFE004` | CTFE logical-heap budget/accounting overflow, allocation-ID exhaustion, or event-count exhaustion |
| `CTFE005` | Invalid include path, identity, bytes, or UTF-8 |
| `CTFE006` | Required CTFE dependency cycle, trap, panic, abort, or uncaught throw |
| `CTFE007` | Required CTFE result is not a promotable canonical value tree |
| `CTFE008` | Required CTFE execution violates an unsafe value/provenance precondition |
| `CORE001` | Invalid Generic Core structure or reference |
| `CORE002` | Invalid Generic Core type, value, CFG, or call contract |
| `CORE003` | Invalid Generic Core move, borrow, cleanup, or unwind contract |
| `CORE004` | Invalid Generic Core effect or sealed evidence |
| `IDENTITY001` | Invalid, noncanonical, or exhausted session/stable declaration, type, or interface identity input |

Compilation phases rank manifest/workspace/dependency/lock; lex/parse;
module/name resolution; declaration/type/trait/coherence; body/call/operator/
pattern; move/borrow/lifetime/unsafe/drop; effect/capability/closure/generator/
thread/ECS; immutable include acquisition/input validation; dependency-ready
RootSlice Core construction/verification then CTFE execution/result promotion
in the canonical root order; identity finalization; then CompleteWorkspace Core
construction/verification. Include bytes may populate a candidate DataRow before
verification, but no evaluator executes an unverified body. A phase with errors prevents dependent later work,
while independent packages/targets/bodies in that phase may continue.

Within a phase diagnostics sort by canonical package-name bytes, target ID,
portable source-path bytes, primary start/end byte, code, then message bytes.
Spanless diagnostics sort after spanned diagnostics for the same target. Exact
duplicates emit once. Secondary labels sort by path/start/end/message; notes use
constructor-defined semantic order. Human output is
`path:line:column: error[CODE]: message` followed by ordered `note:` lines.
Structured diagnostics retain lexer-captured `u64` line/column and byte spans.
Every M27-C error exits `1`; CLI usage remains `2`; M27-C introduces no warning.

Internal semantic goldens use one canonical UTF-8/LF S-expression envelope;
they are test/debug contracts rather than public artifact formats. The first
line is exactly one of `ARCHE-AST-TEXT 1`, `ARCHE-HIR-TEXT 1`,
`ARCHE-TYPE-TEXT 1`, `ARCHE-TRAIT-TEXT 1`, `ARCHE-MIR-TEXT 1`,
`ARCHE-CORE-TEXT 1`, or `ARCHE-CTFE-TEXT 1`, followed by one root expression and
one final LF. Lists preserve the semantic order specified in this section;
mathematical sets use canonical identity order. Node names and field names are
lowercase ASCII with hyphens and fields appear in declaration/opcode order.
Unsigned/signed integers are minimal decimal, stable IDs are 32 uppercase hex,
floating values are exact uppercase bit-pattern hex (`0x` plus 8 or 16 digits),
and source spans spell file ID plus start/end byte/line/column as decimal.
Strings use JSON quoting, escape only control bytes, backslash, and quote, and
otherwise retain exact UTF-8. Host paths, pointer values, capacities, hash-map
order, and debug-derived Rust type names are forbidden. Each C1-C5 slice adds
complete node/opcode goldens within this already-fixed envelope rather than
inventing another printer.

#### M27-C mandatory acceptance

The acceptance corpus contains two structurally different real workspaces from
C1 onward. `language-game` is a library plus authoritative-game binary with an
explicit module tree, recursive utility calls, capability-bearing main,
components/resources/systems/schedules, closures, generators, and trait-based
operators. `language-environment` is a library plus deterministic environment
target with generic collection-heavy state, const computation/includes,
patterns, exceptions, tags, and multiple schedules. Neither workspace name,
item count, declaration ordinal, body digest, or exact source bytes may affect a
production code path. Each slice grows both workspaces without accepting syntax
whose semantics belong to a later slice.

The positive/golden matrix is mandatory:

- C1 snapshots every lexical token/literal form, item/type/generic/path form,
  body/statement/expression/pattern node, visibility/re-export, global FileId,
  symbolic type/const/effect tree, source span/EOF, and source-tree digest.
  Span vectors cover multibyte Unicode, tabs, LF, CRLF, bare CR, multiline
  half-open endpoints, and the unique zero-width EOF/missing-delimiter span.
  Package-provenance, selected-version/source/digest, Normal/Development
  dependency, target/module, declaration-path, and binding/re-export goldens
  cover canonical zero-based PackageNodeId order, per-package zero-based
  TargetId order, exact `IDENTITY001` checked exhaustion of each allocator, and
  two packages with the same TargetId/local-ID tuple suffix without a cross-
  package collision. A mixed-target vector pins library ID zero, then binary IDs
  in `[[bin]]` array order, then environment IDs in `[[environment]]` array
  order. A paired manifest moves `[lib]` and the two target-kind table groups
  without changing either array's internal order and produces identical IDs.
  A selected-registry near-exhaustion vector commits its inclusion-verified
  `[package]` header span in registry-snapshot version 2 and proves the failing
  PackageNodeId diagnostic uses that virtual registry manifest and exact range.
  They also cover the same spelling under
  library/binary/environment roots, and a public re-export through a private
  internal module. Development aliases remain inventory-visible but source-
  invisible. Binary world/main/capability and environment world/profile/reset/
  step/self-play contracts round-trip their exact semantic keys. The semantic-
  inventory skeleton retains an otherwise unreferenced private item and an
  empty target, every declaration/member declared-visibility row, and the
  branded embedded-core version/digest/synthetic-source/virtual-row authority.
  Module and declaration public bindings use distinct payload goldens, including
  a field and inherent method whose visibility differs from its owner. A
  concurrent-source-replacement seam proves that checked bytes and hashed bytes
  are the same retained snapshot. Exact lexer vectors distinguish `1..2`,
  `1..=2`, `1.`, and invalid `1.foo`; grammar negatives cover every empty-comma
  delimiter form and singleton/multielement tuple double comma.
- C2 covers every scalar and const-independent aggregate type plus symbolic
  array/const-generic obligations, safe coercion and explicit conversion,
  direct/generic/method/operator selection, orphan/overlap/default
  specialization, ordinary Map ordering, match ergonomics, every pattern form,
  guards, reachability, and exhaustive decision trees. Literal/type goldens pin
  decimal and hexadecimal f32/f64 round-to-nearest-even results at halfway,
  subnormal, signed-zero, maximum-finite, and overflow boundaries. Ord goldens
  prove negative/zero/positive interpretation, zero/Eq consistency, and that
  result magnitude is ignored. Paired otherwise-identical impls flip only
  `is_default` and prove the impl, owned-method DefinitionIds, specialization
  choice, coherence row, and InterfaceHash change while an inherent impl rejects
  the marker. Two conditional inherent impls over the same nominal target use
  the same method name/signature but different canonical predicates and prove
  distinct owner chains, owned-method DefinitionIds, and public interface rows;
  changing one predicate changes that method ID and InterfaceHash, while
  repeating the method under a byte-identical canonical inherent head is
  rejected.
- C3 covers direct/mutual recursion, all place projections, partial/full moves,
  definite initialization, Copy/Clone/Drop, assignment staging, temporary and
  lexical drop order, NLL reborrows, lifetime variance/elision, raw operations,
  Pin/MaybeUninit, safe/unsafe named-generator construction boundaries, and
  normal/checked/panic/trap cleanup CFGs. Binder-keyed region substitutions are
  golden-tested for `pick<'a,'b>` through direct, trait, function-pointer, and
  closure calls with different local loans, including nested `&'a &'b T`
  referent projection. Bound-region bundles round-trip `id<T>`, pointwise-union
  `select<T>`, and preserve distinct families through Option<T> and Vec<T> in
  both generic templates and closed views. A finite mutual generic permutation
  is accepted, while `f<T> -> f<Option<T>>` and const-growing recursion are
  `CALL001`. Two-element reference arrays prove
  one array family with unioned origins; Pin<&mut T> follows the pointer-owner
  family while Pin<Box<T>> follows Box storage. `Vec<&T>` push, Map insertion, and
  a user `&mut S<'a>` setter prove normal/checked/unwind PlaceAt updates, while a
  direct and function-pointer None-to-Some(&GLOBAL) mutation activates a static
  family and a sealed no-publication failure preserves the pre-call fact.
  Indexed read/PlaceCopy, shared/mutable borrow, unsafe RawAddress, and staged
  Replace cases prove one outer-to-inner CheckedIndex authorization per dynamic
  projection. Paired negatives prove that the same valid authorization never
  legalizes dynamic PlaceMove, PlaceInit, or Drop. A length-changing alias
  mutation invalidates the proof, fixed-array storage does not, and a CFG join
  retains only the predecessor intersection.
  Scoped Mutex<&T> and channel round trips, a pre-clone Arc<Mutex<Option<&T>>>
  updated then read through the other alias, guard mutation, MapIter borrow/next,
  and descriptor-derived query/cursor/resource guards prove their payload-
  bearing storage rows. Reference-bearing
  panic payloads are rejected by UnwindPayload; an owned custom-Drop payload is
  destroyed at terminal panic, adopted/destroyed after trap supersession, and
  deliberately abandoned only by the status-134 double-panic path.
- C4 covers deterministic call/effect SCCs, catch/rethrow remainder sets,
  schedule unions/repeats, every capability class, environment rejection through
  indirect calls, closure capture/Fn classes, named and captured anonymous
  generator factories with initial arguments, construction without body
  effects, multi-yield loops with canonical state/drop rows, every resumed/
  suspended/terminal generator state, repeated Fn/FnMut factory calls,
  conflicting live FnMut-frame rejection, use-after-FnOnce rejection, scoped and
  unscoped Send/Sync/Unpin judgments for every closed aggregate, owner,
  reference/pointer, synchronization primitive/guard, channel, JoinHandle,
  capability, OS, and ECS row; unscoped JoinHandle pending/completed result
  families restricted to Static origins. A first-class named function is
  normalized to its exact tag-25 function-pointer value before the closed
  Send/Sync/Unpin table is applied, while the same syntactic named-path call
  remains a DirectCall. Other cases cover atomic ordering, EcsValue/EcsKey, const-
  independent DefinitionId/TypeId vectors including same-named trait and impl
  methods under their distinct owner chains, a trait/impl method rename that
  changes the enclosing DefinitionId/InterfaceHash, and two differently named
  same-signature methods that retain distinct entries; explicitly pending identity/
  interface skeletons. `verify_semantic_inventory` seals the exact immutable
  workspace/target/module/declaration/body inventory with symbolic pending shapes
  before C5 consumes it. One mutual-recursion fixture changes its actual effect
  set inside an unchanged declared superset and proves its DefinitionIds remain
  stable; a paired fixture changes the declared boundary and therefore its final
  DefinitionIds. A third puts an SCC body beneath a const-dependent owner and
  carries symbolic `throws { Error<K> }`. All prove
  that SemanticBodyKey ordering constructs the same least fixed point before
  IDs exist and after Generic Core rechecks it. Two calls to one named generator
  with borrowed inputs from different scopes share one stable structural state
  TypeId but carry distinct RegionFact origin substitutions; short-scope
  resume/drop succeeds and escape is rejected. A branch-merged reference proves
  exact static/bound/multiple-loan origin union, while a conflicting mutable
  merge is rejected; None/uninitialized/partially moved reference families remain
  absent until initialized. Variant-qualified enum families refine independently,
  regular recursive owners union origins across multiple dynamic depths, and
  raw/function pointers, captures/factories, and each generator state match the
  exhaustive storage-shape table. Two closure instances return stored-capture-
  derived references to distinct locals; direct and generic Fn/FnMut calls
  borrow owned capture storage through ReceiverCapture and use those references
  after call return; mutating, calling, moving, or dropping the callable before
  the final derived reference dies is rejected, while the equivalent FnOnce
  escape is rejected. An anonymous generator yields and returns a
  capture-derived reference. A pinned self-referential multi-yield generator
  maps internal loans to GeneratorSelf only for its own frame/state and drop
  consumes them. Resume replaces its state fact on every yielded/terminal edge.
  Paired anonymous-callable cases spell only `requires` and only `throws` and
  prove the omitted partner remains independently inferred; an expected
  callable type makes both axes declared. Direct and reference-form `str` and
  `[T]` cases prove their complete Send/Sync/Unpin rows. Recursive EcsValue
  cases prove the
  empty Drop-requires rule through custom Drop and containers. Recursive EcsKey
  cases cover every eligible form, nested-float and user-Ord rejection, and
  byte-exact sealed ordering for signed/unsigned integers, char/entity/String,
  sequences, variants, Box, Vec, and nested Map. Generic helpers over direct
  `K: EcsKey`, nested `Vec<Option<K>>`, and a two-bound `(K,L)` key semantically
  select the sealed comparator for every Map operation and explicit Eq/Ord call
  without an ImplRow; their structural bound-leaf paths are respectively one
  root row, one nested row, and two ordered rows. A closed key has zero rows and
  one competing user selection is rejected.
  The environment matrix routes a permissive-signature function pointer through
  a const receipt, immutable static, aggregate, and helper call to a body that
  observes a raw address and proves the finite target/body closure rejects it;
  a repeated selected schedule whose system directly observes an address proves
  every SystemRun seeds its system body even without an indirect call;
  a second pointer parameter with any unknown incoming target is rejected, while
  the same closed flow to two summary-empty bodies is accepted deterministically.
- C5 lowers and verifies dependency-ready RootSlice Core before evaluating each
  closed root, verifies every parameterized template for D-time substitution,
  finalizes dependent identities/judgments from successful results, then lowers
  and independently verifies the CompleteWorkspace Core. Closed-view oracles
  execute generic `id<T>`/`select<T>` only through caller-qualified sealed
  ClosedRegionViews and reuse the structural view safely across recursion.
  Other hermetic CTFE oracles cover integer widths/boundaries, masked shifts,
  signed truncation-toward-zero quotient/remainder and unsigned floor/remainder
  equations for every width/sign combination, and four distinct trap cases for
  every applicable width: division by zero, remainder by zero, signed
  `MIN / -1`, and signed `MIN % -1`. The first two require exact
  `IntegerDivideByZero` and the latter two exact `IntegerSignedOverflow`; exact
  f32/f64 canonical-NaN bits and literal/arithmetic subnormal/signed-zero
  behavior, chars,
  arrays/tuples/structs/enums, recursion, traits, patterns/guards, short circuit,
  caught/rethrown exceptions, closures, direct and generic-Fn generator-factory
  construction followed by first and second resume with the second input used
  as the first yield-expression result, suspended/complete
  generator drop, exact
  drop order, String/Vec/Map/Box/Rc/Arc/Weak/Pin, includes, and every step/depth/
  heap boundary. An allocation-free unit root and a one-String-allocation root
  pin the complete ordered event bytes and trace digests, including the fresh
  zero counters, first `depth-enter(..., 1)`, body-entry charge, final
  `depth-exit(..., 0)` before promotion, and first allocation ID one. A virtual
  counter-boundary vector assigns `u64::MAX` exactly once, marks the counter
  exhausted, and proves the next nonzero reservation is `CTFE004` with no new
  ID, event, or mutation. A separate virtual event-count vector permits the
  completed event that changes `event_count` from `u64::MAX - 1` to
  `u64::MAX`, then proves the next prospective event is `CTFE004` before its
  transition, event bytes, or mutation. Sealed-comparator vectors use equal and
  first-byte/first-child-different String, Vec, and nested-Map keys to pin every
  class-4/class-6 event in canonical short-circuit order. They pass at the exact
  required step budget and produce `CTFE002` one step below it before the next
  visited unit. An explicit Eq/Ord case retains its TraitCall context, point,
  and span; the same keys in Map lower-bound search retain the Map Invoke
  context, point, and span and one continuing ordinal stream across every
  midpoint-entry visit, internal comparison, and suffix visit. Map cases use
  distinct compare-equal keys to prove resident-key retention, incoming-key Drop
  before equal-value publication, and committed removal before removed-key Drop,
  including both panic cleanup outcomes and
  the exact Drop(K) requires summary. Direct `K`, `Vec<Option<K>>`, and two-bound
  key Eq/Ord Core calls carry the exact path-qualified SealedEcsKeyComparison
  rows, and Map intrinsic bounds use the same structural entailment. Closed
  substitution preserves the selection variant, empties its bound rows, and
  independently rederives the comparator without an ImplRow. Generic Core cases transfer non-Copy SSA
  owners through block parameters, install otherwise-unused values into cleanup
  Places, and project non-Copy aggregates only after Place materialization.
  One-byte heap-limit edges cover Box and Vec payloads containing
  function pointers, captured closures/factories, each generator state,
  fixed-max enums, and initialized/uninitialized MaybeUninit. Nested
  `Vec<String>` predecessor materialization fixes allocation IDs and stage/
  publish/rollback order. Promoted String/Vec roots transfer their allocations
  before `final_heap` and then seal an exact zero sample; a separate acyclic
  Rc/RcWeak graph exercises clone, downgrade, failed/valid upgrade, and reverse
  cleanup while also ending at zero before nonsemantic arena teardown. A fan-out fixture has two roots consume one expensive
  predecessor and proves that predecessor executes/meters exactly once under its
  own budget while each consumer pays only its own value-use work. Function-
  pointer goldens promote one named item and two zero-capture closures with the
  same owner and signature but distinct ordinals and bodies; all three retain
  distinct logical targets and digests. One target travels through a predecessor
  receipt and nested aggregate/Vec before indirect invocation, proving the
  points-to fixed point retains exactly its body. Two byte-identical DataRefs
  from distinct literal/include sources prove equal value digests but distinct
  sealed provenance/relocation rows. An immutable static containing String and
  custom Drop is read twice: its initializer executes once as its own root, each
  consumer mounts one shared external slot without heap recharge, and no read or
  evaluator teardown runs its Drop. Successful CTFE values normalize and
  complete every const-dependent DefinitionId and TypeId; result digests certify
  provenance and enter exported const/static InterfaceHash
  vector. CompleteWorkspace independently rebuilds each PackageId, every
  DefinitionId/TypeId, each final TargetRow, and the final InterfaceHash from its
  package/module/export rows; binary and environment rows exactly map the sealed
  semantic target contracts. Dropping the inventory's unreferenced private item
  or empty target, changing only a public re-export path, or globally remapping
  otherwise self-consistent raw IDs is rejected (the re-export mutation also
  changes the hash).
  Two packages set distinct effective step/depth/heap budgets around the same
  root and prove each receipt uses only its declaring sealed row. A nonhermetic
  binary `main` constructs a computed String const through CtfeResultRef, then
  writes it through Stdio; CompleteWorkspace retains the reference in that body,
  canonical RootSlice projection still evaluates only the const dependency, and
  the Complete verifier enumerates every occurrence as the exact mandatory
  M27-D replacement set; C emits no Instance Core.
  Embedded `panic` in a CTFE call is retained solely through the branded virtual
  projection and produces the fixed diagnostic/body behavior.
  Two distinct
  const-definition paths that evaluate to equal bits feed
  the same inferred throws union and prove it collapses to one raw TypeId;
  their distinct receipt keys/digests remain visible in dependency provenance;
  spelling both explicitly in one declared throws set is the paired
  post-finalization `EFFECT001` case.
  Closed generic-Fn CTFE cases additionally distinguish a direct named-function
  path call from its first-class FunctionRef, then instantiate that pointer, an
  ordinary closure, a first-class named generator factory, and an anonymous
  generator factory and prove each selects the exact rewrite operation above. Promotion
  cases pair a cleanly dropped dynamic nonpromotable child (`CTFE007`) with the
  same shape whose enclosing custom Drop panics, proving cleanup precedence and
  suppression of CTFE007. A wildcard catch of a custom-Drop exception routes the
  owned payload through the ordinary Drop terminator, including its panic/
  unwind behavior.

The negative matrix asserts the exact reserved code, primary span, ordered
notes, status `1`, unchanged lock, and complete temporary/spool cleanup. It
covers malformed UTF-8/comments/escapes/numerics/operators/semicolons and every
reserved keyword; invalid path roots, aliases, NFC/case/physical identities,
PackageNodeId/TargetId checked exhaustion (`IDENTITY001`), a missing or corrupt
registry package-manifest span (`DEPENDENCY003` before allocation), visibility,
entry/root-world signatures, types/generic kinds/sized recursion,
integer and finite-float literal fit/rounding/coercion; missing/ambiguous trait selection, orphan/overlap/invalid
specialization and method lookup; invalid/refutable/nonexhaustive/unreachable
patterns and binding modes; every move/init/borrow/lifetime/drop/unsafe/Pin
violation; every effect mismatch, nonexhaustive catch, invalid throw/rethrow,
capability forge/static/capture/environment path; a duplicate inherent method
under one byte-identical canonical head; invalid closure/generator/thread/
atomic/EcsValue/EcsKey judgment, including every sealed auto-trait row and any
invented function-item semantic type or auto-trait judgment,
capability-requiring Drop inside an EcsValue, float at any EcsKey depth, and an
otherwise structural key with user Eq/Ord selection or a generic EcsKey bound
with a competing user comparison selection; all CTFE forbidden operations, cycles,
uncaught outcomes, budget edges, include escapes/aliases/replacement/UTF-8; and
each diagnostic code in the table above.

Constructed unverified-Core tests mutate every record/enum field and every
instruction, terminator, projection, primitive op, panic/trap kind, and
IntrinsicId. They cover unknown/noncanonical/duplicate/out-of-range IDs and
types; wrong toolchain/release/workspace/registry commitment, package origin/name/
selected version/source kind/source digest/PackageId, dependency alias/target/
requirement/kind/order/scope, workspace root, per-package effective CTFE budget,
or an EmbeddedCore source outside the branded slot; wrong target contract variant,
manifest ordinal, root world, main, capability set/order, environment profile,
reset/step/self-play schedule, or final TargetRow; forged/mismatched semantic-
inventory brand, embedded-core Arc/version/digest/PackageId/synthetic file/
virtual definition/type/trait/method/interface/panic-body projection, or source tree, omitted/extra
empty target, private declaration, body key, definition target/module/visibility
provenance, declaration/member visibility row, module parent/file/binding,
cross-package re-export source/target, effective audience, pending/final interface
state, module/declaration payload tag or forbidden payload field, `is_default`
identity/ImplRow/coherence byte, inherent-method owner-head generic/predicate
byte, or derived specialization parent,
re-export-only surface, or InterfaceHash; wrong RootSlice/CompleteWorkspace
scope, root key, predecessor/result
closure, raw/fabricated/mismatched/stale CTFE receipt, source/budget/accounting
provenance, noncanonical CtfeRootKey byte framing, CTFE root initial state/
depth-event order/allocation ID zero/gap/wrap/exhaustion transition, a missing,
extra, or reordered step event, a wrong charge class/context/ProgramPoint/span/
visited ordinal, an ordinal reset between nested or Map-internal comparisons, a
noncanonical sealed-comparator child/entry/String-byte visit, nonzero successful `final_heap`,
missing/extra/wrong-key/wrong-type/
wrong-digest CtfeResultRef in either a CTFE or nonhermetic body,
missing/extra/remapped DataRef provenance, immutable-static binding, or indirect
CTFE target/body closure; missing/extra/unknown environment function-pointer
site/target/body, forged direct forbidden summary, or nonempty environment union;
final-ID state, or CtfeRoot owner; wrong
arity/result/effect/evidence; forged
actual-effect summary, either declared/inferred boundary discriminator, its independent boundary subset/equality,
compiler-callable ConcreteImpl, unrewritten closed callable witness, or
SealedEcsKeyComparison with the wrong obligation/key/path/index, a missing,
extra, duplicate, or reordered bound-leaf row, a nonleaf path, zero rows despite
an unresolved bound leaf, no complete structural proof, or a competing user
selection;
dominance/reachability/block order, asserted unreachable path, shortened or
extended FixedWidthBits, a stable ID substituted for a dense ID or wrong-width
IntrinsicId, inconsistent SourceSpan byte/scalar/line/column/EOF endpoints,
duplicate or abandoned non-Copy SSA use, direct cross-edge non-Copy Value use,
missing cleanup-Place materialization, Copy of a non-Copy Place, or by-value
projection from a non-Copy SSA aggregate; missing/mismatched/non-dominating/
invalidated CheckedIndex authorization, wrong projection ordinal, an
authorization attached to a forbidden dynamic PlaceMove/PlaceInit/Drop, or any
fabricated generic Allocate opcode; enum-payload
refinement, forged SliceMake bounds/form, or removed PlaceDeinit opcode;
invalid places/moves/loans/variant/state/recursive or payload-handle region
families, bound bundles, type/lifetime substitutions, caller-qualified closed
  views/contexts/origin namespaces, nested referent summaries, callable descriptor
  flows or ReceiverCapture loan transfer/liveness, GeneratorSelf token/site/frame/
  state origins, shared-storage token/alias/reaching-other-alias facts, static-
  family activation, mutation-edge facts, unsafe regions, or capture
creation-body/source/frame/transfer rows; missing, duplicated, mistyped, or
wrong-context unwind tokens; catch tag/type mismatches; generator root/state/
factory/descriptor/parameter/Pin errors, including two dependency packages
with the same dense GeneratorId and a cross-package descriptor-ref mutation;
  missing/duplicated/mistyped generated
resume parameters; duplicate/missing/out-of-range/mismatched suspension/drop
rows or nonterminal panic/trap exit; forged capability/ECS evidence;
invalid function-pointer target tag,
owner, closure ordinal, substitution, or signature; and CTFE calls reaching IDs
`100..211`. Every mutation fails verification before evaluation and cannot
construct `VerifiedGenericCore`.
Focused receipt-substitution cases mutate the final root body, one reachable
callee, one cleanup/drop body, selected impl/evidence, one DataRow, one final
TargetRow, and one package Final/Pending/interface-hash projection after a valid
receipt is produced; project_root_slice rejects each before CompleteWorkspace
branding.

C0 and every C1-C6 PR run the repository regression commands applicable to the
then-present crates. C6 must record the exact outputs of this complete set at
the PR head and repeat it on merged `main` where the command is host-applicable:

```text
cargo fmt --manifest-path ./bootstrap/archec0/Cargo.toml --all -- --check
cargo check --locked --workspace --all-targets --manifest-path ./bootstrap/archec0/Cargo.toml
cargo test --locked --workspace --all-targets --manifest-path ./bootstrap/archec0/Cargo.toml
cargo test --locked --release --workspace --all-targets --manifest-path ./bootstrap/archec0/Cargo.toml
cargo clippy --locked --workspace --all-targets --manifest-path ./bootstrap/archec0/Cargo.toml -- -D warnings
cargo tree --locked --workspace --duplicates --manifest-path ./bootstrap/archec0/Cargo.toml
cargo audit --file ./bootstrap/archec0/Cargo.lock
pwsh -NoProfile -File ./tools/test.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
git diff --check
git status --short
```

The first PowerShell command is required on Linux and PowerShell Core on
Windows; the second is required on Windows PowerShell 5.1. Hosted exact-head CI
must keep the existing `Native Linux`, `Windows`, `physical >4GiB source`, and
`sparse >4GiB executable` required contexts green without renaming or weakening
them. Tests assert no lock mutation on failure, no source reopen, no residual
snapshot/sibling temporary, deterministic repeated bytes/diagnostics, no M26
behavioral regression, and a clean exact commit after evidence is recorded.

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

Canonical Value begins with stable `TypeId`, checked payload length, and flags, then type-directed logical bytes. Strings preserve exact UTF-8, vectors logical order, maps the same strict sealed-EcsKey comparator order used by Map operations and CTFE, enums variant plus payload, and `Box` its pointee. A decoder rejects duplicate, descending, user-Ord, or otherwise noncanonical map-key order before publication. It never exposes pointers, padding, spare capacity, allocator state, or hash buckets. Decode builds in staging storage and publishes only after full validation.

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

`Arche.toml` schema 1 is a closed, UTF-8 TOML contract. It starts with the
mandatory integer `schema = 1` and contains `[package]`, `[workspace]`, or both.
A package declares `name`, canonical `version` without build metadata,
`edition = "2026"`, and an `arche` SemVer toolchain requirement. `publish`
defaults to false. Publishable packages also declare a valid SPDX license
expression; archive policy may require separately declared documentation files
when packaging is implemented in M27-J. Unrecognized tables or keys are errors
rather than forward-compatible guesses.
Checking validates every workspace member's `arche` requirement against the
exact selected toolchain before source/HIR work or lock publication.

Targets are explicit. `[lib]` is singular and defaults its path to
`src/lib.arc`. Each `[[bin]]` declares a unique identifier name, source path,
`world = "package::..."`, and a sorted capability list; a sole binary may omit
its path and use `src/main.arc`. Each `[[environment]]` declares an explicit
name, path, owned root world, and environment-profile name. The matching
`[environment-profile.<name>]` declares reset, step, and self-play schedule
paths. Environments cannot request ambient capabilities. `[const-eval]` carries
positive `steps`, `call-depth`, and `heap-bytes`; unpublished packages receive
the scaffold defaults while publishable packages pin all three explicitly.

A mixed-target package uses this schema shape (paths and names are illustrative):

```toml
schema = 1

[package]
name = "example/simulations"
version = "0.1.0"
edition = "2026"
arche = ">=0.0.0"
publish = false

[const-eval]
steps = 10000000
call-depth = 1024
heap-bytes = 67108864

[lib]
path = "src/lib.arc"

[[bin]]
name = "server"
path = "src/main.arc"
world = "package::server::ServerWorld"
capabilities = ["args", "monotonic-clock", "stdio", "udp"]

[[environment]]
name = "grid_pursuit"
path = "src/grid_pursuit.arc"
world = "package::grid_pursuit::GridWorld"
profile = "training"

[environment-profile.training]
reset = "package::grid_pursuit::Reset"
step = "package::grid_pursuit::Step"
self-play = "package::grid_pursuit::SelfPlay"
```

Workspaces use sorted explicit member paths with no globs, nesting, or
outside-root members. A combined package/workspace lists `.` explicitly. Member
paths use `/`; except for the workspace-root `.` they cannot be empty, absolute,
drive-relative, UNC, contain backslashes, `.`/`..` segments, or traverse a
symlink/junction. Default members are a sorted unique subset; omission means all
members. Every path dependency names a declared member, and the workspace has
one lockfile, cache, and target directory. Physical aliases, duplicate package
identities, case-fold/NFC path aliases, and package dependency cycles are
errors.

`[dependencies]` and `[dev-dependencies]` use explicit alias tables only:

```toml
[dependencies]
math = { package = "arche/math", version = "^0.1.0" }
tools = { path = "packages/tools" }
shared = { package = "example/shared", version = "=0.2.0", path = "packages/shared" }
```

Registry dependencies contain exactly `package` and `version`; local-only
dependencies contain exactly `path`; publish-compatible path dependencies
contain all three and must match the target member's identity/version. Packaging
later removes only `path`. Git/URL/custom-registry, build-script, feature,
optional, target-conditional, and string-shorthand dependencies are rejected.
Dependency paths resolve from the declaring package directory. They may use one
or more `..` segments only as a canonical leading prefix so sibling workspace
members are expressible; `.` segments and a parent segment after an ordinary
segment are rejected. Resolution must remain inside the workspace, traverse
exact NFC/case filesystem components without links or junctions, and name one
explicitly declared member.

The pure resolver consumes an immutable validated registry snapshot. It selects
one source and one version for each package identity across the graph, excludes
yanked candidates on fresh resolution, admits prereleases only through an
explicit prerelease requirement, and deterministically backtracks over versions
in descending SemVer order. Its global result lexicographically maximizes the
selected version vector ordered by package name. Input enumeration order cannot
affect the graph or diagnostics. Production index/cache/network acquisition is
owned by M27-H and M27-J, not simulated by M27-B.

Every resolved graph is revalidated before HIR or lock consumption: package
rows and dense node IDs are canonical, names/PackageIds/workspace paths are
unique, root and edge lists are sorted and complete, aliases are unique per
source package, registry nodes cannot depend on workspace nodes, all nodes are
reachable, and cycles are rejected. Resolution compares complete solutions and
maximizes the version vector in canonical package-name order rather than
accepting the first locally highest candidate.

`PackageId` is version-independent. Its canonical preimage, after the M27-A
`ARCHE-PACKAGE-ID\0` domain and fingerprint-version prefix, is the little-endian
`u64` byte length and UTF-8 bytes of
`registry+https://packages.arche-lang.org`, followed by the little-endian `u64`
byte length and UTF-8 bytes of the scoped package name. Local workspace copies
use that intended registry identity too. A resolved package instance separately
records its selected version and source digest; final declaration identity waits
for M27-C's complete declaration-shape encoding.

`Arche.lock` schema 1 is canonical UTF-8 with LF endings, one final newline,
fixed field order, package rows sorted by identity, dependency edges sorted by
alias/target, and no timestamps or host-absolute paths. It pins the exact
toolchain version and release-manifest digest, official registry identity
`registry+https://packages.arche-lang.org` and snapshot digest, every resolved
package version/source, complete reachable graph, workspace source digest,
registry archive/source digests, and provenance/inclusion record digests. All
integrity digests are lowercase `sha256:` followed by 64 hexadecimal digits;
language identity hashes remain the separately domain-separated BLAKE3 values.
The top-level `[workspace] source-digest` commits the exact root authority
manifest through the source-tree encoding, including for a virtual workspace;
member package rows separately commit their own manifests and declared source
trees. Thus changing member/default selection cannot leave the lock unchanged.

The canonical workspace source-tree digest hashes the byte sequence
`ARCHE-SOURCE-TREE\0`, little-endian `u32` version 1, little-endian `u64` entry
count, then each semantic input in portable-path byte order as `u64` path length,
path UTF-8, `u64` content length, and the raw 32-byte SHA-256 digest of the exact
immutable snapshot consumed by the parser. This nested commitment permits
streaming without reopening mutable source paths after checking. Archive and
signed-record digests hash their canonical bytes. A lock decoder validates its complete graph,
sources, requirements, digests, and canonical re-encoding before use. Lock
publication uses a synchronized sibling temporary and atomic replacement;
failure preserves the prior lock and leaves no temporary file. The promise is
atomic visibility, not parent-directory durability.

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

M27-C is substantially larger than the adjacent compiler gates and therefore
uses the following mandatory short-lived implementation slices. They are not
new product milestones: M27-C remains the sole Doing gate until C6 closes, and
M27-D cannot begin merely because an earlier slice merges.

| Slice | Closure boundary |
|---|---|
| **M27-C0 — exact contract** | The full grammar, semantic edge rules, identity inputs, Core opcode/verifier schema, CTFE accounting/includes, diagnostic taxonomy/order, golden formats, and later-gate exclusions are normative. It adds no production language implementation. |
| **M27-C1 — syntax/HIR/type shapes** | Immutable snapshots produce complete AST bodies and package-aware resolved symbolic HIR; every selected type/generic kind and byte-exact identity input tree is represented; AST/HIR/type-shape/generic encoder goldens pass; no accepted body is skipped, reopened, or assigned a provisional stable ID. |
| **M27-C2 — traits/operators/patterns** | Const-independent type checking, explicit conversions, symbolic type-level-const obligations, static trait selection, orphan/coherence/`impl default`, operator dispatch, exhaustive pattern decision trees, and their exact positive/negative goldens pass without a concrete instance graph. Pattern ownership validity remains integrated with C3. A const-dependent type/selection remains explicitly pending and cannot make a target successful before C5. |
| **M27-C3 — MIR/calls/ownership** | Typed generic MIR contains CFGs, direct/mutual recursion, move paths, pattern binding moves, NLL, unsafe regions, definite initialization, Drop, and cleanup tokens/edges driven by declared effects; RHS-before-replacement and exact-once cleanup proofs pass. Runtime allocation/glue, native frames, and native unwind remain later. |
| **M27-C4 — effects/stateful abstractions/semantic IDs** | Deterministic recursive throws/requires fixed points, schedule unions, capability/environment restrictions, closure captures/Fn classes, pinned generators, structural Send/Sync, atomic typing, and generic EcsValue/EcsKey judgments pass. The immutable semantic workspace inventory is sealed. Const-independent semantic shapes produce final DefinitionId/TypeId vectors; const-dependent identities and the interface remain explicitly pending. It performs no host/world/thread execution. |
| **M27-C5 — verified Core/CTFE/interface** | Each dependency-ready closed root first lowers to and passes the RootSlice Generic Core verifier, then one explicit-frame hermetic evaluator covers the complete CTFE language, owned logical values, cleanup, includes, and all budgets across two structurally different workspaces. Successful results finalize const-dependent type/trait judgments and DefinitionId/TypeId/InterfaceHash inputs; only then does every accepted body lower into independently verified CompleteWorkspace Generic Core. Parameterized const templates are verified for D-time substitution. It emits no object, serialized Core, runtime value, observation, Machine IR, or executable. |
| **M27-C6 — public closure** | `arche check` runs the complete B-to-C pipeline before atomic lock publication; failures preserve locks and leave no temporary/snapshot; deterministic repetition, M26 isolation, both PowerShell editions, strict/full local gates, adversarial audit, and exact PR-head/merged-main protected CI pass and are recorded in the sole M27-C closure entry in `WORK_LOG.md`. Only C6 marks M27-C Done and promotes M27-D. |

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
