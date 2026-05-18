# Arche Comprehensive Design Document

**Document version:** 0.1  
**Date:** 2026-05-17  
**Status:** Foundational design draft  
**Primary goal:** Define Arche as an independent, native, ECS-first programming language and software platform.

---

## Table of Contents

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
48. [Implementation Roadmap](#48-implementation-roadmap)
49. [Bootstrap and Self-Hosting](#49-bootstrap-and-self-hosting)
50. [Risks and Mitigations](#50-risks-and-mitigations)
51. [Open Design Questions](#51-open-design-questions)
52. [Appendix A: Initial Grammar Sketch](#52-appendix-a-initial-grammar-sketch)
53. [Appendix B: Initial Runtime Structs](#53-appendix-b-initial-runtime-structs)
54. [Appendix C: Initial Arche Core Example](#54-appendix-c-initial-arche-core-example)
55. [Appendix D: Milestone Acceptance Tests](#55-appendix-d-milestone-acceptance-tests)

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
static ELF64 executable
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
    insert Time { delta: 1.0 }

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

The first platform should be intentionally narrow:

```text
Architecture: x86-64
Operating system: Linux
Executable format: ELF64
Linking model: static
Entry point: _start
Runtime dependency: Arche runtime kernel
C library dependency: none initially
```

This target allows Arche to prove independence quickly:

```bash
archec main.arc -o main
./main
echo $?
```

The first executable milestone:

```arche
world Main

startup {
    exit 42
}
```

Expected behavior:

```bash
./main
echo $?
# 42
```

The emitted machine code can initially be equivalent to:

```asm
_start:
    mov rax, 60      ; Linux exit syscall
    mov rdi, 42      ; exit code
    syscall
```

After this works, Arche can expand to:

```text
x86-64 Windows PE/COFF
x86-64 macOS Mach-O
AArch64 Linux
AArch64 macOS
WebAssembly
```

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
    elapsed: f64
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
query[entity, Enemy, Position]
```

## 8.7 Schedules

Schedules define execution order and barriers.

```arche
schedule Main {
    run Input
    run Move
    run ApplyDamage
    flush
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
    uint32_t archetype_index;
    uint32_t row;
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

## 11.2 Component restrictions for early versions

Early Arche components should be plain data only:

```text
bool
i32
u32
i64
u64
f32
f64
entity
fixed-size arrays later
nested plain structs later
```

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
    uint32_t dense_id;
    const char* name;
    uint32_t size;
    uint32_t align;
    uint32_t field_count;
    const ArcheFieldDesc* fields;
    uint32_t flags;
} ArcheComponentDesc;
```

Field descriptor:

```c
typedef struct ArcheFieldDesc {
    const char* name;
    uint32_t type_kind;
    uint32_t offset;
    uint32_t size;
    uint32_t align;
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
    elapsed: f64
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
    uint32_t dense_id;
    const char* name;
    uint32_t size;
    uint32_t align;
    uint32_t field_count;
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
query[entity, Enemy, Position]
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
| `entity` | Include entity handle in iteration |
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
    uint32_t dense_component_id;
    uint8_t access;
} ArcheQueryTerm;

typedef struct ArcheQueryDesc {
    const char* name;
    uint32_t term_count;
    const ArcheQueryTerm* terms;
} ArcheQueryDesc;
```

## 15.4 Query plan

At runtime or link time, a query descriptor becomes a query plan:

```c
typedef struct ArcheQueryTablePlan {
    uint32_t archetype_index;
    uint32_t entity_column_present;
    uint32_t* component_column_indices;
} ArcheQueryTablePlan;

typedef struct ArcheQueryPlan {
    const ArcheQueryDesc* desc;
    uint32_t table_count;
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
    flush
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
    uint32_t system_index;
    uint32_t dependency_count;
    uint32_t* dependencies;
} ArcheScheduleNode;

typedef struct ArcheScheduleBatch {
    uint32_t node_count;
    ArcheScheduleNode* nodes;
    uint32_t has_flush_after;
} ArcheScheduleBatch;

typedef struct ArcheScheduleDesc {
    const char* name;
    uint32_t batch_count;
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
    uint16_t align;
    uint32_t size;
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
    uint32_t dense_id;
    const char* name;
    uint32_t size;
    uint32_t align;
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

void* arche_resource_get(ArcheWorld* world, uint32_t dense_resource_id);
void arche_resource_insert(ArcheWorld* world, uint32_t dense_resource_id, const void* value);

ArcheQueryPlan* arche_query_prepare(ArcheWorld* world, const ArcheQueryDesc* desc);
void arche_query_begin(ArcheWorld* world, ArcheQueryPlan* plan, ArcheQueryIter* out);
bool arche_query_next_chunk(ArcheQueryIter* iter, ArcheChunkView* out);

void arche_commands_append(ArcheCommandBuffer* buffer, const void* command, uint32_t size);
void arche_commands_flush(ArcheWorld* world, ArcheCommandBuffer* buffer);
```

Compiled systems may eventually bypass some runtime calls for known-safe, preplanned queries.

---

# 22. Arche ABI

The ABI defines stable binary expectations.

## 22.1 Primitive sizes

| Arche type | Size | Alignment | Notes |
|---|---:|---:|---|
| `bool` | 1 | 1 | Stored as 0 or 1 |
| `i8` | 1 | 1 | Future |
| `u8` | 1 | 1 | Future |
| `i16` | 2 | 2 | Future |
| `u16` | 2 | 2 | Future |
| `i32` | 4 | 4 | Initial |
| `u32` | 4 | 4 | Initial |
| `i64` | 8 | 8 | Initial |
| `u64` | 8 | 8 | Initial |
| `f32` | 4 | 4 | Initial |
| `f64` | 8 | 8 | Initial |
| `entity` | 8 | 8 | Packed index + generation |

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

System function signature:

```c
typedef void (*ArcheSystemFn)(
    ArcheWorld* world,
    ArcheFrame* frame,
    ArcheCommandBuffer* commands
);
```

Initial x86-64 internal convention:

```text
rdi = world
rsi = frame
rdx = command buffer
```

## 22.4 Query chunk view ABI

```c
typedef struct ArcheChunkView {
    uint32_t len;
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

A stable component ID should be derived from:

```text
fully qualified name
kind: component/tag/resource/event
field names
field types
field order
schema version, if provided
```

Conceptual:

```text
StableComponentId = fingerprint("Demo.Position{x:f32,y:f32}")
```

## 23.2 Dense IDs

Dense IDs are assigned during linking or startup:

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

---

# 24. Arche Core

Arche Core is the canonical semantic representation of a compiled Arche program.

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
all symbols resolve
all fields exist
all loads/stores have valid types
system effects match actual access
queries do not conflict internally
component references do not escape
structural mutations use commands
query loops only access declared query terms
resources are accessed with declared mutability
```

---

# 25. Arche Object Format

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
ELF64 static executable
x86-64 Linux
```

The compiler or linker must emit:

```text
ELF header
program headers
.text segment
.rodata segment
.data segment
.bss segment
entrypoint address
```

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
insert
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
insert resource
run schedule
exit
expression statement
```

Initial expressions:

```text
literals
variables
field access
binary arithmetic
comparison
struct literal
function call, limited
```

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
    id: u64
}
```

Layout:

```text
x: offset 0
y: offset 4
padding: 0 bytes before id because offset 8 is already aligned to 8
id: offset 8
size: 16
align: 8
```

The layout planner must be deterministic.

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

Early version can execute sequentially but should still produce batch metadata.

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
```

Initial supported code:

```text
exit constant
integer locals
integer arithmetic
float arithmetic
field loads/stores
while loops
if statements
query chunk loops
system calls into runtime kernel
```

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
simple linear-scan or even stack-heavy allocation
```

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

The first executable writer should support static ELF64 output.

Required ELF pieces:

```text
ELF header
program headers
.text segment
.rodata segment
.data segment
.bss segment
entrypoint address
page alignment
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
place .text at executable virtual address
place .rodata read-only
place .data writable
set p_flags appropriately
set entrypoint to _start
write file with executable permissions via build tool
```

The first version may skip relocatable objects and emit full executables directly. However, `.aco` support should be specified early because multi-file compilation depends on it.

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
  initialize platform state
  initialize allocator
  create world
  register linked metadata
  run startup function
  destroy world
  exit
```

## 38.2 Startup block

Source:

```arche
startup {
    insert Time { delta: 0.016 }
    spawn { Position { x: 0.0, y: 0.0 } }
    run Main
    exit 0
}
```

Startup code can perform:

```text
resource insertion
entity spawning
schedule execution
basic control flow
exit
```

## 38.3 Exit

Initial `exit` implementation on x86-64 Linux:

```text
rax = 60
rdi = exit code
syscall
```

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
```

## 43.7 Runtime tests

```text
entity allocation
entity generation
spawn
despawn
add component
remove component
resource insertion
query iteration
command flush
```

## 43.8 End-to-end tests

```text
.arc source -> executable -> expected exit code/output/state
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

## 45.1 `archec`

```bash
archec check file.arc
archec build file.arc -o app
archec build file.arc --emit=ast
archec build file.arc --emit=core
archec build file.arc --emit=layout
archec build file.arc --emit=machine
archec build file.arc --emit=obj
archec build file.arc --emit=elf
```

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

Initial syntax should be ECS-first.

## 46.1 World

```arche
world Demo
```

## 46.2 Components

```arche
component Position {
    x: f32
    y: f32
}
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
```

## 46.5 Systems

```arche
system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}
```

## 46.6 Schedules

```arche
schedule Main {
    run Move
    flush
    run Render
}
```

## 46.7 Startup

```arche
startup {
    insert Time { delta: 0.016 }

    spawn {
        Position { x: 0.0, y: 0.0 }
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
    insert Time { delta: 1.0 }

    spawn {
        Position { x: 0.0, y: 0.0 }
        Velocity { x: 2.0, y: 3.0 }
    }

    run Main
    exit 0
}
```

## 47.4 Health and despawn

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

# 48. Implementation Roadmap

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
insert resource
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

# 51. Open Design Questions

These need future decisions:

1. Should Arche Core be serialized as text, binary, or both?
2. Should `.aco` contain machine code directly, Core, or both?
3. Should the linker assign dense IDs statically, or should runtime startup assign them?
4. Should tags be true zero-sized components or separate signature bits?
5. Should optional query terms return nullable references or option-like values?
6. Should Arche support immediate structural mutation outside query loops?
7. How should deterministic scheduling be specified?
8. Should component schema evolution be versioned explicitly?
9. Should packages be globally named or content-addressed?
10. What is the first string model?
11. What error handling model should Arche use?
12. How much of the runtime kernel should be written in Arche once self-hosting begins?
13. Should relations be built into the core runtime or layered as indexed components?
14. Should events be stored as resources, special streams, or command-buffer-like append logs?
15. Should the first debugger attach to running processes or inspect paused snapshots?

---

# 52. Appendix A: Initial Grammar Sketch

```text
program         := world_decl item*

world_decl      := "world" IDENT

item            := component_decl
                 | resource_decl
                 | tag_decl
                 | event_decl
                 | relation_decl
                 | system_decl
                 | schedule_decl
                 | startup_decl

component_decl  := "component" IDENT "{" field* "}"
resource_decl   := "resource" IDENT "{" field* "}"
tag_decl        := "tag" IDENT

event_decl      := "event" IDENT "{" field* "}"
relation_decl   := "relation" IDENT "{" field* "}"

field           := IDENT ":" type

type            := "bool"
                 | "i32"
                 | "u32"
                 | "i64"
                 | "u64"
                 | "f32"
                 | "f64"
                 | "entity"
                 | IDENT

system_decl     := "system" IDENT "(" param_list? ")" block
param_list      := param ("," param)*
param           := IDENT ":" param_type

param_type      := "read" IDENT
                 | "mut" IDENT
                 | "commands"
                 | "query" "[" query_terms "]"
                 | "events" IDENT
                 | "emit" IDENT

query_terms     := query_term ("," query_term)*
query_term      := IDENT
                 | "mut" IDENT
                 | "!" IDENT
                 | "entity"

schedule_decl   := "schedule" IDENT "{" schedule_item* "}"
schedule_item   := "run" IDENT
                 | "flush"

startup_decl    := "startup" block

block           := "{" stmt* "}"

stmt            := let_stmt
                 | assign_stmt
                 | if_stmt
                 | while_stmt
                 | for_stmt
                 | spawn_stmt
                 | insert_stmt
                 | run_stmt
                 | exit_stmt
                 | expr_stmt

let_stmt        := "let" IDENT (":" type)? "=" expr
assign_stmt     := place assign_op expr
assign_op       := "=" | "+=" | "-=" | "*=" | "/="

if_stmt         := "if" expr block ("else" block)?
while_stmt      := "while" expr block
for_stmt        := "for" pattern "in" expr block

spawn_stmt      := "spawn" "{" component_init* "}"
insert_stmt     := "insert" IDENT struct_literal
run_stmt        := "run" IDENT
exit_stmt       := "exit" expr

expr            := literal
                 | IDENT
                 | field_access
                 | binary_expr
                 | call_expr
                 | struct_literal

field_access    := expr "." IDENT
struct_literal  := IDENT "{" field_init* "}"
field_init      := IDENT ":" expr
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
    uint32_t archetype_index;
    uint32_t row;
} ArcheEntityLocation;

typedef struct ArcheEntityStore {
    ArcheEntityLocation* locations;
    uint32_t len;
    uint32_t cap;

    uint32_t* free_indices;
    uint32_t free_len;
    uint32_t free_cap;
} ArcheEntityStore;

typedef struct ArcheComponentColumn {
    uint32_t dense_id;
    uint32_t size;
    uint32_t align;
    uint32_t stride;
    void* data;
} ArcheComponentColumn;

typedef struct ArchetypeTable {
    uint32_t* component_ids;
    uint32_t component_count;

    ArcheEntity* entities;
    ArcheComponentColumn* columns;

    uint32_t len;
    uint32_t cap;
} ArchetypeTable;

typedef struct ArcheArchetypeStore {
    ArchetypeTable* tables;
    uint32_t len;
    uint32_t cap;
} ArcheArchetypeStore;

typedef struct ArcheResourceSlot {
    uint32_t dense_id;
    void* data;
    uint32_t size;
    uint32_t align;
    uint32_t initialized;
} ArcheResourceSlot;

typedef struct ArcheResourceStore {
    ArcheResourceSlot* slots;
    uint32_t len;
    uint32_t cap;
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
    insert Time { delta: 1.0 }
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
