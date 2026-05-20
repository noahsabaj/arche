#![allow(dead_code)]

use crate::layout::{self, ComponentId};
use crate::parser::{
    ComponentDecl, ComponentLiteralValue, Program, QueryAccess as ParserQueryAccess, ResourceDecl,
    ResourceStatement, RunStatement, ScheduleDecl, ScheduleItem, SpawnStatement, Statement,
    SystemDecl, SystemParam, SystemParamKind,
};
use crate::runtime::{
    stable_query_id, stable_resource_id, stable_schedule_id, stable_system_id, ComponentDescriptor,
    ComponentFieldDescriptor, QueryAccess, QueryDescriptor, QueryTermDescriptor,
    ResourceDescriptor, ResourceFieldDescriptor, ResourceId, ScheduleDescriptor, ScheduleId,
    ScheduleItemDescriptor, SystemAccess, SystemDescriptor, SystemParamDescriptor,
    SystemParamDescriptorKind, SystemQueryTermDescriptor,
};

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
}

pub fn assemble_runtime_program_from_source(
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

        if descriptor_field.type_name != "f32" {
            return Err(assembly_error(format!(
                "unsupported resource field type `{}` for `{}`",
                descriptor_field.type_name, field.name
            )));
        }

        let value = match &field.value {
            ComponentLiteralValue::Float { text, .. } => text.parse::<f32>().map_err(|_| {
                assembly_error(format!(
                    "invalid f32 literal `{}` for resource field `{}`",
                    text, field.name
                ))
            })?,
        };
        let offset = descriptor_field.offset as usize;
        let end = offset + 4;
        if end > payload_bytes.len() {
            return Err(assembly_error(format!(
                "resource field `{}` exceeds payload size",
                field.name
            )));
        }

        payload_bytes[offset..end].copy_from_slice(&value.to_le_bytes());
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

            if descriptor_field.type_name != "f32" {
                return Err(assembly_error(format!(
                    "unsupported component field type `{}` for `{}`",
                    descriptor_field.type_name, field.name
                )));
            }

            let value = match &field.value {
                ComponentLiteralValue::Float { text, .. } => text.parse::<f32>().map_err(|_| {
                    assembly_error(format!(
                        "invalid f32 literal `{}` for component field `{}`",
                        text, field.name
                    ))
                })?,
            };
            let offset = descriptor_field.offset as usize;
            let end = offset + 4;
            if end > payload_bytes.len() {
                return Err(assembly_error(format!(
                    "component field `{}` exceeds payload size",
                    field.name
                )));
            }

            payload_bytes[offset..end].copy_from_slice(&value.to_le_bytes());
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
    use crate::layout::ComponentId;
    use crate::runtime::{
        stable_query_id, stable_resource_id, stable_schedule_id, stable_system_id,
        ComponentFieldDescriptor, QueryAccess, QueryTermDescriptor, ResourceFieldDescriptor,
        ScheduleItemDescriptor, SystemAccess, SystemParamDescriptor, SystemParamDescriptorKind,
        SystemQueryTermDescriptor,
    };
    use crate::{lexer, parser};

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
