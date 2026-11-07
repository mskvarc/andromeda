//! The andromeda tree, its `extension` area: one test per scenario of what this broker does beyond the NGSI-LD
//! specification or chooses where the specification leaves it open, each a sequence of requests and the requirements on
//! their answers, against the brokers of the andromeda stack.

mod accept_language;
mod advertisement;
mod aggregation_grouping;
mod delete_by_query;
mod language_range_filtering;
mod language_tags;
