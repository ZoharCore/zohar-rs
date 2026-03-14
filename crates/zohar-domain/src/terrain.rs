bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TerrainFlags: u8 {
        const BLOCK = 1 << 0;
        const WATER = 1 << 1;
        const SAFEZONE = 1 << 2;
        const OBJECT = 1 << 7;
    }
}

impl TerrainFlags {
    pub fn blocks_movement(self) -> bool {
        self.intersects(Self::BLOCK | Self::OBJECT)
    }

    pub fn is_walkable(self) -> bool {
        !self.blocks_movement()
    }
}

#[cfg(test)]
mod tests {
    use super::TerrainFlags;

    #[test]
    fn block_and_object_prevent_walkability() {
        assert!(TerrainFlags::BLOCK.blocks_movement());
        assert!(TerrainFlags::OBJECT.blocks_movement());
        assert!(!TerrainFlags::WATER.blocks_movement());
        assert!(!TerrainFlags::SAFEZONE.blocks_movement());
        assert!(TerrainFlags::WATER.is_walkable());
    }
}
