//! Neither type should be flagged: UsedConfig is referenced; HasMethods has an
//! impl block (types with behaviour are left to the call-graph checks).
pub struct UsedConfig {
    pub n: u32,
}

pub fn make_config() -> UsedConfig {
    UsedConfig { n: 1 }
}

pub struct HasMethods {
    pub x: u32,
}

impl HasMethods {
    pub fn get(&self) -> u32 {
        self.x
    }
}
