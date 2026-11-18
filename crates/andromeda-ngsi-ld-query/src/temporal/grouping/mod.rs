//! The `aggrGroupBy` grouping grammar (temporal aggregation grouping extension).
//!
//! A fourth small language beside `orderBy` (clause 4.23.3), `pick`/`omit`
//! (clause 4.21) and `q` (clause 4.9). It partitions a temporal aggregation into
//! groups that clause 4.5.19's fixed `(entity, attribute, datasetId, period)`
//! key cannot express: calendar and cyclic time buckets, per-entity-member keys,
//! and value-in-effect keys.
//!
//! The grammar is
//!
//! ```text
//! AggregationGrouping = GroupingTerm *("," GroupingTerm)
//! GroupingTerm        = GroupKey *1(";" BucketTerm)
//! GroupKey            = ValuePath / EntityMember
//! ```
//!
//! where `,` reads as "then": each further term nests one level deeper inside
//! the previous term's groups, and `;` attaches a per-key bucket modifier. A
//! bucket is only accepted on a temporal key (a clause 4.8 temporal property).
pub mod aggregation_grouping;
pub mod bucket_term;
pub mod entity_member;
pub mod group_key;
pub mod grouping_error;
pub mod grouping_term;
