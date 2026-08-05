//! Filters held back from the local query when `splitEntities` is in effect.

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
    /// Deferred `q` query expression, still in compacted form.
    pub q: Option<String>,
    /// Deferred `georel` spatial relationship.
    pub georel: Option<String>,
    /// Deferred `geometry` type.
    pub geometry: Option<String>,
    /// Deferred `coordinates`, as the raw parameter text.
    pub coordinates: Option<String>,
    /// Deferred `geoproperty` the geo-query applies to.
    pub geoproperty: Option<String>,
    /// Deferred `scopeQ` scope query.
    pub scope_q: Option<String>,
}
