//! The closed set of relations that maintenance may inspect or mutate.

use andromeda_postgresql_schema::{
    current::{CurrentAttributeValues, EntityDocuments},
    subscriptions::SubscriptionDeliveryState,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceSchema, MaintenanceStatement, reindex_index_concurrently, vacuum_non_truncating};
use sea_query::Iden;
use strum::{Display, EnumString, IntoStaticStr};

/// An allowlisted `current_attribute_values` index.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, Iden, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum IndexTarget {
    /// Natural entity/attribute/dataset grain.
    UqCurrentAttributeValuesGrain,
    /// Numeric predicate index.
    IdxCavAttrNumber,
    /// String B-tree and bloat canary for the paired trigram rebuild.
    IdxCavAttrString,
    /// Attribute-aware string trigram GIN, queued only through its canary.
    IdxCavAttrStringTrgm,
    /// Boolean predicate index.
    IdxCavAttrBool,
    /// Date-time predicate index.
    IdxCavAttrDatetime,
    /// URI predicate index.
    IdxCavAttrUri,
    /// Attribute-aware geometry `GiST`.
    IdxCavAttrGeo,
    /// Entity/attribute geometry correlation index.
    IdxCavAttrEntityGeo,
}

impl IndexTarget {
    /// Every index that may be rebuilt or recognized during recovery.
    pub const ALL: [IndexTarget; 9] = [
        IndexTarget::UqCurrentAttributeValuesGrain,
        IndexTarget::IdxCavAttrNumber,
        IndexTarget::IdxCavAttrString,
        IndexTarget::IdxCavAttrStringTrgm,
        IndexTarget::IdxCavAttrBool,
        IndexTarget::IdxCavAttrDatetime,
        IndexTarget::IdxCavAttrUri,
        IndexTarget::IdxCavAttrGeo,
        IndexTarget::IdxCavAttrEntityGeo,
    ];

    /// Indexes eligible for direct physical bloat probing.
    pub const PROBEABLE: [IndexTarget; 8] = [
        IndexTarget::UqCurrentAttributeValuesGrain,
        IndexTarget::IdxCavAttrNumber,
        IndexTarget::IdxCavAttrString,
        IndexTarget::IdxCavAttrBool,
        IndexTarget::IdxCavAttrDatetime,
        IndexTarget::IdxCavAttrUri,
        IndexTarget::IdxCavAttrGeo,
        IndexTarget::IdxCavAttrEntityGeo,
    ];

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            IndexTarget::UqCurrentAttributeValuesGrain => "uq_current_attribute_values_grain",
            IndexTarget::IdxCavAttrNumber => "idx_cav_attr_number",
            IndexTarget::IdxCavAttrString => "idx_cav_attr_string",
            IndexTarget::IdxCavAttrStringTrgm => "idx_cav_attr_string_trgm",
            IndexTarget::IdxCavAttrBool => "idx_cav_attr_bool",
            IndexTarget::IdxCavAttrDatetime => "idx_cav_attr_datetime",
            IndexTarget::IdxCavAttrUri => "idx_cav_attr_uri",
            IndexTarget::IdxCavAttrGeo => "idx_cav_attr_geo",
            IndexTarget::IdxCavAttrEntityGeo => "idx_cav_attr_entity_geo",
        }
    }

    /// A fixed concurrent rebuild statement; no external identifier enters SQL.
    #[must_use]
    pub fn reindex_statement(self) -> MaintenanceStatement {
        reindex_index_concurrently(&MaintenanceSchema::Public, &self)
    }

    /// Whether qualification also queues the unsupported trigram probe target.
    #[must_use]
    pub const fn queues_trigram_pair(self) -> bool {
        matches!(self, IndexTarget::IdxCavAttrString)
    }
}

/// A heap relation eligible for catch-up ordinary vacuum.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HeapTarget {
    /// Current attribute value projection.
    CurrentAttributeValues,
    /// Expanded current entity documents.
    EntityDocuments,
    /// Frequently updated subscription delivery counters.
    SubscriptionDeliveryState,
}

impl HeapTarget {
    /// Every heap relation eligible for cheap sampling and catch-up vacuum.
    pub const ALL: [HeapTarget; 3] = [
        HeapTarget::CurrentAttributeValues,
        HeapTarget::EntityDocuments,
        HeapTarget::SubscriptionDeliveryState,
    ];

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub const fn relation_name(self) -> &'static str {
        match self {
            HeapTarget::CurrentAttributeValues => "current_attribute_values",
            HeapTarget::EntityDocuments => "entity_documents",
            HeapTarget::SubscriptionDeliveryState => "subscription_delivery_state",
        }
    }

    /// A fixed non-truncating, non-parallel ordinary vacuum statement.
    #[must_use]
    pub fn vacuum_statement(self) -> MaintenanceStatement {
        match self {
            HeapTarget::CurrentAttributeValues => vacuum_non_truncating(&MaintenanceSchema::Public, &CurrentAttributeValues::Table),
            HeapTarget::EntityDocuments => vacuum_non_truncating(&MaintenanceSchema::Public, &EntityDocuments::Table),
            HeapTarget::SubscriptionDeliveryState => vacuum_non_truncating(&MaintenanceSchema::Public, &SubscriptionDeliveryState::Table),
        }
    }
}

/// A persistable allowlisted maintenance target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaintenanceTarget {
    /// One CAV index.
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
                IndexTarget::UqCurrentAttributeValuesGrain
                | IndexTarget::IdxCavAttrNumber
                | IndexTarget::IdxCavAttrString
                | IndexTarget::IdxCavAttrStringTrgm
                | IndexTarget::IdxCavAttrBool
                | IndexTarget::IdxCavAttrDatetime
                | IndexTarget::IdxCavAttrUri
                | IndexTarget::IdxCavAttrGeo
                | IndexTarget::IdxCavAttrEntityGeo,
            ) => "current_attribute_values",
            MaintenanceTarget::Heap(target) => target.relation_name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::candidate::{HeapTarget, IndexTarget};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn every_allowlisted_index_has_one_fixed_concurrent_command() {
        let commands = IndexTarget::ALL.map(|target| target.reindex_statement().as_sql().to_owned());
        assert!(commands.iter().all(|command| command.starts_with("REINDEX INDEX CONCURRENTLY \"public\".")));
        assert_eq!(commands.into_iter().collect::<HashSet<_>>().len(), IndexTarget::ALL.len());
    }

    #[test]
    fn only_the_string_btree_queues_the_trigram_pair() {
        assert!(IndexTarget::IdxCavAttrString.queues_trigram_pair());
        assert!(!IndexTarget::IdxCavAttrStringTrgm.queues_trigram_pair());
        assert!(!IndexTarget::IdxCavAttrNumber.queues_trigram_pair());
    }

    #[test]
    fn every_heap_command_disables_truncation_and_parallel_workers() {
        for target in HeapTarget::ALL {
            let statement = target.vacuum_statement();
            let command = statement.as_sql();
            assert!(command.starts_with("VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\"."), "{command}");
        }
    }
}
