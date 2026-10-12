//! The result of an attribute write that applied only part of what it was
//! asked to (clauses 5.2.18 and 5.2.19).

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use serde::{Deserialize, Serialize};

/// Clause 5.2.18 `UpdateResult`: the attributes a write updated and those it
/// did not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    /// The attributes actually updated.
    pub updated: Vec<AttributeName>,
    /// The attributes not updated, each with why.
    pub not_updated: Vec<NotUpdatedDetails>,
}

/// Clause 5.2.19 `NotUpdatedDetails`: one attribute a write did not update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotUpdatedDetails {
    /// The attribute.
    pub attribute_name: AttributeName,
    /// Why it was not updated, as human-readable text.
    pub reason: String,
    /// The registration whose distributed operation failed for the
    /// attribute; `None` when the attribute was not updated locally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_id: Option<Identifier>,
}

#[cfg(test)]
mod tests {
    use crate::update_result::{NotUpdatedDetails, UpdateResult};

    #[test]
    fn an_update_result_round_trips_its_members() {
        let text = r#"{"updated":["https://example.org/speed"],"notUpdated":[{"attributeName":"brand","reason":"Already exists"}]}"#;
        let result: UpdateResult = serde_json::from_str(text).expect("an UpdateResult");
        pretty_assertions::assert_eq!(
            result.not_updated,
            vec![NotUpdatedDetails {
                attribute_name: "brand".parse().expect("a name"),
                reason: "Already exists".to_owned(),
                registration_id: None,
            }]
        );
        pretty_assertions::assert_eq!(serde_json::to_string(&result).expect("writes"), text);
    }

    /// Table 5.2.19-1: `registrationId` names the registration of a failed
    /// distributed operation.
    #[test]
    fn a_not_updated_attribute_round_trips_the_registration_that_failed() {
        let text =
            r#"{"updated":[],"notUpdated":[{"attributeName":"speed","reason":"Bad Gateway","registrationId":"urn:ngsi-ld:ContextSourceRegistration:1"}]}"#;
        let result: UpdateResult = serde_json::from_str(text).expect("an UpdateResult");
        pretty_assertions::assert_eq!(
            result.not_updated[0].registration_id,
            Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("valid URI"))
        );
        pretty_assertions::assert_eq!(serde_json::to_string(&result).expect("writes"), text);
    }
}
