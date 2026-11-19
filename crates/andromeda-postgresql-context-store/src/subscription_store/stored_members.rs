//! The members of a stored subscription that are read in the expanded stage.
//!
//! The broker stores a member only after expanding its names, so a stored name is an absolute IRI. A column that holds a name that is not one was not written by the broker, and the subscription it belongs to can no longer be matched by that member. Reading it does not fail the row: the member is left out, the failure is logged, and the subscription lists the member as unreadable, which the matcher answers by selecting nothing. A column that is no member of its kind at all stays a failure of the row.

use crate::{
    error::ContextStoreError,
    json_text::JsonText,
    subscription_store::{mapper::decoded_column, stored_subscription_error::StoredSubscriptionError},
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::{entity_selector::EntitySelector, subscription::unreadable_member::UnreadableMember};
use andromeda_ngsi_ld_query::{
    filter::{Filter, filter_error::FilterError},
    geo::geo_constraint::GeoConstraint,
    name_stage::{Expanded, Written},
};
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use sea_query::Iden;
use serde::de::DeserializeOwned;
use sqlx::{Row, postgres::PgRow};
use std::fmt::Display;
use tracing::error;

/// What the JSON text of a column holds, read in the expanded stage.
#[derive(Debug, PartialEq)]
pub(super) enum StagedColumn<E> {
    /// The member, or none for the JSON `null` literal.
    Read(Option<E>),
    /// A member of its kind that names something that is no expanded name.
    NotExpanded,
}

/// The value the JSON text of a column holds in the expanded stage `E`, `W` being the same member in the written stage: a text that reads only as `W` names something that is no expanded name.
///
/// # Errors
///
/// Returns `malformed` of the column when the text is no `W` either.
pub(super) fn decoded_staged_column<E: DeserializeOwned, W: DeserializeOwned>(
    text: JsonText<'_>,
    malformed: fn(serde_json::Error) -> StoredSubscriptionError,
) -> Result<StagedColumn<E>, StoredSubscriptionError> {
    match decoded_column::<E>(text, malformed) {
        Ok(expanded) => Ok(StagedColumn::Read(expanded)),
        Err(not_expanded) => decoded_column::<W>(text, malformed)
            .map(|_written| StagedColumn::NotExpanded)
            .map_err(|_not_a_member| not_expanded),
    }
}

/// The stored subscription a member is read for, and the members it could not be read in.
pub(super) struct MemberReading<'a> {
    /// The identifier the log lines name.
    pub(super) subscription: &'a Identifier,
    /// The members that could not be read in the expanded stage.
    pub(super) unreadable: Vec<UnreadableMember>,
}

impl MemberReading<'_> {
    /// Record `member` as unreadable, logging `cause`.
    fn give_up(&mut self, member: UnreadableMember, cause: &dyn Display) {
        error!(subscription = %self.subscription, %member, %cause, "a stored member is not in the expanded stage");
        self.unreadable.push(member);
    }

    /// The filter a `q`-shaped text column holds.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the column cannot be read and `malformed` of the column when its text is no clause 4.9 query.
    fn filter(
        &mut self,
        row: &PgRow,
        column: Subscriptions,
        malformed: fn(Box<FilterError>) -> StoredSubscriptionError,
        member: UnreadableMember,
    ) -> Result<Option<Filter<Expanded>>, ContextStoreError> {
        let raw: Option<String> = row.try_get(column.unquoted())?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let written: Filter<Written> = raw.parse().map_err(|error| malformed(Box::new(error)))?;
        Ok(written.into_expanded().map_err(|refused| self.give_up(member, &refused)).ok())
    }

    /// The stored `q`, its trailing members carrying the spelling the subscriber wrote (clause 4.9), which the expanded `q` column alone does not hold.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when either column cannot be decoded, and [`ContextStoreError::StoredSubscription`] when either is no query.
    pub(super) fn q(&mut self, row: &PgRow) -> Result<Option<Filter<Expanded>>, ContextStoreError> {
        let q = self.filter(row, Subscriptions::Q, StoredSubscriptionError::Q, UnreadableMember::Q)?;
        let written_raw: Option<String> = row.try_get(Subscriptions::QWrittenMembers.unquoted())?;
        let written: Option<Filter<Written>> = written_raw
            .map(|raw| raw.parse().map_err(|error| StoredSubscriptionError::QWrittenMembers(Box::new(error))))
            .transpose()?;
        Ok(match (q, written) {
            (Some(q), Some(written)) => Some(q.with_written_members(&written)),
            (q, None) => q,
            (None, Some(_)) => None,
        })
    }

    /// The stored context source filter.
    ///
    /// # Errors
    ///
    /// As [`MemberReading::filter`].
    pub(super) fn csf(&mut self, row: &PgRow) -> Result<Option<Filter<Expanded>>, ContextStoreError> {
        self.filter(row, Subscriptions::Csf, StoredSubscriptionError::Csf, UnreadableMember::Csf)
    }

    /// The value an optional JSON column holds in the expanded stage `E`, `W` being the same member in the written stage: a column whose JSON reads only as `W` names something that is no expanded name.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the column cannot be read and `malformed` of the column when its JSON is no `W` either.
    fn staged_json<E: DeserializeOwned, W: DeserializeOwned>(
        &mut self,
        row: &PgRow,
        column: Subscriptions,
        malformed: fn(serde_json::Error) -> StoredSubscriptionError,
        member: UnreadableMember,
    ) -> Result<Option<E>, ContextStoreError> {
        let text: Option<JsonText<'_>> = row.try_get(column.unquoted())?;
        let Some(text) = text else {
            return Ok(None);
        };
        match decoded_staged_column::<E, W>(text, malformed)? {
            StagedColumn::Read(expanded) => Ok(expanded),
            StagedColumn::NotExpanded => {
                self.give_up(member, &"a name is not an absolute IRI");
                Ok(None)
            }
        }
    }

    /// The stored entity selectors.
    ///
    /// # Errors
    ///
    /// As [`MemberReading::staged_json`].
    pub(super) fn entities(&mut self, row: &PgRow) -> Result<Option<Vec<EntitySelector<Expanded>>>, ContextStoreError> {
        self.staged_json::<Vec<EntitySelector<Expanded>>, Vec<EntitySelector<Written>>>(
            row,
            Subscriptions::Entities,
            StoredSubscriptionError::Entities,
            UnreadableMember::Entities,
        )
    }

    /// The stored `geoQ`.
    ///
    /// # Errors
    ///
    /// As [`MemberReading::staged_json`].
    pub(super) fn geo_q(&mut self, row: &PgRow) -> Result<Option<GeoConstraint<Expanded>>, ContextStoreError> {
        self.staged_json::<GeoConstraint<Expanded>, GeoConstraint<Written>>(row, Subscriptions::GeoQ, StoredSubscriptionError::GeoQ, UnreadableMember::GeoQ)
    }

    /// The attribute names one `TEXT[]` column of expanded names holds. A name that is no absolute IRI names no attribute of an expanded entity, so it is left out and the rest of the list kept.
    ///
    /// `malformed` names the column in the failure, so the three columns that hold attribute names are told apart without carrying the column's text.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the column cannot be read, and `malformed` of the column for an element that is no attribute name.
    pub(super) fn attribute_iris(
        &self,
        row: &PgRow,
        column: Subscriptions,
        malformed: fn(NgsiLdReferenceError) -> StoredSubscriptionError,
    ) -> Result<Option<Vec<AttributeIri>>, ContextStoreError> {
        let stored: Option<Vec<String>> = row.try_get(column.unquoted())?;
        let Some(names) = stored else {
            return Ok(None);
        };
        let mut iris = Vec::with_capacity(names.len());
        for raw in names {
            let name: AttributeName = raw.parse().map_err(malformed)?;
            match name.as_str().parse::<AttributeIri>() {
                Ok(iri) => iris.push(iri),
                Err(refused) => {
                    error!(subscription = %self.subscription, name = %name, %refused, "a stored attribute name is not an absolute IRI and is left out");
                }
            }
        }
        Ok(Some(iris))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        json_text::JsonText,
        subscription_store::{
            stored_members::{StagedColumn, decoded_staged_column},
            stored_subscription_error::StoredSubscriptionError,
        },
    };
    use andromeda_ngsi_ld_model::entity_selector::EntitySelector;
    use andromeda_ngsi_ld_query::name_stage::{Expanded, Written};

    fn entities(text: &[u8]) -> Result<StagedColumn<Vec<EntitySelector<Expanded>>>, StoredSubscriptionError> {
        decoded_staged_column::<Vec<EntitySelector<Expanded>>, Vec<EntitySelector<Written>>>(JsonText::new(text), StoredSubscriptionError::Entities)
    }

    #[test]
    fn a_selector_naming_an_absolute_iri_type_is_read_expanded() {
        let read = entities(br#"[{"type": "https://example.com/Vehicle"}]"#).expect("a stored selector");
        assert!(matches!(read, StagedColumn::Read(Some(selectors)) if selectors.len() == 1));
    }

    #[test]
    fn a_selector_naming_a_bare_term_is_not_expanded_and_fails_no_row() {
        pretty_assertions::assert_eq!(entities(br#"[{"type": "Vehicle"}]"#).expect("a stored selector"), StagedColumn::NotExpanded);
    }

    #[test]
    fn a_column_that_is_no_selector_list_fails_the_row() {
        let refused = entities(br#"[{"type": 5}]"#);
        assert!(matches!(refused, Err(StoredSubscriptionError::Entities(_))), "{refused:?}");
    }
}
