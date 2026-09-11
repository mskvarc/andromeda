//! Typed subscription persistence values and update merging.

use crate::{error::SubscriptionError, lifecycle::DatasetIdPolicy, store::SubscriptionFields};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, spec_version::SpecVersion, uri::Uri};
use andromeda_ngsi_ld_model::{
    attribute_name_csv,
    notification::NotificationTrigger,
    subscription::{EntitySelector, NotificationParams, Subscription},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::str::FromStr;
use url::Url;

/// Owned, validated values for one subscription persistence write.
///
/// This shape mirrors the subscription table boundary. Domain-valued members
/// have already been parsed; only human-readable text and opaque JSON members
/// remain textual.
#[derive(Debug)]
pub struct SubscriptionWriteValues {
    /// Optional human-readable subscription name.
    pub subscription_name: Option<String>,
    /// Optional free-text description.
    pub description: Option<String>,
    /// Validated entity selectors.
    pub entities: Option<Vec<EntitySelector>>,
    /// Validated watched attributes.
    pub watched_attributes: Option<Vec<AttributeName>>,
    /// Validated notification triggers.
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
    /// Periodic notification interval in seconds.
    pub time_interval: Option<f64>,
    /// Parsed NGSI-LD query expression.
    pub q: Option<Expr>,
    /// Geo-spatial query object.
    pub geo_q: Option<Value>,
    /// Parsed scope query expression.
    pub scope_q: Option<ScopeQuery>,
    /// Parsed context-source filter expression.
    pub csf: Option<Expr>,
    /// Validated dataset identifiers.
    pub dataset_id: Option<Vec<Uri>>,
    /// Whether the subscription is currently active.
    pub is_active: bool,
    /// Validated notification delivery parameters.
    pub notification: Option<NotificationParams>,
    /// Expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval between notifications in seconds.
    pub throttling: Option<f64>,
    /// Temporal query object.
    pub temporal_q: Option<Value>,
    /// Attributes whose `q` operands were coerced through the `@context`
    /// (Table 5.2.12-1 `expandValues`).
    pub expand_values: Option<Vec<AttributeName>>,
    /// Attributes whose `q` object keys were expanded while their values were
    /// left uninterpreted (Table 5.2.12-1 `jsonKeys`).
    pub json_keys: Option<Vec<AttributeName>>,
    /// Validated preferred language.
    pub lang: Option<LangTagBuf>,
    /// Validated JSON-LD context URL.
    pub jsonld_context: Option<Url>,
    /// Whether the subscription applies only to local entities.
    pub local_only: bool,
    /// Whether entities are assumed split across sources; `None` defers to
    /// the deployment default (clause 5.2.12 `splitEntities`).
    pub split_entities: Option<bool>,
    /// The NGSI-LD version a notification's Entities are amended to conform to
    /// (Table 5.2.12-1 `ngsildConformance`); `None` amends nothing.
    pub ngsild_conformance: Option<SpecVersion>,
}

impl SubscriptionWriteValues {
    /// Borrow the values in the shape required by the persistence port.
    pub fn to_fields(&self) -> SubscriptionFields<'_> {
        SubscriptionFields {
            subscription_name: self.subscription_name.as_deref(),
            description: self.description.as_deref(),
            entities: self.entities.as_deref(),
            watched_attributes: self.watched_attributes.as_deref(),
            notification_trigger: self.notification_trigger.as_deref(),
            time_interval: self.time_interval,
            q: self.q.as_ref(),
            geo_q: self.geo_q.as_ref(),
            scope_q: self.scope_q.as_ref(),
            csf: self.csf.as_ref(),
            dataset_id: self.dataset_id.as_deref(),
            is_active: self.is_active,
            notification: self.notification.as_ref(),
            expires_at: self.expires_at,
            throttling: self.throttling,
            temporal_q: self.temporal_q.as_ref(),
            expand_values: self.expand_values.as_deref(),
            json_keys: self.json_keys.as_deref(),
            lang: self.lang.as_ref(),
            jsonld_context: self.jsonld_context.as_ref(),
            local_only: self.local_only,
            split_entities: self.split_entities,
            ngsild_conformance: self.ngsild_conformance,
        }
    }
}

fn deserialize_member<T>(payload: &Value, member: &str) -> Result<Option<T>, SubscriptionError>
where
    T: DeserializeOwned,
{
    payload
        .get(member)
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|error| {
                // clone: serde owns the selected JSON member while the request
                // payload remains available to the other field parsers.
                SubscriptionError::validation(format!("Invalid '{member}' value: {error}"))
            })
        })
        .transpose()
}

/// Read one comma-separated attribute-name member of a payload.
///
/// Table 5.2.12-1 types `expandValues` and `jsonKeys` as a single `String`
/// holding a comma separated list, which is the encoding
/// [`attribute_name_csv`] owns; an absent member stays absent so an update
/// fragment that does not mention it leaves the stored list alone.
///
/// # Errors
///
/// Returns [`SubscriptionError::Validation`] when the member is not a string,
/// or names something outside the NGSI-LD attribute-name grammar.
pub(crate) fn attribute_name_csv_member(payload: &Value, member: &str) -> Result<Option<Vec<AttributeName>>, SubscriptionError> {
    let Some(value) = payload.get(member) else {
        return Ok(None);
    };
    attribute_name_csv::deserialize(value).map_err(|error| SubscriptionError::validation(format!("Invalid '{member}' value: {error}")))
}

/// Merge one comma-separated attribute-name member of an update fragment over
/// the list already stored (Table 5.2.12-1 `expandValues` / `jsonKeys`).
///
/// Clause 5.8.3 updates a Subscription with "a fragment of the Subscription",
/// so a member the fragment does not mention leaves the stored list alone. An
/// explicit `null` is how the fragment names a member it wants gone -- the same
/// way `geoQ` and `temporalQ` are removed -- and without it an opt-in list
/// could be replaced but never cleared. Anything else replaces the stored list
/// once [`attribute_name_csv_member`] has validated it.
fn merged_attribute_name_csv(payload: &Value, member: &str, stored: Option<Vec<AttributeName>>) -> Result<Option<Vec<AttributeName>>, SubscriptionError> {
    match payload.get(member) {
        None => Ok(stored),
        Some(Value::Null) => Ok(None),
        Some(Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => attribute_name_csv_member(payload, member),
    }
}

fn expiration(payload: &Value) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
    payload
        .get("expiresAt")
        .and_then(Value::as_str)
        .map(|raw| {
            DateTime::parse_from_rfc3339(raw)
                .map(|date_time| date_time.with_timezone(&Utc))
                .map_err(|error| SubscriptionError::validation(format!("Invalid 'expiresAt' value '{raw}': {error}")))
        })
        .transpose()
}

/// Read the `ngsildConformance` member of a subscription payload.
///
/// Table 5.2.12-1 types the member as "a semantically versioned string in the
/// form major.minor, which conforms to a version of the NGSI-LD
/// specification", so a bare major, a patch component, a non-numeric token and
/// a non-string are each syntactically incorrect according to the API data type
/// definition and refused as clause 5.5.4 `BadRequestData`. Silently dropping
/// one would leave a client that asked for an older payload receiving the
/// current one with no indication that its request was not honoured.
fn ngsild_conformance(payload: &Value) -> Result<Option<SpecVersion>, SubscriptionError> {
    let Some(value) = payload.get("ngsildConformance") else {
        return Ok(None);
    };
    let raw = value
        .as_str()
        .ok_or_else(|| SubscriptionError::validation("'ngsildConformance' must be a string of the form major.minor"))?;
    SpecVersion::from_str(raw)
        .map(Some)
        .map_err(|error| SubscriptionError::validation(format!("Invalid 'ngsildConformance' value: {error}")))
}

/// Parse a create payload into validated persistence values.
pub fn extract_create_fields(payload: &Value, dataset_id_policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
    Ok(SubscriptionWriteValues {
        subscription_name: payload.get("subscriptionName").and_then(Value::as_str).map(String::from),
        description: payload.get("description").and_then(Value::as_str).map(String::from),
        entities: deserialize_member(payload, "entities")?,
        watched_attributes: deserialize_member(payload, "watchedAttributes")?,
        notification_trigger: deserialize_member(payload, "notificationTrigger")?,
        time_interval: payload.get("timeInterval").and_then(Value::as_f64),
        q: deserialize_member(payload, "q")?,
        geo_q: payload.get("geoQ").cloned(), // clone: the write values own request JSON across the database await.
        scope_q: deserialize_member(payload, "scopeQ")?,
        csf: deserialize_member(payload, "csf")?,
        dataset_id: match dataset_id_policy {
            DatasetIdPolicy::Retain => deserialize_member(payload, "datasetId")?,
            DatasetIdPolicy::Discard => None,
        },
        is_active: payload.get("isActive").and_then(Value::as_bool).unwrap_or(true),
        notification: deserialize_member(payload, "notification")?,
        expires_at: expiration(payload)?,
        throttling: payload.get("throttling").and_then(Value::as_f64),
        temporal_q: payload.get("temporalQ").cloned(), // clone: the write values own request JSON across the database await.
        expand_values: attribute_name_csv_member(payload, "expandValues")?,
        json_keys: attribute_name_csv_member(payload, "jsonKeys")?,
        lang: deserialize_member(payload, "lang")?,
        jsonld_context: deserialize_member(payload, "jsonldContext")?,
        local_only: payload.get("localOnly").and_then(Value::as_bool).unwrap_or(false),
        split_entities: payload.get("splitEntities").and_then(Value::as_bool),
        ngsild_conformance: ngsild_conformance(payload)?,
    })
}

/// Merge an update fragment with an existing subscription into validated
/// persistence values.
pub fn merge_update_fields(payload: &Value, existing: Subscription, dataset_id_policy: DatasetIdPolicy) -> Result<SubscriptionWriteValues, SubscriptionError> {
    let Subscription {
        subscription_name,
        description,
        selection,
        filters,
        notification,
        delivery,
        lifecycle,
        ..
    } = existing;

    Ok(SubscriptionWriteValues {
        subscription_name: payload.get("subscriptionName").and_then(Value::as_str).map(String::from).or(subscription_name),
        description: payload.get("description").and_then(Value::as_str).map(String::from).or(description),
        entities: deserialize_member(payload, "entities")?.or(selection.entities),
        watched_attributes: deserialize_member(payload, "watchedAttributes")?.or(selection.watched_attributes),
        notification_trigger: deserialize_member(payload, "notificationTrigger")?.or(selection.notification_trigger),
        time_interval: payload.get("timeInterval").and_then(Value::as_f64).or(delivery.time_interval),
        q: deserialize_member(payload, "q")?.or(filters.q),
        geo_q: payload
            .get("geoQ")
            .cloned() // clone: the merged values own a member borrowed from the update payload.
            .or(filters.geo_q)
            .filter(|value| !value.is_null()),
        scope_q: deserialize_member(payload, "scopeQ")?.or(filters.scope_q),
        csf: deserialize_member(payload, "csf")?.or(filters.csf),
        dataset_id: match dataset_id_policy {
            DatasetIdPolicy::Retain => deserialize_member(payload, "datasetId")?.or(filters.dataset_id),
            DatasetIdPolicy::Discard => None,
        },
        is_active: payload.get("isActive").and_then(Value::as_bool).or(lifecycle.is_active).unwrap_or(true),
        notification: deserialize_member(payload, "notification")?.or(Some(notification)),
        expires_at: expiration(payload)?.or(lifecycle.expires_at),
        throttling: payload.get("throttling").and_then(Value::as_f64).or(delivery.throttling),
        temporal_q: payload
            .get("temporalQ")
            .cloned() // clone: the merged values own a member borrowed from the update payload.
            .or(filters.temporal_q)
            .filter(|value| !value.is_null()),
        expand_values: merged_attribute_name_csv(payload, "expandValues", filters.expand_values)?,
        json_keys: merged_attribute_name_csv(payload, "jsonKeys", filters.json_keys)?,
        lang: deserialize_member(payload, "lang")?.or(delivery.lang),
        jsonld_context: deserialize_member(payload, "jsonldContext")?.or(delivery.jsonld_context),
        local_only: payload.get("localOnly").and_then(Value::as_bool).or(delivery.local_only).unwrap_or(false),
        split_entities: payload.get("splitEntities").and_then(Value::as_bool).or(delivery.split_entities),
        ngsild_conformance: ngsild_conformance(payload)?.or(delivery.ngsild_conformance),
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SubscriptionError,
        fields::{extract_create_fields, merged_attribute_name_csv},
        lifecycle::DatasetIdPolicy,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, spec_version::SpecVersion};

    #[test]
    fn entity_subscription_creation_retains_dataset_identifiers() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({"datasetId": ["urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Retain)?;

        assert_eq!(
            values
                .dataset_id
                .as_deref()
                .and_then(|identifiers| identifiers.first())
                .map(AsRef::<str>::as_ref),
            Some("urn:ngsi-ld:Dataset:one")
        );
        Ok(())
    }

    #[test]
    fn context_source_subscription_creation_discards_dataset_identifiers() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({"datasetId": ["urn:ngsi-ld:Dataset:one"]}), DatasetIdPolicy::Discard)?;

        assert!(values.dataset_id.is_none());
        Ok(())
    }

    /// The stored opt-in list an update fragment is merged over.
    fn stored_names() -> Vec<AttributeName> {
        vec!["status".parse().expect("a valid attribute name")]
    }

    /// One merged opt-in list, as wire text.
    fn spelled(names: Option<&[AttributeName]>) -> Vec<&str> {
        names.unwrap_or_default().iter().map(AttributeName::as_str).collect()
    }

    /// Clause 5.8.3 updates a Subscription with "a fragment of the
    /// Subscription", so a member the fragment does not mention is left alone.
    #[test]
    fn an_update_that_names_no_opt_in_leaves_the_stored_lists_alone() -> Result<(), SubscriptionError> {
        let merged = merged_attribute_name_csv(&serde_json::json!({}), "expandValues", Some(stored_names()))?;

        pretty_assertions::assert_eq!(spelled(merged.as_deref()), vec!["status"]);
        Ok(())
    }

    /// An explicit `null` is how a fragment names a member it wants gone, the
    /// same way `geoQ` and `temporalQ` are removed. Without it an opt-in list
    /// could be replaced but never cleared.
    #[test]
    fn an_explicit_null_clears_a_stored_opt_in_list() -> Result<(), SubscriptionError> {
        let merged = merged_attribute_name_csv(&serde_json::json!({"expandValues": null}), "expandValues", Some(stored_names()))?;

        assert!(merged.is_none(), "an explicit null removes the member");
        Ok(())
    }

    /// Anything else replaces the stored list, once the comma-separated form
    /// Table 5.2.12-1 defines has been validated.
    #[test]
    fn a_named_list_replaces_the_stored_one() -> Result<(), SubscriptionError> {
        let merged = merged_attribute_name_csv(&serde_json::json!({"jsonKeys": "payload,extra"}), "jsonKeys", Some(stored_names()))?;

        pretty_assertions::assert_eq!(spelled(merged.as_deref()), vec!["payload", "extra"]);
        Ok(())
    }

    #[test]
    fn malformed_expiration_is_rejected() {
        let result = extract_create_fields(&serde_json::json!({"expiresAt": "tomorrow"}), DatasetIdPolicy::Retain);
        assert!(matches!(result, Err(SubscriptionError::Validation { .. })));
    }

    /// Table 5.2.12-1 gives `ngsildConformance` the form `major.minor`, so a
    /// well-formed one is carried through to the write values.
    #[test]
    fn a_well_formed_ngsild_conformance_is_parsed_into_a_spec_version() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({"ngsildConformance": "1.4"}), DatasetIdPolicy::Retain)?;

        pretty_assertions::assert_eq!(values.ngsild_conformance, Some(SpecVersion::V1_4));
        Ok(())
    }

    /// A version the fallback tables of clause 4.3.6.8 do not name is still of
    /// the form the table defines, so it is admitted; clause 4.3.6.8
    /// introduces each fallback at a version and applies it to everything
    /// older, which is what decides what `1.5` receives.
    #[test]
    fn a_version_the_fallback_tables_do_not_name_is_still_admitted() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({"ngsildConformance": "1.5"}), DatasetIdPolicy::Retain)?;

        pretty_assertions::assert_eq!(values.ngsild_conformance, Some(SpecVersion::new(1, 5)));
        Ok(())
    }

    /// Anything outside the `major.minor` form Table 5.2.12-1 defines is
    /// syntactically incorrect according to the API data type definition, and
    /// clause 5.5.4 makes that `BadRequestData` rather than a dropped member.
    #[test]
    fn a_malformed_ngsild_conformance_is_rejected() {
        for token in [
            serde_json::json!("1"),
            serde_json::json!("1.2.3"),
            serde_json::json!("abc"),
            serde_json::json!(""),
            serde_json::json!("v1.4"),
            serde_json::json!(1.4),
            serde_json::json!(null),
        ] {
            let result = extract_create_fields(&serde_json::json!({"ngsildConformance": token}), DatasetIdPolicy::Retain);
            assert!(matches!(result, Err(SubscriptionError::Validation { .. })), "{token} must be refused");
        }
    }

    /// An absent member amends nothing, and there is no default version to
    /// fall back to.
    #[test]
    fn an_absent_ngsild_conformance_leaves_no_version() -> Result<(), SubscriptionError> {
        let values = extract_create_fields(&serde_json::json!({}), DatasetIdPolicy::Retain)?;

        assert!(values.ngsild_conformance.is_none());
        Ok(())
    }
}
