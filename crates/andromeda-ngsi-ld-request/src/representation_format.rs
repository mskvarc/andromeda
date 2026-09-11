//! What the `format` parameter names, per operation (Tables 6.3.7-1, 6.3.12-1).
//!
//! Table 6.3.7-1 defines the entity renderings `format` selects and Table
//! 6.3.12-1 the temporal ones, and the two sets are disjoint: a temporal
//! operation does not render `concise`, and an entity operation does not render
//! `aggregatedValues`. Each therefore gets its own grammar, and a value outside
//! it is the clause 6.3.20 failure [`crate::representation_parameter`] defers.
//! The rejection carries the value as written, because that -- not a restated
//! grammar -- is what the client has to correct.

use crate::representation_parameter::{RepresentationGrammar, RepresentationParameter};
use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;

// Table 6.3.7-1: "simplified" and its synonym "keyValues" name one rendering,
// which is why both parse to the same value rather than to two.
impl RepresentationGrammar for ResponseFormat {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Format;

    fn parse(written: &str) -> Result<ResponseFormat, String> {
        match written {
            "keyValues" | "simplified" => Ok(ResponseFormat::Simplified),
            "concise" => Ok(ResponseFormat::Concise),
            "normalized" => Ok(ResponseFormat::Normalized),
            other => Err(other.to_owned()),
        }
    }
}

// Table 6.3.12-1: "It shall be one of: temporalValues, aggregatedValues". The
// normalized temporal representation is what the parameter's absence selects,
// so naming it is not one of the two values the table admits.
impl RepresentationGrammar for TemporalFormat {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Format;

    fn parse(written: &str) -> Result<TemporalFormat, String> {
        match written {
            "temporalValues" => Ok(TemporalFormat::TemporalValues),
            "aggregatedValues" => Ok(TemporalFormat::AggregatedValues),
            other => Err(other.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::representation_parameter::RepresentationGrammar;
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;

    /// Table 6.3.7-1 names three entity renderings under four spellings.
    #[test]
    fn every_entity_rendering_the_table_defines_parses() {
        pretty_assertions::assert_eq!(ResponseFormat::parse("keyValues"), Ok(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(ResponseFormat::parse("simplified"), Ok(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(ResponseFormat::parse("concise"), Ok(ResponseFormat::Concise));
        pretty_assertions::assert_eq!(ResponseFormat::parse("normalized"), Ok(ResponseFormat::Normalized));
    }

    /// Clause 6.3.20: anything else is a value the operation does not support,
    /// including a temporal rendering and an empty value.
    #[test]
    fn a_value_outside_the_entity_table_is_refused() {
        for written in ["invalidFormat", "temporalValues", "aggregatedValues", "", "KEYVALUES"] {
            assert!(ResponseFormat::parse(written).is_err(), "{written} must be refused");
        }
    }

    /// Table 6.3.12-1 admits exactly two temporal renderings.
    #[test]
    fn only_the_two_temporal_renderings_parse() {
        pretty_assertions::assert_eq!(TemporalFormat::parse("temporalValues"), Ok(TemporalFormat::TemporalValues));
        pretty_assertions::assert_eq!(TemporalFormat::parse("aggregatedValues"), Ok(TemporalFormat::AggregatedValues));
        for written in ["invalidFormat", "normalized", "keyValues", "concise", ""] {
            assert!(TemporalFormat::parse(written).is_err(), "{written} must be refused");
        }
    }
}
