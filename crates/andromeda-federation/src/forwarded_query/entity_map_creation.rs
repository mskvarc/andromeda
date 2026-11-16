//! The query a forwarded `EntityMap` creation carries (clauses 5.14.4.3,
//! 5.14.4.4, 5.14.5.3 and 5.14.5.4).
//!
//! A creation takes the parameters of the query it maps, narrowed to each
//! registration as the query itself is, so a source is asked only for what its
//! registration covers. Of those, the creation reads the selection, the filters
//! and the temporal query: clauses 5.14.4.3 and 5.14.5.3 ignore the page limit,
//! and a map lists identifiers, so no page, representation, aggregation or
//! projection shapes it. It adds the suggested lifetime of the map.

use crate::{forwarded_parameter::ForwardedParameter, forwarded_query::ForwardedQuery};
use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;

impl ForwardedQuery {
    /// The creation of an `EntityMap` for this narrowed query: the members a
    /// creation reads, in the order they were written, then the `lifetime` the
    /// client suggested, when it suggested one.
    pub(crate) fn into_entity_map_creation(mut self, lifetime: Option<&Iso8601Duration>) -> ForwardedQuery {
        self.pairs.retain(|(parameter, _)| is_read_by_a_creation(*parameter));
        self.push_optional(ForwardedParameter::EntityMapLifetime, lifetime.map(ToString::to_string));
        self
    }
}

/// Whether a creation reads `parameter` (clauses 5.14.4.3 and 5.14.5.3).
const fn is_read_by_a_creation(parameter: ForwardedParameter) -> bool {
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
        | ForwardedParameter::Join
        | ForwardedParameter::JoinLevel
        | ForwardedParameter::ContainedBy
        | ForwardedParameter::ExpandValues
        | ForwardedParameter::EntityMapLifetime => true,
        ForwardedParameter::Limit
        | ForwardedParameter::Offset
        | ForwardedParameter::Format
        | ForwardedParameter::Options
        | ForwardedParameter::AggrMethods
        | ForwardedParameter::AggrPeriodDuration
        | ForwardedParameter::Pick
        | ForwardedParameter::Omit => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{forwarded_parameter::ForwardedParameter, forwarded_query::ForwardedQuery};
    use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;

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
            narrowed.into_entity_map_creation(Some(&lifetime)).to_query_string(),
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
            narrowed.into_entity_map_creation(None).to_query_string(),
            "type=Vehicle&timerel=before&timeAt=2022-01-01T00%3A00%3A00Z&timeproperty=modifiedAt&lastN=3"
        );
    }
}
