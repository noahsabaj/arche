//! Deferred Structural Commands and Transactional Epoch Buffering (M27-F).

use crate::runtime::def::{CanonicalValue, EntityHandle};
use arche_foundation::identity::TypeId;
use std::collections::BTreeMap;

/// A deferred structural mutation command to be applied at a schedule sync barrier.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Spawn {
        handle: EntityHandle,
        components: BTreeMap<TypeId, CanonicalValue>,
    },
    Despawn(EntityHandle),
    AddComponent {
        handle: EntityHandle,
        component_type_id: TypeId,
        value: CanonicalValue,
    },
    RemoveComponent {
        handle: EntityHandle,
        component_type_id: TypeId,
    },
    InsertResource {
        resource_type_id: TypeId,
        value: CanonicalValue,
    },
    RemoveResource {
        resource_type_id: TypeId,
    },
}

/// A deterministic deferred command buffer supporting transactional system execution epochs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandBuffer {
    pub staged_commands: Vec<Command>,
    pub epoch_buffer: Option<Vec<Command>>,
}

impl CommandBuffer {
    /// Creates a new empty command buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begins a transactional system execution epoch.
    pub fn begin_epoch(&mut self) {
        self.epoch_buffer = Some(Vec::new());
    }

    /// Commits all commands recorded during the current epoch into the staged queue.
    pub fn commit_epoch(&mut self) {
        if let Some(epoch_cmds) = self.epoch_buffer.take() {
            self.staged_commands.extend(epoch_cmds);
        }
    }

    /// Discards all commands recorded during the current epoch (e.g. upon exception or trap).
    pub fn rollback_epoch(&mut self) {
        self.epoch_buffer = None;
    }

    /// Records a command into the active epoch or staged queue.
    pub fn push_command(&mut self, cmd: Command) {
        if let Some(epoch) = &mut self.epoch_buffer {
            epoch.push(cmd);
        } else {
            self.staged_commands.push(cmd);
        }
    }

    /// Queues an entity spawn command.
    pub fn spawn(&mut self, handle: EntityHandle, components: BTreeMap<TypeId, CanonicalValue>) {
        self.push_command(Command::Spawn { handle, components });
    }

    /// Queues an entity despawn command.
    pub fn despawn(&mut self, handle: EntityHandle) {
        self.push_command(Command::Despawn(handle));
    }

    /// Queues an AddComponent command.
    pub fn add_component(
        &mut self,
        handle: EntityHandle,
        component_type_id: TypeId,
        value: CanonicalValue,
    ) {
        self.push_command(Command::AddComponent {
            handle,
            component_type_id,
            value,
        });
    }

    /// Queues a RemoveComponent command.
    pub fn remove_component(&mut self, handle: EntityHandle, component_type_id: TypeId) {
        self.push_command(Command::RemoveComponent {
            handle,
            component_type_id,
        });
    }

    /// Queues an InsertResource command.
    pub fn insert_resource(&mut self, resource_type_id: TypeId, value: CanonicalValue) {
        self.push_command(Command::InsertResource {
            resource_type_id,
            value,
        });
    }

    /// Queues a RemoveResource command.
    pub fn remove_resource(&mut self, resource_type_id: TypeId) {
        self.push_command(Command::RemoveResource { resource_type_id });
    }

    /// Drains and clears all staged commands for execution at a sync barrier.
    pub fn drain_staged(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.staged_commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_commit_and_rollback() {
        let mut buf = CommandBuffer::new();

        let e1 = EntityHandle {
            slot_index: 1,
            generation: 1,
        };
        let e2 = EntityHandle {
            slot_index: 2,
            generation: 1,
        };

        // Successful epoch
        buf.begin_epoch();
        buf.despawn(e1);
        buf.commit_epoch();
        assert_eq!(buf.staged_commands.len(), 1);

        // Failed epoch (rolled back)
        buf.begin_epoch();
        buf.despawn(e2);
        buf.rollback_epoch();
        assert_eq!(buf.staged_commands.len(), 1); // Still 1
        assert_eq!(buf.staged_commands[0], Command::Despawn(e1));
    }
}
