//! One kind of predicate a store may decline.

use strum::Display;

/// One kind of predicate a store may decline.
///
/// Spelled as the NGSI-LD query parameter it comes from (clause 5.7.2), because
/// the only thing this vocabulary is used for is telling a client which part of
/// its request could not be answered.
///
/// All seven members of an `EntitySelection` are named, not just the three
/// `StorageProfile` has axes for today. The residual *is* a selection, so
/// `ResidualFilter::kinds` must be able to name whatever a selection can
/// carry; a shorter enum would make some selections unnameable. Four of the
/// seven therefore have no production constructor, and the in-memory readings
/// of those four are held to the pushed-down renderings by the profile-driven
/// conformance tests rather than by a live caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum ResidualPredicate {
    /// The `id` list.
    #[strum(serialize = "id")]
    Identifier,
    /// The `idPattern` regular expression.
    #[strum(serialize = "idPattern")]
    IdentifierPattern,
    /// The `type` selection expression (clause 4.17).
    #[strum(serialize = "type")]
    EntityType,
    /// The `attrs` list.
    #[strum(serialize = "attrs")]
    Attributes,
    /// The `q` filter (clause 4.9).
    #[strum(serialize = "q")]
    ValueFilter,
    /// The `georel` / `geometry` / `coordinates` triple (clause 4.10).
    #[strum(serialize = "georel")]
    GeoQuery,
    /// The `scopeQ` expression (clause 4.18).
    #[strum(serialize = "scopeQ")]
    ScopeQuery,
}
