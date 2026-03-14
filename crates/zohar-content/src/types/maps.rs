use super::empires::Empire;
use bitflags::bitflags;

#[derive(Debug, Clone)]
pub struct ContentMap {
    pub map_id: i64,
    pub code: String,
    pub name: String,
    pub map_width: f32,
    pub map_height: f32,
    pub empire: Option<Empire>,
    pub base_x: Option<f32>,
    pub base_y: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct MapTownSpawn {
    pub map_id: i64,
    pub empire: Empire,
    pub x: f32,
    pub y: f32,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TerrainFlags: u8 {
        const BLOCK = 1 << 0;
        const WATER = 1 << 1;
        const SAFEZONE = 1 << 2;
        const OBJECT = 1 << 7;
    }
}

#[derive(Debug, Clone)]
pub struct TerrainFlagsGrid {
    pub map_id: i64,
    pub cell_size_m: f32,
    pub grid_width: usize,
    pub grid_height: usize,
    pub data: Vec<TerrainFlags>,
}
