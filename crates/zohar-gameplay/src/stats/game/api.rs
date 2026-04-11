use std::cmp::Ord;
use std::collections::BTreeMap;

use thiserror::Error;

use super::actor::{ActorImmuneFlags, ActorResources, ActorStatState, StatWriteError};
use super::change_set::StatChangeSet;
use super::contribution::{CompiledModifier, CompiledStatContribution};
use super::progression::PlayerProgressionState;
use super::resource::{QueuedRecovery, ResourceApplication, ResourceApplicationResult};
use super::source::{ActorStatSource, PlayerResourceCapacity, PlayerStatSource};
use super::stat::StatExt;
use super::store::PointValueStore;
use super::view::{StatValueView, read_stat_value};
use super::{Stat, StatModifierInstance};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatRecomputeReport {
    changes: StatChangeSet,
    runtime_dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterAppearance {
    pub level: u32,
    pub move_speed: u8,
    pub attack_speed: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterUpdate {
    pub appearance: CharacterAppearance,
    pub immune_flags: ActorImmuneFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatDelta {
    pub stat: Stat,
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatSnapshot {
    values: BTreeMap<Stat, u32>,
}

impl StatSnapshot {
    fn from_state(state: &ActorStatState<impl Ord + Copy, impl Sized>) -> Self {
        let mut values = BTreeMap::new();

        for (stat, _) in state.computed().iter() {
            if matches!(
                stat,
                Stat::ArmorDefence
                    | Stat::MaxHpPrePctBonus
                    | Stat::BonusMaxHp
                    | Stat::BonusMaxSp
                    | Stat::BonusMaxStamina
            ) {
                continue;
            }
            values.insert(
                stat,
                read_stat_value(state, None, stat, StatValueView::WireCompatible).max(0) as u32,
            );
        }

        for (stat, _) in state.base().iter() {
            if !matches!(stat, Stat::Level | Stat::Exp | Stat::NextExp | Stat::Gold) {
                continue;
            }
            values.insert(
                stat,
                read_stat_value(state, None, stat, StatValueView::WireCompatible).max(0) as u32,
            );
        }

        for stat in [
            Stat::Hp,
            Stat::Sp,
            Stat::Stamina,
            Stat::HpRecovery,
            Stat::SpRecovery,
            Stat::Polymorph,
            Stat::Mount,
        ] {
            values.insert(
                stat,
                read_stat_value(state, None, stat, StatValueView::WireCompatible).max(0) as u32,
            );
        }

        let progression = state.progression();
        values.insert(Stat::Exp, progression.exp_in_level);
        values.insert(Stat::NextExp, progression.next_exp_in_level);

        Self { values }
    }

    pub fn get(&self, stat: Stat) -> u32 {
        self.values.get(&stat).copied().unwrap_or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Stat, u32)> + '_ {
        self.values.iter().map(|(stat, value)| (*stat, *value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsSync {
    pub changes: StatChangeSet,
    pub stat_deltas: Vec<StatDelta>,
    pub character_update: Option<CharacterUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapStatsSync {
    pub changes: StatChangeSet,
    pub stat_snapshot: StatSnapshot,
    pub character_update: CharacterUpdate,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SourceBundleError {
    #[error("{stat:?} must be routed through the recovery channel")]
    RecoveryStatInModifierChannel { stat: Stat },
    #[error("{stat:?} does not accept modifier contributions")]
    UnsupportedModifierStat { stat: Stat },
}

pub struct GameStatsApi<'a, Source, Detail>
where
    Source: Ord + Copy,
{
    source: &'a ActorStatSource,
    state: &'a mut ActorStatState<Source, Detail>,
}

impl<'a, Source, Detail> GameStatsApi<'a, Source, Detail>
where
    Source: Ord + Copy,
{
    pub fn new(source: &'a ActorStatSource, state: &'a mut ActorStatState<Source, Detail>) -> Self {
        assert_eq!(
            state.kind(),
            source.actor_kind(),
            "actor state kind must match stat source kind"
        );
        Self { source, state }
    }

    pub fn read_limited(&self, stat: Stat) -> i32 {
        read_stat_value(self.state, Some(self.source), stat, StatValueView::Limited)
    }

    pub fn read_packet(&self, stat: Stat) -> i32 {
        read_stat_value(self.state, None, stat, StatValueView::WireCompatible)
    }

    pub fn computed_value(&self, stat: Stat) -> i32 {
        self.state.computed().get(stat)
    }

    pub fn is_dirty(&self) -> bool {
        self.state.is_dirty()
    }

    pub fn stat_snapshot(&self) -> StatSnapshot {
        StatSnapshot::from_state(self.state)
    }

    pub fn appearance(&self) -> CharacterAppearance {
        CharacterAppearance {
            level: self.read_packet(Stat::Level).max(0) as u32,
            move_speed: saturating_u8(self.read_limited(Stat::MovSpeed)),
            attack_speed: saturating_u8(self.read_limited(Stat::AttSpeed)),
        }
    }

    pub fn character_update(&self) -> CharacterUpdate {
        let runtime = *self.state.runtime();
        CharacterUpdate {
            appearance: self.appearance(),
            immune_flags: runtime.immune_flags,
        }
    }

    pub fn set_stored_stat(&mut self, stat: Stat, value: i32) -> Result<(), StatWriteError> {
        validate_stored_write_limit(self.source, stat, value)?;
        self.state.set_stored_stat(stat, value)
    }

    pub fn set_player_progression(&mut self, progression: PlayerProgressionState) {
        self.state.set_player_progression(progression);
    }

    pub fn set_stable_id(&mut self, stable_id: u64) {
        self.state.set_stable_id(Some(stable_id));
    }

    pub fn clear_stable_id(&mut self) {
        self.state.set_stable_id(None);
    }

    pub fn set_resource(&mut self, stat: Stat, value: i32) -> Result<i32, StatWriteError> {
        let capped = value.clamp(0, current_resource_cap(self.state, stat)?);
        self.state
            .set_resource_stat(stat, capped)
            .expect("resource stat was already validated");
        Ok(capped)
    }

    pub fn change_resource(&mut self, stat: Stat, delta: i32) -> Result<i32, StatWriteError> {
        let next = current_resource_value(self.state, stat)? + delta;
        self.set_resource(stat, next)
    }

    pub fn apply_resource(
        &mut self,
        application: ResourceApplication,
    ) -> Result<ResourceApplicationResult, StatWriteError> {
        let previous = current_resource_value(self.state, application.stat)?;
        let upper_bound = if application.clamp_to_cap {
            current_resource_cap(self.state, application.stat)?
        } else {
            i32::MAX
        };
        let next = previous
            .saturating_add(application.delta)
            .clamp(application.min_value, upper_bound);
        self.state
            .set_resource_stat(application.stat, next)
            .expect("resource stat was already validated");

        Ok(ResourceApplicationResult {
            stat: application.stat,
            previous,
            current: next,
            applied_delta: next - previous,
            was_clamped: next != previous.saturating_add(application.delta),
        })
    }

    pub fn queue_recovery(
        &mut self,
        stat: Stat,
        amount: i32,
    ) -> Result<Option<QueuedRecovery>, StatWriteError> {
        let bucket = recovery_bucket_for_resource(stat)?;
        let previous = current_resource_value(self.state, bucket)?;
        if current_resource_value(self.state, stat)?.saturating_add(previous)
            >= current_resource_cap(self.state, stat)?
        {
            return Ok(None);
        }

        let queued_amount = amount.max(0);
        let next = previous.saturating_add(queued_amount);
        self.state.set_resource_stat(bucket, next)?;
        Ok(Some(QueuedRecovery {
            stat,
            previous_pending: previous,
            current_pending: next,
            queued_amount,
        }))
    }

    pub fn queue_auto_recovery(
        &mut self,
        stat: Stat,
        available_amount: i32,
    ) -> Result<Option<QueuedRecovery>, StatWriteError> {
        let bucket = recovery_bucket_for_resource(stat)?;
        let previous = current_resource_value(self.state, bucket)?;
        let missing = current_resource_cap(self.state, stat)?
            .saturating_sub(current_resource_value(self.state, stat)?.saturating_add(previous));
        let queued_amount = available_amount.max(0).min(missing);
        if queued_amount <= 0 {
            return Ok(None);
        }

        let next = previous.saturating_add(queued_amount);
        self.state.set_resource_stat(bucket, next)?;
        Ok(Some(QueuedRecovery {
            stat,
            previous_pending: previous,
            current_pending: next,
            queued_amount,
        }))
    }

    pub fn apply_pending_recovery(
        &mut self,
        stat: Stat,
        amount: i32,
    ) -> Result<ResourceApplicationResult, StatWriteError> {
        let bucket = recovery_bucket_for_resource(stat)?;
        let pending = current_resource_value(self.state, bucket)?;
        let current = current_resource_value(self.state, stat)?;
        let max = current_resource_cap(self.state, stat)?;
        if current >= max {
            self.state.set_resource_stat(bucket, 0)?;
            return self.apply_resource(ResourceApplication::restore(stat, 0));
        }

        let scheduled = amount.max(0).min(pending);
        let result = self.apply_resource(ResourceApplication::restore(stat, scheduled))?;
        if scheduled > 0 {
            self.state
                .set_resource_stat(bucket, pending.saturating_sub(scheduled))?;
        }
        Ok(result)
    }

    pub fn replace_source_bundle(
        &mut self,
        source: Source,
        contribution: CompiledStatContribution<Detail>,
    ) -> Result<(), SourceBundleError>
    where
        Detail: Clone,
    {
        for modifier in contribution.modifiers() {
            validate_modifier_stat(modifier.stat)?;
        }
        self.state.replace_modifier_source(
            source,
            contribution
                .modifiers()
                .iter()
                .cloned()
                .map(|modifier| compiled_modifier_to_instance(source, modifier)),
        );
        Ok(())
    }

    pub fn remove_source_bundle(&mut self, source: Source) {
        self.state.remove_modifier_source(source);
    }

    pub fn set_external_immune_flags(&mut self, flags: ActorImmuneFlags) {
        self.state.set_external_immune_flags(flags);
    }

    pub fn recompute(&mut self) -> StatChangeSet {
        self.recompute_report().changes
    }

    fn recompute_report(&mut self) -> StatRecomputeReport {
        let old_computed = self.state.computed().clone();
        let explicit_changes = self.state.take_explicit_changes();

        let seeded = seed_rebuild_state(self.state, self.source);
        let derived = rebuild_derived_state(self.state, self.source, &seeded);

        self.state.replace_computed(derived);

        let previous_resources = self.state.resources();
        self.state.clamp_resources_to_computed_caps();
        let resource_changes = resource_changes(previous_resources, self.state.resources());

        let derived_immunity = derive_immune_flags(self.state);
        self.state.set_derived_immune_flags(derived_immunity);

        let explicit_state_changes = explicit_state_changes(explicit_changes);
        let final_computed_changes = diff_store(&old_computed, self.state.computed());
        let runtime_dirty = self.state.take_runtime_dirty();
        self.state.clear_dirty();

        StatRecomputeReport {
            changes: merge_changes([
                &final_computed_changes,
                &resource_changes,
                &explicit_state_changes,
            ]),
            runtime_dirty,
        }
    }

    pub fn sync(&mut self) -> StatsSync {
        let report = self.recompute_report();
        self.build_sync(report.changes, report.runtime_dirty)
    }

    pub fn bootstrap_sync(&mut self) -> BootstrapStatsSync {
        let report = self.recompute_report();
        self.build_full_sync(report.changes, report.runtime_dirty)
    }

    pub fn sync_if_dirty(&mut self) -> StatsSync {
        let (changes, runtime_dirty) = if self.state.is_dirty() {
            let report = self.recompute_report();
            (report.changes, report.runtime_dirty)
        } else {
            (StatChangeSet::default(), false)
        };

        self.build_sync(changes, runtime_dirty)
    }

    fn build_sync(&mut self, changes: StatChangeSet, runtime_dirty: bool) -> StatsSync {
        let character_update = if runtime_dirty || appearance_changed(&changes) {
            Some(self.character_update())
        } else {
            None
        };

        let stat_deltas = changes
            .iter()
            .map(|stat| {
                let value = read_stat_value(self.state, None, stat, StatValueView::WireCompatible);
                StatDelta { stat, value }
            })
            .collect();

        StatsSync {
            changes,
            stat_deltas,
            character_update,
        }
    }

    fn build_full_sync(
        &mut self,
        changes: StatChangeSet,
        _runtime_dirty: bool,
    ) -> BootstrapStatsSync {
        BootstrapStatsSync {
            changes,
            stat_snapshot: self.stat_snapshot(),
            character_update: self.character_update(),
        }
    }
}

fn validate_modifier_stat(stat: Stat) -> Result<(), SourceBundleError> {
    if matches!(stat, Stat::HpRecovery | Stat::SpRecovery) {
        return Err(SourceBundleError::RecoveryStatInModifierChannel { stat });
    }
    if !stat.accepts_source_contribution() {
        return Err(SourceBundleError::UnsupportedModifierStat { stat });
    }
    Ok(())
}

fn seed_rebuild_state<Source, Detail>(
    state: &ActorStatState<Source, Detail>,
    source: &ActorStatSource,
) -> PointValueStore
where
    Source: Ord + Copy,
{
    let mut seeded = state.base().clone();

    apply_source_seed(&mut seeded, source);

    for modifier in state.modifiers().iter() {
        if modifier.stat.accepts_source_contribution() {
            seeded.add(modifier.stat, modifier.amount);
        }
    }

    seeded
}

fn apply_source_seed(seeded: &mut PointValueStore, source: &ActorStatSource) {
    if let Some(mob) = source.mob() {
        seeded.add(Stat::Level, mob.level);
        seeded.add(Stat::St, mob.core.st);
        seeded.add(Stat::Ht, mob.core.ht);
        seeded.add(Stat::Dx, mob.core.dx);
        seeded.add(Stat::Iq, mob.core.iq);
    }
}

fn rebuild_derived_state<Source, Detail>(
    state: &ActorStatState<Source, Detail>,
    source: &ActorStatSource,
    seeded: &PointValueStore,
) -> PointValueStore
where
    Source: Ord + Copy,
{
    let mut computed = seeded.clone();

    match *source {
        ActorStatSource::Player(player) => rebuild_player_state(&mut computed, state, player),
        ActorStatSource::Mob(mob) => rebuild_mob_state(&mut computed, mob),
    }

    computed
}

fn rebuild_player_state<Source, Detail>(
    computed: &mut PointValueStore,
    state: &ActorStatState<Source, Detail>,
    player: PlayerStatSource,
) where
    Source: Ord + Copy,
{
    let lvl = computed.get(Stat::Level);
    let st = computed.get(Stat::St);
    let ht = computed.get(Stat::Ht);
    let dx = computed.get(Stat::Dx);
    let iq = computed.get(Stat::Iq);

    let flat_att_speed = computed.get(Stat::AttSpeed);
    let flat_att_grade = computed.get(Stat::AttGrade);
    let flat_def_grade = computed.get(Stat::DefGrade);
    let flat_mov_speed = computed.get(Stat::MovSpeed);
    let flat_casting_speed = computed.get(Stat::CastingSpeed);
    let flat_magic_att_grade = computed.get(Stat::MagicAttGrade);
    let flat_magic_def_grade = computed.get(Stat::MagicDefGrade);

    let (random_hp, random_sp, random_stamina) = state
        .stable_id()
        .map(|stable_id| {
            (
                player.growth.random_hp(stable_id, lvl),
                player.growth.random_sp(stable_id, lvl),
                player.growth.random_stamina(stable_id, lvl),
            )
        })
        .unwrap_or((0, 0, 0));

    set_player_resource_capacity(
        computed,
        player,
        PlayerResourceCapacity::Hp,
        player.resources.base_max_hp + random_hp + ht * player.resources.hp_per_ht,
    );

    set_player_resource_capacity(
        computed,
        player,
        PlayerResourceCapacity::Sp,
        player.resources.base_max_sp + random_sp + iq * player.resources.sp_per_iq,
    );

    set_player_resource_capacity(
        computed,
        player,
        PlayerResourceCapacity::Stamina,
        player.resources.base_max_stamina + random_stamina + ht * player.resources.stamina_per_ht,
    );

    computed.set(Stat::MovSpeed, flat_mov_speed + player.speeds.move_speed);
    computed.set(
        Stat::AttSpeed,
        flat_att_speed + player.speeds.attack_speed + computed.get(Stat::PartyHasteBonus),
    );
    computed.set(
        Stat::CastingSpeed,
        flat_casting_speed + player.speeds.casting_speed,
    );

    let stat_attack = (st * player.balance.attack.st_numerator
        + dx * player.balance.attack.dx_numerator
        + iq * player.balance.attack.iq_numerator)
        / player.balance.attack.divisor.max(1);

    computed.set(
        Stat::AttGrade,
        flat_att_grade + lvl * 2 + stat_attack + computed.get(Stat::AttGradeBonus),
    );

    let physical_defence_bonus = computed.get(Stat::ArmorDefence)
        + computed.get(Stat::DefGradeBonus)
        + computed.get(Stat::PartyDefenderBonus);
    computed.set(
        Stat::DefGrade,
        flat_def_grade + lvl + (ht * 4) / 5 + physical_defence_bonus,
    );

    computed.set(
        Stat::DisplayedDefGrade,
        flat_def_grade + lvl + ht + physical_defence_bonus,
    );

    computed.set(
        Stat::MagicAttGrade,
        flat_magic_att_grade + lvl * 2 + iq * 2 + computed.get(Stat::MagicAttGradeBonus),
    );

    computed.set(
        Stat::MagicDefGrade,
        flat_magic_def_grade
            + lvl
            + (iq * 3 + ht) / 3
            + physical_defence_bonus / 2
            + computed.get(Stat::MagicDefGradeBonus),
    );
}

fn rebuild_mob_state(computed: &mut PointValueStore, mob: super::source::MobStatSource) {
    let lvl = computed.get(Stat::Level);
    let st = computed.get(Stat::St);
    let ht = computed.get(Stat::Ht);

    let flat_att_speed = computed.get(Stat::AttSpeed);
    let flat_att_grade = computed.get(Stat::AttGrade);
    let flat_def_grade = computed.get(Stat::DefGrade);
    let flat_mov_speed = computed.get(Stat::MovSpeed);
    let flat_casting_speed = computed.get(Stat::CastingSpeed);
    let flat_magic_att_grade = computed.get(Stat::MagicAttGrade);
    let flat_magic_def_grade = computed.get(Stat::MagicDefGrade);

    computed.set(Stat::MaxHp, computed.get(Stat::BonusMaxHp) + mob.max_hp);
    let att_grade = flat_att_grade + lvl * 2 + st * 2;
    computed.set(Stat::AttGrade, att_grade);

    let def_grade = flat_def_grade + lvl + ht + mob.def_grade_flat;
    computed.set(Stat::DefGrade, def_grade);
    computed.set(Stat::DisplayedDefGrade, def_grade);
    computed.set(Stat::MagicAttGrade, flat_magic_att_grade + att_grade);
    computed.set(Stat::MagicDefGrade, flat_magic_def_grade + def_grade);
    computed.set(Stat::AttSpeed, flat_att_speed + mob.speeds.attack_speed);
    computed.set(Stat::MovSpeed, flat_mov_speed + mob.speeds.move_speed);
    computed.set(
        Stat::CastingSpeed,
        flat_casting_speed + mob.speeds.casting_speed,
    );
}

fn set_player_resource_capacity(
    computed: &mut PointValueStore,
    player: PlayerStatSource,
    resource: PlayerResourceCapacity,
    base: i32,
) {
    let pre_percentage_base = base
        + optional_stat_value(computed, resource.flat_bonus_stat())
        + optional_stat_value(computed, resource.pre_percentage_bonus_stat());
    let percentage_bonus = resource
        .capped_percentage_stat()
        .map(|stat| {
            capped_percentage_bonus(
                pre_percentage_base,
                computed.get(stat),
                player
                    .balance
                    .capped_percentage_bonus_max(resource)
                    .unwrap_or(i32::MAX),
            )
        })
        .unwrap_or_default();
    let post_percentage_bonus =
        optional_stat_value(computed, resource.post_percentage_bonus_stat());

    computed.set(
        resource.cap_stat(),
        pre_percentage_base + percentage_bonus + post_percentage_bonus,
    );
}

fn optional_stat_value(computed: &PointValueStore, stat: Option<Stat>) -> i32 {
    stat.map(|stat| computed.get(stat)).unwrap_or_default()
}

fn derive_immune_flags<Source, Detail>(state: &ActorStatState<Source, Detail>) -> ActorImmuneFlags
where
    Source: Ord + Copy,
{
    state.runtime().external_immune_flags.or(ActorImmuneFlags {
        stun: state.computed().get(Stat::ImmuneStun) > 0,
        slow: state.computed().get(Stat::ImmuneSlow) > 0,
        fall: state.computed().get(Stat::ImmuneFall) > 0,
    })
}

fn percentage_bonus(value: i32, pct: i32) -> i32 {
    (value * pct) / 100
}

fn capped_percentage_bonus(value: i32, pct: i32, max_bonus: i32) -> i32 {
    percentage_bonus(value, pct).min(max_bonus)
}

fn compiled_modifier_to_instance<Source, Detail>(
    source: Source,
    modifier: CompiledModifier<Detail>,
) -> StatModifierInstance<Source, Detail>
where
    Source: Ord + Copy,
{
    StatModifierInstance {
        source,
        stat: modifier.stat,
        amount: modifier.amount,
        detail: modifier.detail,
    }
}

fn appearance_changed(changes: &StatChangeSet) -> bool {
    changes.contains(Stat::Level)
        || changes.contains(Stat::MovSpeed)
        || changes.contains(Stat::AttSpeed)
}

fn saturating_u8(value: i32) -> u8 {
    value.clamp(0, u8::MAX as i32) as u8
}

fn clamp_optional(value: i32, min: Option<i32>, max: Option<i32>) -> i32 {
    let lower = min.unwrap_or(i32::MIN);
    let upper = max.unwrap_or(i32::MAX);
    value.clamp(lower, upper)
}

fn validate_stored_write_limit(
    source: &ActorStatSource,
    stat: Stat,
    value: i32,
) -> Result<(), StatWriteError> {
    let Some(max) = source.stored_write_limit(stat) else {
        return Ok(());
    };

    if clamp_optional(value, Some(0), Some(max)) == value {
        return Ok(());
    }

    Err(StatWriteError::OutOfRange {
        stat,
        value,
        min: Some(0),
        max,
    })
}

fn current_resource_cap<Source, Detail>(
    state: &ActorStatState<Source, Detail>,
    stat: Stat,
) -> Result<i32, StatWriteError>
where
    Source: Ord + Copy,
{
    match stat {
        Stat::Hp => Ok(state.computed().get(Stat::MaxHp)),
        Stat::Sp => Ok(state.computed().get(Stat::MaxSp)),
        Stat::Stamina => Ok(state.computed().get(Stat::MaxStamina)),
        _ => Err(StatWriteError::NotResource { stat }),
    }
}

fn current_resource_value<Source, Detail>(
    state: &ActorStatState<Source, Detail>,
    stat: Stat,
) -> Result<i32, StatWriteError>
where
    Source: Ord + Copy,
{
    match stat {
        Stat::Hp => Ok(state.resources().hp),
        Stat::Sp => Ok(state.resources().sp),
        Stat::Stamina => Ok(state.resources().stamina),
        Stat::HpRecovery => Ok(state.resources().hp_recovery),
        Stat::SpRecovery => Ok(state.resources().sp_recovery),
        _ => Err(StatWriteError::NotResource { stat }),
    }
}

fn recovery_bucket_for_resource(stat: Stat) -> Result<Stat, StatWriteError> {
    match stat {
        Stat::Hp => Ok(Stat::HpRecovery),
        Stat::Sp => Ok(Stat::SpRecovery),
        _ => Err(StatWriteError::NotResource { stat }),
    }
}

fn insert_resource_changes(
    previous: ActorResources,
    current: ActorResources,
    changes: &mut StatChangeSet,
) {
    if previous.hp != current.hp {
        changes.insert(Stat::Hp);
    }
    if previous.sp != current.sp {
        changes.insert(Stat::Sp);
    }
    if previous.stamina != current.stamina {
        changes.insert(Stat::Stamina);
    }
}

fn explicit_state_changes(stats: impl IntoIterator<Item = Stat>) -> StatChangeSet {
    let mut changes = StatChangeSet::default();
    for stat in stats {
        changes.insert(stat);
    }
    changes
}

fn diff_store(old: &PointValueStore, new: &PointValueStore) -> StatChangeSet {
    let mut changes = StatChangeSet::default();
    for stat in old.iter().map(|(stat, _)| stat) {
        if old.get(stat) != new.get(stat) {
            changes.insert(stat);
        }
    }
    for stat in new.iter().map(|(stat, _)| stat) {
        if old.get(stat) != new.get(stat) {
            changes.insert(stat);
        }
    }
    changes
}

fn resource_changes(old: ActorResources, new: ActorResources) -> StatChangeSet {
    let mut changes = StatChangeSet::default();
    insert_resource_changes(old, new, &mut changes);
    changes
}

fn merge_changes<'a>(phase_changes: impl IntoIterator<Item = &'a StatChangeSet>) -> StatChangeSet {
    let mut changes = StatChangeSet::default();
    for phase in phase_changes {
        for stat in phase.iter() {
            changes.insert(stat);
        }
    }
    changes
}
