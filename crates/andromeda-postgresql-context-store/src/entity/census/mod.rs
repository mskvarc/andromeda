//! The discovery census: exact clause 5.7 entity type and attribute counts,
//! kept by the statements that write the entities rather than computed from
//! the payloads when a discovery request arrives.
//!
//! Three ledgers (`entity_type_census`, `entity_type_attribute_census`,
//! `attribute_census`) each hold signed changes to one count, and a count is
//! the sum of the rows sharing its key. The entity statements add their
//! changes in the same statement as the search rows they write ([`delta`]),
//! discovery reads the sums ([`read`]), and a periodic fold keeps each ledger
//! near one row per key ([`fold`]). Every count equals what the search rows'
//! type and instance arrays add up to, which is the invariant the integration
//! suites check the census against.

/// The changes a set-write adds beside its `entity_search` write.
pub(crate) mod delta;
/// Replacing each key's rows with their sum.
pub mod fold;
/// The discovery answers read from the ledger sums.
pub mod read;
