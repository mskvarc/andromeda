//! The allowlisted indexes and the columns their density-measured GINs cover.

use crate::{branch::Branch, candidate::heap_target::HeapTarget};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceCommand, reindex_index_concurrently};
use sea_query::Iden;
use strum::{Display, EnumString, IntoStaticStr};

/// An allowlisted index of a hot or queue heap.
///
/// `idx_ca_value_long_text` and `idx_ca_value_long_uri` are left out: both are
/// keyed on the attribute key alone, so a value update leaves their key
/// unchanged and bottom-up index deletion removes the version churn as it
/// happens. Of the queue indexes, only those of `notification_deliveries` that
/// every delivery attempt writes are targets: an attempt leases the row through
/// the claim expression, so it is never a heap-only update and adds an entry
/// to each of them. An index a row enters once, and one on a table a claim
/// does not update, frees space only as fast as the queue's retention reaps
/// rows, and new entries take that space back. Every index of the temporal
/// stream catalogue is a target: a write that grows a stream's value extent or
/// shape mask changes an indexed column, so it is never a heap-only update and
/// adds an entry to each of them.
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
    /// Deliveries still in the queue in claim order, partial on undelivered
    /// and unexhausted rows; emptied whenever the queue drains.
    IdxNotificationDeliveriesClaim,
    /// Deliveries by their notification event.
    IdxNotificationDeliveriesEvent,
    /// Deliveries by their subscription.
    IdxNotificationDeliveriesSubscription,
    /// The deliveries' primary key.
    PkNotificationDeliveries,
    /// Identifier trigram GIN, measured by its density: a pending list
    /// flushed into its posting trees leaves their pages part full.
    IdxEntitiesIdTrgm,
    /// The stream catalogue's primary key.
    PkTemporalAttributeStreams,
    /// The stream catalogue's grain constraint index.
    UqTemporalAttributeStreamsGrain,
    /// The stream catalogue's attribute-led index.
    IdxTemporalAttributeStreamsAttributeNameKeyEntityKey,
    /// The stream catalogue's value extent `GiST`.
    IdxTemporalAttributeStreamsValueExtent,
    /// The stream catalogue's shape mask index.
    IdxTemporalAttributeStreamsAttributeNameKeyShapeMask,
}

/// The column whose non-null rows a density-measured GIN covers: each such GIN
/// is partial on that column being non-null, or the column is never null and
/// the GIN covers every row of its table.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum DensityColumn {
    /// `sub_eq_keys`, covered by `idx_ca_sub_eq_keys`.
    SubEqKeys,
    /// `value_object`, covered by `idx_ca_value_object`.
    ValueObject,
    /// `value_long_text`, covered by `idx_ca_value_long_text_trgm`.
    ValueLongText,
    /// `entities.id`, covered by `idx_entities_id_trgm`; never null, so every
    /// row of `entities` is covered.
    Id,
}

impl DensityColumn {
    /// Every column a density-measured GIN covers.
    pub const ALL: [DensityColumn; 4] = [
        DensityColumn::SubEqKeys,
        DensityColumn::ValueObject,
        DensityColumn::ValueLongText,
        DensityColumn::Id,
    ];

    /// The GIN that covers this column's non-null rows.
    #[must_use]
    pub const fn gin(self) -> IndexTarget {
        match self {
            DensityColumn::SubEqKeys => IndexTarget::IdxCaSubEqKeys,
            DensityColumn::ValueObject => IndexTarget::IdxCaValueObject,
            DensityColumn::ValueLongText => IndexTarget::IdxCaValueLongTextTrgm,
            DensityColumn::Id => IndexTarget::IdxEntitiesIdTrgm,
        }
    }
}

impl IndexTarget {
    /// Every index that may be rebuilt or recognized during recovery.
    pub const ALL: [IndexTarget; 22] = [
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
        IndexTarget::IdxNotificationDeliveriesClaim,
        IndexTarget::IdxNotificationDeliveriesEvent,
        IndexTarget::IdxNotificationDeliveriesSubscription,
        IndexTarget::PkNotificationDeliveries,
        IndexTarget::IdxEntitiesIdTrgm,
        IndexTarget::PkTemporalAttributeStreams,
        IndexTarget::UqTemporalAttributeStreamsGrain,
        IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey,
        IndexTarget::IdxTemporalAttributeStreamsValueExtent,
        IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask,
    ];

    /// Indexes eligible for direct physical bloat probing.
    pub const PROBEABLE: [IndexTarget; 17] = [
        IndexTarget::UqCurrentAttributesGrain,
        IndexTarget::IdxCaValueNumber,
        IndexTarget::IdxCaValueString,
        IndexTarget::IdxCaValueBoolean,
        IndexTarget::IdxCaValueDateTime,
        IndexTarget::IdxCaValueUri,
        IndexTarget::IdxCaValueGeometry,
        IndexTarget::IdxCaEntityGeometry,
        IndexTarget::IdxNotificationDeliveriesClaim,
        IndexTarget::IdxNotificationDeliveriesEvent,
        IndexTarget::IdxNotificationDeliveriesSubscription,
        IndexTarget::PkNotificationDeliveries,
        IndexTarget::PkTemporalAttributeStreams,
        IndexTarget::UqTemporalAttributeStreamsGrain,
        IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey,
        IndexTarget::IdxTemporalAttributeStreamsValueExtent,
        IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask,
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
            | IndexTarget::IdxCaValueLongTextTrgm
            | IndexTarget::IdxNotificationDeliveriesClaim
            | IndexTarget::IdxNotificationDeliveriesEvent
            | IndexTarget::IdxNotificationDeliveriesSubscription
            | IndexTarget::PkNotificationDeliveries
            | IndexTarget::IdxEntitiesIdTrgm
            | IndexTarget::PkTemporalAttributeStreams
            | IndexTarget::UqTemporalAttributeStreamsGrain
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey
            | IndexTarget::IdxTemporalAttributeStreamsValueExtent
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask => None,
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
            | IndexTarget::IdxCaValueLongTextTrgm
            | IndexTarget::IdxNotificationDeliveriesClaim
            | IndexTarget::IdxNotificationDeliveriesEvent
            | IndexTarget::IdxNotificationDeliveriesSubscription
            | IndexTarget::PkNotificationDeliveries
            | IndexTarget::IdxEntitiesIdTrgm
            | IndexTarget::PkTemporalAttributeStreams
            | IndexTarget::UqTemporalAttributeStreamsGrain
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey
            | IndexTarget::IdxTemporalAttributeStreamsValueExtent
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask => None,
        }
    }

    /// The column whose non-null rows this GIN covers, `Some` exactly for a
    /// GIN measured by its density rather than through a canary.
    #[must_use]
    pub const fn density_column(self) -> Option<DensityColumn> {
        match self {
            IndexTarget::IdxCaSubEqKeys => Some(DensityColumn::SubEqKeys),
            IndexTarget::IdxCaValueObject => Some(DensityColumn::ValueObject),
            IndexTarget::IdxCaValueLongTextTrgm => Some(DensityColumn::ValueLongText),
            IndexTarget::IdxEntitiesIdTrgm => Some(DensityColumn::Id),
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueString
            | IndexTarget::IdxCaValueStringTrgm
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry
            | IndexTarget::IdxNotificationDeliveriesClaim
            | IndexTarget::IdxNotificationDeliveriesEvent
            | IndexTarget::IdxNotificationDeliveriesSubscription
            | IndexTarget::PkNotificationDeliveries
            | IndexTarget::PkTemporalAttributeStreams
            | IndexTarget::UqTemporalAttributeStreamsGrain
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey
            | IndexTarget::IdxTemporalAttributeStreamsValueExtent
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask => None,
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
            IndexTarget::IdxNotificationDeliveriesClaim
            | IndexTarget::IdxNotificationDeliveriesEvent
            | IndexTarget::IdxNotificationDeliveriesSubscription
            | IndexTarget::PkNotificationDeliveries => HeapTarget::NotificationDeliveries,
            IndexTarget::IdxEntitiesIdTrgm => HeapTarget::Entities,
            IndexTarget::PkTemporalAttributeStreams
            | IndexTarget::UqTemporalAttributeStreamsGrain
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey
            | IndexTarget::IdxTemporalAttributeStreamsValueExtent
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask => HeapTarget::TemporalAttributeStreams,
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
            IndexTarget::IdxNotificationDeliveriesClaim => "idx_notification_deliveries_claim",
            IndexTarget::IdxNotificationDeliveriesEvent => "idx_notification_deliveries_event",
            IndexTarget::IdxNotificationDeliveriesSubscription => "idx_notification_deliveries_subscription",
            IndexTarget::PkNotificationDeliveries => "pk_notification_deliveries",
            IndexTarget::IdxEntitiesIdTrgm => "idx_entities_id_trgm",
            IndexTarget::PkTemporalAttributeStreams => "pk_temporal_attribute_streams",
            IndexTarget::UqTemporalAttributeStreamsGrain => "uq_temporal_attribute_streams_grain",
            IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey => "idx_temporal_attribute_streams_attribute_name_key_entity_key",
            IndexTarget::IdxTemporalAttributeStreamsValueExtent => "idx_temporal_attribute_streams_value_extent",
            IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask => "idx_temporal_attribute_streams_attribute_name_key_shape_mask",
        }
    }

    /// A fixed concurrent rebuild statement in `branch`'s schema; no external
    /// identifier enters SQL.
    pub(crate) fn reindex_statement(self, branch: Branch) -> MaintenanceCommand {
        reindex_index_concurrently(branch.schema(), self)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::{
            heap_target::HeapTarget,
            index_target::{DensityColumn, IndexTarget},
        },
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_seaquery::maintenance::reindex_index_concurrently;
    use pretty_assertions::assert_eq;
    use sea_query::Alias;
    use std::{collections::HashSet, str::FromStr};

    #[test]
    fn every_allowlisted_index_has_one_fixed_concurrent_command() {
        for target in IndexTarget::ALL {
            assert_eq!(
                target.reindex_statement(Branch::Live),
                reindex_index_concurrently(Alias::new("public"), Alias::new(target.relation_name())),
                "{target}"
            );
        }
        assert_eq!(
            IndexTarget::ALL.map(IndexTarget::relation_name).into_iter().collect::<HashSet<_>>().len(),
            IndexTarget::ALL.len()
        );
    }

    /// `pgstattuple` reads no GIN index.
    #[test]
    fn no_gin_index_target_is_probed_directly() {
        for target in [
            IndexTarget::IdxCaValueStringTrgm,
            IndexTarget::IdxCaSubEqKeys,
            IndexTarget::IdxCaValueObject,
            IndexTarget::IdxCaValueLongTextTrgm,
            IndexTarget::IdxEntitiesIdTrgm,
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
                ("sub_eq_keys", "idx_ca_sub_eq_keys"),
                ("value_object", "idx_ca_value_object"),
                ("value_long_text", "idx_ca_value_long_text_trgm"),
                ("id", "idx_entities_id_trgm"),
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
    fn the_queue_index_targets_are_the_delivery_indexes_every_attempt_writes() {
        let queue_indexes = IndexTarget::ALL
            .into_iter()
            .filter(|target| target.heap() == HeapTarget::NotificationDeliveries)
            .map(IndexTarget::relation_name)
            .collect::<Vec<_>>();
        assert_eq!(
            queue_indexes,
            [
                "idx_notification_deliveries_claim",
                "idx_notification_deliveries_event",
                "idx_notification_deliveries_subscription",
                "pk_notification_deliveries",
            ]
        );
        for name in [
            "idx_notification_deliveries_delivered_at",
            "idx_notification_deliveries_exhausted_at",
            "pk_notification_events",
            "uq_notification_events_write_identity",
            "idx_pse_claim",
            "idx_temporal_append_intents_claim",
            "idx_snapshot_notification_outbox_claim",
        ] {
            assert!(IndexTarget::from_str(name).is_err(), "{name}");
        }
    }

    #[test]
    fn every_queue_index_target_is_probed_and_rebuilt_in_its_own_heap_schema() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        for target in IndexTarget::ALL
            .into_iter()
            .filter(|target| target.heap() == HeapTarget::NotificationDeliveries)
        {
            assert!(IndexTarget::PROBEABLE.contains(&target), "{target}");
            assert_eq!(IndexTarget::from_str(target.relation_name()), Ok(target));
            assert_eq!(
                target.reindex_statement(branch),
                reindex_index_concurrently(Alias::new("snap_7"), Alias::new(target.relation_name()))
            );
        }
    }

    /// A history write that grows a stream's value extent or shape mask
    /// adds an entry to every index of the stream catalogue, so each is probed
    /// and rebuilt under the catalogue's own gates.
    #[test]
    fn every_stream_catalogue_index_is_probed_under_the_catalogue_heap() {
        let catalogue = IndexTarget::ALL
            .into_iter()
            .filter(|target| target.heap() == HeapTarget::TemporalAttributeStreams)
            .collect::<Vec<_>>();
        assert_eq!(
            catalogue.iter().map(|target| target.relation_name()).collect::<Vec<_>>(),
            [
                "pk_temporal_attribute_streams",
                "uq_temporal_attribute_streams_grain",
                "idx_temporal_attribute_streams_attribute_name_key_entity_key",
                "idx_temporal_attribute_streams_value_extent",
                "idx_temporal_attribute_streams_attribute_name_key_shape_mask",
            ]
        );
        for target in catalogue {
            assert!(IndexTarget::PROBEABLE.contains(&target), "{target}");
        }
    }
}
