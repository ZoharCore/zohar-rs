use super::Stat;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledModifier<Detail = ()> {
    pub stat: Stat,
    pub amount: i32,
    pub detail: Detail,
}

impl CompiledModifier<()> {
    pub const fn plain(stat: Stat, amount: i32) -> Self {
        Self {
            stat,
            amount,
            detail: (),
        }
    }
}

impl<Detail> CompiledModifier<Detail> {
    pub const fn new(stat: Stat, amount: i32, detail: Detail) -> Self {
        Self {
            stat,
            amount,
            detail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledStatContribution<Detail = ()> {
    modifiers: Vec<CompiledModifier<Detail>>,
}

impl<Detail> CompiledStatContribution<Detail> {
    pub fn new() -> Self {
        Self {
            modifiers: Vec::new(),
        }
    }

    pub fn modifiers(&self) -> &[CompiledModifier<Detail>] {
        &self.modifiers
    }

    pub fn with_modifier(mut self, modifier: CompiledModifier<Detail>) -> Self {
        self.modifiers.push(modifier);
        self
    }

    pub fn push_modifier(&mut self, modifier: CompiledModifier<Detail>) {
        self.modifiers.push(modifier);
    }
}

impl<Detail> Default for CompiledStatContribution<Detail> {
    fn default() -> Self {
        Self::new()
    }
}
