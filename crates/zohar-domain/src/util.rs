use bitflags::Flags;

pub struct FlagsMapper<'a, Src, Dst> {
    rules: &'a [(Src, Dst)],
}

impl<'a, Src, Dst> FlagsMapper<'a, Src, Dst>
where
    Src: Flags + Copy + 'static,
    Dst: Flags + Copy + 'static,
{
    pub const fn new(rules: &'a [(Src, Dst)]) -> Self {
        Self { rules }
    }

    pub fn map(&self, source: Src) -> Dst {
        self.rules
            .iter()
            .filter_map(|(src, dst)| source.contains(*src).then_some(*dst))
            .fold(Dst::empty(), |acc, flag| acc.union(flag))
    }
}
