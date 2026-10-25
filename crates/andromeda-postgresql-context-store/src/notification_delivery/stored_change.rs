//! Reading back one stored entity change: its lifecycle, its changed
//! instances and the system attributes of an Entity a deletion removed.
//!
//! A `notification_events` row and a `pending_change_event` record store a
//! change in the same columns under the same codes, so both read it back here.

use crate::notification_delivery::change_code::{change_kind_of, lifecycle_of};
use andromeda_ngsi_ld_entity::reads::{attribute_change::AttributeChange, attribute_instance_key::AttributeInstanceKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::IdentifierParseError};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use andromeda_postgresql_codes::{attribute_change_kind::AttributeChangeKindCode, entity_lifecycle::EntityLifecycleCode, registry::UnknownCompactCode};
use chrono::{DateTime, Utc};
use thiserror::Error;

/// Why a stored change does not read back as the change it was written as.
#[derive(Debug, Error)]
pub enum StoredChangeError {
    /// The stored number names no Entity lifecycle transition, which is what a
    /// row written by a release that knows more transitions looks like.
    #[error("carries an unreadable lifecycle")]
    UnknownLifecycle {
        /// Registry rejection naming the number.
        #[source]
        source: UnknownCompactCode,
    },
    /// A stored number names no Attribute instance transition.
    #[error("carries an unreadable instance transition")]
    UnknownChangeKind {
        /// Registry rejection naming the number.
        #[source]
        source: UnknownCompactCode,
    },
    /// The per-instance arrays disagree in length, so no instance can be
    /// told apart from its neighbour.
    #[error("carries {kinds} instance transitions for {attributes} attributes and {datasets} datasetIds")]
    ChangeArity {
        /// Number of stored transitions.
        kinds: usize,
        /// Number of attribute names the dictionary resolved.
        attributes: usize,
        /// Number of stored dataset discriminators.
        datasets: usize,
    },
    /// The stored dataset discriminator is not a valid URI.
    #[error("references an invalid datasetId")]
    InvalidDatasetId {
        /// URI validation failure.
        #[source]
        source: IdentifierParseError,
    },
    /// The removed entity's `createdAt` and `modifiedAt` are not stored
    /// together, which the stored form's own check rules out.
    #[error("carries an incomplete set of entity system attributes")]
    IncompleteSystemAttributes,
}

/// Validate the stored change: the lifecycle and, position for position, each
/// changed instance's transition, attribute and `datasetId`.
///
/// # Errors
///
/// Returns [`StoredChangeError`] when a code is unassigned, the per-instance
/// arrays disagree in length or a `datasetId` is no URI.
pub fn decode_change(
    lifecycle: i16,
    kinds: Vec<i16>,
    attributes: Vec<AttributeName>,
    datasets: Vec<Option<String>>,
) -> Result<EntityChange, StoredChangeError> {
    let lifecycle = EntityLifecycleCode::try_from(lifecycle)
        .map(lifecycle_of)
        .map_err(|source| StoredChangeError::UnknownLifecycle { source })?;
    if kinds.len() != attributes.len() || kinds.len() != datasets.len() {
        return Err(StoredChangeError::ChangeArity {
            kinds: kinds.len(),
            attributes: attributes.len(),
            datasets: datasets.len(),
        });
    }
    let instances = kinds
        .into_iter()
        .zip(attributes)
        .zip(datasets)
        .map(|((kind, attr), dataset)| {
            let kind = AttributeChangeKindCode::try_from(kind)
                .map(change_kind_of)
                .map_err(|source| StoredChangeError::UnknownChangeKind { source })?;
            let dataset_id = dataset
                .map(|value| value.parse().map_err(|source| StoredChangeError::InvalidDatasetId { source }))
                .transpose()?;
            Ok(AttributeChange {
                key: AttributeInstanceKey { attr, dataset_id },
                kind,
            })
        })
        .collect::<Result<Vec<AttributeChange>, StoredChangeError>>()?;
    Ok(EntityChange::new(lifecycle, instances))
}

/// Read the removed entity's stored system attributes back: none, or a
/// `createdAt` and `modifiedAt` together with the optional `expiresAt`.
///
/// # Errors
///
/// Returns [`StoredChangeError::IncompleteSystemAttributes`] when only one of
/// `createdAt` and `modifiedAt` is stored, or `expiresAt` without them.
pub const fn decode_system_attributes(
    created_at: Option<DateTime<Utc>>,
    modified_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<SysAttrsSnapshot>, StoredChangeError> {
    match (created_at, modified_at, expires_at) {
        (Some(created_at), Some(modified_at), expires_at) => Ok(Some(SysAttrsSnapshot {
            created_at,
            modified_at,
            expires_at,
        })),
        (None, None, None) => Ok(None),
        (Some(_), None, _) | (None, Some(_), _) | (None, None, Some(_)) => Err(StoredChangeError::IncompleteSystemAttributes),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_delivery::stored_change::{StoredChangeError, decode_change, decode_system_attributes};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use chrono::Utc;

    fn attribute(name: &str) -> AttributeName {
        name.parse().expect("a valid attribute name")
    }

    #[test]
    fn a_lifecycle_number_the_registry_never_assigned_is_rejected() {
        assert!(matches!(
            decode_change(99, Vec::new(), Vec::new(), Vec::new()),
            Err(StoredChangeError::UnknownLifecycle { .. })
        ));
    }

    #[test]
    fn per_instance_arrays_of_different_lengths_are_rejected() {
        assert!(matches!(
            decode_change(2, vec![2, 3], vec![attribute("temperature")], vec![None, None]),
            Err(StoredChangeError::ChangeArity {
                kinds: 2,
                attributes: 1,
                datasets: 2
            })
        ));
    }

    #[test]
    fn an_invalid_dataset_identifier_is_rejected() {
        assert!(matches!(
            decode_change(2, vec![2], vec![attribute("temperature")], vec![Some("not a uri".to_owned())]),
            Err(StoredChangeError::InvalidDatasetId { .. })
        ));
    }

    #[test]
    fn an_expiry_without_the_entitys_creation_instants_is_rejected() {
        assert!(matches!(
            decode_system_attributes(None, None, Some(Utc::now())),
            Err(StoredChangeError::IncompleteSystemAttributes)
        ));
    }
}
