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

#[cfg(test)]
mod tests {
    use super::*;

    bitflags::bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct LegacyFlags : u32 {
           const ALPHA = 1 << 0;
           const BETA = 1 << 1;
           const GAMMA = 1 << 2;
           const DELTA = 1 << 3;
           const THETA = 1 << 7;
        }
    }

    bitflags::bitflags! {
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct ModernizedFlags : u64 {
           const A = 1 << 0;
           const B = 1 << 1;
           const C = 1 << 2;
           const D = 1 << 3;
        }
    }

    #[test]
    fn flags_mapper_smoke() {
        const MAPPER: FlagsMapper<LegacyFlags, ModernizedFlags> = FlagsMapper::new(&[
            (LegacyFlags::ALPHA, ModernizedFlags::A),
            (LegacyFlags::BETA, ModernizedFlags::B),
            (LegacyFlags::GAMMA, ModernizedFlags::C),
            (LegacyFlags::DELTA, ModernizedFlags::D),
        ]);

        let flags_instance = LegacyFlags::ALPHA | LegacyFlags::GAMMA | LegacyFlags::THETA;
        let mapped = MAPPER.map(flags_instance);

        assert!(mapped.contains(ModernizedFlags::A));
        assert!(mapped.contains(ModernizedFlags::C));
        assert!(!mapped.contains(ModernizedFlags::B));
        assert!(!mapped.contains(ModernizedFlags::D));
    }

    #[test]
    fn flags_mapper_empty() {
        const MAPPER: FlagsMapper<LegacyFlags, ModernizedFlags> =
            FlagsMapper::new(&[(LegacyFlags::ALPHA, ModernizedFlags::A)]);

        assert_eq!(MAPPER.map(LegacyFlags::empty()), ModernizedFlags::empty());
        assert_eq!(MAPPER.map(LegacyFlags::BETA), ModernizedFlags::empty());
    }
}
