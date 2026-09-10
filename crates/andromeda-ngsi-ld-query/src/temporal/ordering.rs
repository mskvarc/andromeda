//! How a temporal query orders the entities it matched (clause 5.7.4).

use crate::{
    error::QueryError,
    ordering::{
        collation::IcuCollation,
        parameters::OrderingParameters,
        sort_direction::SortDirection,
        term::{OrderDirection, OrderTerm, RawOrderTarget},
    },
};
use typed_builder::TypedBuilder;

/// The clause-4.23 ordering a temporal query is answered under.
///
/// Clause 5.7.4.3 admits "A preferred ordering of Entities retrieved - only the
/// Entity member name "id" can be used (Entity ordering attributes as defined
/// by clause 4.23)", and Table 6.18.3.2-1 spells the parameter as "The Entity
/// member ("id") appended with an optional sorting style ("asc", "desc")". The
/// whole ordering is therefore one direction plus the collation the identifiers
/// are compared under: there is no term list to hold, no attribute name to
/// expand and no reference geometry, which is what separates this from the
/// entity query's [`ResultOrdering`].
///
/// [`ResultOrdering`]: crate::ordering::compiled::ResultOrdering
#[derive(Debug, Clone, PartialEq, Eq, TypedBuilder)]
pub struct TemporalEntityOrdering {
    /// Which way the entity identifiers run.
    pub direction: SortDirection,
    /// The ICU collation the identifiers are compared under.
    ///
    /// `None` selects the clause 4.23.1 default, the ICU "root" collation
    /// [`IcuCollation::ROOT`].
    #[builder(default = Default::default())]
    pub collation: Option<IcuCollation>,
}

impl TemporalEntityOrdering {
    /// Read the clause-5.7.4.3 ordering out of a request's ordering parameters.
    ///
    /// Returns `None` when the request named no `orderBy`: clause 6.18.3.2
    /// makes `collation` "part of Entity ordering", so a collation on its own
    /// orders nothing and the broker's own stable order stands.
    ///
    /// Only the first term is read. Clause 5.7.4.3 admits no member but `id`,
    /// and an entity identifier is unique within a broker, so every later term
    /// would break a tie the first key has already broken and cannot change the
    /// sequence whatever it says.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::TemporalOrderingMember`] when a term names an
    /// Entity member other than `id`, which clause 5.7.4.4 makes
    /// `BadRequestData`, and [`QueryError::TemporalOrderingDirection`] when a
    /// term sorts by distance: Table 6.18.3.2-1 admits only the sorting styles
    /// `asc` and `desc`, and clause 4.23.1 limits sorting by distance to
    /// `GeoProperties`, which the `id` member is not.
    pub fn of_parameters(ordering: &OrderingParameters) -> Result<Option<TemporalEntityOrdering>, QueryError> {
        for term in &ordering.order_by {
            check_term(term)?;
        }
        let Some(first) = ordering.order_by.first() else {
            return Ok(None);
        };
        Ok(Some(TemporalEntityOrdering {
            direction: SortDirection::from(first.direction),
            // clone: the compiled ordering owns its collation independently of the reusable request parameters.
            collation: ordering.collation.clone(),
        }))
    }
}

/// Reject an ordering term clause 5.7.4.3 does not admit on a temporal query.
fn check_term(term: &OrderTerm) -> Result<(), QueryError> {
    match RawOrderTarget::classify(&term.attr) {
        RawOrderTarget::Id => {}
        RawOrderTarget::Type | RawOrderTarget::Scope | RawOrderTarget::Attribute(_) => {
            return Err(QueryError::TemporalOrderingMember(term.attr.to_string()));
        }
    }
    match term.direction {
        OrderDirection::Asc | OrderDirection::Desc => Ok(()),
        direction @ (OrderDirection::DistAsc | OrderDirection::DistDesc) => Err(QueryError::TemporalOrderingDirection(direction)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        ordering::{collation::IcuCollation, parameters::OrderingParameters, sort_direction::SortDirection},
        temporal::ordering::TemporalEntityOrdering,
    };

    fn parameters(order_by: &str) -> OrderingParameters {
        serde_json::from_value(serde_json::json!({"orderBy": order_by})).expect("a well-formed orderBy")
    }

    /// Clause 6.18.3.2 makes `collation` part of Entity ordering, so a request
    /// that orders nothing is not turned into one that does.
    #[test]
    fn a_collation_without_an_order_by_orders_nothing() {
        let ordering = OrderingParameters::builder()
            .collation(Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")))
            .build();
        assert_eq!(TemporalEntityOrdering::of_parameters(&ordering).expect("a collation alone is admissible"), None);
    }

    /// Table 6.18.3.2-1: "The Entity member ("id") appended with an optional
    /// sorting style ("asc", "desc")". An unsuffixed member is ascending,
    /// which clause 4.23.3 makes the default.
    #[test]
    fn ordering_by_the_id_member_defaults_to_ascending() {
        let ordering = TemporalEntityOrdering::of_parameters(&parameters("id"))
            .expect("id is the member clause 5.7.4.3 admits")
            .expect("an orderBy was requested");
        assert_eq!(ordering.direction, SortDirection::Ascending);
        assert_eq!(ordering.collation, None);
    }

    #[test]
    fn the_descending_sorting_style_is_carried_through() {
        let ordering = TemporalEntityOrdering::of_parameters(&parameters("id;desc"))
            .expect("desc is a sorting style Table 6.18.3.2-1 admits")
            .expect("an orderBy was requested");
        assert_eq!(ordering.direction, SortDirection::Descending);
    }

    /// The collation reaches the compiled ordering, which is what lets the
    /// store compare identifiers under it (Table 6.18.3.2-1).
    #[test]
    fn the_requested_collation_reaches_the_compiled_ordering() {
        let mut ordering = parameters("id");
        ordering.collation = Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation"));
        let compiled = TemporalEntityOrdering::of_parameters(&ordering)
            .expect("id ordering under a collation is admissible")
            .expect("an orderBy was requested");
        assert_eq!(compiled.collation.as_ref().map(IcuCollation::as_str), Some("de-u-co-phonebk"));
    }

    /// Clause 5.7.4.4: "If the ordering parameter is present and refers an
    /// entity name other than "id", then an error of type `BadRequestData`
    /// shall be raised."
    #[test]
    fn a_member_other_than_id_is_rejected() {
        for rejected in ["type", "scope", "speed"] {
            let error = TemporalEntityOrdering::of_parameters(&parameters(rejected)).expect_err("only id may order a temporal query");
            assert!(matches!(error, QueryError::TemporalOrderingMember(_)), "{rejected}: got {error:?}");
        }
    }

    /// Table 6.18.3.2-1 admits only the sorting styles `asc` and `desc`, and
    /// clause 4.23.1 limits sorting by distance to `GeoProperties`.
    #[test]
    fn a_distance_sorting_style_is_rejected() {
        for rejected in ["id;dist-asc", "id;dist-desc"] {
            let error = TemporalEntityOrdering::of_parameters(&parameters(rejected)).expect_err("a distance ordering has no GeoProperty to measure");
            assert!(matches!(error, QueryError::TemporalOrderingDirection(_)), "{rejected}: got {error:?}");
        }
    }

    /// A later term can only name `id` again, and an identifier is unique, so
    /// the first key has already decided the sequence.
    #[test]
    fn a_repeated_id_term_cannot_change_the_sequence() {
        let ordering = TemporalEntityOrdering::of_parameters(&parameters("id;desc,id"))
            .expect("every term names the admitted member")
            .expect("an orderBy was requested");
        assert_eq!(ordering.direction, SortDirection::Descending);
    }

    /// A later term is still held to clause 5.7.4.4, so a list cannot smuggle
    /// an inadmissible member past the first position.
    #[test]
    fn a_later_term_naming_another_member_is_still_rejected() {
        let error = TemporalEntityOrdering::of_parameters(&parameters("id,speed")).expect_err("every term is checked");
        assert!(matches!(error, QueryError::TemporalOrderingMember(_)), "got {error:?}");
    }
}
