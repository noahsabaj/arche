#![allow(dead_code)]

use crate::layout::ComponentId;
use crate::runtime::{
    ComponentDescriptor, QueryDescriptor, ResourceDescriptor, ResourceId, ScheduleDescriptor,
    ScheduleId, SystemDescriptor,
};

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
