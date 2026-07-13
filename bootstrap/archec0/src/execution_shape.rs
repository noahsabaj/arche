use std::collections::HashSet;

use crate::core::{
    CoreInstruction, CoreProgram, CoreQueryAccess, CoreScheduleItem, CoreSystemExpression,
    CoreSystemParamKind, CoreSystemPlace, CoreSystemStatement, CoreTerminator, CoreType,
};
use crate::core_verify;
use crate::layout::ComponentId;
use crate::runtime::{
    stable_query_id, QueryAccess, QueryId, ResourceId, ScheduleId, ScheduleItemDescriptor,
    SystemAccess, SystemId, SystemParamDescriptorKind,
};
use crate::runtime_assembly::{RuntimeProgramAssembly, StartupOperation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionShapeError {
    pub(crate) message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedCoreExecutionShape {
    pub(crate) schedule: VerifiedScheduleExecutionShape,
    pub(crate) system: VerifiedSystemExecutionShape,
    pub(crate) resource: VerifiedReadResourceShape,
    pub(crate) query: VerifiedQueryExecutionShape,
    pub(crate) lanes: [VerifiedF32MultiplyAddLane; 2],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedScheduleExecutionShape {
    pub(crate) startup_operation_index: usize,
    pub(crate) id: ScheduleId,
    pub(crate) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedSystemExecutionShape {
    pub(crate) id: SystemId,
    pub(crate) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedReadResourceShape {
    pub(crate) param_name: String,
    pub(crate) id: ResourceId,
    pub(crate) name: String,
    pub(crate) size: u32,
    pub(crate) align: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedQueryExecutionShape {
    pub(crate) id: QueryId,
    pub(crate) name: String,
    pub(crate) param_name: String,
    pub(crate) target: VerifiedQueryComponentShape,
    pub(crate) source: VerifiedQueryComponentShape,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedQueryComponentShape {
    pub(crate) binding_name: String,
    pub(crate) id: ComponentId,
    pub(crate) name: String,
    pub(crate) access: QueryAccess,
    pub(crate) size: u32,
    pub(crate) align: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedF32MultiplyAddLane {
    pub(crate) target_field_name: String,
    pub(crate) target_field_offset: u32,
    pub(crate) source_field_name: String,
    pub(crate) source_field_offset: u32,
    pub(crate) resource_field_name: String,
    pub(crate) resource_field_offset: u32,
}

pub(crate) fn derive_verified_core_execution_shape(
    core: &CoreProgram,
    assembly: &RuntimeProgramAssembly,
) -> Result<VerifiedCoreExecutionShape, ExecutionShapeError> {
    core_verify::verify_core_program(core).map_err(|error| {
        shape_error(format!(
            "cannot derive execution shape from invalid Core: {}",
            error.message
        ))
    })?;
    if core.world.name != assembly.world_name {
        return Err(shape_error("Core and runtime assembly worlds do not match"));
    }
    verify_core_schemas_match_runtime_descriptors(core, assembly)?;
    require_supported_startup_exit(core)?;

    let startup_schedule_operations = assembly
        .startup_operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| matches!(operation, StartupOperation::RunSchedule { .. }))
        .collect::<Vec<_>>();
    let [(
        startup_operation_index,
        StartupOperation::RunSchedule {
            schedule_id: startup_schedule_id,
            schedule_name: startup_schedule_name,
        },
    )] = startup_schedule_operations.as_slice()
    else {
        return Err(unsupported_shape(
            "exactly one startup schedule run is required",
        ));
    };

    let [core_schedule] = core.schedules.as_slice() else {
        return Err(unsupported_shape("exactly one Core schedule is required"));
    };
    let [schedule_descriptor] = assembly.schedule_descriptors.as_slice() else {
        return Err(unsupported_shape(
            "exactly one runtime schedule descriptor is required",
        ));
    };
    if core_schedule.id != startup_schedule_id.0
        || schedule_descriptor.id != *startup_schedule_id
        || schedule_descriptor.name != *startup_schedule_name
        || schedule_descriptor.name != format!("{}.{}", core.world.name, core_schedule.name)
    {
        return Err(shape_error(
            "startup schedule identity does not match verified Core and descriptors",
        ));
    }
    let [CoreScheduleItem::Run {
        system_id,
        system_name,
    }] = core_schedule.items.as_slice()
    else {
        return Err(unsupported_shape(
            "the supported schedule must contain exactly one sequential system run",
        ));
    };
    let [ScheduleItemDescriptor::Run {
        system_id: descriptor_system_id,
        system_name: descriptor_system_name,
    }] = schedule_descriptor.items.as_slice()
    else {
        return Err(unsupported_shape(
            "the runtime schedule descriptor must contain exactly one system run",
        ));
    };

    let [core_system] = core.systems.as_slice() else {
        return Err(unsupported_shape("exactly one Core system is required"));
    };
    let [system_descriptor] = assembly.system_descriptors.as_slice() else {
        return Err(unsupported_shape(
            "exactly one runtime system descriptor is required",
        ));
    };
    if core_system.id != *system_id
        || system_descriptor.id.0 != *system_id
        || system_descriptor.name != *system_name
        || system_descriptor.id != *descriptor_system_id
        || system_descriptor.name != *descriptor_system_name
        || system_descriptor.name != format!("{}.{}", core.world.name, core_system.name)
    {
        return Err(shape_error(
            "scheduled system identity does not match verified Core and descriptors",
        ));
    }

    let mut resource_params = core_system
        .params
        .iter()
        .filter_map(|param| match &param.kind {
            CoreSystemParamKind::ReadResource { resource_id, name } => {
                Some((param.name.as_str(), *resource_id, name.as_str()))
            }
            CoreSystemParamKind::Query { .. } => None,
        });
    let Some((resource_param_name, resource_id, resource_name)) = resource_params.next() else {
        return Err(unsupported_shape(
            "exactly one read-resource parameter is required",
        ));
    };
    if resource_params.next().is_some() {
        return Err(unsupported_shape(
            "exactly one read-resource parameter is required",
        ));
    }
    if core.resources.len() != 1 {
        return Err(unsupported_shape("exactly one Core resource is required"));
    }
    let resource_descriptor = assembly
        .resource_descriptors
        .iter()
        .find(|descriptor| descriptor.id.0 == resource_id)
        .ok_or_else(|| shape_error("verified resource is absent from runtime descriptors"))?;
    if assembly.resource_descriptors.len() != 1 || resource_descriptor.name != resource_name {
        return Err(unsupported_shape(
            "exactly one matching runtime resource descriptor is required",
        ));
    }
    let resource_payloads = assembly
        .startup_operations
        .iter()
        .filter_map(|operation| match operation {
            StartupOperation::ResourcePayload {
                resource_id,
                resource_name,
                payload_bytes,
            } => Some((resource_id, resource_name, payload_bytes)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(startup_resource_id, startup_resource_name, startup_resource_payload)] =
        resource_payloads.as_slice()
    else {
        return Err(unsupported_shape(
            "exactly one startup resource payload is required",
        ));
    };
    if **startup_resource_id != resource_descriptor.id
        || *startup_resource_name != &resource_descriptor.name
        || startup_resource_payload.len() != resource_descriptor.size as usize
    {
        return Err(shape_error(
            "startup resource payload does not match its verified descriptor",
        ));
    }

    let query_params = core_system
        .params
        .iter()
        .filter_map(|param| match &param.kind {
            CoreSystemParamKind::Query { terms } => Some((param.name.as_str(), terms)),
            CoreSystemParamKind::ReadResource { .. } => None,
        })
        .collect::<Vec<_>>();
    let [(query_param_name, query_terms)] = query_params.as_slice() else {
        return Err(unsupported_shape("exactly one query parameter is required"));
    };
    let [first_term, second_term] = query_terms.as_slice() else {
        return Err(unsupported_shape(
            "the supported query must contain exactly two terms",
        ));
    };
    let (target_term, source_term) = match (first_term.access, second_term.access) {
        (CoreQueryAccess::Mut, CoreQueryAccess::Read) => (first_term, second_term),
        (CoreQueryAccess::Read, CoreQueryAccess::Mut) => (second_term, first_term),
        _ => {
            return Err(unsupported_shape(
                "the supported query requires one mutable target and one read-only source",
            ))
        }
    };
    if target_term.component_id == source_term.component_id {
        return Err(unsupported_shape(
            "the supported query target and source must be distinct components",
        ));
    }

    let [CoreSystemStatement::QueryLoop(query_loop)] = core_system.body.statements.as_slice()
    else {
        return Err(unsupported_shape(
            "the supported system body requires exactly one query loop",
        ));
    };
    if query_loop.query_param != *query_param_name || query_loop.bindings.len() != 2 {
        return Err(shape_error(
            "Core query loop does not match its verified query parameter",
        ));
    }
    let target_binding = query_loop
        .bindings
        .iter()
        .find(|binding| binding.component_id == target_term.component_id)
        .ok_or_else(|| shape_error("mutable query binding is absent from the Core loop"))?;
    let source_binding = query_loop
        .bindings
        .iter()
        .find(|binding| binding.component_id == source_term.component_id)
        .ok_or_else(|| shape_error("read-only query binding is absent from the Core loop"))?;
    if target_binding.access != CoreQueryAccess::Mut
        || source_binding.access != CoreQueryAccess::Read
    {
        return Err(shape_error(
            "Core query-loop binding access does not match the query terms",
        ));
    }

    let target_descriptor = assembly
        .component_descriptors
        .iter()
        .find(|descriptor| descriptor.id.0 == target_term.component_id)
        .ok_or_else(|| shape_error("mutable query component is absent from descriptors"))?;
    let source_descriptor = assembly
        .component_descriptors
        .iter()
        .find(|descriptor| descriptor.id.0 == source_term.component_id)
        .ok_or_else(|| shape_error("read-only query component is absent from descriptors"))?;
    if target_descriptor.name != target_term.name || source_descriptor.name != source_term.name {
        return Err(shape_error(
            "query component names do not match runtime descriptors",
        ));
    }

    let query_id = stable_query_id(&core.world.name, &core_system.name, query_param_name);
    let query_descriptor = assembly
        .query_descriptors
        .iter()
        .find(|descriptor| descriptor.id == query_id)
        .ok_or_else(|| shape_error("verified query is absent from runtime descriptors"))?;
    let expected_query_name = format!(
        "{}.{}.{}",
        core.world.name, core_system.name, query_param_name
    );
    if assembly.query_descriptors.len() != 1
        || query_descriptor.name != expected_query_name
        || query_descriptor.terms.len() != 2
        || !query_descriptor.terms.iter().zip(query_terms.iter()).all(
            |(descriptor_term, core_term)| {
                descriptor_term.component_id.0 == core_term.component_id
                    && descriptor_term.name == core_term.name
                    && matches!(
                        (&descriptor_term.access, core_term.access),
                        (QueryAccess::Read, CoreQueryAccess::Read)
                            | (QueryAccess::Mut, CoreQueryAccess::Mut)
                    )
            },
        )
        || system_descriptor.params.len() != core_system.params.len()
    {
        return Err(unsupported_shape(
            "exactly one matching two-term query descriptor is required",
        ));
    }
    for (descriptor_param, core_param) in system_descriptor.params.iter().zip(&core_system.params) {
        let matches = match (&descriptor_param.kind, &core_param.kind) {
            (
                SystemParamDescriptorKind::ReadResource {
                    resource_id: descriptor_id,
                    name: descriptor_name,
                },
                CoreSystemParamKind::ReadResource { resource_id, name },
            ) => descriptor_id.0 == *resource_id && descriptor_name == name,
            (
                SystemParamDescriptorKind::Query {
                    terms: descriptor_terms,
                },
                CoreSystemParamKind::Query { terms },
            ) => {
                descriptor_terms.len() == terms.len()
                    && descriptor_terms
                        .iter()
                        .zip(terms)
                        .all(|(descriptor_term, term)| {
                            descriptor_term.component_id.0 == term.component_id
                                && descriptor_term.name == term.name
                                && matches!(
                                    (&descriptor_term.access, term.access),
                                    (SystemAccess::Read, CoreQueryAccess::Read)
                                        | (SystemAccess::Mut, CoreQueryAccess::Mut)
                                )
                        })
            }
            _ => false,
        };
        if descriptor_param.name != core_param.name || !matches {
            return Err(shape_error(
                "runtime system parameters do not match verified Core",
            ));
        }
    }

    let [first_lane, second_lane] = query_loop.body.as_slice() else {
        return Err(unsupported_shape(
            "the supported query loop requires exactly two multiply-add lanes",
        ));
    };
    let lanes = [
        derive_lane(
            first_lane,
            target_binding.name.as_str(),
            target_descriptor,
            source_binding.name.as_str(),
            source_descriptor,
            resource_param_name,
            resource_descriptor,
        )?,
        derive_lane(
            second_lane,
            target_binding.name.as_str(),
            target_descriptor,
            source_binding.name.as_str(),
            source_descriptor,
            resource_param_name,
            resource_descriptor,
        )?,
    ];
    let distinct_target_offsets = lanes
        .iter()
        .map(|lane| lane.target_field_offset)
        .collect::<HashSet<_>>();
    if distinct_target_offsets.len() != lanes.len() {
        return Err(unsupported_shape(
            "multiply-add lanes must update distinct target fields",
        ));
    }

    Ok(VerifiedCoreExecutionShape {
        schedule: VerifiedScheduleExecutionShape {
            startup_operation_index: *startup_operation_index,
            id: *startup_schedule_id,
            name: startup_schedule_name.clone(),
        },
        system: VerifiedSystemExecutionShape {
            id: system_descriptor.id,
            name: system_descriptor.name.clone(),
        },
        resource: VerifiedReadResourceShape {
            param_name: resource_param_name.to_string(),
            id: resource_descriptor.id,
            name: resource_descriptor.name.clone(),
            size: resource_descriptor.size,
            align: resource_descriptor.align,
        },
        query: VerifiedQueryExecutionShape {
            id: query_descriptor.id,
            name: query_descriptor.name.clone(),
            param_name: (*query_param_name).to_string(),
            target: VerifiedQueryComponentShape {
                binding_name: target_binding.name.clone(),
                id: target_descriptor.id,
                name: target_descriptor.name.clone(),
                access: QueryAccess::Mut,
                size: target_descriptor.size,
                align: target_descriptor.align,
            },
            source: VerifiedQueryComponentShape {
                binding_name: source_binding.name.clone(),
                id: source_descriptor.id,
                name: source_descriptor.name.clone(),
                access: QueryAccess::Read,
                size: source_descriptor.size,
                align: source_descriptor.align,
            },
        },
        lanes,
    })
}

fn require_supported_startup_exit(core: &CoreProgram) -> Result<(), ExecutionShapeError> {
    let startup_functions = core
        .functions
        .iter()
        .filter(|function| function.name == "startup")
        .collect::<Vec<_>>();
    let [startup] = startup_functions.as_slice() else {
        return Err(unsupported_shape(
            "exactly one Core startup function is required",
        ));
    };
    let [entry] = startup.blocks.as_slice() else {
        return Err(unsupported_shape(
            "the Core startup function must contain exactly one entry block",
        ));
    };
    if entry.id != startup.entry {
        return Err(shape_error(
            "the Core startup block does not match the startup entry",
        ));
    }

    let CoreTerminator::Exit { value: exit_value } = entry.terminator;
    let direct_exit_value = entry
        .instructions
        .iter()
        .find_map(|instruction| match instruction {
            CoreInstruction::I32Const { result, value } if *result == exit_value => Some(*value),
            _ => None,
        });
    if direct_exit_value != Some(0) {
        return Err(unsupported_shape(
            "the Core startup exit must be the direct i32 literal 0",
        ));
    }

    Ok(())
}

fn verify_core_schemas_match_runtime_descriptors(
    core: &CoreProgram,
    assembly: &RuntimeProgramAssembly,
) -> Result<(), ExecutionShapeError> {
    if core.components.len() != assembly.component_descriptors.len() {
        return Err(shape_error(
            "Core and runtime component schema counts do not match",
        ));
    }
    for component in &core.components {
        let descriptor = assembly
            .component_descriptors
            .iter()
            .find(|descriptor| descriptor.id.0 == component.id)
            .ok_or_else(|| {
                shape_error(format!(
                    "Core component schema `{}` is absent from runtime descriptors",
                    component.name
                ))
            })?;
        let (expected_fields, expected_size, expected_align) =
            expected_core_schema_layout(&component.fields)?;
        if descriptor.name != component.name
            || descriptor.size != expected_size
            || descriptor.align != expected_align
            || descriptor.fields.len() != expected_fields.len()
            || !descriptor.fields.iter().zip(&expected_fields).all(
                |(descriptor_field, expected_field)| {
                    descriptor_field.name == expected_field.name
                        && descriptor_field.type_name == expected_field.type_name
                        && descriptor_field.offset == expected_field.offset
                },
            )
        {
            return Err(shape_error(format!(
                "runtime component schema `{}` does not match verified Core layout",
                descriptor.name
            )));
        }
    }

    if core.resources.len() != assembly.resource_descriptors.len() {
        return Err(shape_error(
            "Core and runtime resource schema counts do not match",
        ));
    }
    for resource in &core.resources {
        let descriptor = assembly
            .resource_descriptors
            .iter()
            .find(|descriptor| descriptor.id.0 == resource.id)
            .ok_or_else(|| {
                shape_error(format!(
                    "Core resource schema `{}` is absent from runtime descriptors",
                    resource.name
                ))
            })?;
        let (expected_fields, expected_size, expected_align) =
            expected_core_schema_layout(&resource.fields)?;
        if descriptor.name != resource.name
            || descriptor.size != expected_size
            || descriptor.align != expected_align
            || descriptor.fields.len() != expected_fields.len()
            || !descriptor.fields.iter().zip(&expected_fields).all(
                |(descriptor_field, expected_field)| {
                    descriptor_field.name == expected_field.name
                        && descriptor_field.type_name == expected_field.type_name
                        && descriptor_field.offset == expected_field.offset
                },
            )
        {
            return Err(shape_error(format!(
                "runtime resource schema `{}` does not match verified Core layout",
                descriptor.name
            )));
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedCoreFieldLayout {
    name: String,
    type_name: &'static str,
    offset: u32,
}

fn expected_core_schema_layout(
    fields: &[crate::core::CoreField],
) -> Result<(Vec<ExpectedCoreFieldLayout>, u32, u32), ExecutionShapeError> {
    let mut expected_fields = Vec::new();
    expected_fields.try_reserve(fields.len()).map_err(|error| {
        shape_error(format!(
            "failed to reserve verified Core schema layout: {error}"
        ))
    })?;
    let mut cursor = 0_u32;
    let mut schema_align = 1_u32;
    for field in fields {
        let (type_name, size, align) = match field.ty {
            CoreType::I32 => ("i32", 4_u32, 4_u32),
            CoreType::F32 => ("f32", 4_u32, 4_u32),
        };
        cursor = checked_align_up(cursor, align)?;
        schema_align = schema_align.max(align);
        expected_fields.push(ExpectedCoreFieldLayout {
            name: field.name.clone(),
            type_name,
            offset: cursor,
        });
        cursor = cursor
            .checked_add(size)
            .ok_or_else(|| shape_error("verified Core schema size overflows u32"))?;
    }
    let size = checked_align_up(cursor, schema_align)?;
    Ok((expected_fields, size, schema_align))
}

fn checked_align_up(value: u32, align: u32) -> Result<u32, ExecutionShapeError> {
    debug_assert!(align > 0 && align.is_power_of_two());
    let mask = align - 1;
    value
        .checked_add(mask)
        .map(|aligned| aligned & !mask)
        .ok_or_else(|| shape_error("verified Core schema alignment overflows u32"))
}

fn derive_lane(
    statement: &CoreSystemStatement,
    target_binding: &str,
    target_descriptor: &crate::runtime::ComponentDescriptor,
    source_binding: &str,
    source_descriptor: &crate::runtime::ComponentDescriptor,
    resource_param: &str,
    resource_descriptor: &crate::runtime::ResourceDescriptor,
) -> Result<VerifiedF32MultiplyAddLane, ExecutionShapeError> {
    let CoreSystemStatement::AddAssign { target, value } = statement else {
        return Err(unsupported_shape(
            "each supported query-loop statement must be an add-assign",
        ));
    };
    let CoreSystemPlace::ComponentField {
        binding,
        component_id,
        component_name,
        field_name: target_field_name,
    } = target;
    if binding != target_binding
        || *component_id != target_descriptor.id.0
        || component_name != &target_descriptor.name
    {
        return Err(unsupported_shape(
            "multiply-add target must be a field of the mutable query binding",
        ));
    }
    let target_field = f32_component_field(target_descriptor, target_field_name, "target")?;

    let CoreSystemExpression::Binary { op, left, right } = value else {
        return Err(unsupported_shape(
            "multiply-add value must be a binary f32 multiplication",
        ));
    };
    if *op != crate::core::CoreSystemBinaryOp::F32Multiply {
        return Err(unsupported_shape(
            "multiply-add value must use f32 multiplication",
        ));
    }
    let (source_field_name, resource_field_name) = match (
        source_field_expression(left, source_binding, source_descriptor),
        resource_field_expression(right, resource_param, resource_descriptor),
    ) {
        (Some(source), Some(resource)) => (source, resource),
        _ => match (
            source_field_expression(right, source_binding, source_descriptor),
            resource_field_expression(left, resource_param, resource_descriptor),
        ) {
            (Some(source), Some(resource)) => (source, resource),
            _ => return Err(unsupported_shape(
                "multiply-add value must multiply the read component field by the resource field",
            )),
        },
    };
    let source_field = f32_component_field(source_descriptor, source_field_name, "source")?;
    let resource_field = resource_descriptor
        .fields
        .iter()
        .find(|field| field.name == resource_field_name)
        .ok_or_else(|| shape_error("resource field is absent from its descriptor"))?;
    if resource_field.type_name != "f32" {
        return Err(unsupported_shape(
            "multiply-add resource field must have type f32",
        ));
    }
    if resource_field
        .offset
        .checked_add(4)
        .is_none_or(|end| end > resource_descriptor.size)
    {
        return Err(shape_error(
            "multiply-add resource field exceeds its descriptor payload",
        ));
    }

    Ok(VerifiedF32MultiplyAddLane {
        target_field_name: target_field.name.clone(),
        target_field_offset: target_field.offset,
        source_field_name: source_field.name.clone(),
        source_field_offset: source_field.offset,
        resource_field_name: resource_field.name.clone(),
        resource_field_offset: resource_field.offset,
    })
}

fn source_field_expression<'a>(
    expression: &'a CoreSystemExpression,
    expected_binding: &str,
    descriptor: &crate::runtime::ComponentDescriptor,
) -> Option<&'a str> {
    let CoreSystemExpression::ComponentField {
        binding,
        component_id,
        component_name,
        field_name,
    } = expression
    else {
        return None;
    };
    (binding == expected_binding
        && *component_id == descriptor.id.0
        && component_name == &descriptor.name)
        .then_some(field_name.as_str())
}

fn resource_field_expression<'a>(
    expression: &'a CoreSystemExpression,
    expected_param: &str,
    descriptor: &crate::runtime::ResourceDescriptor,
) -> Option<&'a str> {
    let CoreSystemExpression::ResourceField {
        param,
        resource_id,
        resource_name,
        field_name,
    } = expression
    else {
        return None;
    };
    (param == expected_param
        && *resource_id == descriptor.id.0
        && resource_name == &descriptor.name)
        .then_some(field_name.as_str())
}

fn f32_component_field<'a>(
    descriptor: &'a crate::runtime::ComponentDescriptor,
    field_name: &str,
    role: &str,
) -> Result<&'a crate::runtime::ComponentFieldDescriptor, ExecutionShapeError> {
    let field = descriptor
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .ok_or_else(|| shape_error(format!("{role} field is absent from its descriptor")))?;
    if field.type_name != "f32" {
        return Err(unsupported_shape(format!(
            "multiply-add {role} field must have type f32"
        )));
    }
    if field
        .offset
        .checked_add(4)
        .is_none_or(|end| end > descriptor.size)
    {
        return Err(shape_error(format!(
            "multiply-add {role} field exceeds its descriptor payload"
        )));
    }
    Ok(field)
}

fn unsupported_shape(detail: impl Into<String>) -> ExecutionShapeError {
    shape_error(format!(
        "unsupported verified Core execution shape: {}",
        detail.into()
    ))
}

fn shape_error(message: impl Into<String>) -> ExecutionShapeError {
    ExecutionShapeError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{checker, core_lower, lexer, parser, runtime_assembly};

    #[test]
    fn derives_supported_execution_shape_for_both_acceptance_programs() {
        let demo = shape(include_str!("../../../examples/move_system_two_rows.arc"));
        let arena = shape(include_str!("../../../examples/arena_recovery.arc"));

        assert_eq!(demo.schedule.name, "Demo.Main");
        assert_eq!(arena.schedule.name, "Arena.Step");
        assert_ne!(demo.system.id, arena.system.id);
        assert_ne!(demo.resource.id, arena.resource.id);
        assert_ne!(demo.query.target.id, arena.query.target.id);
        assert_eq!((demo.query.target.size, demo.query.source.size), (8, 8));
        assert_eq!((arena.query.target.size, arena.query.source.size), (8, 12));
        assert_eq!(
            demo.lanes
                .iter()
                .map(|lane| (
                    lane.target_field_offset,
                    lane.source_field_offset,
                    lane.resource_field_offset,
                ))
                .collect::<Vec<_>>(),
            [(0, 0, 0), (4, 4, 0)]
        );
        assert_eq!(
            arena
                .lanes
                .iter()
                .map(|lane| (
                    lane.target_field_offset,
                    lane.source_field_offset,
                    lane.resource_field_offset,
                ))
                .collect::<Vec<_>>(),
            [(0, 0, 0), (4, 4, 0)]
        );
    }

    #[test]
    fn rejects_unsupported_verified_core_execution_shape() {
        let source = include_str!("../../../examples/arena_recovery.arc");
        let tokens = lexer::lex(source).expect("Arena lexes");
        let program = parser::parse_program(&tokens).expect("Arena parses");
        checker::check_program(&program).expect("Arena checks");
        let mut core = core_lower::lower_program_to_core(&program).expect("Arena Core lowers");
        let assembly =
            runtime_assembly::assemble_runtime_program_from_verified_core(&program, &core)
                .expect("Arena assembly builds");

        let CoreSystemStatement::QueryLoop(query_loop) = &mut core.systems[0].body.statements[0]
        else {
            panic!("Arena system has a query loop");
        };
        query_loop.body.push(query_loop.body[0].clone());
        core_verify::verify_core_program(&core).expect("three-lane Core remains generally valid");
        let error = derive_verified_core_execution_shape(&core, &assembly)
            .expect_err("the temporary supported shape must reject a third lane");
        assert!(error.message.contains("exactly two multiply-add lanes"));
    }

    #[test]
    fn rejects_nonzero_and_nonliteral_startup_exits() {
        let source = include_str!("../../../examples/arena_recovery.arc");
        let (mut nonzero_core, nonzero_assembly) = core_and_assembly(source);
        let nonzero_entry = startup_entry_mut(&mut nonzero_core);
        let CoreTerminator::Exit { value: exit_value } = nonzero_entry.terminator;
        let CoreInstruction::I32Const { value, .. } = nonzero_entry
            .instructions
            .iter_mut()
            .find(|instruction| {
                matches!(
                    instruction,
                    CoreInstruction::I32Const { result, .. } if *result == exit_value
                )
            })
            .expect("Arena exit is initially a direct i32 constant")
        else {
            unreachable!("matched the direct exit constant")
        };
        *value = 7;
        core_verify::verify_core_program(&nonzero_core)
            .expect("a nonzero i32 exit remains generally valid Core");
        let error = derive_verified_core_execution_shape(&nonzero_core, &nonzero_assembly)
            .expect_err("M25 must reject a nonzero startup exit");
        assert!(error.message.contains("direct i32 literal 0"));

        let (mut computed_core, computed_assembly) = core_and_assembly(source);
        let computed_entry = startup_entry_mut(&mut computed_core);
        let CoreTerminator::Exit { value: exit_value } = computed_entry.terminator;
        let producer_index = computed_entry
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    CoreInstruction::I32Const { result, .. } if *result == exit_value
                )
            })
            .expect("Arena exit is initially a direct i32 constant");
        let fresh_value = crate::core::ValueId(u32::MAX);
        assert_ne!(fresh_value, exit_value);
        computed_entry.instructions.splice(
            producer_index..=producer_index,
            [
                CoreInstruction::I32Const {
                    result: fresh_value,
                    value: 0,
                },
                CoreInstruction::I32Binary {
                    result: exit_value,
                    op: crate::core::CoreBinaryOp::Add,
                    left: fresh_value,
                    right: fresh_value,
                },
            ],
        );
        core_verify::verify_core_program(&computed_core)
            .expect("a computed-zero exit remains generally valid Core");
        let error = derive_verified_core_execution_shape(&computed_core, &computed_assembly)
            .expect_err("M25 must reject a nonliteral startup exit");
        assert!(error.message.contains("direct i32 literal 0"));
    }

    #[test]
    fn rejects_runtime_schema_layout_drift_from_verified_core() {
        let source = include_str!("../../../examples/arena_recovery.arc");

        let (core, mut reordered_fields) = core_and_assembly(source);
        reordered_fields
            .component_descriptors
            .iter_mut()
            .find(|descriptor| descriptor.name == "Arena.Regeneration")
            .expect("Arena Regeneration descriptor exists")
            .fields
            .swap(0, 1);
        let error = derive_verified_core_execution_shape(&core, &reordered_fields)
            .expect_err("descriptor field order drift must be rejected");
        assert!(error
            .message
            .contains("component schema `Arena.Regeneration`"));

        let (core, mut shifted_lane_field) = core_and_assembly(source);
        shifted_lane_field
            .component_descriptors
            .iter_mut()
            .find(|descriptor| descriptor.name == "Arena.Regeneration")
            .expect("Arena Regeneration descriptor exists")
            .fields
            .iter_mut()
            .find(|field| field.name == "current_rate")
            .expect("Arena current_rate field exists")
            .offset = 8;
        let error = derive_verified_core_execution_shape(&core, &shifted_lane_field)
            .expect_err("an in-bounds lane field offset drift must be rejected");
        assert!(error
            .message
            .contains("component schema `Arena.Regeneration`"));

        let (core, mut unused_component_drift) = core_and_assembly(source);
        unused_component_drift
            .component_descriptors
            .iter_mut()
            .find(|descriptor| descriptor.name == "Arena.Faction")
            .expect("Arena Faction descriptor exists")
            .align = 8;
        let error = derive_verified_core_execution_shape(&core, &unused_component_drift)
            .expect_err("even an unused descriptor layout drift must be rejected");
        assert!(error.message.contains("component schema `Arena.Faction`"));

        let (core, mut resource_drift) = core_and_assembly(source);
        resource_drift
            .resource_descriptors
            .iter_mut()
            .find(|descriptor| descriptor.name == "Arena.Tick")
            .expect("Arena Tick descriptor exists")
            .align = 8;
        let error = derive_verified_core_execution_shape(&core, &resource_drift)
            .expect_err("resource descriptor layout drift must be rejected");
        assert!(error.message.contains("resource schema `Arena.Tick`"));
    }

    fn shape(source: &str) -> VerifiedCoreExecutionShape {
        let (core, assembly) = core_and_assembly(source);
        derive_verified_core_execution_shape(&core, &assembly)
            .expect("fixture supported execution shape derives")
    }

    fn core_and_assembly(source: &str) -> (CoreProgram, RuntimeProgramAssembly) {
        let tokens = lexer::lex(source).expect("fixture lexes");
        let program = parser::parse_program(&tokens).expect("fixture parses");
        checker::check_program(&program).expect("fixture checks");
        let core = core_lower::lower_program_to_core(&program).expect("fixture Core lowers");
        let assembly =
            runtime_assembly::assemble_runtime_program_from_verified_core(&program, &core)
                .expect("fixture assembly builds");
        (core, assembly)
    }

    fn startup_entry_mut(core: &mut CoreProgram) -> &mut crate::core::CoreBlock {
        let startup = core
            .functions
            .iter_mut()
            .find(|function| function.name == "startup")
            .expect("fixture has a startup function");
        let entry = startup.entry;
        startup
            .blocks
            .iter_mut()
            .find(|block| block.id == entry)
            .expect("fixture has its startup entry block")
    }
}
