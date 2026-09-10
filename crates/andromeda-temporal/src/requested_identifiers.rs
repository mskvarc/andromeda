//! Whether the entity identifiers a request specified narrow a listing of what
//! it matches (clause 5.5.14).

/// Whether the entity identifiers a request specified narrow a listing of the
/// identifiers it matches.
///
/// A temporal query is always answered under its own `id` list, so this decides
/// nothing about the history that comes back; it decides what a listing made
/// *about* the request means. The two readings differ exactly once, and clause
/// 5.5.14 is where: a request that runs under an `EntityMap` may specify its own
/// entity identifiers while every other parameter is held to the request that
/// created the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedIdentifiers {
    /// The listing is narrowed by them, exactly as the query itself is: what a
    /// request matched is what a map minted for it fixes.
    Narrow,
    /// The listing ignores them, leaving the filters alone to decide. Clause
    /// 4.3.6.7 rechecks "queries, geoqueries, scope queries or attribute
    /// filters, as provided in the original query request" before an entity
    /// joins a result page, and the identifiers a follow-up names are not among
    /// them: a request asking about one entity of a map has checked nothing
    /// about the others.
    Ignore,
}
