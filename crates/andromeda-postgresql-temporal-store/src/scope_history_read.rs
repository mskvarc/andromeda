//! Reading the reified temporal scope of one entity back out of `scope_history`.
//!
//! Clause 4.18 reifies the Scope in the temporal representation of an Entity
//! and gives that Entity at most one instance of the scope Property, so one
//! write is one rendered instance however many Scopes it named: "The value of
//! scope is represented as a JSON array in case there is more than one Scope."
//! Storage spreads a multi-Scope write over one row per path, because the
//! `ltree` column holds a single label path, so the rows of a write are folded
//! back together here.
//!
//! Clause 4.5.6 makes the reified form "a temporal representation of a Property
//! (clause 4.5.7)", which is why a folded event leaves this module as an
//! ordinary [`TemporalHistoryRow`] keyed by the scope IRI: the renderer has one
//! path for every attribute-shaped row and the Scope travels down it rather
//! than down a second one of its own.

use crate::{error::TemporalStoreError, scope_history_value::ScopeHistoryValue, semantic_time_axes::SemanticTimeAxes};
use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_ngsi_ld_model::{
    scope_instance_value::scope_instance_value,
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_codes::scope_value_kind::ScopeValueKind;
use andromeda_postgresql_query::ltree::ltree_to_scope;
use andromeda_postgresql_temporal_query::scope_history::{SCOPE_EVENT_AT, SCOPE_PATH_TEXT};
use andromeda_temporal::{append_intent::TemporalObservationIdentity, port::history_reader::TemporalHistoryObservation};
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// One decoded `scope_history` row, before the rows of one event are folded
/// into the single instance clause 4.18 allows an Entity.
#[derive(Debug)]
struct StoredScopeRow {
    /// The atomic write the row belongs to, and the row's place within it.
    identity: TemporalObservationIdentity,
    /// The row's instant on the axis the request selected, which is what
    /// separates one scope event from the next inside a write.
    event_at: DateTime<Utc>,
    /// The instant the broker durably accepted the write.
    recorded_at: DateTime<Utc>,
    /// The NGSI-LD time axes the row declares (clause 4.5.6).
    axes: SemanticTimeAxes,
    /// What the row records: one scope path, or the removal of the scope.
    value: ScopeHistoryValue,
    /// The identifier the instance is published to clients under.
    instance: InstanceIdentity,
}

/// Decode the scope statement's rows into the reified scope instances the
/// temporal representation carries.
///
/// The statement orders the rows so that the rows of one event arrive together
/// and in write order, so folding is over consecutive runs rather than over a
/// map. The run is keyed by the write the rows belong to *and* the instant they
/// share on the requested axis: clause 4.18 renders the several Scopes of one
/// write as one instance holding an array, while clause 4.5.7 lets that same
/// write state several instances, "each one representing an instance of the
/// referred Property [...] at a particular point in time".
///
/// # Errors
///
/// Returns [`TemporalStoreError::Decode`] when a row carries a value shape, a
/// label path, an instance identity or a deletion instant that `scope_history`
/// cannot have written.
pub(crate) fn decode_scope_history(rows: &[PgRow]) -> Result<Vec<TemporalHistoryObservation>, TemporalStoreError> {
    let scope_attribute: AttributeName = expanded_properties::SCOPE
        .as_str()
        .parse()
        .map_err(|error| TemporalStoreError::Decode(format!("the scope IRI does not name an attribute: {error}")))?;

    let mut decoded = Vec::with_capacity(rows.len());
    for row in rows {
        decoded.push(decode_scope_row(row)?);
    }
    let decoded = decoded;

    let mut events: Vec<TemporalHistoryObservation> = Vec::new();
    let mut event: Vec<&StoredScopeRow> = Vec::new();
    for row in &decoded {
        if event
            .first()
            .is_some_and(|first| first.identity.write_id != row.identity.write_id || first.event_at != row.event_at)
        {
            events.push(fold_event(&event, &scope_attribute)?);
            event.clear();
        }
        event.push(row);
    }
    if !event.is_empty() {
        events.push(fold_event(&event, &scope_attribute)?);
    }
    Ok(events)
}

/// Fold the rows of one scope event into the one instance it recorded.
///
/// The instance identity and the four Temporal Properties come from the event's
/// first row: every row of an event shares them, because clause 4.5.6 has the
/// broker set them from the write itself rather than per path.
fn fold_event(event: &[&StoredScopeRow], scope_attribute: &AttributeName) -> Result<TemporalHistoryObservation, TemporalStoreError> {
    let write = event;
    let first = write
        .first()
        .ok_or_else(|| TemporalStoreError::Decode("a scope event was folded from no rows at all".to_owned()))?;

    let row = TemporalHistoryRow {
        attribute_name: scope_attribute.clone(), // clone: every folded instance owns the name it is keyed by.
        // Annex C clause C.5.16 renders the reified scope as `"type":
        // "Property"`, which clause 4.5.6 mandates by making it "a temporal
        // representation of a Property (clause 4.5.7)".
        attribute_type: AttributeType::Property,
        // Clause 4.5.4 gives a Scope no dataset dimension, so there is no
        // multi-attribute discriminator to carry.
        dataset_id: None,
        value: scope_value(write)?,
        observed_at: first.axes.observed_at,
        recorded_at: first.recorded_at,
        created_at: first.axes.created_at,
        modified_at: first.axes.modified_at,
        instance: first.instance.clone(), // clone: the folded instance owns its published identity.
        // Clause 4.5.6 restricts the reified scope to the four non-reified
        // Temporal Properties, so it carries no sub-attributes to merge.
        metadata: None,
        // `lastN` does not bound the Scope (clause 5.7.4.4 defines it over
        // Attribute instances), so there is no partition to report a total for.
        partition_total: None,
    };
    Ok(TemporalHistoryObservation {
        row,
        identity: Some(first.identity),
    })
}

/// The value one write's rows amount to.
///
/// Clause 4.18: "The value of scope is represented as a JSON array in case
/// there is more than one Scope", so a single Scope is the bare path and
/// several are the array of them, in the order the write recorded them. A
/// removal is its own row and never shares a write with a path — clause 4.18
/// admits the NGSI-LD Null "only in case of deleted scopes" — so a write whose
/// first row is a removal is a removal.
fn scope_value(write: &[&StoredScopeRow]) -> Result<TemporalValue, TemporalStoreError> {
    let mut paths = Vec::with_capacity(write.len());
    for row in write {
        match &row.value {
            ScopeHistoryValue::Tombstone { deleted_at } => return Ok(TemporalValue::Tombstone { deleted_at: *deleted_at }),
            ScopeHistoryValue::Path(path) => paths.push(path.clone()), // clone: the folded value owns the paths the rows hold.
        }
    }
    let paths = paths;
    scope_instance_value(&paths).ok_or_else(|| TemporalStoreError::Decode("a scope write recorded neither a path nor a removal".to_owned()))
}

/// Decode one `scope_history` row.
fn decode_scope_row(row: &PgRow) -> Result<StoredScopeRow, TemporalStoreError> {
    Ok(StoredScopeRow {
        identity: decode_write_identity(row)?,
        event_at: required_instant(row, SCOPE_EVENT_AT)?,
        recorded_at: required_instant(row, "recorded_at")?,
        // Each axis is read from its own column: a row that recorded no instant
        // on an axis keeps none, because the operational write clock is not a
        // Temporal Property (clauses 4.11 and 5.2.21).
        axes: SemanticTimeAxes {
            observed_at: nullable_instant(row, "observed_at")?,
            created_at: nullable_instant(row, "created_at")?,
            modified_at: nullable_instant(row, "modified_at")?,
        },
        value: decode_scope_value(row)?,
        instance: decode_instance_identity(row)?,
    })
}

/// Read the row's value from the column its stored value kind names.
fn decode_scope_value(row: &PgRow) -> Result<ScopeHistoryValue, TemporalStoreError> {
    let stored: i16 = row
        .try_get("value_kind")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.value_kind: {error}")))?;
    let kind = ScopeValueKind::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))?;
    match kind {
        ScopeValueKind::Tombstone => Ok(ScopeHistoryValue::Tombstone {
            deleted_at: required_instant(row, "deleted_at")?,
        }),
        ScopeValueKind::ScopePath => {
            let stored: String = row
                .try_get::<Option<String>, _>(SCOPE_PATH_TEXT)
                .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.scope_path: {error}")))?
                .ok_or_else(|| TemporalStoreError::Decode("scope_history.scope_path is empty but the row declares a path".to_owned()))?;
            ltree_to_scope(&stored)
                .map(ScopeHistoryValue::Path)
                .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.scope_path '{stored}': {error}")))
        }
    }
}

/// Reassemble the identifier this scope instance is published to clients under.
///
/// The two identifier columns are mutually exclusive by check constraint, so a
/// row populating both or neither is corrupt rather than ambiguous, and is
/// reported instead of being resolved by precedence.
fn decode_instance_identity(row: &PgRow) -> Result<InstanceIdentity, TemporalStoreError> {
    let generated: Option<Uuid> = row
        .try_get("generated_instance_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.generated_instance_id: {error}")))?;
    let verbatim: Option<String> = row
        .try_get("verbatim_instance_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.verbatim_instance_id: {error}")))?;
    match (generated, verbatim) {
        (Some(uuid), None) => Ok(InstanceIdentity::Generated(uuid)),
        (None, Some(raw)) => Ok(InstanceIdentity::Verbatim(raw.parse::<Identifier>().map_err(|error| {
            TemporalStoreError::Decode(format!("invalid scope_history.verbatim_instance_id '{raw}': {error}"))
        })?)),
        (Some(_), Some(_)) => Err(TemporalStoreError::Decode(
            "scope_history row populates both instance identifier columns".to_owned(),
        )),
        (None, None) => Err(TemporalStoreError::Decode(
            "scope_history row populates neither instance identifier column".to_owned(),
        )),
    }
}

/// Read the identity of the atomic write that produced the row.
fn decode_write_identity(row: &PgRow) -> Result<TemporalObservationIdentity, TemporalStoreError> {
    let write_id: Uuid = row
        .try_get("write_id")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.write_id: {error}")))?;
    let ordinal: i32 = row
        .try_get("write_ordinal")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.write_ordinal: {error}")))?;
    Ok(TemporalObservationIdentity {
        write_id: WriteId::from_uuid(write_id),
        ordinal: WriteOrdinal::try_from(ordinal).map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.write_ordinal: {error}")))?,
    })
}

/// Read a non-null timestamp column.
fn required_instant(row: &PgRow, column: &str) -> Result<DateTime<Utc>, TemporalStoreError> {
    row.try_get::<Option<DateTime<Utc>>, _>(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.{column}: {error}")))?
        .ok_or_else(|| TemporalStoreError::Decode(format!("scope_history.{column} is absent and has no substitute")))
}

/// Read a nullable timestamp column, preserving its absence.
fn nullable_instant(row: &PgRow, column: &str) -> Result<Option<DateTime<Utc>>, TemporalStoreError> {
    row.try_get::<Option<DateTime<Utc>>, _>(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid scope_history.{column}: {error}")))
}

#[cfg(test)]
mod tests {
    use crate::{
        scope_history_read::{StoredScopeRow, fold_event},
        scope_history_value::ScopeHistoryValue,
        semantic_time_axes::SemanticTimeAxes,
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        instance_identity::InstanceIdentity,
        scope::path::ScopePath,
    };
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use andromeda_temporal::append_intent::TemporalObservationIdentity;
    use chrono::{DateTime, TimeZone, Utc};
    use uuid::Uuid;

    fn instant(epoch_seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(epoch_seconds, 0).single().expect("a representable instant")
    }

    fn scope_attribute() -> AttributeName {
        expanded_properties::SCOPE.as_str().parse().expect("the scope IRI names an attribute")
    }

    fn stored(ordinal: u32, value: ScopeHistoryValue) -> StoredScopeRow {
        StoredScopeRow {
            identity: TemporalObservationIdentity {
                write_id: WriteId::from_uuid(Uuid::from_u128(7)),
                ordinal: WriteOrdinal::try_from(i32::try_from(ordinal).expect("a small ordinal")).expect("a valid ordinal"),
            },
            event_at: instant(1_533_121_200),
            recorded_at: instant(1_533_121_200),
            axes: SemanticTimeAxes {
                observed_at: Some(instant(1_533_121_200)),
                created_at: Some(instant(1_533_121_200)),
                modified_at: Some(instant(1_533_121_200)),
            },
            value,
            instance: InstanceIdentity::Generated(Uuid::from_u128(u128::from(ordinal) + 1)),
        }
    }

    fn scope_path(raw: &str) -> ScopePath {
        raw.parse::<ScopePath>().expect("a valid scope path")
    }

    fn path(raw: &str) -> ScopeHistoryValue {
        ScopeHistoryValue::Path(scope_path(raw))
    }

    /// Clause 4.18: an Entity assigned to one Scope carries that Scope as the
    /// bare value, which is how annex C clause C.5.16 renders Vehicle B9211.
    #[test]
    fn a_write_naming_one_scope_folds_into_the_bare_path() {
        let rows = [stored(0, path("/Madrid/Centro"))];
        let folded = fold_event(&rows.iter().collect::<Vec<_>>(), &scope_attribute()).expect("a folded scope event");
        pretty_assertions::assert_eq!(folded.row.value, TemporalValue::Scopes(vec![scope_path("/Madrid/Centro")]));
        pretty_assertions::assert_eq!(folded.row.attribute_type, AttributeType::Property);
        pretty_assertions::assert_eq!(folded.row.attribute_name, scope_attribute());
        assert!(folded.row.dataset_id.is_none(), "clause 4.5.4 gives a Scope no dataset dimension");
    }

    /// Clause 4.18: "The value of scope is represented as a JSON array in case
    /// there is more than one Scope", which annex C clause C.5.16 renders for
    /// Vehicle A8311 as one instance holding both of its Scopes.
    #[test]
    fn a_write_naming_several_scopes_folds_into_one_instance_holding_the_array() {
        let rows = [stored(0, path("/Madrid/Centro")), stored(1, path("/Company123/UnitA"))];
        let folded = fold_event(&rows.iter().collect::<Vec<_>>(), &scope_attribute()).expect("a folded scope event");
        pretty_assertions::assert_eq!(
            folded.row.value,
            TemporalValue::Scopes(vec![scope_path("/Madrid/Centro"), scope_path("/Company123/UnitA")])
        );
    }

    /// Clause 4.18 reserves the NGSI-LD Null for a deleted scope, which the
    /// store holds as a removal row and the renderer turns back into the null
    /// value beside its `deletedAt`.
    #[test]
    fn a_removal_folds_into_the_tombstone_that_carries_its_instant() {
        let deleted_at = instant(1_533_124_800);
        let rows = [stored(0, ScopeHistoryValue::Tombstone { deleted_at })];
        let folded = fold_event(&rows.iter().collect::<Vec<_>>(), &scope_attribute()).expect("a folded scope event");
        pretty_assertions::assert_eq!(folded.row.value, TemporalValue::Tombstone { deleted_at });
    }

    /// Clause 4.5.6 gives the reified scope the four non-reified Temporal
    /// Properties and nothing else, so the folded instance carries the axes the
    /// write declared and no sub-attributes.
    #[test]
    fn the_folded_instance_carries_the_axes_the_write_declared_and_no_sub_attributes() {
        let rows = [stored(0, path("/Madrid/Centro"))];
        let folded = fold_event(&rows.iter().collect::<Vec<_>>(), &scope_attribute()).expect("a folded scope event");
        pretty_assertions::assert_eq!(folded.row.observed_at, Some(instant(1_533_121_200)));
        pretty_assertions::assert_eq!(folded.row.created_at, Some(instant(1_533_121_200)));
        pretty_assertions::assert_eq!(folded.row.modified_at, Some(instant(1_533_121_200)));
        assert!(folded.row.metadata.is_none());
        assert!(folded.row.partition_total.is_none());
    }

    /// Clause 4.18 gives an Entity at most one instance of the scope Property,
    /// so the several rows of one write publish one identifier rather than one
    /// each.
    #[test]
    fn one_write_publishes_one_instance_identifier() {
        let rows = [stored(0, path("/Madrid/Centro")), stored(1, path("/Company123/UnitA"))];
        let folded = fold_event(&rows.iter().collect::<Vec<_>>(), &scope_attribute()).expect("a folded scope event");
        pretty_assertions::assert_eq!(folded.row.instance, InstanceIdentity::Generated(Uuid::from_u128(1)));
    }
}
