use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericModifierInstance<Source, Stat, Detail> {
    pub source: Source,
    pub stat: Stat,
    pub amount: i32,
    pub detail: Detail,
}

impl<Source, Stat, Detail> GenericModifierInstance<Source, Stat, Detail> {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_detail<NewDetail>(
        self,
        detail: NewDetail,
    ) -> GenericModifierInstance<Source, Stat, NewDetail> {
        GenericModifierInstance {
            source: self.source,
            stat: self.stat,
            amount: self.amount,
            detail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericModifierLedger<Source, Stat, Detail>
where
    Source: Ord,
{
    by_source: BTreeMap<Source, Vec<GenericModifierInstance<Source, Stat, Detail>>>,
}

impl<Source, Stat, Detail> Default for GenericModifierLedger<Source, Stat, Detail>
where
    Source: Ord,
{
    fn default() -> Self {
        Self {
            by_source: BTreeMap::new(),
        }
    }
}

impl<Source, Stat, Detail> GenericModifierLedger<Source, Stat, Detail>
where
    Source: Ord + Copy,
    Stat: Copy + PartialEq,
{
    #[cfg_attr(not(test), allow(dead_code))]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.by_source.is_empty()
    }

    pub fn replace_source(
        &mut self,
        source: Source,
        modifiers: impl IntoIterator<Item = GenericModifierInstance<Source, Stat, Detail>>,
    ) {
        let modifiers = modifiers.into_iter().collect::<Vec<_>>();
        if modifiers.is_empty() {
            self.by_source.remove(&source);
        } else {
            self.by_source.insert(source, modifiers);
        }
    }

    pub fn remove_source(
        &mut self,
        source: Source,
    ) -> Option<Vec<GenericModifierInstance<Source, Stat, Detail>>> {
        self.by_source.remove(&source)
    }

    #[allow(dead_code)]
    pub fn total_for_stat(&self, stat: Stat) -> i32 {
        self.by_source
            .values()
            .flat_map(|instances| instances.iter())
            .filter(|instance| instance.stat == stat)
            .map(|instance| instance.amount)
            .sum()
    }

    pub fn iter(&self) -> impl Iterator<Item = &GenericModifierInstance<Source, Stat, Detail>> {
        self.by_source
            .values()
            .flat_map(|instances| instances.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum TestSource {
        A,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestStat {
        X,
        Y,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestDetail {
        None,
        Tagged,
    }

    #[test]
    fn replacement_is_source_scoped() {
        let source = TestSource::A;
        let mut ledger = GenericModifierLedger::default();
        ledger.replace_source(
            source,
            [GenericModifierInstance {
                source,
                stat: TestStat::X,
                amount: 12,
                detail: TestDetail::None,
            }],
        );
        assert_eq!(ledger.total_for_stat(TestStat::X), 12);

        ledger.replace_source(
            source,
            [GenericModifierInstance {
                source,
                stat: TestStat::X,
                amount: 5,
                detail: TestDetail::Tagged,
            }],
        );
        assert_eq!(ledger.total_for_stat(TestStat::X), 5);
    }

    #[test]
    fn removing_source_drops_its_modifiers() {
        let source = TestSource::A;
        let mut ledger = GenericModifierLedger::default();
        ledger.replace_source(
            source,
            [
                GenericModifierInstance {
                    source,
                    stat: TestStat::X,
                    amount: 10,
                    detail: TestDetail::None,
                },
                GenericModifierInstance {
                    source,
                    stat: TestStat::Y,
                    amount: 2,
                    detail: TestDetail::Tagged,
                },
            ],
        );

        assert!(ledger.remove_source(source).is_some());
        assert_eq!(ledger.total_for_stat(TestStat::X), 0);
        assert_eq!(ledger.total_for_stat(TestStat::Y), 0);
    }
}
