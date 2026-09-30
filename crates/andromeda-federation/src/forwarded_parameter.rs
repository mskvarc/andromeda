//! The vocabulary of parameters a forwarded read request is written with.

use strum::{Display, EnumString};

/// What the value of one forwarded parameter names, which decides how it is
/// spelled in a forwarding context (clause 4.3.6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParameterNames {
    /// A clause 4.17 type selection.
    TypeSelection,
    /// A comma-separated list of attribute names.
    AttributeList,
    /// One attribute name.
    Attribute,
    /// A clause 4.21 projection.
    Projection,
    /// A clause 4.9 query, whose literal JSON keys `jsonKeys` names.
    Query,
    /// A context source filter, a query over Context Source Property names.
    SourceFilter,
    /// No name at all.
    Nothing,
}

/// One parameter of a forwarded request, spelled as the binding tables
/// (6.4.3.2-1, 6.5.3.1-1, 6.18.3.2-1 and 6.19.3.1-1) spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
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
    /// The attributes whose members the clause 4.9 query addresses as literal
    /// JSON keys (clause 4.9 EXAMPLE 11).
    JsonKeys,
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
    /// The attributes whose values are expanded as IRIs (clause 4.5.18).
    ExpandValues,
    /// The clause 4.21 projection keeping members.
    Pick,
    /// The clause 4.21 projection removing members.
    Omit,
}

impl ForwardedParameter {
    /// What this parameter's value names.
    pub(crate) const fn names(self) -> ParameterNames {
        match self {
            ForwardedParameter::Type => ParameterNames::TypeSelection,
            ForwardedParameter::Attrs | ForwardedParameter::JsonKeys | ForwardedParameter::ExpandValues => ParameterNames::AttributeList,
            ForwardedParameter::Geoproperty => ParameterNames::Attribute,
            ForwardedParameter::Pick | ForwardedParameter::Omit => ParameterNames::Projection,
            ForwardedParameter::Q => ParameterNames::Query,
            ForwardedParameter::Csf => ParameterNames::SourceFilter,
            ForwardedParameter::Id
            | ForwardedParameter::IdPattern
            | ForwardedParameter::Limit
            | ForwardedParameter::Offset
            | ForwardedParameter::Georel
            | ForwardedParameter::Geometry
            | ForwardedParameter::Coordinates
            | ForwardedParameter::ScopeQ
            | ForwardedParameter::DatasetId
            | ForwardedParameter::Lang
            | ForwardedParameter::Format
            | ForwardedParameter::Options
            | ForwardedParameter::Timerel
            | ForwardedParameter::TimeAt
            | ForwardedParameter::EndTimeAt
            | ForwardedParameter::Timeproperty
            | ForwardedParameter::LastN
            | ForwardedParameter::AggrMethods
            | ForwardedParameter::AggrPeriodDuration
            | ForwardedParameter::Join
            | ForwardedParameter::JoinLevel
            | ForwardedParameter::ContainedBy => ParameterNames::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::forwarded_parameter::{ForwardedParameter, ParameterNames};

    /// The wire spellings are the ones the binding tables fix, case included.
    #[test]
    fn every_parameter_is_spelled_as_the_binding_tables_spell_it() {
        for (parameter, spelled) in [
            (ForwardedParameter::Id, "id"),
            (ForwardedParameter::Type, "type"),
            (ForwardedParameter::IdPattern, "idPattern"),
            (ForwardedParameter::Offset, "offset"),
            (ForwardedParameter::ScopeQ, "scopeQ"),
            (ForwardedParameter::JsonKeys, "jsonKeys"),
            (ForwardedParameter::DatasetId, "datasetId"),
            (ForwardedParameter::Geoproperty, "geoproperty"),
            (ForwardedParameter::Timeproperty, "timeproperty"),
            (ForwardedParameter::EndTimeAt, "endTimeAt"),
            (ForwardedParameter::LastN, "lastN"),
            (ForwardedParameter::AggrPeriodDuration, "aggrPeriodDuration"),
            (ForwardedParameter::ContainedBy, "containedBy"),
        ] {
            pretty_assertions::assert_eq!(parameter.to_string(), spelled);
            pretty_assertions::assert_eq!(spelled.parse::<ForwardedParameter>(), Ok(parameter));
        }
    }

    /// A parameter the binding tables do not name reads as none of them, and a
    /// spelling differing in case is another parameter.
    #[test]
    fn only_the_binding_spellings_parse() {
        assert!("orderBy".parse::<ForwardedParameter>().is_err());
        assert!("jsonkeys".parse::<ForwardedParameter>().is_err());
        pretty_assertions::assert_eq!("expandValues".parse::<ForwardedParameter>(), Ok(ForwardedParameter::ExpandValues));
    }

    #[test]
    fn every_name_bearing_parameter_says_what_it_names() {
        for (parameter, names) in [
            (ForwardedParameter::Type, ParameterNames::TypeSelection),
            (ForwardedParameter::Attrs, ParameterNames::AttributeList),
            (ForwardedParameter::JsonKeys, ParameterNames::AttributeList),
            (ForwardedParameter::ExpandValues, ParameterNames::AttributeList),
            (ForwardedParameter::Geoproperty, ParameterNames::Attribute),
            (ForwardedParameter::Pick, ParameterNames::Projection),
            (ForwardedParameter::Omit, ParameterNames::Projection),
            (ForwardedParameter::Q, ParameterNames::Query),
            (ForwardedParameter::Csf, ParameterNames::SourceFilter),
            (ForwardedParameter::Limit, ParameterNames::Nothing),
        ] {
            pretty_assertions::assert_eq!(parameter.names(), names, "{parameter}");
        }
    }
}
