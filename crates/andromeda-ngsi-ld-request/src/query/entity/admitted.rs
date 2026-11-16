//! An entity query whose `q` filter request validation admitted.

use crate::{
    deferred_split_filters::DeferredSplitFilters,
    query::{entity::parameters::EntityQueryParameters, selection_mut::SelectionMut},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_query::filter::compilation::AdmittedFilter;
use derive_more::Deref;

/// The parameters of an entity query together with the admission of their `q`
/// (clauses 5.5.6 and 5.7.2.4).
///
/// Validation makes one for Query Entities, the Entity Map it creates and
/// Purge Entities, and every compilation reads the filter from it, so a
/// filter is admitted once, by the request that wrote it. The admitted filter
/// is always the admission of the parameters' `q`: the parameters are read
/// through [`Deref`](std::ops::Deref) and changed only through operations
/// that leave the `q` and its admission together.
#[derive(Debug, Deref)]
pub struct AdmittedEntityQuery {
    /// The parameters validation read.
    #[deref]
    params: EntityQueryParameters,
    /// The admission of `params.q`, `None` exactly when `params.q` is.
    filter: Option<AdmittedFilter>,
}

impl AdmittedEntityQuery {
    /// The parameters `filter` admitted the `q` of.
    pub(crate) const fn admitted(params: EntityQueryParameters, filter: Option<AdmittedFilter>) -> AdmittedEntityQuery {
        AdmittedEntityQuery { params, filter }
    }

    /// The admission of the query's `q`, `None` when it carries none.
    #[must_use]
    pub const fn filter(&self) -> Option<&AdmittedFilter> {
        self.filter.as_ref()
    }

    /// Take the attribute-reading filters out of the query, leaving it selecting
    /// on identity alone so every source (and the local read) answers with
    /// whatever it holds (clause 5.7.2.4).
    ///
    /// With `splitEntities` an entity's attributes may be spread across several
    /// context sources, so no single reader can decide whether the entity
    /// matches: `q`, the geoquery and `scopeQ` are held back and evaluated once
    /// over the merged result. `jsonKeys` leaves with `q`, the filter whose
    /// term expansion it governs (clause 4.9 EXAMPLE 11), and so does
    /// `expandValues`, which governs its operands; with `q` deferred neither
    /// has anything left to say to the local query. The admission of the `q`
    /// leaves with it.
    pub fn defer_split_filters(&mut self) -> DeferredSplitFilters {
        self.params.q = None;
        DeferredSplitFilters {
            q: self.filter.take(),
            geo: self.params.geo.0.take(),
            scope_q: self.params.scope_q.take(),
            json_keys: self.params.json_keys.take(),
            expand_values: self.params.expand_values.take(),
        }
    }

    /// The natural-language preference the query names, which the response
    /// is rendered by rather than the query selecting by it. It stays in the
    /// query, which every source the query is forwarded to reads it from
    /// (clause 5.7.2.4 forwards the request).
    #[must_use]
    pub const fn language(&self) -> Option<&LanguageFilter> {
        self.params.lang.as_ref()
    }

    /// The members an Entity Map holds the query to (clause 5.5.14).
    pub const fn selection_mut(&mut self) -> SelectionMut<'_> {
        SelectionMut {
            id: &mut self.params.id,
            entity_selectors: &mut self.params.entity_selectors,
        }
    }
}
