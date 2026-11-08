//! The query parameters a request may name, and the set each operation reads.
//!
//! Clause 6.3.20 refuses a parameter an operation does not define with
//! `InvalidRequest`. The vocabulary is closed and typed so that refusal, and
//! the refusal of a value one of them carries, names the parameter as the
//! specification spells it rather than echoing whatever the client wrote.

use andromeda_ngsi_ld_query::ordering::parameters::OrderingParameters;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// A query parameter of the NGSI-LD API (clause 6.3 and the resource tables of
/// clause 6), spelled as the specification spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum QueryParameter {
    /// Entity identifiers (`id`).
    Id,
    /// Entity type selection (`type`).
    Type,
    /// Entity identifier pattern (`idPattern`).
    IdPattern,
    /// Attribute projection (`attrs`).
    Attrs,
    /// Filter expression (`q`).
    Q,
    /// Context source filter (`csf`).
    Csf,
    /// Geoquery relation (`georel`).
    Georel,
    /// Geoquery reference geometry type (`geometry`).
    Geometry,
    /// Geoquery reference coordinates (`coordinates`).
    Coordinates,
    /// Geoquery target property (`geoproperty`).
    Geoproperty,
    /// The `GeoProperty` a `GeoJSON` answer uses (`geometryProperty`).
    GeometryProperty,
    /// Scope query (`scopeQ`).
    ScopeQ,
    /// Members the answer keeps (`pick`).
    Pick,
    /// Members the answer drops (`omit`).
    Omit,
    /// Language selection (`lang`).
    Lang,
    /// Temporal relation (`timerel`).
    Timerel,
    /// Temporal reference instant (`timeAt`).
    TimeAt,
    /// Temporal interval end (`endTimeAt`).
    EndTimeAt,
    /// Temporal property (`timeproperty`).
    Timeproperty,
    /// Page size (`limit`).
    Limit,
    /// Page offset (`offset`).
    Offset,
    /// Cursor of a paged walk (`cursor`).
    Cursor,
    /// The `@context` a followed page pointer was read under (`pageContext`).
    PageContext,
    /// The digest of the query a followed cursor walk belongs to (`pageQuery`).
    PageQuery,
    /// Whether the answer reports a total (`count`).
    Count,
    /// Representation options (`options`).
    Options,
    /// Confinement to this broker (`local`).
    Local,
    /// Ordering terms (`orderBy`).
    OrderBy,
    /// Ordering collation (`collation`).
    Collation,
    /// Distance ordering reference coordinates (`orderFrom`).
    OrderFrom,
    /// Distance ordering reference geometry type (`orderGeometry`).
    OrderGeometry,
    /// Dataset selection (`datasetId`).
    DatasetId,
    /// Attributes whose `q` values are expanded (`expandValues`).
    ExpandValues,
    /// Attributes whose `q` paths address JSON keys (`jsonKeys`).
    JsonKeys,
    /// Representation format (`format`).
    Format,
    /// Linked entity retrieval (`join`).
    Join,
    /// Linked entity retrieval depth (`joinLevel`).
    JoinLevel,
    /// Entities the result is linked from (`containedBy`).
    ContainedBy,
    /// Entity map creation (`entityMap`).
    EntityMap,
    /// Entity map lifetime (`entityMapLifetime`).
    EntityMapLifetime,
    /// Merging of split entities (`splitEntities`).
    SplitEntities,
    /// Members a purge drops (`drop`).
    Drop,
    /// Members a purge keeps (`keep`).
    Keep,
    /// Deletion of every dataset instance (`deleteAll`).
    DeleteAll,
    /// A purge that only reports (`dryRun`).
    DryRun,
    /// Observation instant of a merge (`observedAt`).
    ObservedAt,
    /// Most recent instances per attribute (`lastN`).
    LastN,
    /// Aggregation functions (`aggrMethods`).
    AggrMethods,
    /// Aggregation period (`aggrPeriodDuration`).
    AggrPeriodDuration,
    /// Aggregation grouping (`aggrGroupBy`).
    AggrGroupBy,
    /// Kind of stored `@context` (`kind`).
    Kind,
    /// Detail level of a stored `@context` (`details`).
    Details,
    /// Reload instead of delete of a cached `@context` (`reload`).
    Reload,
}

/// The query parameters one operation defines; clause 6.3.20 refuses any other
/// with `InvalidRequest`.
pub trait QueryParameterSet {
    /// Whether the operation defines `parameter`.
    fn defines(parameter: QueryParameter) -> bool;
}

/// Implement [`QueryParameterSet`] for a parameter struct that defines the
/// listed parameters and every parameter of the parameter structs it flattens.
///
/// `query_parameter_set!(Set { Local, Options } + PageParameters)` expands to an
/// impl whose `defines` matches `QueryParameter::Local | QueryParameter::Options`
/// or defers to `PageParameters`. The caller imports `QueryParameter` and
/// `QueryParameterSet`.
#[macro_export]
macro_rules! query_parameter_set {
    ($set:ty { $($parameter:ident),+ $(,)? } $(+ $flattened:ident)*) => {
        impl QueryParameterSet for $set {
            fn defines(parameter: QueryParameter) -> bool {
                matches!(parameter, $(QueryParameter::$parameter)|+) $(|| <$flattened as QueryParameterSet>::defines(parameter))*
            }
        }
    };
}

query_parameter_set!(OrderingParameters {
    OrderBy,
    Collation,
    OrderFrom,
    OrderGeometry
});

#[cfg(test)]
mod tests {
    use crate::{
        attribute_deletion::{DeleteAttrParams, EntityAttributeDeletionParams},
        discovery::DiscoveryListParams,
        empty::EmptyParams,
        entity::{AppendParams, AttributeWriteParams, GetEntityParams, MergeParams},
        entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
        entity_write::{EntityWriteParams, UpdateAttributesParams},
        jsonld_context::{DeleteParams, ListParams as ContextListParams, ServeParams},
        local::LocalOnlyParams,
        query::{
            csr::CsrQueryParameters,
            entity::{parameters::EntityQueryParameters, purge::PurgeEntitiesParameters},
            temporal::{
                TemporalQueryParameters,
                delete_parameters::TemporalDeleteParameters,
                post_parameters::TemporalPostQueryParams,
                retrieve_parameters::TemporalRetrieveParameters,
            },
        },
        query_parameter::{QueryParameter, QueryParameterSet},
        registration_retrieval::RegistrationRetrieveParams,
        subscription_listing::ListParams as SubscriptionListParams,
        sys_attrs_parameters::SysAttrsParams,
    };
    use serde::de::DeserializeOwned;
    use std::{any::type_name, str::FromStr};
    use strum::IntoEnumIterator;

    /// Serde's report of a parameter a parameter struct does not read.
    const UNKNOWN_PARAMETER: &str = "unknown field";

    /// Assert the parameters `T` declares are exactly the ones its
    /// deserialization reads by name.
    fn assert_set_matches_deserialization<T: QueryParameterSet + DeserializeOwned>() {
        for parameter in QueryParameter::iter() {
            let unknown = serde_urlencoded::from_str::<T>(&format!("{parameter}="))
                .err()
                .is_some_and(|error| error.to_string().contains(UNKNOWN_PARAMETER));
            pretty_assertions::assert_eq!(unknown, !T::defines(parameter), "{parameter} for {}", type_name::<T>());
        }
    }

    #[test]
    fn every_parameter_set_declares_exactly_what_it_deserializes() {
        assert_set_matches_deserialization::<EmptyParams>();
        assert_set_matches_deserialization::<LocalOnlyParams>();
        assert_set_matches_deserialization::<SysAttrsParams>();
        assert_set_matches_deserialization::<SubscriptionListParams>();
        assert_set_matches_deserialization::<ContextListParams>();
        assert_set_matches_deserialization::<ServeParams>();
        assert_set_matches_deserialization::<DeleteParams>();
        assert_set_matches_deserialization::<DiscoveryListParams>();
        assert_set_matches_deserialization::<RegistrationRetrieveParams>();
        assert_set_matches_deserialization::<AppendParams>();
        assert_set_matches_deserialization::<AttributeWriteParams>();
        assert_set_matches_deserialization::<MergeParams>();
        assert_set_matches_deserialization::<GetEntityParams>();
        assert_set_matches_deserialization::<UpsertParams>();
        assert_set_matches_deserialization::<UpdateParams>();
        assert_set_matches_deserialization::<PostQueryParams>();
        assert_set_matches_deserialization::<EntityWriteParams>();
        assert_set_matches_deserialization::<UpdateAttributesParams>();
        assert_set_matches_deserialization::<EntityAttributeDeletionParams>();
        assert_set_matches_deserialization::<DeleteAttrParams>();
        assert_set_matches_deserialization::<CsrQueryParameters>();
        assert_set_matches_deserialization::<EntityQueryParameters>();
        assert_set_matches_deserialization::<PurgeEntitiesParameters>();
        assert_set_matches_deserialization::<TemporalQueryParameters>();
        assert_set_matches_deserialization::<TemporalDeleteParameters>();
        assert_set_matches_deserialization::<TemporalPostQueryParams>();
        assert_set_matches_deserialization::<TemporalRetrieveParameters>();
    }

    #[test]
    fn every_parameter_round_trips_through_its_specification_spelling() {
        for parameter in QueryParameter::iter() {
            pretty_assertions::assert_eq!(QueryParameter::from_str(&parameter.to_string()), Ok(parameter));
        }
    }

    #[test]
    fn spellings_follow_the_specification() {
        for (parameter, spelling) in [
            (QueryParameter::Q, "q"),
            (QueryParameter::IdPattern, "idPattern"),
            (QueryParameter::Georel, "georel"),
            (QueryParameter::Timeproperty, "timeproperty"),
            (QueryParameter::ScopeQ, "scopeQ"),
            (QueryParameter::LastN, "lastN"),
            (QueryParameter::PageContext, "pageContext"),
            (QueryParameter::PageQuery, "pageQuery"),
        ] {
            pretty_assertions::assert_eq!(parameter.to_string(), spelling);
        }
    }

    #[test]
    fn a_name_outside_the_vocabulary_is_no_parameter() {
        assert!(QueryParameter::from_str("limitt").is_err());
        assert!(QueryParameter::from_str("Limit").is_err());
    }
}
