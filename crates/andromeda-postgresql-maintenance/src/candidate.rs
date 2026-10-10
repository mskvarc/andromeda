//! The closed set of relations that maintenance may inspect or mutate.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    notification::{NotificationDeliveries, NotificationEvents},
    queue::{PendingSideEffects, TemporalAppendIntents},
    snapshot::SnapshotNotificationOutbox,
    subscriptions::SubscriptionDeliveryState,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceSchema, MaintenanceStatement, reindex_index_concurrently, vacuum_non_truncating};
use sea_query::Iden;
use strum::{Display, EnumString, IntoStaticStr};

/// An allowlisted `current_attributes` index.
///
/// `idx_ca_value_long_text` and `idx_ca_value_long_uri` are left out: both are
/// keyed on the attribute key alone, so a value update leaves their key
/// unchanged and bottom-up index deletion removes the version churn as it
/// happens.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, Iden, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum IndexTarget {
    /// Natural entity/attribute/dataset grain.
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
    /// Sub-attribute equality key GIN, measured by its density.
    IdxCaSubEqKeys,
    /// JSON object containment GIN, measured by its density.
    IdxCaValueObject,
    /// Attribute-aware long-string trigram GIN, measured by its density.
    IdxCaValueLongTextTrgm,
}

/// The nullable column whose non-null rows a density-measured GIN covers: each
/// such GIN is partial on that column being non-null.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum DensityColumn {
    /// `sub_attributes`, covered by `idx_ca_sub_eq_keys`.
    SubAttributes,
    /// `value_object`, covered by `idx_ca_value_object`.
    ValueObject,
    /// `value_long_text`, covered by `idx_ca_value_long_text_trgm`.
    ValueLongText,
}

impl DensityColumn {
    /// Every column a density-measured GIN covers.
    pub const ALL: [DensityColumn; 3] = [DensityColumn::SubAttributes, DensityColumn::ValueObject, DensityColumn::ValueLongText];

    /// The GIN that covers this column's non-null rows.
    #[must_use]
    pub const fn gin(self) -> IndexTarget {
        match self {
            DensityColumn::SubAttributes => IndexTarget::IdxCaSubEqKeys,
            DensityColumn::ValueObject => IndexTarget::IdxCaValueObject,
            DensityColumn::ValueLongText => IndexTarget::IdxCaValueLongTextTrgm,
        }
    }
}

impl IndexTarget {
    /// Every index that may be rebuilt or recognized during recovery.
    pub const ALL: [IndexTarget; 12] = [
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
        IndexTarget::IdxCaValueObject,
        IndexTarget::IdxCaValueLongTextTrgm,
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
    /// `pgstattuple` reads no GIN index, so a GIN is never probed itself. A
    /// canary stands in for it only when every change that bloats the GIN also
    /// leaves a dead canary entry that only vacuum removes: the string B-tree's
    /// key contains `value_string`, and it covers exactly the rows the trigram
    /// GIN covers. Every other GIN is measured by its density.
    #[must_use]
    pub const fn paired_gin(self) -> Option<IndexTarget> {
        match self {
            IndexTarget::IdxCaValueString => Some(IndexTarget::IdxCaValueStringTrgm),
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueStringTrgm
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry
            | IndexTarget::IdxCaSubEqKeys
            | IndexTarget::IdxCaValueObject
            | IndexTarget::IdxCaValueLongTextTrgm => None,
        }
    }

    /// The probeable B-tree whose qualification queues this GIN index, the
    /// inverse of [`IndexTarget::paired_gin`].
    #[must_use]
    pub const fn canary(self) -> Option<IndexTarget> {
        match self {
            IndexTarget::IdxCaValueStringTrgm => Some(IndexTarget::IdxCaValueString),
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueString
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry
            | IndexTarget::IdxCaSubEqKeys
            | IndexTarget::IdxCaValueObject
            | IndexTarget::IdxCaValueLongTextTrgm => None,
        }
    }

    /// The column whose non-null rows this GIN covers, `Some` exactly for a
    /// GIN measured by its density rather than through a canary.
    #[must_use]
    pub const fn density_column(self) -> Option<DensityColumn> {
        match self {
            IndexTarget::IdxCaSubEqKeys => Some(DensityColumn::SubAttributes),
            IndexTarget::IdxCaValueObject => Some(DensityColumn::ValueObject),
            IndexTarget::IdxCaValueLongTextTrgm => Some(DensityColumn::ValueLongText),
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueString
            | IndexTarget::IdxCaValueStringTrgm
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry => None,
        }
    }

    /// The heap the index belongs to, whose load and lock gates apply to its
    /// rebuild.
    #[must_use]
    pub const fn heap(self) -> HeapTarget {
        match self {
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueString
            | IndexTarget::IdxCaValueStringTrgm
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry
            | IndexTarget::IdxCaSubEqKeys
            | IndexTarget::IdxCaValueObject
            | IndexTarget::IdxCaValueLongTextTrgm => HeapTarget::CurrentAttributes,
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
            IndexTarget::IdxCaValueObject => "idx_ca_value_object",
            IndexTarget::IdxCaValueLongTextTrgm => "idx_ca_value_long_text_trgm",
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
    /// Notification events awaiting fan-out to their deliveries.
    NotificationEvents,
    /// Notification deliveries, leased by every claim.
    NotificationDeliveries,
    /// Write side effects awaiting their retry.
    PendingSideEffects,
    /// Temporal history appends awaiting their retry.
    TemporalAppendIntents,
    /// Snapshot notifications awaiting delivery, leased by every claim.
    SnapshotNotificationOutbox,
}

impl HeapTarget {
    /// Every heap relation eligible for cheap sampling and catch-up vacuum.
    pub const ALL: [HeapTarget; 8] = [
        HeapTarget::CurrentAttributes,
        HeapTarget::Entities,
        HeapTarget::SubscriptionDeliveryState,
        HeapTarget::NotificationEvents,
        HeapTarget::NotificationDeliveries,
        HeapTarget::PendingSideEffects,
        HeapTarget::TemporalAppendIntents,
        HeapTarget::SnapshotNotificationOutbox,
    ];

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            HeapTarget::CurrentAttributes => "current_attributes",
            HeapTarget::Entities => "entities",
            HeapTarget::SubscriptionDeliveryState => "subscription_delivery_state",
            HeapTarget::NotificationEvents => "notification_events",
            HeapTarget::NotificationDeliveries => "notification_deliveries",
            HeapTarget::PendingSideEffects => "pending_side_effects",
            HeapTarget::TemporalAppendIntents => "temporal_append_intents",
            HeapTarget::SnapshotNotificationOutbox => "snapshot_notification_outbox",
        }
    }

    /// A fixed non-truncating, non-parallel ordinary vacuum statement.
    #[must_use]
    pub fn vacuum_statement(self) -> MaintenanceStatement {
        match self {
            HeapTarget::CurrentAttributes => vacuum_non_truncating(&MaintenanceSchema::Public, &CurrentAttributes::Table),
            HeapTarget::Entities => vacuum_non_truncating(&MaintenanceSchema::Public, &Entities::Table),
            HeapTarget::SubscriptionDeliveryState => vacuum_non_truncating(&MaintenanceSchema::Public, &SubscriptionDeliveryState::Table),
            HeapTarget::NotificationEvents => vacuum_non_truncating(&MaintenanceSchema::Public, &NotificationEvents::Table),
            HeapTarget::NotificationDeliveries => vacuum_non_truncating(&MaintenanceSchema::Public, &NotificationDeliveries::Table),
            HeapTarget::PendingSideEffects => vacuum_non_truncating(&MaintenanceSchema::Public, &PendingSideEffects::Table),
            HeapTarget::TemporalAppendIntents => vacuum_non_truncating(&MaintenanceSchema::Public, &TemporalAppendIntents::Table),
            HeapTarget::SnapshotNotificationOutbox => vacuum_non_truncating(&MaintenanceSchema::Public, &SnapshotNotificationOutbox::Table),
        }
    }
}

/// A persistable allowlisted maintenance target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaintenanceTarget {
    /// One allowlisted index.
    Index(IndexTarget),
    /// One hot or queue heap.
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

    /// The heap whose load and lock gates apply to the target: the target
    /// itself, or the heap an index belongs to.
    #[must_use]
    pub const fn heap(self) -> HeapTarget {
        match self {
            MaintenanceTarget::Index(target) => target.heap(),
            MaintenanceTarget::Heap(target) => target,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::candidate::{DensityColumn, HeapTarget, IndexTarget, MaintenanceTarget};
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, str::FromStr};

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
    fn every_index_target_names_the_heap_that_owns_it() {
        let expected = [
            (IndexTarget::UqCurrentAttributesGrain, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueNumber, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueString, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueStringTrgm, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueBoolean, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueDateTime, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueUri, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueGeometry, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaEntityGeometry, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaSubEqKeys, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueObject, HeapTarget::CurrentAttributes),
            (IndexTarget::IdxCaValueLongTextTrgm, HeapTarget::CurrentAttributes),
        ];
        assert_eq!(IndexTarget::ALL.map(|target| (target, target.heap())), expected);
        for (index, heap) in expected {
            assert_eq!(MaintenanceTarget::Index(index).heap(), heap, "{index}");
        }
        for heap in HeapTarget::ALL {
            assert_eq!(MaintenanceTarget::Heap(heap).heap(), heap);
        }
    }

    /// `pgstattuple` reads no GIN index.
    #[test]
    fn no_gin_index_target_is_probed_directly() {
        for target in [
            IndexTarget::IdxCaValueStringTrgm,
            IndexTarget::IdxCaSubEqKeys,
            IndexTarget::IdxCaValueObject,
            IndexTarget::IdxCaValueLongTextTrgm,
        ] {
            assert!(!IndexTarget::PROBEABLE.contains(&target), "{target}");
        }
    }

    /// An index outside the probe set is a GIN, never measured itself; without
    /// exactly one signal it would bloat unattended, or be queued twice.
    #[test]
    fn every_gin_target_is_measured_by_canary_or_density() {
        for target in IndexTarget::ALL.into_iter().filter(|target| !IndexTarget::PROBEABLE.contains(target)) {
            let by_canary = target
                .canary()
                .is_some_and(|canary| IndexTarget::PROBEABLE.contains(&canary) && canary.paired_gin() == Some(target));
            let by_density = target.density_column().is_some_and(|column| column.gin() == target);
            assert!(by_canary != by_density, "{target}: canary {by_canary}, density {by_density}");
        }
        for target in IndexTarget::PROBEABLE {
            assert_eq!(target.density_column(), None, "{target} is probed");
        }
    }

    #[test]
    fn only_the_string_pair_uses_a_canary() {
        for target in IndexTarget::ALL {
            let expected_gin = (target == IndexTarget::IdxCaValueString).then_some(IndexTarget::IdxCaValueStringTrgm);
            let expected_canary = (target == IndexTarget::IdxCaValueStringTrgm).then_some(IndexTarget::IdxCaValueString);
            assert_eq!(target.paired_gin(), expected_gin, "{target}");
            assert_eq!(target.canary(), expected_canary, "{target}");
        }
    }

    #[test]
    fn each_density_column_names_the_gin_partial_on_it() {
        assert_eq!(
            DensityColumn::ALL.map(|column| (<&str>::from(column), column.gin().relation_name())),
            [
                ("sub_attributes", "idx_ca_sub_eq_keys"),
                ("value_object", "idx_ca_value_object"),
                ("value_long_text", "idx_ca_value_long_text_trgm"),
            ]
        );
        for column in DensityColumn::ALL {
            assert_eq!(column.gin().density_column(), Some(column), "{column}");
        }
    }

    #[test]
    fn long_text_and_long_uri_btrees_are_not_targets() {
        for name in ["idx_ca_value_long_text", "idx_ca_value_long_uri"] {
            assert!(IndexTarget::from_str(name).is_err(), "{name}");
        }
        assert_eq!(IndexTarget::from_str("idx_ca_value_long_text_trgm"), Ok(IndexTarget::IdxCaValueLongTextTrgm));
        assert_eq!(IndexTarget::from_str("idx_ca_value_object"), Ok(IndexTarget::IdxCaValueObject));
    }

    #[test]
    fn the_heap_targets_are_the_hot_heaps_and_the_queues() {
        assert_eq!(
            HeapTarget::ALL.map(HeapTarget::relation_name),
            [
                "current_attributes",
                "entities",
                "subscription_delivery_state",
                "notification_events",
                "notification_deliveries",
                "pending_side_effects",
                "temporal_append_intents",
                "snapshot_notification_outbox",
            ]
        );
    }

    #[test]
    fn every_queue_heap_has_a_fixed_non_truncating_vacuum() {
        for (heap, table) in [
            (HeapTarget::NotificationEvents, "notification_events"),
            (HeapTarget::NotificationDeliveries, "notification_deliveries"),
            (HeapTarget::PendingSideEffects, "pending_side_effects"),
            (HeapTarget::TemporalAppendIntents, "temporal_append_intents"),
            (HeapTarget::SnapshotNotificationOutbox, "snapshot_notification_outbox"),
        ] {
            let statement = heap.vacuum_statement();
            assert_eq!(
                statement.as_sql(),
                format!("VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\".\"{table}\""),
                "{heap}"
            );
        }
    }
}
