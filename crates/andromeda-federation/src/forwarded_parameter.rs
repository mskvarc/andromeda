//! The vocabulary of parameters a forwarded read request is written with.

use strum::Display;

/// One parameter of a forwarded request, spelled as the binding tables
/// (6.4.3.2-1, 6.5.3.1-1, 6.18.3.2-1 and 6.19.3.1-1) spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "camelCase")]
pub enum ForwardedParameter {
    /// The entity identifiers.
    Id,
    /// The clause 4.17 type selector.
    Type,
    /// The identifier pattern.
    IdPattern,
    /// The source-side page limit used while the broker aggregates results.
    Limit,
    /// The source-local offset used by an explicit range walk.
    Offset,
    /// The attribute list.
    Attrs,
    /// The clause 4.9 query.
    Q,
    /// The geo-query relationship.
    Georel,
    /// The geo-query geometry type.
    Geometry,
    /// The geo-query coordinates.
    Coordinates,
    /// The `GeoProperty` the geo-query evaluates.
    Geoproperty,
    /// The clause 4.19 scope query.
    ScopeQ,
    /// The context source filter.
    Csf,
    /// The `datasetId` selection.
    DatasetId,
    /// The clause 4.15 language filter.
    Lang,
    /// The representation.
    Format,
    /// The representation options.
    Options,
    /// The temporal relationship.
    Timerel,
    /// The temporal anchor instant.
    TimeAt,
    /// The temporal closing instant.
    EndTimeAt,
    /// The temporal property.
    Timeproperty,
    /// The per-attribute instance limit.
    LastN,
    /// The aggregation methods.
    AggrMethods,
    /// The aggregation period.
    AggrPeriodDuration,
    /// The Linked Entity retrieval mode.
    Join,
    /// The Linked Entity retrieval depth.
    JoinLevel,
    /// The entities the requester already holds.
    ContainedBy,
}

#[cfg(test)]
mod tests {
    use crate::forwarded_parameter::ForwardedParameter;

    /// The wire spellings are the ones the binding tables fix, case included.
    #[test]
    fn every_parameter_is_spelled_as_the_binding_tables_spell_it() {
        for (parameter, spelled) in [
            (ForwardedParameter::Id, "id"),
            (ForwardedParameter::Type, "type"),
            (ForwardedParameter::IdPattern, "idPattern"),
            (ForwardedParameter::Offset, "offset"),
            (ForwardedParameter::ScopeQ, "scopeQ"),
            (ForwardedParameter::DatasetId, "datasetId"),
            (ForwardedParameter::Geoproperty, "geoproperty"),
            (ForwardedParameter::Timeproperty, "timeproperty"),
            (ForwardedParameter::EndTimeAt, "endTimeAt"),
            (ForwardedParameter::LastN, "lastN"),
            (ForwardedParameter::AggrPeriodDuration, "aggrPeriodDuration"),
            (ForwardedParameter::ContainedBy, "containedBy"),
        ] {
            pretty_assertions::assert_eq!(parameter.to_string(), spelled);
        }
    }
}
