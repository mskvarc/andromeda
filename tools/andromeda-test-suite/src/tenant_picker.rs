//! Which tenant of a set each request of a load goes to: uniformly, or skewed so a few tenants are hot and the rest
//! cold, as a Zipf law with the hottest tenant first. Seeded, so a run picks the same sequence again.

use rand::{
    RngExt,
    SeedableRng,
    distr::{
        Distribution,
        weighted::{Error as WeightError, WeightedIndex},
    },
    rngs::StdRng,
};
use std::sync::{Mutex, PoisonError};

/// Why a picker could not be built.
#[derive(Debug, thiserror::Error)]
pub enum PickerError {
    /// A set of no tenant has none to pick.
    #[error("a picker needs at least one tenant")]
    NoTenant,
    /// The Zipf weights are not weights.
    #[error("the Zipf weights are not weights")]
    Weights(#[from] WeightError),
}

/// Picks every tenant of a set equally often.
#[derive(Debug)]
pub struct UniformPicker {
    /// The seeded generator.
    rng: Mutex<StdRng>,
    /// How many tenants the set holds.
    tenants: usize,
}

impl UniformPicker {
    /// A picker over `tenants` tenants, seeded with `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`PickerError::NoTenant`] for a set of no tenant.
    pub fn new(tenants: usize, seed: u64) -> Result<UniformPicker, PickerError> {
        if tenants == 0 {
            return Err(PickerError::NoTenant);
        }
        Ok(UniformPicker {
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
            tenants,
        })
    }

    /// The index of the next tenant.
    #[must_use]
    pub fn pick(&self) -> usize {
        self.rng.lock().unwrap_or_else(PoisonError::into_inner).random_range(0..self.tenants)
    }
}

/// Picks the tenant of rank `k` (from 1) with a weight of `1 / k^exponent`.
#[derive(Debug)]
pub struct ZipfPicker {
    /// The seeded generator.
    rng: Mutex<StdRng>,
    /// The weight of each tenant, the hottest first.
    weights: WeightedIndex<f64>,
}

impl ZipfPicker {
    /// A picker over `tenants` tenants skewed by `exponent`, seeded with `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`PickerError`] for a set of no tenant or an exponent giving no usable weights.
    pub fn new(tenants: u16, exponent: f64, seed: u64) -> Result<ZipfPicker, PickerError> {
        if tenants == 0 {
            return Err(PickerError::NoTenant);
        }
        let weights = WeightedIndex::new((1..=tenants).map(|rank| 1.0 / f64::from(rank).powf(exponent)))?;
        Ok(ZipfPicker {
            rng: Mutex::new(StdRng::seed_from_u64(seed)),
            weights,
        })
    }

    /// The index of the next tenant, 0 the hottest.
    #[must_use]
    pub fn pick(&self) -> usize {
        self.weights.sample(&mut *self.rng.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_picker::{UniformPicker, ZipfPicker};
    use googletest::prelude::*;

    #[gtest]
    fn a_uniform_picker_stays_within_its_set() -> Result<()> {
        let picker = UniformPicker::new(5, 7)?;
        verify_that!((0..1_000).map(|_| picker.pick()).all(|index| index < 5), eq(true))
    }

    #[gtest]
    fn a_zipf_picker_picks_its_hottest_tenant_most() -> Result<()> {
        let picker = ZipfPicker::new(100, 1.1, 7)?;
        let picks = (0..10_000).map(|_| picker.pick()).collect::<Vec<_>>();
        let hottest = picks.iter().filter(|index| **index == 0).count();
        let coldest = picks.iter().filter(|index| **index == 99).count();
        expect_that!(picks.iter().all(|index| *index < 100), eq(true));
        verify_that!(hottest, gt(coldest * 10))
    }

    #[gtest]
    fn a_set_of_no_tenant_is_refused() {
        expect_that!(UniformPicker::new(0, 1).is_err(), eq(true));
        expect_that!(ZipfPicker::new(0, 1.1, 1).is_err(), eq(true));
    }
}
