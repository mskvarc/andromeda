//! Whether a `near` geo-query may bound its distance from both sides.
//!
//! The clause 4.10 `nearRel` grammar admits one distance modifier, so a `near`
//! carrying both `maxDistance` and `minDistance` is outside it and clause
//! 5.7.2.4 answers it with `BadRequestData`. Andromeda's `near_distance_band`
//! extension reads the pair as the band between the two distances; the broker
//! decides once which of the two applies and every request-side geo-query is
//! admitted under that decision.

use crate::error::QueryError;
use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;

/// Whether the distance band is admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NearDistanceBand {
    /// Clause 4.10 as written: a `near` with both distances is refused.
    Refused,
    /// The `near_distance_band` extension: a `near` with both distances
    /// matches the entities at least `minDistance` and at most `maxDistance`
    /// away.
    Admitted,
}

impl NearDistanceBand {
    /// Admit `georel` under this decision.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::NearDistanceBand`] for a distance band while the
    /// band is [`NearDistanceBand::Refused`].
    pub const fn admit(self, georel: &GeoRel) -> Result<(), QueryError> {
        match self {
            NearDistanceBand::Refused if georel.is_distance_band() => Err(QueryError::NearDistanceBand),
            NearDistanceBand::Refused | NearDistanceBand::Admitted => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::QueryError, geo::near_distance_band::NearDistanceBand};
    use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;

    fn georel(raw: &str) -> GeoRel {
        raw.parse().expect(raw)
    }

    #[test]
    fn a_band_is_refused_without_the_extension() {
        assert!(matches!(
            NearDistanceBand::Refused.admit(&georel("near;minDistance==500;maxDistance==10000")),
            Err(QueryError::NearDistanceBand)
        ));
    }

    #[test]
    fn a_band_is_admitted_with_the_extension() {
        assert!(NearDistanceBand::Admitted.admit(&georel("near;minDistance==500;maxDistance==10000")).is_ok());
    }

    #[test]
    fn a_single_distance_is_admitted_either_way() {
        for band in [NearDistanceBand::Refused, NearDistanceBand::Admitted] {
            for raw in ["near;maxDistance==500", "near;minDistance==500", "within"] {
                assert!(band.admit(&georel(raw)).is_ok(), "{band:?} {raw}");
            }
        }
    }
}
