//! What the clause 4.3.6.8 fallback does to the instances of one attribute
//! member, beside the member-by-member rows the walk asks of
//! [`crate::version::fallback`].
//!
//! Two decisions need more than one instance's members: which instance of a
//! multi-attribute a target below 1.3 receives (Table 4.3.6.8-1 notes 2 and
//! 3), and how a value reformatted as a Property's reads back once the
//! concise representation of clause 4.5.2.3 writes it bare or without `type`.

use crate::entity_body::family_value::FamilyValue;
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_representation::concise_reading::{bare_value_family, typeless_value_family};
use thiserror::Error;

/// The one instance of a multi-attribute a target below 1.3 receives: the
/// default instance, the one carrying no `datasetId` (clause 4.5.5), or, when
/// every instance carries one, the first in the order they are held. Every
/// representation keeps the same instance, so the normalized, concise and
/// simplified answers of one entity agree on it.
pub(crate) fn kept_instance<T>(instances: &[T], dataset_id: impl Fn(&T) -> Option<&Identifier>) -> Option<&T> {
    instances.iter().find(|instance| dataset_id(instance).is_none()).or_else(|| instances.first())
}

/// Why a reformatted value could not be read back.
#[derive(Debug, Error)]
pub(crate) enum GeneralizationError {
    /// The value could not be written as JSON text.
    #[error("the reformatted value cannot be written: {0}")]
    Written(#[source] serde_json::Error),
    /// The written text is no admissible JSON value.
    #[error("the reformatted value is no JSON value: {0}")]
    Admitted(#[source] JsonLiteralError),
}

/// The value of a family Table 4.3.6.8-1 reformats as a Property -- a
/// language map, vocabulary terms, a JSON value or a value list -- as the JSON
/// it is written as, so clause 4.5.2.3 can read it the way it reads any
/// Property value.
#[derive(Debug)]
pub(crate) struct GeneralizedValue(JsonLiteral);

impl GeneralizedValue {
    /// The written form of `value`.
    ///
    /// # Errors
    ///
    /// Returns [`GeneralizationError`] when the value cannot be written as
    /// JSON text, which no held value causes.
    pub(crate) fn of(value: &FamilyValue<'_>) -> Result<GeneralizedValue, GeneralizationError> {
        let text = serde_json::to_string(value).map_err(GeneralizationError::Written)?;
        JsonLiteral::admit(&text).map(GeneralizedValue).map_err(GeneralizationError::Admitted)
    }

    /// The family the value, written bare in an attribute's place, reads back
    /// as the value of (clause 4.5.2.3), or `None` when it reads as something
    /// else.
    pub(crate) fn bare_family(&self) -> Option<AttributeType> {
        bare_value_family(self.0.piece())
    }

    /// The family a typeless instance carrying the value under `value` reads
    /// back as (clause 4.5.2.3, "type").
    pub(crate) fn typeless_family(&self) -> AttributeType {
        typeless_value_family(self.0.piece())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_body::instance_fallback::kept_instance;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    /// An instance named by its value, with or without a `datasetId`.
    struct Instance {
        value: u8,
        dataset_id: Option<Identifier>,
    }

    fn instance(value: u8, dataset_id: Option<&str>) -> Instance {
        Instance {
            value,
            dataset_id: dataset_id.map(|raw| raw.parse().expect("a dataset identifier")),
        }
    }

    fn kept(instances: &[Instance]) -> Option<u8> {
        kept_instance(instances, |instance| instance.dataset_id.as_ref()).map(|instance| instance.value)
    }

    /// Table 4.3.6.8-1 note 2: "preference to the default instance", wherever
    /// it stands among the instances.
    #[test]
    fn the_default_instance_is_kept_wherever_it_stands() {
        assert_eq!(
            kept(&[
                instance(1, Some("urn:ngsi-ld:Dataset:a")),
                instance(2, None),
                instance(3, Some("urn:ngsi-ld:Dataset:b"))
            ]),
            Some(2)
        );
    }

    /// Without a default instance the first held one stands in, whatever its
    /// `datasetId` sorts as.
    #[test]
    fn without_a_default_instance_the_first_held_instance_is_kept() {
        assert_eq!(
            kept(&[instance(1, Some("urn:ngsi-ld:Dataset:z")), instance(2, Some("urn:ngsi-ld:Dataset:a"))]),
            Some(1)
        );
        assert_eq!(kept(&[]), None);
    }
}
