//! HTTP request model for updating an EntityMap.

use chrono::{DateTime, Utc};
use serde::{Deserialize, de::IgnoredAny};

/// Request body for `PATCH /entityMaps/{id}` (clauses 5.14.2 and 6.32.3.2).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEntityMapBody {
    /// New expiration time.
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "entityMap")]
    _entity_map: Option<IgnoredAny>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "linkedMaps")]
    _linked_maps: Option<IgnoredAny>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "type")]
    _type: Option<IgnoredAny>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "id")]
    _id: Option<IgnoredAny>,
}

#[cfg(test)]
mod tests {
    use crate::body::entity_map::UpdateEntityMapBody;

    #[test]
    fn output_only_members_are_accepted_without_becoming_domain_values() {
        let body: UpdateEntityMapBody = serde_json::from_value(serde_json::json!({
            "expiresAt": "2026-01-01T00:00:00Z",
            "entityMap": {"anything": true},
            "linkedMaps": ["anything"],
            "type": "EntityMap",
            "id": "urn:ngsi-ld:EntityMap:one"
        }))
        .expect("output-only members are ignored");
        assert!(body.expires_at.is_some());
    }

    #[test]
    fn an_unknown_member_is_rejected() {
        assert!(serde_json::from_value::<UpdateEntityMapBody>(serde_json::json!({"bogus": true})).is_err());
    }
}
