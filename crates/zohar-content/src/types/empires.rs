#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum Empire {
    #[strum(serialize = "SHINSOO")]
    Red,
    #[strum(serialize = "CHUNJO")]
    Yellow,
    #[strum(serialize = "JINNO")]
    Blue,
}

#[derive(Debug, Clone)]
pub struct EmpireStartConfig {
    pub empire: Empire,
    pub start_map_id: i64,
    pub start_x: f32,
    pub start_y: f32,
}
