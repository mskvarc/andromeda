//! The members of a stored subscription that are read in the expanded stage.
//!
//! The broker stores a member only after expanding its names, so a stored name is an absolute IRI. A column that holds a name that is not one was not written by the broker, and the subscription it belongs to cannot be matched by that member. Reading it does not fail the row: the member is left out whole, the failure is logged, and the subscription lists the member as unreadable, which the matcher answers by selecting nothing. A list is never kept in part, because a list that lost a name selects more than it did. A column that is no member of its kind at all, or a list element that is no name in either stage, stays a failure of the row.

use crate::{
    error::ContextStoreError,
    json_text::JsonText,
    subscription_store::{mapper::decoded_column, notification_lists::NotificationLists, stored_subscription_error::StoredSubscriptionError},
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::{
    entity_selector::EntitySelector,
    subscription::{NotificationParams, unreadable_member::UnreadableMember},
};
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
        Ok(match staged_filter(&raw, malformed)? {
            StagedColumn::Read(filter) => filter,
            StagedColumn::NotExpanded => {
                self.give_up(member, &"a name is not an absolute IRI");
                None
            }
        })
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
        Ok(spelled_as_written(q, written))
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

    /// The attribute names one `TEXT[]` column of expanded names holds. A list that holds a name that is no absolute IRI names no attribute of an expanded entity, and it is left out whole and listed as `member`: a list that lost a name would select more than it did.
    ///
    /// `malformed` names the column in the failure, so the three columns that hold attribute names are told apart without carrying the column's text.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the column cannot be read, and `malformed` of the column for an element that is no attribute name.
    pub(super) fn attribute_iris(
        &mut self,
        row: &PgRow,
        column: Subscriptions,
        malformed: fn(NgsiLdReferenceError) -> StoredSubscriptionError,
        member: UnreadableMember,
    ) -> Result<Option<Vec<AttributeIri>>, ContextStoreError> {
        let stored: Option<Vec<String>> = row.try_get(column.unquoted())?;
        let Some(names) = stored else {
            return Ok(None);
        };
        Ok(match staged_names(names, malformed)? {
            StagedColumn::Read(iris) => iris,
            StagedColumn::NotExpanded => {
                self.give_up(member, &"a name is not an absolute IRI");
                None
            }
        })
    }

    /// The notification parameters of the `notification` column, the lists of which that hold a name that is no absolute IRI are left out whole and listed as unreadable.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the column cannot be read, [`StoredSubscriptionError::NotificationAbsent`] when it holds no parameters, and [`StoredSubscriptionError::Notification`] when it is no notification or a list holds an element that is no name.
    pub(super) fn notification(&mut self, row: &PgRow) -> Result<NotificationParams, ContextStoreError> {
        let text: Option<JsonText<'_>> = row.try_get(Subscriptions::Notification.unquoted())?;
        let text = text.ok_or(StoredSubscriptionError::NotificationAbsent)?;
        let (notification, withheld) = decoded_notification(text)?;
        for member in withheld {
            self.give_up(member, &"a name is not an absolute IRI");
        }
        Ok(notification)
    }
}

/// The filter the text of a `q`-shaped column holds, as the expanded stage reads it: a text that names something that is no absolute IRI reads only as written.
///
/// # Errors
///
/// Returns `malformed` of the column when the text is no clause 4.9 query.
fn staged_filter(raw: &str, malformed: fn(Box<FilterError>) -> StoredSubscriptionError) -> Result<StagedColumn<Filter<Expanded>>, StoredSubscriptionError> {
    let written: Filter<Written> = raw.parse().map_err(|error| malformed(Box::new(error)))?;
    Ok(match written.into_expanded() {
        Ok(expanded) => StagedColumn::Read(Some(expanded)),
        Err(_not_an_iri) => StagedColumn::NotExpanded,
    })
}

/// The expanded `q` with its trailing members carrying the spelling `written` holds; a `q` that could not be read stays absent.
fn spelled_as_written(q: Option<Filter<Expanded>>, written: Option<Filter<Written>>) -> Option<Filter<Expanded>> {
    match (q, written) {
        (Some(q), Some(written)) => Some(q.with_written_members(&written)),
        (q, None) => q,
        (None, Some(_)) => None,
    }
}

/// The attribute names a `TEXT[]` column holds, as the expanded stage reads them: a list holding a name that is no absolute IRI reads only as written.
///
/// # Errors
///
/// Returns `malformed` of the column for an element that is no attribute name.
fn staged_names(
    names: Vec<String>,
    malformed: fn(NgsiLdReferenceError) -> StoredSubscriptionError,
) -> Result<StagedColumn<Vec<AttributeIri>>, StoredSubscriptionError> {
    let written = names
        .into_iter()
        .map(|raw| raw.parse::<AttributeName>().map_err(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        match written.iter().map(|name| name.as_str().parse::<AttributeIri>()).collect::<Result<Vec<_>, _>>() {
            Ok(iris) => StagedColumn::Read(Some(iris)),
            Err(_not_absolute) => StagedColumn::NotExpanded,
        },
    )
}

/// The notification parameters the JSON text of the `notification` column holds, and the members of it that were left out because a list holds a name that is no absolute IRI.
///
/// # Errors
///
/// Returns [`StoredSubscriptionError::Notification`] when the text is no notification or a list holds an element that is no name in either stage, and [`StoredSubscriptionError::NotificationAbsent`] for the JSON `null` literal.
fn decoded_notification(text: JsonText<'_>) -> Result<(NotificationParams, Vec<UnreadableMember>), StoredSubscriptionError> {
    let lists = decoded_column::<NotificationLists>(text, StoredSubscriptionError::Notification)?;
    let notification = decoded_column::<NotificationParams>(text, StoredSubscriptionError::Notification)?.ok_or(StoredSubscriptionError::NotificationAbsent)?;
    Ok((notification, lists.map_or_default(|lists| lists.unreadable())))
}

#[cfg(test)]
mod tests {
    use crate::{
        json_text::JsonText,
        subscription_store::{
            member_reading::{StagedColumn, decoded_notification, decoded_staged_column, spelled_as_written, staged_filter, staged_names},
            stored_subscription_error::StoredSubscriptionError,
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
    use andromeda_ngsi_ld_model::{
        entity_selector::EntitySelector,
        subscription::{NotificationParams, unreadable_member::UnreadableMember},
    };
    use andromeda_ngsi_ld_query::{
        filter::Filter,
        name_stage::{Expanded, Written},
    };

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

    fn names(raw: &[&str]) -> Result<StagedColumn<Vec<AttributeIri>>, StoredSubscriptionError> {
        staged_names(raw.iter().map(ToString::to_string).collect(), StoredSubscriptionError::WatchedAttribute)
    }

    #[test]
    fn a_list_of_absolute_iris_is_read_whole() {
        let read = names(&["https://example.com/speed", "https://example.com/colour"]).expect("attribute names");
        assert!(matches!(read, StagedColumn::Read(Some(iris)) if iris.len() == 2));
    }

    #[test]
    fn a_list_holding_a_bare_name_is_not_expanded_whole_and_fails_no_row() {
        pretty_assertions::assert_eq!(
            names(&["https://example.com/speed", "bare"]).expect("attribute names"),
            StagedColumn::NotExpanded
        );
    }

    #[test]
    fn a_list_element_that_is_no_attribute_name_fails_the_row() {
        let refused = names(&["https://example.com/speed", ""]);
        assert!(matches!(refused, Err(StoredSubscriptionError::WatchedAttribute(_))), "{refused:?}");
    }

    #[test]
    fn a_q_naming_an_absolute_iri_is_read_expanded() {
        let read = staged_filter("\"https://example.com/speed\">1", StoredSubscriptionError::Q).expect("a query");
        assert!(matches!(read, StagedColumn::Read(Some(_))), "{read:?}");
    }

    #[test]
    fn a_q_naming_a_bare_term_is_not_expanded_and_fails_no_row() {
        pretty_assertions::assert_eq!(
            staged_filter("speed>1", StoredSubscriptionError::Csf).expect("a query"),
            StagedColumn::NotExpanded
        );
    }

    #[test]
    fn a_text_that_is_no_query_fails_the_row_naming_its_column() {
        let refused = staged_filter(">>", StoredSubscriptionError::Csf);
        assert!(matches!(refused, Err(StoredSubscriptionError::Csf(_))), "{refused:?}");
    }

    fn written_filter(text: &str) -> Filter<Written> {
        text.parse().expect("a query")
    }

    #[test]
    fn a_q_takes_the_member_spelling_the_subscriber_wrote() {
        let q = written_filter("\"https://example.com/address\"[\"https://example.com/city\"]==\"Berlin\"")
            .into_expanded()
            .expect("absolute names");
        let written = written_filter("\"https://example.com/address\"[city]==\"Berlin\"");

        let joined = spelled_as_written(Some(q), Some(written)).expect("a q");

        pretty_assertions::assert_eq!(joined.written_member_text(), "\"https://example.com/address\"[city]==\"Berlin\"");
    }

    #[test]
    fn a_q_without_a_written_spelling_stays_as_read_and_a_missing_q_stays_missing() {
        let q = written_filter("\"https://example.com/speed\">1").into_expanded().expect("absolute names");

        pretty_assertions::assert_eq!(spelled_as_written(Some(q.clone()), None), Some(q));
        assert!(spelled_as_written(None, Some(written_filter("speed>1"))).is_none());
    }

    const ENDPOINT: &str = r#""endpoint": {"uri": "https://receiver.example.com/notify"}"#;

    fn notification(members: &str) -> Result<(NotificationParams, Vec<UnreadableMember>), StoredSubscriptionError> {
        let text = format!("{{{ENDPOINT}, {members}}}");
        decoded_notification(JsonText::new(text.as_bytes()))
    }

    #[test]
    fn a_notification_naming_absolute_iris_is_read_whole_with_nothing_unreadable() {
        let (read, unreadable) = notification(
            r#""attributes": ["https://example.com/speed"], "pick": [[{"name": "https://example.com/owner", "linked": []}]], "omit": [[{"name": "id", "linked": []}]]"#,
        )
        .expect("a notification");
        assert!(read.attributes.is_some() && read.pick.is_some() && read.omit.is_some());
        assert!(unreadable.is_empty(), "{unreadable:?}");
    }

    #[test]
    fn a_notification_attribute_list_holding_a_bare_name_is_unreadable_and_absent() {
        let (read, unreadable) = notification(r#""attributes": ["bare"]"#).expect("a notification");
        assert!(read.attributes.is_none());
        pretty_assertions::assert_eq!(unreadable, vec![UnreadableMember::NotificationAttributes]);
    }

    #[test]
    fn a_notification_attribute_list_keeps_no_name_when_only_one_is_bare() {
        let (read, unreadable) = notification(r#""attributes": ["https://example.com/speed", "bare"]"#).expect("a notification");
        assert!(read.attributes.is_none());
        pretty_assertions::assert_eq!(unreadable, vec![UnreadableMember::NotificationAttributes]);
    }

    #[test]
    fn a_pick_and_an_omit_naming_a_bare_attribute_are_unreadable() {
        let (read, unreadable) = notification(
            r#""pick": [[{"name": "owner", "linked": []}]], "omit": [[{"name": "https://example.com/owner", "linked": [{"name": "bare", "linked": []}]}]]"#,
        )
        .expect("a notification");
        assert!(read.pick.is_none() && read.omit.is_none());
        pretty_assertions::assert_eq!(unreadable, vec![UnreadableMember::Pick, UnreadableMember::Omit]);
    }

    #[test]
    fn a_notification_attribute_that_is_no_name_fails_the_row() {
        let refused = notification(r#""attributes": [5]"#);
        assert!(matches!(refused, Err(StoredSubscriptionError::Notification(_))), "{refused:?}");
    }

    #[test]
    fn a_notification_of_the_json_null_literal_is_absent() {
        let refused = decoded_notification(JsonText::new(b"null"));
        assert!(matches!(refused, Err(StoredSubscriptionError::NotificationAbsent)), "{refused:?}");
    }
}
