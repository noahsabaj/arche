#![allow(dead_code)]

use crate::core::{
    CoreInstruction, CoreProgram, CoreQueryAccess, CoreScheduleItem, CoreSpawnComponent,
    CoreSpawnFieldValue, CoreSystemParamKind, CoreType,
};
use crate::core_verify;
use crate::layout::{self, ComponentId};
use crate::parser::{
    ComponentDecl, ComponentLiteralValue, Program, QueryAccess as ParserQueryAccess, ResourceDecl,
    ResourceStatement, RunStatement, ScheduleDecl, ScheduleItem, SpawnStatement, Statement,
    SystemDecl, SystemParam, SystemParamKind,
};
use crate::runtime::{
    stable_query_id, stable_resource_id, stable_schedule_id, stable_system_id, ArcheEntity,
    ArcheWorld, ComponentDescriptor, ComponentFieldDescriptor, ComponentPayload, QueryAccess,
    QueryDescriptor, QueryTermDescriptor, ResourceDescriptor, ResourceFieldDescriptor, ResourceId,
    ScheduleDescriptor, ScheduleId, ScheduleItemDescriptor, SystemAccess, SystemDescriptor,
    SystemParamDescriptor, SystemParamDescriptorKind, SystemQueryTermDescriptor,
};
use std::alloc::Layout;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAssemblyError {
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeProgramAssembly {
    pub world_name: String,
    pub component_descriptors: Vec<ComponentDescriptor>,
    pub resource_descriptors: Vec<ResourceDescriptor>,
    pub system_descriptors: Vec<SystemDescriptor>,
    pub query_descriptors: Vec<QueryDescriptor>,
    pub schedule_descriptors: Vec<ScheduleDescriptor>,
    pub startup_operations: Vec<StartupOperation>,
}

impl RuntimeProgramAssembly {
    pub fn new(world_name: impl Into<String>) -> Self {
        Self {
            world_name: world_name.into(),
            component_descriptors: Vec::new(),
            resource_descriptors: Vec::new(),
            system_descriptors: Vec::new(),
            query_descriptors: Vec::new(),
            schedule_descriptors: Vec::new(),
            startup_operations: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.component_descriptors.is_empty()
            && self.resource_descriptors.is_empty()
            && self.system_descriptors.is_empty()
            && self.query_descriptors.is_empty()
            && self.schedule_descriptors.is_empty()
            && self.startup_operations.is_empty()
    }

    pub fn requires_ecs_metadata(&self) -> bool {
        !self.resource_descriptors.is_empty()
            || !self.system_descriptors.is_empty()
            || !self.query_descriptors.is_empty()
            || !self.schedule_descriptors.is_empty()
            || !self.startup_operations.is_empty()
    }
}

pub fn assemble_runtime_program_from_source(
    program: &Program,
) -> Result<RuntimeProgramAssembly, RuntimeAssemblyError> {
    let mut assembly = assemble_runtime_program_descriptors(program)?;
    if let Some(startup) = &program.startup {
        assembly.startup_operations = assemble_startup_operations(
            &program.world.name,
            &assembly.component_descriptors,
            &assembly.resource_descriptors,
            &assembly.schedule_descriptors,
            &startup.statements,
        )?;
    }

    Ok(assembly)
}

pub fn assemble_runtime_program_from_verified_core(
    program: &Program,
    core: &CoreProgram,
) -> Result<RuntimeProgramAssembly, RuntimeAssemblyError> {
    core_verify::verify_core_program(core).map_err(|error| {
        assembly_error(format!(
            "cannot assemble runtime metadata from invalid Core: {}",
            error.message
        ))
    })?;
    if core.world.name != program.world.name {
        return Err(assembly_error(format!(
            "verified Core world `{}` does not match source world `{}`",
            core.world.name, program.world.name
        )));
    }

    let mut assembly = assemble_runtime_program_descriptors(program)?;
    verify_core_runtime_descriptors(core, &assembly)?;
    let core_spawns = verified_core_startup_spawns(core)?;
    if let Some(startup) = &program.startup {
        assembly.startup_operations = assemble_startup_operations_from_verified_core(
            &program.world.name,
            &assembly.component_descriptors,
            &assembly.resource_descriptors,
            &assembly.schedule_descriptors,
            &startup.statements,
            &core_spawns,
        )?;
    } else if !core_spawns.is_empty() {
        return Err(assembly_error(
            "verified Core contains startup spawns but source has no startup block",
        ));
    }

    Ok(assembly)
}

fn assemble_runtime_program_descriptors(
    program: &Program,
) -> Result<RuntimeProgramAssembly, RuntimeAssemblyError> {
    let mut assembly = RuntimeProgramAssembly::new(program.world.name.clone());
    assembly.component_descriptors = program
        .components
        .iter()
        .map(|component| assemble_component_descriptor(&program.world.name, component))
        .collect::<Result<Vec<_>, _>>()?;
    assembly.resource_descriptors = program
        .resources
        .iter()
        .map(|resource| assemble_resource_descriptor(&program.world.name, resource))
        .collect::<Result<Vec<_>, _>>()?;
    assembly.system_descriptors = program
        .systems
        .iter()
        .map(|system| assemble_system_descriptor(&program.world.name, system))
        .collect();
    assembly.query_descriptors = program
        .systems
        .iter()
        .flat_map(|system| assemble_query_descriptors(&program.world.name, system))
        .collect();
    assembly.schedule_descriptors = program
        .schedules
        .iter()
        .map(|schedule| assemble_schedule_descriptor(&program.world.name, schedule))
        .collect();
    Ok(assembly)
}

fn verify_core_runtime_descriptors(
    core: &CoreProgram,
    assembly: &RuntimeProgramAssembly,
) -> Result<(), RuntimeAssemblyError> {
    verify_core_component_schemas(core, &assembly.component_descriptors)?;
    verify_core_resource_schemas(core, &assembly.resource_descriptors)?;
    verify_core_system_descriptors(core, &assembly.system_descriptors)?;
    verify_core_query_descriptors(core, &assembly.query_descriptors)?;
    verify_core_schedule_descriptors(core, &assembly.schedule_descriptors)
}

fn verify_core_component_schemas(
    core: &CoreProgram,
    descriptors: &[ComponentDescriptor],
) -> Result<(), RuntimeAssemblyError> {
    if core.components.len() != descriptors.len() {
        return Err(assembly_error(
            "verified Core and source component schema counts do not match",
        ));
    }

    for descriptor in descriptors {
        let component = core
            .components
            .iter()
            .find(|component| component.id == descriptor.id.0)
            .ok_or_else(|| {
                assembly_error(format!(
                    "source component descriptor `{}` is absent from verified Core",
                    descriptor.name
                ))
            })?;
        if component.name != descriptor.name || component.fields.len() != descriptor.fields.len() {
            return Err(assembly_error(format!(
                "source component descriptor `{}` does not match verified Core",
                descriptor.name
            )));
        }
        for (field_index, (core_field, descriptor_field)) in
            component.fields.iter().zip(&descriptor.fields).enumerate()
        {
            let core_type_name = core_type_name(core_field.ty);
            if core_field.name != descriptor_field.name
                || core_type_name != descriptor_field.type_name
                || descriptor_field.offset != (field_index as u32) * 4
            {
                return Err(assembly_error(format!(
                    "source component field `{}.{}` does not match verified Core",
                    descriptor.name, descriptor_field.name
                )));
            }
        }
        let expected_size = u32::try_from(component.fields.len())
            .ok()
            .and_then(|count| count.checked_mul(4))
            .ok_or_else(|| assembly_error("verified Core component size overflow"))?;
        let expected_align = if component.fields.is_empty() { 1 } else { 4 };
        if descriptor.size != expected_size || descriptor.align != expected_align {
            return Err(assembly_error(format!(
                "source component layout `{}` does not match verified Core",
                descriptor.name
            )));
        }
    }

    Ok(())
}

fn verify_core_resource_schemas(
    core: &CoreProgram,
    descriptors: &[ResourceDescriptor],
) -> Result<(), RuntimeAssemblyError> {
    if core.resources.len() != descriptors.len() {
        return Err(assembly_error(
            "verified Core and source resource schema counts do not match",
        ));
    }
    for descriptor in descriptors {
        let resource = core
            .resources
            .iter()
            .find(|resource| resource.id == descriptor.id.0)
            .ok_or_else(|| {
                assembly_error(format!(
                    "source resource descriptor `{}` is absent from verified Core",
                    descriptor.name
                ))
            })?;
        if resource.name != descriptor.name || resource.fields.len() != descriptor.fields.len() {
            return Err(assembly_error(format!(
                "source resource descriptor `{}` does not match verified Core",
                descriptor.name
            )));
        }
        for (field_index, (core_field, descriptor_field)) in
            resource.fields.iter().zip(&descriptor.fields).enumerate()
        {
            if core_field.name != descriptor_field.name
                || core_type_name(core_field.ty) != descriptor_field.type_name
                || descriptor_field.offset != (field_index as u32) * 4
            {
                return Err(assembly_error(format!(
                    "source resource field `{}.{}` does not match verified Core",
                    descriptor.name, descriptor_field.name
                )));
            }
        }
        let expected_size = u32::try_from(resource.fields.len())
            .ok()
            .and_then(|count| count.checked_mul(4))
            .ok_or_else(|| assembly_error("verified Core resource size overflow"))?;
        let expected_align = if resource.fields.is_empty() { 1 } else { 4 };
        if descriptor.size != expected_size || descriptor.align != expected_align {
            return Err(assembly_error(format!(
                "source resource layout `{}` does not match verified Core",
                descriptor.name
            )));
        }
    }
    Ok(())
}

fn verify_core_system_descriptors(
    core: &CoreProgram,
    descriptors: &[SystemDescriptor],
) -> Result<(), RuntimeAssemblyError> {
    if core.systems.len() != descriptors.len() {
        return Err(assembly_error(
            "verified Core and source system descriptor counts do not match",
        ));
    }
    for descriptor in descriptors {
        let system = core
            .systems
            .iter()
            .find(|system| system.id == descriptor.id.0)
            .ok_or_else(|| {
                assembly_error(format!(
                    "source system descriptor `{}` is absent from verified Core",
                    descriptor.name
                ))
            })?;
        if descriptor.name != qualified_name(&core.world.name, &system.name)
            || descriptor.params.len() != system.params.len()
        {
            return Err(assembly_error(format!(
                "source system descriptor `{}` does not match verified Core",
                descriptor.name
            )));
        }
        for (core_param, descriptor_param) in system.params.iter().zip(&descriptor.params) {
            let matches = match (&core_param.kind, &descriptor_param.kind) {
                (
                    CoreSystemParamKind::ReadResource { resource_id, name },
                    SystemParamDescriptorKind::ReadResource {
                        resource_id: descriptor_id,
                        name: descriptor_name,
                    },
                ) => *resource_id == descriptor_id.0 && name == descriptor_name,
                (
                    CoreSystemParamKind::Query { terms },
                    SystemParamDescriptorKind::Query {
                        terms: descriptor_terms,
                    },
                ) => {
                    terms.len() == descriptor_terms.len()
                        && terms
                            .iter()
                            .zip(descriptor_terms)
                            .all(|(term, descriptor_term)| {
                                term.component_id == descriptor_term.component_id.0
                                    && term.name == descriptor_term.name
                                    && core_access_matches_system(
                                        term.access,
                                        &descriptor_term.access,
                                    )
                            })
                }
                _ => false,
            };
            if core_param.name != descriptor_param.name || !matches {
                return Err(assembly_error(format!(
                    "source system parameter `{}.{}` does not match verified Core",
                    descriptor.name, descriptor_param.name
                )));
            }
        }
    }
    Ok(())
}

fn verify_core_query_descriptors(
    core: &CoreProgram,
    descriptors: &[QueryDescriptor],
) -> Result<(), RuntimeAssemblyError> {
    let expected_count = core
        .systems
        .iter()
        .flat_map(|system| &system.params)
        .filter(|param| matches!(&param.kind, CoreSystemParamKind::Query { .. }))
        .count();
    if expected_count != descriptors.len() {
        return Err(assembly_error(
            "verified Core and source query descriptor counts do not match",
        ));
    }
    for system in &core.systems {
        for param in &system.params {
            let CoreSystemParamKind::Query { terms } = &param.kind else {
                continue;
            };
            let expected_id = stable_query_id(&core.world.name, &system.name, &param.name);
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.id == expected_id)
                .ok_or_else(|| {
                    assembly_error(format!(
                        "verified Core query `{}.{}.{}` is absent from source descriptors",
                        core.world.name, system.name, param.name
                    ))
                })?;
            let expected_name = format!("{}.{}.{}", core.world.name, system.name, param.name);
            if descriptor.name != expected_name
                || descriptor.terms.len() != terms.len()
                || !terms
                    .iter()
                    .zip(&descriptor.terms)
                    .all(|(term, descriptor_term)| {
                        term.component_id == descriptor_term.component_id.0
                            && term.name == descriptor_term.name
                            && core_access_matches_query(term.access, &descriptor_term.access)
                    })
            {
                return Err(assembly_error(format!(
                    "source query descriptor `{}` does not match verified Core",
                    descriptor.name
                )));
            }
        }
    }
    Ok(())
}

fn verify_core_schedule_descriptors(
    core: &CoreProgram,
    descriptors: &[ScheduleDescriptor],
) -> Result<(), RuntimeAssemblyError> {
    if core.schedules.len() != descriptors.len() {
        return Err(assembly_error(
            "verified Core and source schedule descriptor counts do not match",
        ));
    }
    for descriptor in descriptors {
        let schedule = core
            .schedules
            .iter()
            .find(|schedule| schedule.id == descriptor.id.0)
            .ok_or_else(|| {
                assembly_error(format!(
                    "source schedule descriptor `{}` is absent from verified Core",
                    descriptor.name
                ))
            })?;
        if descriptor.name != qualified_name(&core.world.name, &schedule.name)
            || descriptor.items.len() != schedule.items.len()
            || !schedule
                .items
                .iter()
                .zip(&descriptor.items)
                .all(|(item, descriptor_item)| match (item, descriptor_item) {
                    (
                        CoreScheduleItem::Run {
                            system_id,
                            system_name,
                        },
                        ScheduleItemDescriptor::Run {
                            system_id: descriptor_id,
                            system_name: descriptor_name,
                        },
                    ) => *system_id == descriptor_id.0 && system_name == descriptor_name,
                })
        {
            return Err(assembly_error(format!(
                "source schedule descriptor `{}` does not match verified Core",
                descriptor.name
            )));
        }
    }
    Ok(())
}

fn core_type_name(ty: CoreType) -> &'static str {
    match ty {
        CoreType::I32 => "i32",
        CoreType::F32 => "f32",
    }
}

fn core_access_matches_system(core: CoreQueryAccess, descriptor: &SystemAccess) -> bool {
    matches!(
        (core, descriptor),
        (CoreQueryAccess::Read, &SystemAccess::Read) | (CoreQueryAccess::Mut, &SystemAccess::Mut)
    )
}

fn core_access_matches_query(core: CoreQueryAccess, descriptor: &QueryAccess) -> bool {
    matches!(
        (core, descriptor),
        (CoreQueryAccess::Read, &QueryAccess::Read) | (CoreQueryAccess::Mut, &QueryAccess::Mut)
    )
}

fn verified_core_startup_spawns(
    core: &CoreProgram,
) -> Result<Vec<&[CoreSpawnComponent]>, RuntimeAssemblyError> {
    Ok(verified_core_startup_instructions(core)?
        .iter()
        .filter_map(|instruction| match instruction {
            CoreInstruction::Spawn { components } => Some(components.as_slice()),
            _ => None,
        })
        .collect())
}

pub(crate) fn verified_core_startup_instructions(
    core: &CoreProgram,
) -> Result<&[CoreInstruction], RuntimeAssemblyError> {
    let startup_functions = core
        .functions
        .iter()
        .filter(|function| function.name == "startup")
        .collect::<Vec<_>>();
    if startup_functions.len() != 1 {
        return Err(assembly_error(
            "verified Core must contain exactly one `startup` function",
        ));
    }
    let startup = startup_functions[0];
    let [entry] = startup.blocks.as_slice() else {
        return Err(assembly_error(
            "verified bootstrap Core startup must contain exactly one block",
        ));
    };
    if entry.id != startup.entry {
        return Err(assembly_error(
            "verified bootstrap Core startup block must be the entry block",
        ));
    }

    Ok(&entry.instructions)
}

pub fn register_assembly_descriptors_into_world(
    assembly: &RuntimeProgramAssembly,
    world: &mut ArcheWorld,
) -> Result<(), RuntimeAssemblyError> {
    for descriptor in &assembly.component_descriptors {
        if !world.register_component_descriptor(descriptor.clone()) {
            return Err(assembly_error(format!(
                "duplicate component descriptor `{}`",
                descriptor.name
            )));
        }
    }

    for descriptor in &assembly.resource_descriptors {
        if !world.register_resource_descriptor(descriptor.clone()) {
            return Err(assembly_error(format!(
                "duplicate resource descriptor `{}`",
                descriptor.name
            )));
        }
    }

    for descriptor in &assembly.system_descriptors {
        if !world.register_system_descriptor(descriptor.clone()) {
            return Err(assembly_error(format!(
                "duplicate system descriptor `{}`",
                descriptor.name
            )));
        }
    }

    for descriptor in &assembly.query_descriptors {
        if !world.register_query_descriptor(descriptor.clone()) {
            return Err(assembly_error(format!(
                "duplicate query descriptor `{}`",
                descriptor.name
            )));
        }
    }

    for descriptor in &assembly.schedule_descriptors {
        if !world.register_schedule_descriptor(descriptor.clone()) {
            return Err(assembly_error(format!(
                "duplicate schedule descriptor `{}`",
                descriptor.name
            )));
        }
    }

    Ok(())
}

pub fn execute_startup_resource_payload_operation(
    operation: &StartupOperation,
    world: &mut ArcheWorld,
) -> Result<(), RuntimeAssemblyError> {
    let (resource_id, resource_name, payload_bytes) = match operation {
        StartupOperation::ResourcePayload {
            resource_id,
            resource_name,
            payload_bytes,
        } => (*resource_id, resource_name, payload_bytes),
        _ => {
            return Err(assembly_error(
                "startup operation is not a resource payload",
            ));
        }
    };

    let descriptor = world
        .resource_descriptors()
        .get(resource_id)
        .cloned()
        .ok_or_else(|| {
            assembly_error(format!(
                "resource descriptor `{resource_name}` is not registered"
            ))
        })?;

    let byte_size = descriptor.size as usize;
    let byte_align = descriptor.align as usize;
    if byte_size == 0 {
        return Err(assembly_error(format!(
            "resource descriptor `{resource_name}` has zero size"
        )));
    }
    Layout::from_size_align(byte_size, byte_align).map_err(|_| {
        assembly_error(format!(
            "resource descriptor `{resource_name}` has invalid layout size {byte_size} align {byte_align}"
        ))
    })?;
    if payload_bytes.len() != byte_size {
        return Err(assembly_error(format!(
            "resource payload size {} does not match descriptor size {} for `{resource_name}`",
            payload_bytes.len(),
            descriptor.size
        )));
    }

    let allocated = world
        .allocate_resource_storage(&descriptor)
        .map_err(|error| assembly_error(error.message))?;
    if !allocated {
        return Err(assembly_error(format!(
            "resource storage `{resource_name}` is already allocated"
        )));
    }

    world
        .store_resource_payload(resource_id, payload_bytes)
        .map_err(|error| assembly_error(error.message))
}

pub fn execute_startup_spawn_operation(
    operation: &StartupOperation,
    world: &mut ArcheWorld,
) -> Result<ArcheEntity, RuntimeAssemblyError> {
    let components = match operation {
        StartupOperation::Spawn { components } => components,
        _ => {
            return Err(assembly_error("startup operation is not a spawn"));
        }
    };

    let payloads = components
        .iter()
        .map(|component| ComponentPayload {
            component_id: component.component_id,
            payload_bytes: &component.payload_bytes,
        })
        .collect::<Vec<_>>();

    world
        .spawn_entity_with_payloads(&payloads)
        .map_err(|error| assembly_error(error.message))
}

pub fn execute_startup_run_schedule_operation(
    operation: &StartupOperation,
    world: &mut ArcheWorld,
) -> Result<(), RuntimeAssemblyError> {
    let (schedule_id, schedule_name) = match operation {
        StartupOperation::RunSchedule {
            schedule_id,
            schedule_name,
        } => (*schedule_id, schedule_name),
        _ => {
            return Err(assembly_error("startup operation is not a run schedule"));
        }
    };

    let schedule = world
        .schedule_descriptors()
        .get(schedule_id)
        .cloned()
        .ok_or_else(|| {
            assembly_error(format!(
                "schedule descriptor `{schedule_name}` is not registered"
            ))
        })?;
    let plan = world
        .build_schedule_plan(&schedule)
        .map_err(|error| assembly_error(error.message))?;

    world
        .execute_schedule_plan(&plan)
        .map_err(|error| assembly_error(error.message))
}

pub fn execute_runtime_program_assembly(
    assembly: &RuntimeProgramAssembly,
) -> Result<ArcheWorld, RuntimeAssemblyError> {
    let mut world = ArcheWorld::create();

    register_assembly_descriptors_into_world(assembly, &mut world)?;
    for operation in &assembly.startup_operations {
        match operation {
            StartupOperation::ResourcePayload { .. } => {
                execute_startup_resource_payload_operation(operation, &mut world)?;
            }
            StartupOperation::Spawn { .. } => {
                execute_startup_spawn_operation(operation, &mut world)?;
            }
            StartupOperation::RunSchedule { .. } => {
                execute_startup_run_schedule_operation(operation, &mut world)?;
            }
        }
    }

    Ok(world)
}

fn assemble_component_descriptor(
    world_name: &str,
    component: &ComponentDecl,
) -> Result<ComponentDescriptor, RuntimeAssemblyError> {
    let component_layout = layout::compute_component_layout(component)
        .map_err(|error| assembly_error(error.message))?;

    Ok(ComponentDescriptor {
        id: layout::stable_component_id(world_name, &component.name),
        name: layout::component_qualified_name(world_name, &component.name),
        size: component_layout.size,
        align: component_layout.align,
        fields: component_layout
            .fields
            .into_iter()
            .map(|field| ComponentFieldDescriptor {
                name: field.name,
                type_name: field.type_name,
                offset: field.offset,
            })
            .collect(),
    })
}

fn assemble_resource_descriptor(
    world_name: &str,
    resource: &ResourceDecl,
) -> Result<ResourceDescriptor, RuntimeAssemblyError> {
    let resource_layout = compute_resource_layout(resource)?;

    Ok(ResourceDescriptor {
        id: stable_resource_id(world_name, &resource.name),
        name: qualified_name(world_name, &resource.name),
        size: resource_layout.size,
        align: resource_layout.align,
        fields: resource_layout.fields,
    })
}

fn assemble_system_descriptor(world_name: &str, system: &SystemDecl) -> SystemDescriptor {
    SystemDescriptor {
        id: stable_system_id(world_name, &system.name),
        name: qualified_name(world_name, &system.name),
        params: system
            .params
            .iter()
            .map(|param| assemble_system_param_descriptor(world_name, param))
            .collect(),
    }
}

fn assemble_system_param_descriptor(
    world_name: &str,
    param: &SystemParam,
) -> SystemParamDescriptor {
    SystemParamDescriptor {
        name: param.name.clone(),
        kind: match &param.kind {
            SystemParamKind::ReadResource { resource_name, .. } => {
                SystemParamDescriptorKind::ReadResource {
                    resource_id: stable_resource_id(world_name, resource_name),
                    name: qualified_name(world_name, resource_name),
                }
            }
            SystemParamKind::Query { terms } => SystemParamDescriptorKind::Query {
                terms: terms
                    .iter()
                    .map(|term| SystemQueryTermDescriptor {
                        access: assemble_system_access(term.access),
                        component_id: layout::stable_component_id(world_name, &term.component_name),
                        name: layout::component_qualified_name(world_name, &term.component_name),
                    })
                    .collect(),
            },
        },
    }
}

fn assemble_query_descriptors(world_name: &str, system: &SystemDecl) -> Vec<QueryDescriptor> {
    system
        .params
        .iter()
        .filter_map(|param| match &param.kind {
            SystemParamKind::Query { terms } => Some(QueryDescriptor {
                id: stable_query_id(world_name, &system.name, &param.name),
                name: format!(
                    "{}.{}",
                    qualified_name(world_name, &system.name),
                    param.name
                ),
                terms: terms
                    .iter()
                    .map(|term| QueryTermDescriptor {
                        access: assemble_query_access(term.access),
                        component_id: layout::stable_component_id(world_name, &term.component_name),
                        name: layout::component_qualified_name(world_name, &term.component_name),
                    })
                    .collect(),
            }),
            _ => None,
        })
        .collect()
}

fn assemble_schedule_descriptor(world_name: &str, schedule: &ScheduleDecl) -> ScheduleDescriptor {
    ScheduleDescriptor {
        id: stable_schedule_id(world_name, &schedule.name),
        name: qualified_name(world_name, &schedule.name),
        items: schedule
            .items
            .iter()
            .map(|item| match item {
                ScheduleItem::Run { system_name, .. } => ScheduleItemDescriptor::Run {
                    system_id: stable_system_id(world_name, system_name),
                    system_name: qualified_name(world_name, system_name),
                },
            })
            .collect(),
    }
}

fn assemble_system_access(access: ParserQueryAccess) -> SystemAccess {
    match access {
        ParserQueryAccess::Read => SystemAccess::Read,
        ParserQueryAccess::Mut => SystemAccess::Mut,
    }
}

fn assemble_query_access(access: ParserQueryAccess) -> QueryAccess {
    match access {
        ParserQueryAccess::Read => QueryAccess::Read,
        ParserQueryAccess::Mut => QueryAccess::Mut,
    }
}

fn assemble_startup_operations(
    world_name: &str,
    components: &[ComponentDescriptor],
    resources: &[ResourceDescriptor],
    schedules: &[ScheduleDescriptor],
    statements: &[Statement],
) -> Result<Vec<StartupOperation>, RuntimeAssemblyError> {
    let mut operations = Vec::new();

    for statement in statements {
        match statement {
            Statement::Resource(resource) => {
                operations.push(assemble_resource_payload_operation(
                    world_name, resources, resource,
                )?);
            }
            Statement::Spawn(spawn) => {
                operations.push(assemble_spawn_operation(world_name, components, spawn)?);
            }
            Statement::Run(run) => {
                operations.push(assemble_run_schedule_operation(world_name, schedules, run)?);
            }
            _ => {}
        }
    }

    Ok(operations)
}

fn assemble_startup_operations_from_verified_core(
    world_name: &str,
    components: &[ComponentDescriptor],
    resources: &[ResourceDescriptor],
    schedules: &[ScheduleDescriptor],
    statements: &[Statement],
    core_spawns: &[&[CoreSpawnComponent]],
) -> Result<Vec<StartupOperation>, RuntimeAssemblyError> {
    let mut operations = Vec::new();
    let mut core_spawn_index = 0usize;

    for statement in statements {
        match statement {
            Statement::Resource(resource) => {
                operations.push(assemble_resource_payload_operation(
                    world_name, resources, resource,
                )?);
            }
            Statement::Spawn(_) => {
                let core_components = core_spawns.get(core_spawn_index).ok_or_else(|| {
                    assembly_error(format!(
                        "source startup spawn {core_spawn_index} is absent from verified Core"
                    ))
                })?;
                operations.push(assemble_core_spawn_operation(components, core_components)?);
                core_spawn_index = core_spawn_index
                    .checked_add(1)
                    .ok_or_else(|| assembly_error("startup spawn index overflow"))?;
            }
            Statement::Run(run) => {
                operations.push(assemble_run_schedule_operation(world_name, schedules, run)?);
            }
            _ => {}
        }
    }

    if core_spawn_index != core_spawns.len() {
        return Err(assembly_error(format!(
            "verified Core contains {} startup spawns but source contains {core_spawn_index}",
            core_spawns.len()
        )));
    }

    Ok(operations)
}

fn assemble_resource_payload_operation(
    world_name: &str,
    resources: &[ResourceDescriptor],
    resource: &ResourceStatement,
) -> Result<StartupOperation, RuntimeAssemblyError> {
    let resource_name = qualified_name(world_name, &resource.name);
    let descriptor = resources
        .iter()
        .find(|descriptor| descriptor.name == resource_name)
        .ok_or_else(|| assembly_error(format!("unknown resource `{}`", resource.name)))?;
    let mut payload_bytes = vec![0; descriptor.size as usize];

    for field in &resource.fields {
        let descriptor_field = descriptor
            .fields
            .iter()
            .find(|descriptor_field| descriptor_field.name == field.name)
            .ok_or_else(|| {
                assembly_error(format!(
                    "unknown field `{}` for resource `{}`",
                    field.name, resource.name
                ))
            })?;

        let value = encode_source_literal(
            &descriptor_field.type_name,
            &field.value,
            &format!("resource field `{}.{}`", resource.name, field.name),
        )?;
        let offset = descriptor_field.offset as usize;
        let end = offset
            .checked_add(value.len())
            .ok_or_else(|| assembly_error("resource field payload range overflow"))?;
        if end > payload_bytes.len() {
            return Err(assembly_error(format!(
                "resource field `{}` exceeds payload size",
                field.name
            )));
        }

        payload_bytes[offset..end].copy_from_slice(&value);
    }

    Ok(StartupOperation::ResourcePayload {
        resource_id: descriptor.id,
        resource_name,
        payload_bytes,
    })
}

fn assemble_spawn_operation(
    world_name: &str,
    components: &[ComponentDescriptor],
    spawn: &SpawnStatement,
) -> Result<StartupOperation, RuntimeAssemblyError> {
    let mut operation_components = Vec::new();

    for component in &spawn.components {
        let component_name = layout::component_qualified_name(world_name, &component.name);
        let descriptor = components
            .iter()
            .find(|descriptor| descriptor.name == component_name)
            .ok_or_else(|| assembly_error(format!("unknown component `{}`", component.name)))?;
        let mut payload_bytes = vec![0; descriptor.size as usize];

        for field in &component.fields {
            let descriptor_field = descriptor
                .fields
                .iter()
                .find(|descriptor_field| descriptor_field.name == field.name)
                .ok_or_else(|| {
                    assembly_error(format!(
                        "unknown field `{}` for component `{}`",
                        field.name, component.name
                    ))
                })?;

            let value = encode_source_literal(
                &descriptor_field.type_name,
                &field.value,
                &format!("component field `{}.{}`", component.name, field.name),
            )?;
            let offset = descriptor_field.offset as usize;
            let end = offset
                .checked_add(value.len())
                .ok_or_else(|| assembly_error("component field payload range overflow"))?;
            if end > payload_bytes.len() {
                return Err(assembly_error(format!(
                    "component field `{}` exceeds payload size",
                    field.name
                )));
            }

            payload_bytes[offset..end].copy_from_slice(&value);
        }

        operation_components.push(StartupSpawnComponent {
            component_id: descriptor.id,
            component_name,
            payload_bytes,
        });
    }

    Ok(StartupOperation::Spawn {
        components: operation_components,
    })
}

fn assemble_core_spawn_operation(
    descriptors: &[ComponentDescriptor],
    components: &[CoreSpawnComponent],
) -> Result<StartupOperation, RuntimeAssemblyError> {
    let mut operation_components = Vec::with_capacity(components.len());

    for component in components {
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.id.0 == component.component_id)
            .ok_or_else(|| {
                assembly_error(format!(
                    "verified Core spawn component `{}` has no source descriptor",
                    component.name
                ))
            })?;
        if component.name != descriptor.name {
            return Err(assembly_error(format!(
                "verified Core spawn component `{}` does not match source descriptor `{}`",
                component.name, descriptor.name
            )));
        }

        let mut payload_bytes = vec![0; descriptor.size as usize];
        for field in &component.fields {
            let descriptor_field = descriptor
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .ok_or_else(|| {
                    assembly_error(format!(
                        "verified Core field `{}.{}` has no source descriptor",
                        component.name, field.name
                    ))
                })?;
            let (actual_type, value) = match field.value {
                CoreSpawnFieldValue::F32Bits(bits) => ("f32", bits.to_le_bytes()),
                CoreSpawnFieldValue::I32(value) => ("i32", value.to_le_bytes()),
            };
            if descriptor_field.type_name != actual_type {
                return Err(assembly_error(format!(
                    "verified Core field `{}.{}` has type {actual_type}, expected {}",
                    component.name, field.name, descriptor_field.type_name
                )));
            }
            let start = descriptor_field.offset as usize;
            let end = start
                .checked_add(value.len())
                .ok_or_else(|| assembly_error("Core component field payload range overflow"))?;
            let target = payload_bytes.get_mut(start..end).ok_or_else(|| {
                assembly_error(format!(
                    "verified Core field `{}.{}` exceeds its source descriptor payload",
                    component.name, field.name
                ))
            })?;
            target.copy_from_slice(&value);
        }

        operation_components.push(StartupSpawnComponent {
            component_id: descriptor.id,
            component_name: descriptor.name.clone(),
            payload_bytes,
        });
    }

    Ok(StartupOperation::Spawn {
        components: operation_components,
    })
}

fn encode_source_literal(
    type_name: &str,
    value: &ComponentLiteralValue,
    label: &str,
) -> Result<[u8; 4], RuntimeAssemblyError> {
    match (type_name, value) {
        ("f32", ComponentLiteralValue::Float { text, .. }) => text
            .parse::<f32>()
            .map(f32::to_le_bytes)
            .map_err(|_| assembly_error(format!("invalid f32 literal `{text}` for {label}"))),
        ("i32", ComponentLiteralValue::Integer { value, .. }) => i32::try_from(*value)
            .map(i32::to_le_bytes)
            .map_err(|_| assembly_error(format!("integer literal does not fit i32 for {label}"))),
        (expected, ComponentLiteralValue::Float { .. }) => Err(assembly_error(format!(
            "float literal cannot initialize {expected} {label}"
        ))),
        (expected, ComponentLiteralValue::Integer { .. }) => Err(assembly_error(format!(
            "integer literal cannot initialize {expected} {label}"
        ))),
    }
}

fn assemble_run_schedule_operation(
    world_name: &str,
    schedules: &[ScheduleDescriptor],
    run: &RunStatement,
) -> Result<StartupOperation, RuntimeAssemblyError> {
    let schedule_name = qualified_name(world_name, &run.schedule_name);
    let descriptor = schedules
        .iter()
        .find(|descriptor| descriptor.name == schedule_name)
        .ok_or_else(|| assembly_error(format!("unknown schedule `{}`", run.schedule_name)))?;

    Ok(StartupOperation::RunSchedule {
        schedule_id: descriptor.id,
        schedule_name,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResourceLayout {
    fields: Vec<ResourceFieldDescriptor>,
    size: u32,
    align: u32,
}

fn compute_resource_layout(
    resource: &ResourceDecl,
) -> Result<ResourceLayout, RuntimeAssemblyError> {
    let mut fields = Vec::new();
    let mut cursor = 0;
    let mut resource_align = 1;

    for field in &resource.fields {
        let field_layout =
            layout::primitive_type_layout(&field.type_name.name).ok_or_else(|| {
                assembly_error(format!(
                    "unknown primitive type `{}` for resource field `{}`",
                    field.type_name.name, field.name
                ))
            })?;

        cursor = align_to(cursor, field_layout.align);
        resource_align = resource_align.max(field_layout.align);
        fields.push(ResourceFieldDescriptor {
            name: field.name.clone(),
            type_name: field.type_name.name.clone(),
            offset: cursor,
        });
        cursor += field_layout.size;
    }

    Ok(ResourceLayout {
        fields,
        size: align_to(cursor, resource_align),
        align: resource_align,
    })
}

fn qualified_name(world_name: &str, item_name: &str) -> String {
    format!("{world_name}.{item_name}")
}

fn align_to(value: u32, align: u32) -> u32 {
    debug_assert!(align > 0);
    let remainder = value % align;
    if remainder == 0 {
        value
    } else {
        value + (align - remainder)
    }
}

fn assembly_error(message: impl Into<String>) -> RuntimeAssemblyError {
    RuntimeAssemblyError {
        message: message.into(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupOperation {
    ResourcePayload {
        resource_id: ResourceId,
        resource_name: String,
        payload_bytes: Vec<u8>,
    },
    Spawn {
        components: Vec<StartupSpawnComponent>,
    },
    RunSchedule {
        schedule_id: ScheduleId,
        schedule_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupSpawnComponent {
    pub component_id: ComponentId,
    pub component_name: String,
    pub payload_bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BlockId, CoreBlock, CoreInstruction, CoreTerminator, ValueId};
    use crate::layout::ComponentId;
    use crate::runtime::{
        stable_query_id, stable_resource_id, stable_schedule_id, stable_system_id, ArchetypeKey,
        ComponentFieldDescriptor, QueryAccess, QueryTermDescriptor, ResourceFieldDescriptor,
        ScheduleItemDescriptor, SystemAccess, SystemParamDescriptor, SystemParamDescriptorKind,
        SystemQueryTermDescriptor,
    };
    use crate::{checker, core_lower, core_verify, lexer, parser};

    #[test]
    fn defines_runtime_program_assembly_model() {
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = stable_resource_id("Demo", "Time");
        let move_id = stable_system_id("Demo", "Move");
        let movers_id = stable_query_id("Demo", "Move", "movers");
        let main_id = stable_schedule_id("Demo", "Main");

        let empty = RuntimeProgramAssembly::new("Demo");
        assert_eq!(empty.world_name, "Demo");
        assert!(empty.is_empty());

        let mut assembly = RuntimeProgramAssembly::new("Demo");
        assembly.component_descriptors = vec![
            xy_component_descriptor(position_id, "Demo.Position"),
            xy_component_descriptor(velocity_id, "Demo.Velocity"),
        ];
        assembly.resource_descriptors = vec![ResourceDescriptor {
            id: time_id,
            name: "Demo.Time".to_string(),
            size: 4,
            align: 4,
            fields: vec![ResourceFieldDescriptor {
                name: "delta".to_string(),
                type_name: "f32".to_string(),
                offset: 0,
            }],
        }];
        assembly.system_descriptors = vec![SystemDescriptor {
            id: move_id,
            name: "Demo.Move".to_string(),
            params: vec![
                SystemParamDescriptor {
                    name: "time".to_string(),
                    kind: SystemParamDescriptorKind::ReadResource {
                        resource_id: time_id,
                        name: "Demo.Time".to_string(),
                    },
                },
                SystemParamDescriptor {
                    name: "movers".to_string(),
                    kind: SystemParamDescriptorKind::Query {
                        terms: vec![
                            SystemQueryTermDescriptor {
                                access: SystemAccess::Mut,
                                component_id: position_id,
                                name: "Demo.Position".to_string(),
                            },
                            SystemQueryTermDescriptor {
                                access: SystemAccess::Read,
                                component_id: velocity_id,
                                name: "Demo.Velocity".to_string(),
                            },
                        ],
                    },
                },
            ],
        }];
        assembly.query_descriptors = vec![QueryDescriptor {
            id: movers_id,
            name: "Demo.Move.movers".to_string(),
            terms: vec![
                QueryTermDescriptor {
                    access: QueryAccess::Mut,
                    component_id: position_id,
                    name: "Demo.Position".to_string(),
                },
                QueryTermDescriptor {
                    access: QueryAccess::Read,
                    component_id: velocity_id,
                    name: "Demo.Velocity".to_string(),
                },
            ],
        }];
        assembly.schedule_descriptors = vec![ScheduleDescriptor {
            id: main_id,
            name: "Demo.Main".to_string(),
            items: vec![ScheduleItemDescriptor::Run {
                system_id: move_id,
                system_name: "Demo.Move".to_string(),
            }],
        }];
        assembly.startup_operations = vec![
            StartupOperation::ResourcePayload {
                resource_id: time_id,
                resource_name: "Demo.Time".to_string(),
                payload_bytes: f32_payload(1.0),
            },
            StartupOperation::Spawn {
                components: vec![
                    StartupSpawnComponent {
                        component_id: position_id,
                        component_name: "Demo.Position".to_string(),
                        payload_bytes: f32_pair_payload(1.0, 2.0),
                    },
                    StartupSpawnComponent {
                        component_id: velocity_id,
                        component_name: "Demo.Velocity".to_string(),
                        payload_bytes: f32_pair_payload(3.0, 4.0),
                    },
                ],
            },
            StartupOperation::RunSchedule {
                schedule_id: main_id,
                schedule_name: "Demo.Main".to_string(),
            },
        ];

        assert!(!assembly.is_empty());
        assert_eq!(assembly.world_name, "Demo");
        assert_eq!(assembly.component_descriptors.len(), 2);
        assert_eq!(assembly.resource_descriptors.len(), 1);
        assert_eq!(assembly.system_descriptors.len(), 1);
        assert_eq!(assembly.query_descriptors.len(), 1);
        assert_eq!(assembly.schedule_descriptors.len(), 1);
        assert_eq!(assembly.startup_operations.len(), 3);

        assert_eq!(
            assembly,
            RuntimeProgramAssembly {
                world_name: "Demo".to_string(),
                component_descriptors: vec![
                    xy_component_descriptor(position_id, "Demo.Position"),
                    xy_component_descriptor(velocity_id, "Demo.Velocity"),
                ],
                resource_descriptors: vec![ResourceDescriptor {
                    id: ResourceId(0x7924ce11db524521),
                    name: "Demo.Time".to_string(),
                    size: 4,
                    align: 4,
                    fields: vec![ResourceFieldDescriptor {
                        name: "delta".to_string(),
                        type_name: "f32".to_string(),
                        offset: 0,
                    }],
                }],
                system_descriptors: vec![SystemDescriptor {
                    id: move_id,
                    name: "Demo.Move".to_string(),
                    params: vec![
                        SystemParamDescriptor {
                            name: "time".to_string(),
                            kind: SystemParamDescriptorKind::ReadResource {
                                resource_id: time_id,
                                name: "Demo.Time".to_string(),
                            },
                        },
                        SystemParamDescriptor {
                            name: "movers".to_string(),
                            kind: SystemParamDescriptorKind::Query {
                                terms: vec![
                                    SystemQueryTermDescriptor {
                                        access: SystemAccess::Mut,
                                        component_id: position_id,
                                        name: "Demo.Position".to_string(),
                                    },
                                    SystemQueryTermDescriptor {
                                        access: SystemAccess::Read,
                                        component_id: velocity_id,
                                        name: "Demo.Velocity".to_string(),
                                    },
                                ],
                            },
                        },
                    ],
                }],
                query_descriptors: vec![QueryDescriptor {
                    id: movers_id,
                    name: "Demo.Move.movers".to_string(),
                    terms: vec![
                        QueryTermDescriptor {
                            access: QueryAccess::Mut,
                            component_id: position_id,
                            name: "Demo.Position".to_string(),
                        },
                        QueryTermDescriptor {
                            access: QueryAccess::Read,
                            component_id: velocity_id,
                            name: "Demo.Velocity".to_string(),
                        },
                    ],
                }],
                schedule_descriptors: vec![ScheduleDescriptor {
                    id: ScheduleId(0xed3d905325519b05),
                    name: "Demo.Main".to_string(),
                    items: vec![ScheduleItemDescriptor::Run {
                        system_id: move_id,
                        system_name: "Demo.Move".to_string(),
                    }],
                }],
                startup_operations: vec![
                    StartupOperation::ResourcePayload {
                        resource_id: time_id,
                        resource_name: "Demo.Time".to_string(),
                        payload_bytes: vec![0x00, 0x00, 0x80, 0x3f],
                    },
                    StartupOperation::Spawn {
                        components: vec![
                            StartupSpawnComponent {
                                component_id: position_id,
                                component_name: "Demo.Position".to_string(),
                                payload_bytes: vec![0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40,],
                            },
                            StartupSpawnComponent {
                                component_id: velocity_id,
                                component_name: "Demo.Velocity".to_string(),
                                payload_bytes: vec![0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40,],
                            },
                        ],
                    },
                    StartupOperation::RunSchedule {
                        schedule_id: main_id,
                        schedule_name: "Demo.Main".to_string(),
                    },
                ],
            }
        );
    }

    #[test]
    fn assembles_component_and_resource_descriptors_from_source() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");

        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");

        assert_eq!(assembly.world_name, "Demo");
        assert_eq!(
            assembly.component_descriptors,
            vec![
                xy_component_descriptor(ComponentId(0x002202c6aeb4f27b), "Demo.Position"),
                xy_component_descriptor(ComponentId(0x2cf8a68bcb7f913b), "Demo.Velocity"),
            ]
        );
        assert_eq!(
            assembly.resource_descriptors,
            vec![ResourceDescriptor {
                id: ResourceId(0x7924ce11db524521),
                name: "Demo.Time".to_string(),
                size: 4,
                align: 4,
                fields: vec![ResourceFieldDescriptor {
                    name: "delta".to_string(),
                    type_name: "f32".to_string(),
                    offset: 0,
                }],
            }]
        );
        assert!(!assembly.is_empty());
        assert_eq!(assembly.system_descriptors.len(), 1);
        assert_eq!(assembly.query_descriptors.len(), 1);
        assert_eq!(assembly.schedule_descriptors.len(), 1);
    }

    #[test]
    fn assembles_system_query_and_schedule_descriptors_from_source() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");

        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = ResourceId(0x7924ce11db524521);
        let move_id = stable_system_id("Demo", "Move");
        let movers_id = stable_query_id("Demo", "Move", "movers");
        let main_id = stable_schedule_id("Demo", "Main");

        assert_eq!(assembly.world_name, "Demo");
        assert_eq!(assembly.component_descriptors.len(), 2);
        assert_eq!(assembly.resource_descriptors.len(), 1);
        assert_eq!(
            assembly.system_descriptors,
            vec![SystemDescriptor {
                id: move_id,
                name: "Demo.Move".to_string(),
                params: vec![
                    SystemParamDescriptor {
                        name: "time".to_string(),
                        kind: SystemParamDescriptorKind::ReadResource {
                            resource_id: time_id,
                            name: "Demo.Time".to_string(),
                        },
                    },
                    SystemParamDescriptor {
                        name: "movers".to_string(),
                        kind: SystemParamDescriptorKind::Query {
                            terms: vec![
                                SystemQueryTermDescriptor {
                                    access: SystemAccess::Mut,
                                    component_id: position_id,
                                    name: "Demo.Position".to_string(),
                                },
                                SystemQueryTermDescriptor {
                                    access: SystemAccess::Read,
                                    component_id: velocity_id,
                                    name: "Demo.Velocity".to_string(),
                                },
                            ],
                        },
                    },
                ],
            }]
        );
        assert_eq!(
            assembly.query_descriptors,
            vec![QueryDescriptor {
                id: movers_id,
                name: "Demo.Move.movers".to_string(),
                terms: vec![
                    QueryTermDescriptor {
                        access: QueryAccess::Mut,
                        component_id: position_id,
                        name: "Demo.Position".to_string(),
                    },
                    QueryTermDescriptor {
                        access: QueryAccess::Read,
                        component_id: velocity_id,
                        name: "Demo.Velocity".to_string(),
                    },
                ],
            }]
        );
        assert_eq!(
            assembly.schedule_descriptors,
            vec![ScheduleDescriptor {
                id: main_id,
                name: "Demo.Main".to_string(),
                items: vec![ScheduleItemDescriptor::Run {
                    system_id: move_id,
                    system_name: "Demo.Move".to_string(),
                }],
            }]
        );
        assert!(!assembly.is_empty());
    }

    #[test]
    fn assembles_startup_resource_payload_operation() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");

        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");

        assert_eq!(assembly.component_descriptors.len(), 2);
        assert_eq!(assembly.resource_descriptors.len(), 1);
        assert_eq!(assembly.system_descriptors.len(), 1);
        assert_eq!(assembly.query_descriptors.len(), 1);
        assert_eq!(assembly.schedule_descriptors.len(), 1);
        assert_eq!(
            assembly.startup_operations.first(),
            Some(&StartupOperation::ResourcePayload {
                resource_id: ResourceId(0x7924ce11db524521),
                resource_name: "Demo.Time".to_string(),
                payload_bytes: vec![0x00, 0x00, 0x80, 0x3f],
            })
        );
        assert_eq!(assembly.startup_operations.len(), 3);
    }

    #[test]
    fn assembles_startup_spawn_operation() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");

        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");

        assert_eq!(assembly.component_descriptors.len(), 2);
        assert_eq!(assembly.resource_descriptors.len(), 1);
        assert_eq!(assembly.system_descriptors.len(), 1);
        assert_eq!(assembly.query_descriptors.len(), 1);
        assert_eq!(assembly.schedule_descriptors.len(), 1);
        assert_eq!(assembly.startup_operations.len(), 3);
        assert_eq!(
            &assembly.startup_operations[0],
            &StartupOperation::ResourcePayload {
                resource_id: ResourceId(0x7924ce11db524521),
                resource_name: "Demo.Time".to_string(),
                payload_bytes: vec![0x00, 0x00, 0x80, 0x3f],
            }
        );
        assert_eq!(
            &assembly.startup_operations[1],
            &StartupOperation::Spawn {
                components: vec![
                    StartupSpawnComponent {
                        component_id: ComponentId(0x002202c6aeb4f27b),
                        component_name: "Demo.Position".to_string(),
                        payload_bytes: vec![0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40,],
                    },
                    StartupSpawnComponent {
                        component_id: ComponentId(0x2cf8a68bcb7f913b),
                        component_name: "Demo.Velocity".to_string(),
                        payload_bytes: vec![0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40,],
                    },
                ],
            }
        );
    }

    #[test]
    fn assembles_typed_startup_payloads_from_verified_core() {
        let source = "world Bounds
component Values { zero: i32 max: i32 scalar: f32 }
resource Limits { zero: i32 max: i32 scalar: f32 }
startup {
  resource Limits { zero: 0, max: 2147483647, scalar: 1.5 }
  spawn { Values { zero: 0, max: 2147483647, scalar: 2.5 } }
  exit 0
}";
        let tokens = lexer::lex(source).expect("typed startup fixture lexes");
        let program = parser::parse_program(&tokens).expect("typed startup fixture parses");
        checker::check_program(&program).expect("typed startup fixture checks");
        let mut core =
            core_lower::lower_program_to_core(&program).expect("typed startup fixture lowers");
        core_verify::verify_core_program(&core).expect("typed startup Core verifies");

        let assembly = assemble_runtime_program_from_verified_core(&program, &core)
            .expect("typed startup assembly builds from verified Core");
        assert_eq!(
            assembly.startup_operations[0],
            StartupOperation::ResourcePayload {
                resource_id: stable_resource_id("Bounds", "Limits"),
                resource_name: "Bounds.Limits".to_string(),
                payload_bytes: [
                    0i32.to_le_bytes().as_slice(),
                    i32::MAX.to_le_bytes().as_slice(),
                    1.5f32.to_le_bytes().as_slice(),
                ]
                .concat(),
            }
        );
        let StartupOperation::Spawn { components } = &assembly.startup_operations[1] else {
            panic!("startup operation one must be the typed spawn");
        };
        assert_eq!(
            components[0].payload_bytes,
            [
                0i32.to_le_bytes().as_slice(),
                i32::MAX.to_le_bytes().as_slice(),
                2.5f32.to_le_bytes().as_slice(),
            ]
            .concat()
        );

        let core_max = core.functions[0].blocks[0]
            .instructions
            .iter_mut()
            .find_map(|instruction| match instruction {
                crate::core::CoreInstruction::Spawn { components } => components[0]
                    .fields
                    .iter_mut()
                    .find(|field| field.name == "max"),
                _ => None,
            })
            .expect("typed spawn contains max field");
        core_max.value = CoreSpawnFieldValue::I32(7);
        let core_authoritative = assemble_runtime_program_from_verified_core(&program, &core)
            .expect("changed valid Core remains assembly authority");
        let StartupOperation::Spawn { components } = &core_authoritative.startup_operations[1]
        else {
            panic!("startup operation one must remain the typed spawn");
        };
        assert_eq!(&components[0].payload_bytes[4..8], &7i32.to_le_bytes());
    }

    #[test]
    fn rejects_non_entry_startup_blocks_as_materialization_authority() {
        let source = include_str!("../../../examples/spawn_position.arc");
        let tokens = lexer::lex(source).expect("spawn fixture lexes");
        let program = parser::parse_program(&tokens).expect("spawn fixture parses");
        checker::check_program(&program).expect("spawn fixture checks");
        let mut core = core_lower::lower_program_to_core(&program).expect("spawn fixture lowers");
        core.functions[0].blocks.push(CoreBlock {
            id: BlockId(99),
            instructions: vec![CoreInstruction::I32Const {
                result: ValueId(99),
                value: 0,
            }],
            terminator: CoreTerminator::Exit { value: ValueId(99) },
        });
        core_verify::verify_core_program(&core)
            .expect("the general Core verifier permits an additional isolated block");

        let error = assemble_runtime_program_from_verified_core(&program, &core)
            .expect_err("bootstrap materialization must reject unreachable startup blocks");
        assert!(error.message.contains("exactly one block"));
    }

    #[test]
    fn rejects_source_descriptors_absent_from_verified_core() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("movement fixture lexes");
        let program = parser::parse_program(&tokens).expect("movement fixture parses");
        checker::check_program(&program).expect("movement fixture checks");
        let mut core =
            core_lower::lower_program_to_core(&program).expect("movement fixture lowers");
        core.resources.clear();
        core.systems.clear();
        core.schedules.clear();
        core_verify::verify_core_program(&core)
            .expect("the reduced Core remains internally self-consistent");

        let error = assemble_runtime_program_from_verified_core(&program, &core)
            .expect_err("source descriptors absent from Core must not be published");
        assert!(error
            .message
            .contains("resource schema counts do not match"));
    }

    #[test]
    fn rejects_invalid_typed_source_literals_during_direct_assembly() {
        for (source, expected) in [
            (
                "world Bounds component Values { value: i32 } startup { spawn { Values { value: 2147483648 } } exit 0 }",
                "does not fit i32",
            ),
            (
                "world Bounds resource Limits { value: f32 } startup { resource Limits { value: 1 } exit 0 }",
                "integer literal cannot initialize f32",
            ),
        ] {
            let tokens = lexer::lex(source).expect("invalid typed assembly fixture lexes");
            let program =
                parser::parse_program(&tokens).expect("invalid typed assembly fixture parses");
            let error = assemble_runtime_program_from_source(&program)
                .expect_err("direct assembly must validate typed source literals");
            assert!(
                error.message.contains(expected),
                "expected `{expected}`, got `{}`",
                error.message
            );
        }
    }

    #[test]
    fn assembles_startup_run_operation() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");

        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");

        assert_eq!(assembly.component_descriptors.len(), 2);
        assert_eq!(assembly.resource_descriptors.len(), 1);
        assert_eq!(assembly.system_descriptors.len(), 1);
        assert_eq!(assembly.query_descriptors.len(), 1);
        assert_eq!(assembly.schedule_descriptors.len(), 1);
        assert_eq!(
            assembly.startup_operations,
            vec![
                StartupOperation::ResourcePayload {
                    resource_id: ResourceId(0x7924ce11db524521),
                    resource_name: "Demo.Time".to_string(),
                    payload_bytes: vec![0x00, 0x00, 0x80, 0x3f],
                },
                StartupOperation::Spawn {
                    components: vec![
                        StartupSpawnComponent {
                            component_id: ComponentId(0x002202c6aeb4f27b),
                            component_name: "Demo.Position".to_string(),
                            payload_bytes: vec![0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40,],
                        },
                        StartupSpawnComponent {
                            component_id: ComponentId(0x2cf8a68bcb7f913b),
                            component_name: "Demo.Velocity".to_string(),
                            payload_bytes: vec![0x00, 0x00, 0x40, 0x40, 0x00, 0x00, 0x80, 0x40,],
                        },
                    ],
                },
                StartupOperation::RunSchedule {
                    schedule_id: ScheduleId(0xed3d905325519b05),
                    schedule_name: "Demo.Main".to_string(),
                },
            ]
        );
    }

    #[test]
    fn registers_assembly_descriptors_into_world() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut world = ArcheWorld::create();

        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = ResourceId(0x7924ce11db524521);
        let move_id = stable_system_id("Demo", "Move");
        let movers_id = stable_query_id("Demo", "Move", "movers");
        let main_id = stable_schedule_id("Demo", "Main");

        assert_eq!(world.component_descriptors().len(), 2);
        assert_eq!(world.resource_descriptors().len(), 1);
        assert_eq!(world.system_descriptors().len(), 1);
        assert_eq!(world.query_descriptors().len(), 1);
        assert_eq!(world.schedule_descriptors().len(), 1);
        assert_eq!(
            world.component_descriptors().get(position_id),
            Some(&xy_component_descriptor(position_id, "Demo.Position"))
        );
        assert_eq!(
            world.component_descriptors().get(velocity_id),
            Some(&xy_component_descriptor(velocity_id, "Demo.Velocity"))
        );
        assert_eq!(
            world.resource_descriptors().get(time_id),
            Some(&ResourceDescriptor {
                id: time_id,
                name: "Demo.Time".to_string(),
                size: 4,
                align: 4,
                fields: vec![ResourceFieldDescriptor {
                    name: "delta".to_string(),
                    type_name: "f32".to_string(),
                    offset: 0,
                }],
            })
        );
        assert_eq!(
            world.system_descriptors().get(move_id),
            Some(&SystemDescriptor {
                id: move_id,
                name: "Demo.Move".to_string(),
                params: vec![
                    SystemParamDescriptor {
                        name: "time".to_string(),
                        kind: SystemParamDescriptorKind::ReadResource {
                            resource_id: time_id,
                            name: "Demo.Time".to_string(),
                        },
                    },
                    SystemParamDescriptor {
                        name: "movers".to_string(),
                        kind: SystemParamDescriptorKind::Query {
                            terms: vec![
                                SystemQueryTermDescriptor {
                                    access: SystemAccess::Mut,
                                    component_id: position_id,
                                    name: "Demo.Position".to_string(),
                                },
                                SystemQueryTermDescriptor {
                                    access: SystemAccess::Read,
                                    component_id: velocity_id,
                                    name: "Demo.Velocity".to_string(),
                                },
                            ],
                        },
                    },
                ],
            })
        );
        assert_eq!(
            world.query_descriptors().get(movers_id),
            Some(&QueryDescriptor {
                id: movers_id,
                name: "Demo.Move.movers".to_string(),
                terms: vec![
                    QueryTermDescriptor {
                        access: QueryAccess::Mut,
                        component_id: position_id,
                        name: "Demo.Position".to_string(),
                    },
                    QueryTermDescriptor {
                        access: QueryAccess::Read,
                        component_id: velocity_id,
                        name: "Demo.Velocity".to_string(),
                    },
                ],
            })
        );
        assert_eq!(
            world.schedule_descriptors().get(main_id),
            Some(&ScheduleDescriptor {
                id: main_id,
                name: "Demo.Main".to_string(),
                items: vec![ScheduleItemDescriptor::Run {
                    system_id: move_id,
                    system_name: "Demo.Move".to_string(),
                }],
            })
        );
        assert_eq!(world.entities().len(), 0);
        assert_eq!(world.resource_storage_count(), 0);
        assert_eq!(world.archetype_count(), 0);
        assert_eq!(assembly.startup_operations.len(), 3);
        assert!(matches!(
            assembly.startup_operations[0],
            StartupOperation::ResourcePayload { .. }
        ));
        assert!(matches!(
            assembly.startup_operations[1],
            StartupOperation::Spawn { .. }
        ));
        assert!(matches!(
            assembly.startup_operations[2],
            StartupOperation::RunSchedule { .. }
        ));
    }

    #[test]
    fn executes_startup_resource_payload_operation() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut world = ArcheWorld::create();

        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");
        execute_startup_resource_payload_operation(&assembly.startup_operations[0], &mut world)
            .expect("startup resource payload executes");

        let time_id = ResourceId(0x7924ce11db524521);
        assert_eq!(world.resource_storage_count(), 1);
        assert_eq!(
            world
                .resource_payload(time_id)
                .expect("time payload exists"),
            &[0x00, 0x00, 0x80, 0x3f]
        );
        assert_eq!(
            world
                .read_resource_f32_field(time_id, "delta")
                .expect("delta decodes"),
            1.0
        );
        assert_eq!(world.entities().len(), 0);
        assert_eq!(world.archetype_count(), 0);
        assert_eq!(assembly.startup_operations.len(), 3);
        assert!(matches!(
            assembly.startup_operations[0],
            StartupOperation::ResourcePayload { .. }
        ));
        assert!(matches!(
            assembly.startup_operations[1],
            StartupOperation::Spawn { .. }
        ));
        assert!(matches!(
            assembly.startup_operations[2],
            StartupOperation::RunSchedule { .. }
        ));
    }

    #[test]
    fn invalid_startup_resource_payload_does_not_allocate_storage() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut invalid_operation = assembly.startup_operations[0].clone();
        let StartupOperation::ResourcePayload { payload_bytes, .. } = &mut invalid_operation else {
            panic!("fixture startup operation zero must be a resource payload");
        };
        payload_bytes.truncate(2);
        let mut world = ArcheWorld::create();
        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");

        let error = execute_startup_resource_payload_operation(&invalid_operation, &mut world)
            .expect_err("invalid resource payload must fail before allocation");

        assert!(error.message.contains("payload size"));
        assert_eq!(world.resource_storage_count(), 0);
        assert!(world
            .resource_payload(ResourceId(0x7924ce11db524521))
            .is_err());
        assert_eq!(world.entities().len(), 0);
        assert_eq!(world.archetype_count(), 0);
    }

    #[test]
    fn executes_startup_spawn_operation() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut world = ArcheWorld::create();

        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");
        execute_startup_resource_payload_operation(&assembly.startup_operations[0], &mut world)
            .expect("startup resource payload executes");
        let entity = execute_startup_spawn_operation(&assembly.startup_operations[1], &mut world)
            .expect("startup spawn executes");

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = ResourceId(0x7924ce11db524521);
        let position_payload = f32_pair_payload(1.0, 2.0);
        let velocity_payload = f32_pair_payload(3.0, 4.0);
        let key = ArchetypeKey::new(vec![position_id, velocity_id]);

        assert_eq!(entity.index(), 0);
        assert_eq!(entity.generation(), 0);
        assert!(world.entities().is_alive(entity));
        assert_eq!(world.resource_storage_count(), 1);
        assert_eq!(
            world
                .read_resource_f32_field(time_id, "delta")
                .expect("delta decodes"),
            1.0
        );
        assert_eq!(world.archetype_count(), 1);

        let table = world.archetype(&key).expect("spawn archetype exists");
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(entity));
        assert_eq!(table.column_count(), 2);
        assert_eq!(
            table
                .column(position_id)
                .expect("position column exists")
                .row_bytes(0),
            Some(position_payload.as_slice())
        );
        assert_eq!(
            table
                .column(velocity_id)
                .expect("velocity column exists")
                .row_bytes(0),
            Some(velocity_payload.as_slice())
        );
        assert!(matches!(
            assembly.startup_operations[2],
            StartupOperation::RunSchedule { .. }
        ));
    }

    #[test]
    fn executes_two_startup_spawns_with_grown_aligned_columns() {
        let source = include_str!("../../../examples/move_system_two_rows.arc");
        let tokens = lexer::lex(source).expect("move_system_two_rows.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system_two_rows.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut world = ArcheWorld::create();

        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");
        execute_startup_resource_payload_operation(&assembly.startup_operations[0], &mut world)
            .expect("startup resource payload executes");
        let first = execute_startup_spawn_operation(&assembly.startup_operations[1], &mut world)
            .expect("first startup spawn executes");
        let second = execute_startup_spawn_operation(&assembly.startup_operations[2], &mut world)
            .expect("second startup spawn executes");
        execute_startup_run_schedule_operation(&assembly.startup_operations[3], &mut world)
            .expect("schedule executes for both rows");

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let key = ArchetypeKey::new(vec![position_id, velocity_id]);
        let expected_positions = [f32_pair_payload(4.0, 6.0), f32_pair_payload(11.0, 22.0)];
        let expected_velocities = [f32_pair_payload(3.0, 4.0), f32_pair_payload(1.0, 2.0)];
        let table = world.archetype(&key).expect("spawn archetype exists");
        let position = table.column(position_id).expect("position column exists");
        let velocity = table.column(velocity_id).expect("velocity column exists");

        assert_eq!(first.index(), 0);
        assert_eq!(second.index(), 1);
        assert!(world.entities().is_alive(first));
        assert!(world.entities().is_alive(second));
        assert_eq!(table.entity_count(), 2);
        assert_eq!(position.row_count(), 2);
        assert_eq!(velocity.row_count(), 2);
        assert_eq!(position.row_capacity(), 2);
        assert_eq!(velocity.row_capacity(), 2);
        for row in 0..2 {
            assert_eq!(
                position.row_bytes(row),
                Some(expected_positions[row].as_slice())
            );
            assert_eq!(
                velocity.row_bytes(row),
                Some(expected_velocities[row].as_slice())
            );
        }
    }

    #[test]
    fn failed_repeated_startup_spawn_does_not_publish_partial_state() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut invalid_spawn = assembly.startup_operations[1].clone();
        let StartupOperation::Spawn { components } = &mut invalid_spawn else {
            panic!("fixture startup operation one must be a spawn");
        };
        components[1].payload_bytes.truncate(4);
        let mut world = ArcheWorld::create();

        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");
        let first = execute_startup_spawn_operation(&assembly.startup_operations[1], &mut world)
            .expect("first startup spawn executes");
        let error = execute_startup_spawn_operation(&invalid_spawn, &mut world)
            .expect_err("invalid repeated spawn must fail");

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let key = ArchetypeKey::new(vec![position_id, velocity_id]);
        let initial_position = f32_pair_payload(1.0, 2.0);
        let initial_velocity = f32_pair_payload(3.0, 4.0);
        let table = world.archetype(&key).expect("original archetype remains");
        let position = table.column(position_id).expect("position column exists");
        let velocity = table.column(velocity_id).expect("velocity column exists");

        assert!(error.message.contains("payload size"));
        assert_eq!(world.entities().len(), 1);
        assert!(world.entities().is_alive(first));
        assert_eq!(world.archetype_count(), 1);
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(first));
        assert_eq!(position.row_count(), 1);
        assert_eq!(velocity.row_count(), 1);
        assert_eq!(position.row_capacity(), 1);
        assert_eq!(velocity.row_capacity(), 1);
        assert_eq!(position.row_bytes(0), Some(initial_position.as_slice()));
        assert_eq!(velocity.row_bytes(0), Some(initial_velocity.as_slice()));
    }

    #[test]
    fn executes_startup_run_schedule_operation() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");
        let mut world = ArcheWorld::create();

        register_assembly_descriptors_into_world(&assembly, &mut world)
            .expect("assembly descriptors register into world");
        execute_startup_resource_payload_operation(&assembly.startup_operations[0], &mut world)
            .expect("startup resource payload executes");
        let entity = execute_startup_spawn_operation(&assembly.startup_operations[1], &mut world)
            .expect("startup spawn executes");
        execute_startup_run_schedule_operation(&assembly.startup_operations[2], &mut world)
            .expect("startup run schedule executes");

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = ResourceId(0x7924ce11db524521);
        let expected_position = f32_pair_payload(4.0, 6.0);
        let initial_velocity = f32_pair_payload(3.0, 4.0);
        let key = ArchetypeKey::new(vec![position_id, velocity_id]);

        assert_eq!(world.resource_storage_count(), 1);
        assert_eq!(
            world
                .read_resource_f32_field(time_id, "delta")
                .expect("delta decodes"),
            1.0
        );
        assert_eq!(world.archetype_count(), 1);

        let table = world.archetype(&key).expect("spawn archetype exists");
        assert_eq!(table.entity_count(), 1);
        assert_eq!(table.entity(0), Some(entity));
        assert_eq!(
            table
                .column(position_id)
                .expect("position column exists")
                .row_bytes(0),
            Some(expected_position.as_slice())
        );
        assert_eq!(
            table
                .column(velocity_id)
                .expect("velocity column exists")
                .row_bytes(0),
            Some(initial_velocity.as_slice())
        );
        assert!(world.entities().is_alive(entity));
    }

    #[test]
    fn executes_move_system_source_runtime_vertical_slice() {
        let source = include_str!("../../../examples/move_system.arc");
        let tokens = lexer::lex(source).expect("move_system.arc lexes");
        let program = parser::parse_program(&tokens).expect("move_system.arc parses");
        let assembly =
            assemble_runtime_program_from_source(&program).expect("runtime descriptors assemble");

        let world = execute_runtime_program_assembly(&assembly).expect("runtime assembly executes");

        let position_id = ComponentId(0x002202c6aeb4f27b);
        let velocity_id = ComponentId(0x2cf8a68bcb7f913b);
        let time_id = ResourceId(0x7924ce11db524521);
        let expected_position = f32_pair_payload(4.0, 6.0);
        let initial_velocity = f32_pair_payload(3.0, 4.0);
        let key = ArchetypeKey::new(vec![position_id, velocity_id]);

        assert_eq!(world.component_descriptors().len(), 2);
        assert_eq!(world.resource_descriptors().len(), 1);
        assert_eq!(world.system_descriptors().len(), 1);
        assert_eq!(world.query_descriptors().len(), 1);
        assert_eq!(world.schedule_descriptors().len(), 1);
        assert_eq!(world.resource_storage_count(), 1);
        assert_eq!(
            world
                .read_resource_f32_field(time_id, "delta")
                .expect("delta decodes"),
            1.0
        );
        assert_eq!(world.entities().len(), 1);
        assert_eq!(world.archetype_count(), 1);

        let table = world.archetype(&key).expect("spawn archetype exists");
        assert_eq!(table.entity_count(), 1);
        let entity = table.entity(0).expect("row 0 entity exists");
        assert_eq!(entity.index(), 0);
        assert_eq!(entity.generation(), 0);
        assert_eq!(
            table
                .column(position_id)
                .expect("position column exists")
                .row_bytes(0),
            Some(expected_position.as_slice())
        );
        assert_eq!(
            table
                .column(velocity_id)
                .expect("velocity column exists")
                .row_bytes(0),
            Some(initial_velocity.as_slice())
        );
        assert!(world.entities().is_alive(entity));
    }

    fn xy_component_descriptor(id: ComponentId, name: &str) -> ComponentDescriptor {
        ComponentDescriptor {
            id,
            name: name.to_string(),
            size: 8,
            align: 4,
            fields: vec![
                ComponentFieldDescriptor {
                    name: "x".to_string(),
                    type_name: "f32".to_string(),
                    offset: 0,
                },
                ComponentFieldDescriptor {
                    name: "y".to_string(),
                    type_name: "f32".to_string(),
                    offset: 4,
                },
            ],
        }
    }

    fn f32_payload(value: f32) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn f32_pair_payload(x: f32, y: f32) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&x.to_le_bytes());
        payload.extend_from_slice(&y.to_le_bytes());
        payload
    }
}
