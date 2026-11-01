//! What `splitEntities` resolves to for one request or subscription.

use andromeda_configuration::schema::federation::query::SplitEntitiesDefault;
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;

/// What one request or subscription resolved `splitEntities` to (clauses
/// 4.3.6.7, 5.7.2.4 and 5.7.4.4).
///
/// Under [`SplitEntities::Split`] no single source can be assumed to hold a
/// whole entity, so the filters that read attributes (`q`, the geoquery, the
/// scope query and the attribute list) are removed before forwarding and
/// applied once to the aggregated entities; under
/// [`SplitEntities::Complete`] every source holds complete entities and the
/// filters travel with the forwarded request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitEntities {
    /// Entities may be distributed across sources.
    Split,
    /// Every source holds complete entities.
    Complete,
}

impl SplitEntities {
    /// Whether filters are deferred until after the aggregation.
    #[must_use]
    pub const fn is_split(self) -> bool {
        matches!(self, SplitEntities::Split)
    }

    /// The Scope query a read under `self` selects its registrations by.
    ///
    /// Clauses 5.7.2.4 and 5.7.4.4: under [`SplitEntities::Split`] the Scope
    /// query is removed before forwarding and applied once the entities of
    /// every source are merged, and clause 5.12 matching compares no scope. A
    /// source declaring no `scope` can hold attributes of an entity whose scope
    /// another holder supplies, so no registration is passed over for it.
    #[must_use]
    pub const fn registration_scope_query(self, scope_q: Option<&ScopeQuery>) -> Option<&ScopeQuery> {
        match self {
            SplitEntities::Split => None,
            SplitEntities::Complete => scope_q,
        }
    }
}

/// Resolve a request's explicit `splitEntities` against the configured default.
#[must_use]
pub const fn resolve_split_entities(default: SplitEntitiesDefault, explicit: Option<bool>) -> SplitEntities {
    match explicit {
        Some(true) => SplitEntities::Split,
        Some(false) => SplitEntities::Complete,
        None => match default {
            SplitEntitiesDefault::Split => SplitEntities::Split,
            SplitEntitiesDefault::Complete => SplitEntities::Complete,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::split_entities::{SplitEntities, resolve_split_entities};
    use andromeda_configuration::schema::federation::query::SplitEntitiesDefault;
    use andromeda_ngsi_ld_query::{error::QueryError, scope::query::ScopeQuery};

    #[test]
    fn an_explicit_split_entities_wins_and_an_absent_one_takes_the_default() {
        pretty_assertions::assert_eq!(resolve_split_entities(SplitEntitiesDefault::Complete, Some(true)), SplitEntities::Split);
        pretty_assertions::assert_eq!(resolve_split_entities(SplitEntitiesDefault::Split, Some(false)), SplitEntities::Complete);
        pretty_assertions::assert_eq!(resolve_split_entities(SplitEntitiesDefault::Split, None), SplitEntities::Split);
        pretty_assertions::assert_eq!(resolve_split_entities(SplitEntitiesDefault::Complete, None), SplitEntities::Complete);
        assert!(SplitEntities::Split.is_split());
        assert!(!SplitEntities::Complete.is_split());
    }

    #[test]
    fn a_split_read_selects_its_registrations_by_no_scope_query() -> Result<(), QueryError> {
        let madrid = ScopeQuery::parse("/Madrid")?;
        pretty_assertions::assert_eq!(SplitEntities::Split.registration_scope_query(Some(&madrid)), None);
        pretty_assertions::assert_eq!(SplitEntities::Complete.registration_scope_query(Some(&madrid)), Some(&madrid));
        pretty_assertions::assert_eq!(SplitEntities::Complete.registration_scope_query(None), None);
        Ok(())
    }
}
