use super::empires::Empire;

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
