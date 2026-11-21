//! Drawing a value from a seed, so that a generated case is the same on every run.

use googletest::prelude::*;
use proptest::{
    strategy::{Strategy, ValueTree},
    test_runner::{Config, RngSeed, TestRunner},
};

/// The value `strategy` draws from `seed`, the same on every run.
///
/// # Errors
///
/// Fails when the strategy rejects its draw.
pub fn drawn_from_seed<S: Strategy>(seed: u64, strategy: &S) -> Result<S::Value> {
    let config = Config {
        rng_seed: RngSeed::Fixed(seed),
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner = TestRunner::new(config);
    Ok(strategy.new_tree(&mut runner).or_fail()?.current())
}
