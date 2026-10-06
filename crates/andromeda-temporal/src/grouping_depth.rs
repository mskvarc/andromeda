//! How many grouping terms nest the aggregates of a read.

use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;

/// How many grouping terms nest the triples of each aggregation method (study
/// section 4.6): the grouped extension format nests one map key per term, the
/// standard format none.
pub(crate) fn grouping_depth(compiled: &CompiledTemporalQuery) -> usize {
    if compiled.representation.format.is_grouped() {
        compiled.representation.grouping.as_ref().map_or(0, |grouping| grouping.terms().len())
    } else {
        0
    }
}
