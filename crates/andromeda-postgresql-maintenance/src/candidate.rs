//! The closed set of relations that maintenance may inspect or mutate.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    subscriptions::SubscriptionDeliveryState,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceSchema, MaintenanceStatement, reindex_index_concurrently, vacuum_non_truncating};
use sea_query::Iden;
use strum::{Display, EnumString, IntoStaticStr};

/// An allowlisted `current_attributes` index.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, Iden, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum IndexTarget {
    /// Natural entity/attribute/dataset grain, and bloat canary for the paired
    /// sub-attribute key rebuild.
    UqCurrentAttributesGrain,
    /// Numeric predicate index.
    IdxCaValueNumber,
    /// String B-tree and bloat canary for the paired trigram rebuild.
    IdxCaValueString,
    /// Attribute-aware string trigram GIN, queued only through its canary.
    IdxCaValueStringTrgm,
    /// Boolean predicate index.
    IdxCaValueBoolean,
    /// Date-time predicate index.
    IdxCaValueDateTime,
    /// URI predicate index.
    IdxCaValueUri,
    /// Attribute-aware geometry `GiST`.
    IdxCaValueGeometry,
    /// Entity/attribute geometry correlation index.
    IdxCaEntityGeometry,
    /// Sub-attribute equality key GIN, queued only through its canary.
    IdxCaSubEqKeys,
}

impl IndexTarget {
    /// Every index that may be rebuilt or recognized during recovery.
    pub const ALL: [IndexTarget; 10] = [
        IndexTarget::UqCurrentAttributesGrain,
        IndexTarget::IdxCaValueNumber,
        IndexTarget::IdxCaValueString,
        IndexTarget::IdxCaValueStringTrgm,
        IndexTarget::IdxCaValueBoolean,
        IndexTarget::IdxCaValueDateTime,
        IndexTarget::IdxCaValueUri,
        IndexTarget::IdxCaValueGeometry,
        IndexTarget::IdxCaEntityGeometry,
        IndexTarget::IdxCaSubEqKeys,
    ];

    /// Indexes eligible for direct physical bloat probing.
    pub const PROBEABLE: [IndexTarget; 8] = [
        IndexTarget::UqCurrentAttributesGrain,
        IndexTarget::IdxCaValueNumber,
        IndexTarget::IdxCaValueString,
        IndexTarget::IdxCaValueBoolean,
        IndexTarget::IdxCaValueDateTime,
        IndexTarget::IdxCaValueUri,
        IndexTarget::IdxCaValueGeometry,
        IndexTarget::IdxCaEntityGeometry,
    ];

    /// The GIN index queued together with this probeable B-tree, its bloat
    /// canary.
    ///
    /// `pgstattuple` reads no GIN index, so a GIN is never measured itself: the
    /// B-tree over the rows whose churn bloats it stands in for it. The string
    /// B-tree covers exactly the rows the trigram GIN covers. The grain covers
    /// every row the sub-attribute key GIN covers, and a row whose
    /// `sub_attributes` change is never a heap-only update (the key expression
    /// reads the column), so each such change also leaves a dead grain entry.
    #[must_use]
    pub const fn paired_gin(self) -> Option<IndexTarget> {
        match self {
            IndexTarget::IdxCaValueString => Some(IndexTarget::IdxCaValueStringTrgm),
            IndexTarget::UqCurrentAttributesGrain => Some(IndexTarget::IdxCaSubEqKeys),
            IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueStringTrgm
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry
            | IndexTarget::IdxCaSubEqKeys => None,
        }
    }

    /// The probeable B-tree whose qualification queues this GIN index, the
    /// inverse of [`IndexTarget::paired_gin`].
    #[must_use]
    pub const fn canary(self) -> Option<IndexTarget> {
        match self {
            IndexTarget::IdxCaValueStringTrgm => Some(IndexTarget::IdxCaValueString),
            IndexTarget::IdxCaSubEqKeys => Some(IndexTarget::UqCurrentAttributesGrain),
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueString
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry => None,
        }
    }

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            IndexTarget::UqCurrentAttributesGrain => "uq_current_attributes_grain",
            IndexTarget::IdxCaValueNumber => "idx_ca_value_number",
            IndexTarget::IdxCaValueString => "idx_ca_value_string",
            IndexTarget::IdxCaValueStringTrgm => "idx_ca_value_string_trgm",
            IndexTarget::IdxCaValueBoolean => "idx_ca_value_boolean",
            IndexTarget::IdxCaValueDateTime => "idx_ca_value_date_time",
            IndexTarget::IdxCaValueUri => "idx_ca_value_uri",
            IndexTarget::IdxCaValueGeometry => "idx_ca_value_geometry",
            IndexTarget::IdxCaEntityGeometry => "idx_ca_entity_geometry",
            IndexTarget::IdxCaSubEqKeys => "idx_ca_sub_eq_keys",
        }
    }

    /// A fixed concurrent rebuild statement; no external identifier enters SQL.
    #[must_use]
    pub fn reindex_statement(self) -> MaintenanceStatement {
        reindex_index_concurrently(&MaintenanceSchema::Public, &self)
    }
}

/// A heap relation eligible for catch-up ordinary vacuum.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HeapTarget {
    /// Current attribute instances.
    CurrentAttributes,
    /// Current entities, whose version token every write updates.
    Entities,
    /// Frequently updated subscription delivery counters.
    SubscriptionDeliveryState,
}

impl HeapTarget {
    /// Every heap relation eligible for cheap sampling and catch-up vacuum.
    pub const ALL: [HeapTarget; 3] = [HeapTarget::CurrentAttributes, HeapTarget::Entities, HeapTarget::SubscriptionDeliveryState];

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            HeapTarget::CurrentAttributes => "current_attributes",
            HeapTarget::Entities => "entities",
            HeapTarget::SubscriptionDeliveryState => "subscription_delivery_state",
        }
    }

    /// A fixed non-truncating, non-parallel ordinary vacuum statement.
    #[must_use]
    pub fn vacuum_statement(self) -> MaintenanceStatement {
        match self {
            HeapTarget::CurrentAttributes => vacuum_non_truncating(&MaintenanceSchema::Public, &CurrentAttributes::Table),
            HeapTarget::Entities => vacuum_non_truncating(&MaintenanceSchema::Public, &Entities::Table),
            HeapTarget::SubscriptionDeliveryState => vacuum_non_truncating(&MaintenanceSchema::Public, &SubscriptionDeliveryState::Table),
        }
    }
}

/// A persistable allowlisted maintenance target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaintenanceTarget {
    /// One `current_attributes` index.
    Index(IndexTarget),
    /// One hot heap.
    Heap(HeapTarget),
}

impl MaintenanceTarget {
    /// The exact database identifier of the target, a bounded-cardinality
    /// label for logs.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            MaintenanceTarget::Index(target) => target.relation_name(),
            MaintenanceTarget::Heap(target) => target.relation_name(),
        }
    }

    /// Parent heap whose load and lock gates apply to the target.
    #[must_use]
    pub const fn heap_name(self) -> &'static str {
        match self {
            MaintenanceTarget::Index(
                IndexTarget::UqCurrentAttributesGrain
                | IndexTarget::IdxCaValueNumber
                | IndexTarget::IdxCaValueString
                | IndexTarget::IdxCaValueStringTrgm
                | IndexTarget::IdxCaValueBoolean
                | IndexTarget::IdxCaValueDateTime
                | IndexTarget::IdxCaValueUri
                | IndexTarget::IdxCaValueGeometry
                | IndexTarget::IdxCaEntityGeometry
                | IndexTarget::IdxCaSubEqKeys,
            ) => "current_attributes",
            MaintenanceTarget::Heap(target) => target.relation_name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::candidate::{HeapTarget, IndexTarget, MaintenanceTarget};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn every_allowlisted_index_has_one_fixed_concurrent_command() {
        let commands = IndexTarget::ALL.map(|target| target.reindex_statement().as_sql().to_owned());
        assert!(commands.iter().all(|command| command.starts_with("REINDEX INDEX CONCURRENTLY \"public\".")));
        assert_eq!(commands.into_iter().collect::<HashSet<_>>().len(), IndexTarget::ALL.len());
    }

    #[test]
    fn every_heap_command_disables_truncation_and_parallel_workers() {
        for target in HeapTarget::ALL {
            let statement = target.vacuum_statement();
            let command = statement.as_sql();
            assert!(command.starts_with("VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\"."), "{command}");
        }
    }

    #[test]
    fn every_index_target_is_a_current_attributes_index_and_the_key_index_is_one() {
        for target in IndexTarget::ALL {
            let name = target.relation_name();
            assert!(name.starts_with("idx_ca_") || name == "uq_current_attributes_grain", "{name}");
            assert_eq!(MaintenanceTarget::Index(target).heap_name(), "current_attributes");
        }
        assert!(IndexTarget::ALL.contains(&IndexTarget::IdxCaSubEqKeys));
        assert_eq!(IndexTarget::IdxCaSubEqKeys.relation_name(), "idx_ca_sub_eq_keys");
    }

    /// `pgstattuple` reads no GIN index.
    #[test]
    fn no_gin_index_target_is_probed_directly() {
        for target in [IndexTarget::IdxCaValueStringTrgm, IndexTarget::IdxCaSubEqKeys] {
            assert!(!IndexTarget::PROBEABLE.contains(&target), "{target}");
        }
    }

    /// An index outside the probe set is never measured, so only a canary can
    /// queue it; without one it would bloat unattended.
    #[test]
    fn every_index_no_probe_measures_is_queued_through_a_probeable_canary() {
        for target in IndexTarget::ALL.into_iter().filter(|target| !IndexTarget::PROBEABLE.contains(target)) {
            assert_eq!(
                target.canary().map(|canary| (IndexTarget::PROBEABLE.contains(&canary), canary.paired_gin())),
                Some((true, Some(target))),
                "{target} is queued by a probed canary pairing it"
            );
        }
    }

    #[test]
    fn the_grain_is_the_canary_of_the_sub_attribute_key_gin() {
        assert_eq!(IndexTarget::UqCurrentAttributesGrain.paired_gin(), Some(IndexTarget::IdxCaSubEqKeys));
        assert_eq!(IndexTarget::IdxCaSubEqKeys.canary(), Some(IndexTarget::UqCurrentAttributesGrain));
        assert_eq!(IndexTarget::IdxCaValueString.paired_gin(), Some(IndexTarget::IdxCaValueStringTrgm));
    }

    #[test]
    fn a_canary_and_its_gin_name_each_other_and_no_other_index_is_paired() {
        for target in IndexTarget::ALL {
            if let Some(gin) = target.paired_gin() {
                assert_eq!(gin.canary(), Some(target), "{target}");
            }
            if let Some(canary) = target.canary() {
                assert_eq!(canary.paired_gin(), Some(target), "{target}");
                assert_eq!(target.paired_gin(), None, "{target} is a GIN, never a canary");
            }
        }
    }

    #[test]
    fn the_heap_targets_are_the_attribute_rows_the_entities_and_the_delivery_state() {
        assert_eq!(
            HeapTarget::ALL.map(HeapTarget::relation_name),
            ["current_attributes", "entities", "subscription_delivery_state"]
        );
    }
}
