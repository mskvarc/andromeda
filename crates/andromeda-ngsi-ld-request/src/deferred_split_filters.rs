//! Filters held back from the local query when `splitEntities` is in effect.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::input::GeoQueryInput, scope::query::ScopeQuery};

/// The `q`, `geoQ` and `scopeQ` parameters a `splitEntities` request removes
/// from the query it runs locally, so they can be evaluated once against the
/// merged local-plus-forwarded result instead (NGSI-LD 5.7.2.4 for entities,
/// 5.7.4.4 for temporal entities).
///
/// Applying them before the merge would drop entities whose attributes are
/// split across Context Sources: each source sees only its own fragment, so a
/// predicate over an attribute another source holds cannot be satisfied there.
///
/// [`Default`] is the not-deferring case — every field `None` — which is what a
/// request without `splitEntities` needs.
#[derive(Debug, Default)]
pub struct DeferredSplitFilters {
    /// Deferred `q` query expression parsed at request extraction.
    pub q: Option<Expr>,
    /// Deferred complete geo-query.
    pub geo: Option<GeoQueryInput>,
    /// Deferred `scopeQ` scope query.
    pub scope_q: Option<ScopeQuery>,
    /// The request's `jsonKeys` declaration, which travels with the deferred
    /// `q` because it decides how that filter's terms expand.
    ///
    /// Clause 4.9 EXAMPLE 11: with
    /// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`
    /// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue
    /// using the core @context". Expanding the deferred filter without the
    /// declaration would address `ngsi-ld:hasValue` where the entity holds a
    /// literal `value` key, and every entity the client asked for would be
    /// dropped after the merge.
    pub json_keys: Option<Vec<AttributeName>>,
}
