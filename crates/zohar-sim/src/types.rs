use bevy::prelude::Component;
#[cfg(feature = "admin-brp")]
use bevy::prelude::ReflectComponent;
use zohar_domain::MapId;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstanceId(pub u64);

impl InstanceId {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapInstanceKey {
    pub channel_id: u32,
    pub map_id: MapId,
    pub instance: MapInstanceKind,
}

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapInstanceKind {
    Shared,
    Instanced(InstanceId),
}

impl MapInstanceKey {
    pub fn shared(channel_id: u32, map_id: MapId) -> Self {
        Self {
            channel_id,
            map_id,
            instance: MapInstanceKind::Shared,
        }
    }

    pub fn instanced(channel_id: u32, map_id: MapId, instance_id: InstanceId) -> Self {
        Self {
            channel_id,
            map_id,
            instance: MapInstanceKind::Instanced(instance_id),
        }
    }

    pub fn template_map_id(self) -> MapId {
        self.map_id
    }
}
