//! How long a `q` filter of a hundred terms takes to compile.
//!
//! Every query and every Subscription compiles its filter once, so these fix
//! the cost of the steps a compiled filter goes through before it is
//! evaluated: the parse of the written text, the expansion of every attribute
//! name through the request's `@context`, the lowering to a reading, its
//! normal form, and the compiled filter that holds them.

use andromeda_ngsi_ld_query::{
    filter::{
        Filter,
        compiled_filter::CompiledFilter,
        expansion::expand_filter,
        filter_error::FilterError,
        json_key_attributes::JsonKeyAttributes,
        language_matching::LanguageMatching,
        reading::filter_reading::FilterReading,
    },
    namespace_expander::EXAMPLE_NAMESPACE,
    query_error::QueryError,
};
use criterion::{BatchSize, Criterion};
use std::{hint::black_box, io};
use tokio::runtime::Builder;

/// A filter of a hundred terms over twenty Attributes, every operator among
/// them, in groups of five joined by `|`.
fn hundred_terms() -> String {
    let terms: Vec<String> = (0..100)
        .map(|position| {
            let name = format!("a{}", position % 20);
            match position % 5 {
                0 => format!("{name}=={position}"),
                1 => format!("{name}!=\"v{position}\""),
                2 => format!("{name}>{position}"),
                3 => format!("{name}~=\"^v.*\""),
                _ => format!("{name}.sub==1..{position}"),
            }
        })
        .collect();
    terms.chunks(5).map(|group| format!("({})", group.join(";"))).collect::<Vec<_>>().join("|")
}

/// Why the benchmark could not be set up.
#[derive(Debug, thiserror::Error)]
enum SetupError {
    /// The filter does not parse.
    #[error("the filter does not parse")]
    Filter(#[from] FilterError),
    /// A term of the filter does not expand through the `@context`.
    #[error("the filter does not expand")]
    Expansion(#[from] QueryError),
    /// The runtime the expansion awaits on cannot be built.
    #[error("the runtime cannot be built")]
    Runtime(#[from] io::Error),
}

/// Run every benchmark, or fail with the reason the fixture cannot be built.
fn main() -> Result<(), SetupError> {
    let written = hundred_terms();
    let parsed = written.parse::<Filter>()?;
    let runtime = Builder::new_current_thread().build()?;
    let json_keys = JsonKeyAttributes::default();
    let mut criterion = Criterion::default().configure_from_args();
    let mut group = criterion.benchmark_group("compile-100-terms");
    group.bench_function("parse", |bencher| bencher.iter(|| black_box(&written).parse::<Filter>()));
    group.bench_function("expand", |bencher| {
        bencher.iter(|| runtime.block_on(expand_filter(black_box(parsed.clone()), &json_keys, &EXAMPLE_NAMESPACE)));
    });
    let expanded = runtime.block_on(expand_filter(parsed, &json_keys, &EXAMPLE_NAMESPACE))?;
    let lowered = FilterReading::lowered(&expanded, LanguageMatching::TagEquality);
    group.bench_function("lower", |bencher| {
        bencher.iter(|| FilterReading::lowered(black_box(&expanded), LanguageMatching::TagEquality));
    });
    group.bench_function("normal-form", |bencher| {
        bencher.iter_batched(|| lowered.clone(), FilterReading::normal_form, BatchSize::SmallInput);
    });
    group.bench_function("compile", |bencher| {
        bencher.iter_batched(
            || expanded.clone(),
            |filter| CompiledFilter::new(filter, LanguageMatching::TagEquality),
            BatchSize::SmallInput,
        );
    });
    group.finish();
    criterion.final_summary();
    Ok(())
}
