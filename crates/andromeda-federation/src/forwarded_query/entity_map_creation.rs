//! The query a forwarded `EntityMap` creation carries (clauses 5.14.4.3,
//! 5.14.4.4, 5.14.5.3 and 5.14.5.4).
//!
//! A creation takes the parameters of the query it maps, narrowed to each
//! registration as the query itself is, so a source is asked only for what its
//! registration covers. Of those, the creation keeps the selection, the filters
//! and the temporal query: clauses 5.14.4.3 and 5.14.5.3 ignore the page limit,
//! and a map lists identifiers, so no page, representation, aggregation or
//! projection shapes it. It writes the client's Linked Entity retrieval and the
//! suggested lifetime of the map itself, since a narrowed query carries
//! neither.

use crate::{forwarded_parameter::ForwardedParameter, forwarded_query::ForwardedQuery};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::num::NonZeroU32;
use typed_builder::TypedBuilder;

/// What a creation adds to the narrowed query it is made from (clauses
/// 5.14.4.3 and 5.14.5.3).
#[derive(Debug, Clone, Copy, TypedBuilder)]
pub(crate) struct CreationMembers<'a> {
    /// The clause 4.5.23 Linked Entity retrieval the client asked for. It
    /// travels so the source reads a `q` over Linked Entity Attributes, which
    /// clause 5.14.4.4 refuses as too deep a query without it.
    #[builder(default = None)]
    pub(crate) join: Option<JoinMode>,
    /// The depth of that retrieval.
    #[builder(default = None)]
    pub(crate) join_level: Option<NonZeroU32>,
    /// The Linked Entities the client already holds.
    #[builder(default = None)]
    pub(crate) contained_by: Option<&'a [Identifier]>,
    /// The lifetime the client suggested for the map.
    #[builder(default = None)]
    pub(crate) lifetime: Option<&'a Iso8601Duration>,
}

impl ForwardedQuery {
    /// The creation of an `EntityMap` for this narrowed query: the members a
    /// creation keeps, in the order they were written, then the Linked Entity
    /// retrieval and the lifetime of `members`, when the client gave them.
    pub(crate) fn into_entity_map_creation(mut self, members: CreationMembers<'_>) -> ForwardedQuery {
        self.pairs.retain(|(parameter, _)| is_kept_by_a_creation(*parameter));
        self.push_traversal(members.join.map(|join| join.to_string()), members.join_level, members.contained_by);
        self.push_optional(ForwardedParameter::EntityMapLifetime, members.lifetime.map(ToString::to_string));
        self
    }
}

/// Whether a creation keeps `parameter` of the narrowed query (clauses
/// 5.14.4.3 and 5.14.5.3). The Linked Entity retrieval and the lifetime are
/// written by the creation from the client's query, so none of the narrowed
/// query's is kept.
const fn is_kept_by_a_creation(parameter: ForwardedParameter) -> bool {
    match parameter {
        ForwardedParameter::Id
        | ForwardedParameter::Type
        | ForwardedParameter::IdPattern
        | ForwardedParameter::Attrs
        | ForwardedParameter::Q
        | ForwardedParameter::JsonKeys
        | ForwardedParameter::Georel
        | ForwardedParameter::Geometry
        | ForwardedParameter::Coordinates
        | ForwardedParameter::Geoproperty
        | ForwardedParameter::ScopeQ
        | ForwardedParameter::Csf
        | ForwardedParameter::DatasetId
        | ForwardedParameter::Lang
        | ForwardedParameter::Timerel
        | ForwardedParameter::TimeAt
        | ForwardedParameter::EndTimeAt
        | ForwardedParameter::Timeproperty
        | ForwardedParameter::LastN
        | ForwardedParameter::ExpandValues => true,
        ForwardedParameter::Limit
        | ForwardedParameter::Offset
        | ForwardedParameter::Format
        | ForwardedParameter::Options
        | ForwardedParameter::AggrMethods
        | ForwardedParameter::AggrPeriodDuration
        | ForwardedParameter::Pick
        | ForwardedParameter::Omit
        | ForwardedParameter::Join
        | ForwardedParameter::JoinLevel
        | ForwardedParameter::ContainedBy
        | ForwardedParameter::EntityMapLifetime => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{ForwardedQuery, entity_map_creation::CreationMembers},
    };
    use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::{num::NonZeroU32, slice};

    fn written(pairs: &[(ForwardedParameter, &str)]) -> ForwardedQuery {
        let mut query = ForwardedQuery::default();
        for (parameter, value) in pairs {
            query.push(*parameter, (*value).to_owned());
        }
        query
    }

    /// Clause 5.14.4.3 ignores the limit, and a map renders no entity, so the
    /// page and the system-attribute option stay behind while the selection
    /// and the filters travel, the suggested lifetime after them.
    #[test]
    fn a_creation_keeps_the_selection_and_the_filters_and_adds_the_lifetime() {
        let narrowed = written(&[
            (ForwardedParameter::Id, "urn:ngsi-ld:Vehicle:1"),
            (ForwardedParameter::Type, "Vehicle"),
            (ForwardedParameter::Limit, "1000"),
            (ForwardedParameter::Offset, "20"),
            (ForwardedParameter::Attrs, "speed"),
            (ForwardedParameter::Q, "speed>10"),
            (ForwardedParameter::ScopeQ, "/Madrid"),
            (ForwardedParameter::Csf, "tier==\"gold\""),
            (ForwardedParameter::DatasetId, "@none"),
            (ForwardedParameter::Lang, "fr,en;q=0.5"),
            (ForwardedParameter::Options, "sysAttrs"),
        ]);
        let lifetime: Iso8601Duration = "PT1H".parse().expect("a valid duration");
        pretty_assertions::assert_eq!(
            narrowed
                .into_entity_map_creation(CreationMembers::builder().lifetime(Some(&lifetime)).build())
                .to_query_string(),
            "id=urn%3Angsi-ld%3AVehicle%3A1&type=Vehicle&attrs=speed&q=speed%3E10&scopeQ=%2FMadrid&csf=tier%3D%3D%22gold%22&datasetId=%40none&lang=fr%2Cen%3Bq%3D0.5&entityMapLifetime=PT1H"
        );
    }

    /// Clause 5.14.5.3 takes the temporal query and `lastN`, and names no
    /// representation or aggregation of its own.
    #[test]
    fn a_temporal_creation_keeps_the_window_and_last_n_and_drops_the_aggregation() {
        let narrowed = written(&[
            (ForwardedParameter::Type, "Vehicle"),
            (ForwardedParameter::Limit, "1000"),
            (ForwardedParameter::Timerel, "before"),
            (ForwardedParameter::TimeAt, "2022-01-01T00:00:00Z"),
            (ForwardedParameter::Timeproperty, "modifiedAt"),
            (ForwardedParameter::LastN, "3"),
            (ForwardedParameter::Format, "aggregatedValues"),
            (ForwardedParameter::AggrMethods, "avg"),
            (ForwardedParameter::AggrPeriodDuration, "PT1H"),
            (ForwardedParameter::Options, "sysAttrs"),
        ]);
        pretty_assertions::assert_eq!(
            narrowed.into_entity_map_creation(CreationMembers::builder().build()).to_query_string(),
            "type=Vehicle&timerel=before&timeAt=2022-01-01T00%3A00%3A00Z&timeproperty=modifiedAt&lastN=3"
        );
    }

    /// Clause 5.14.4.4 refuses a `q` over Linked Entity Attributes without
    /// `join` as too deep, so the client's Linked Entity retrieval travels with
    /// the creation, after the members it keeps and before the lifetime.
    #[test]
    fn a_creation_carries_the_linked_entity_retrieval_of_the_client() {
        let narrowed = written(&[
            (ForwardedParameter::Type, "Vehicle"),
            (ForwardedParameter::Q, "isParkedIn{category}==\"commercial\""),
        ]);
        let held: Identifier = "urn:ngsi-ld:Building:1".parse().expect("a valid identifier");
        let lifetime: Iso8601Duration = "PT1H".parse().expect("a valid duration");
        let members = CreationMembers::builder()
            .join(Some(JoinMode::Flat))
            .join_level(NonZeroU32::new(2))
            .contained_by(Some(slice::from_ref(&held)))
            .lifetime(Some(&lifetime))
            .build();
        pretty_assertions::assert_eq!(
            narrowed.into_entity_map_creation(members).to_query_string(),
            "type=Vehicle&q=isParkedIn%7Bcategory%7D%3D%3D%22commercial%22&join=flat&joinLevel=2&containedBy=urn%3Angsi-ld%3ABuilding%3A1&entityMapLifetime=PT1H"
        );
    }
}
