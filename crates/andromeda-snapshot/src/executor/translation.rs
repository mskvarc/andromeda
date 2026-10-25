//! Translation from canonical snapshot queries to policy query parameters.

use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};

pub(crate) fn entity_parameters(query: Query) -> Result<EntityQueryParameters, ParameterError> {
    EntityQueryParameters::try_from(query)
}

pub(crate) fn temporal_parameters(query: Query) -> Result<TemporalQueryParameters, ParameterError> {
    TemporalQueryParameters::try_from(query)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::executor::translation::{entity_parameters, temporal_parameters};
    use andromeda_ngsi_ld_model::query::body::Query;

    #[test]
    fn entity_translation_rejects_temporal_members() {
        let query = serde_json::from_str::<Query>(r#"{"type":"Query","temporalQ":{"timerel":"after","timeAt":"2025-01-01T00:00:00Z"}}"#)
            .expect("valid canonical query");
        assert!(entity_parameters(query).is_err());
    }

    #[test]
    fn temporal_translation_requires_temporal_query() {
        let query = serde_json::from_str::<Query>(r#"{"type":"Query"}"#).expect("valid canonical query");
        assert!(temporal_parameters(query).is_err());
    }
}
