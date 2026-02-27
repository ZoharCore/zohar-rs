use anyhow::{Result, bail};
use zohar_domain::Empire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmpireStartMaps {
    red: String,
    yellow: String,
    blue: String,
}

impl Default for EmpireStartMaps {
    fn default() -> Self {
        Self {
            red: "metin2_map_a1".into(),
            yellow: "metin2_map_b1".into(),
            blue: "metin2_map_c1".into(),
        }
    }
}

impl EmpireStartMaps {
    pub fn from_options(red: Option<String>, yellow: Option<String>, blue: Option<String>) -> Self {
        let defaults = Self::default();

        Self {
            red: red.unwrap_or(defaults.red),
            yellow: yellow.unwrap_or(defaults.yellow),
            blue: blue.unwrap_or(defaults.blue),
        }
    }

    pub fn validate(self) -> Result<Self> {
        if self.red.trim().is_empty() {
            bail!("red start map code cannot be empty");
        }
        if self.yellow.trim().is_empty() {
            bail!("yellow start map code cannot be empty");
        }
        if self.blue.trim().is_empty() {
            bail!("blue start map code cannot be empty");
        }
        Ok(self)
    }

    pub fn map_code_for_empire(&self, empire: Empire) -> &str {
        match empire {
            Empire::Red => &self.red,
            Empire::Yellow => &self.yellow,
            Empire::Blue => &self.blue,
        }
    }
}
