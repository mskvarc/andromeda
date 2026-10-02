//! How a set-write stages one hoisted geometry of one entity row.
//!
//! The search row holds the projection of the default `location`,
//! `observationSpace` and `operationSpace` geometries (clause 4.7). Projecting
//! one parses its EWKB, validates it and compares it with the stored column, in
//! proportion to its positions, so an update that leaves a geometry as the row
//! already holds it stages no geometry at all and the row keeps its column.

use crate::{error::ContextStoreError, store_invariant::StoreInvariant};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;

/// What one set-write row says of one hoisted geometry column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedGeometry<G> {
    /// The row keeps the geometry its stored search row holds.
    ///
    /// Only an update that holds against the version it read may stage this:
    /// the search row is then still the projection of the state the update
    /// was computed from, whose geometry the update did not change.
    Kept,
    /// The row takes this geometry, `None` being no geometry.
    Written(Option<G>),
}

impl StagedGeometry<&GeometryForm> {
    /// The staging of a geometry the statement binds as EWKB.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Encoding`] when the geometry cannot be
    /// encoded as EWKB.
    pub fn encoded(self) -> Result<StagedGeometry<Vec<u8>>, ContextStoreError> {
        Ok(match self {
            StagedGeometry::Kept => StagedGeometry::Kept,
            StagedGeometry::Written(form) => StagedGeometry::Written(geometry_bytes(form)?),
        })
    }
}

impl StagedGeometry<Vec<u8>> {
    /// The EWKB a statement binds for this row, none for a kept geometry.
    #[must_use]
    pub fn ewkb(&self) -> Option<&[u8]> {
        match self {
            StagedGeometry::Kept => None,
            StagedGeometry::Written(ewkb) => ewkb.as_deref(),
        }
    }

    /// Whether the row keeps its stored geometry, as the update binds it.
    #[must_use]
    pub const fn is_kept(&self) -> bool {
        matches!(self, StagedGeometry::Kept)
    }

    /// The EWKB a row that creates its search row binds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreInvariant::KeptGeometryWithoutStoredRow`] for a kept
    /// geometry: a row that is being created holds no geometry to keep.
    pub fn written(&self) -> Result<Option<&[u8]>, StoreInvariant> {
        match self {
            StagedGeometry::Kept => Err(StoreInvariant::KeptGeometryWithoutStoredRow),
            StagedGeometry::Written(ewkb) => Ok(ewkb.as_deref()),
        }
    }
}

/// The EWKB every row of an inserting statement binds for one geometry column.
///
/// # Errors
///
/// Returns [`StoreInvariant::KeptGeometryWithoutStoredRow`] when a row stages
/// a kept geometry.
pub(crate) fn written_column(column: &[StagedGeometry<Vec<u8>>]) -> Result<Vec<Option<&[u8]>>, StoreInvariant> {
    column.iter().map(StagedGeometry::written).collect()
}

/// The EWKB every row of an updating statement binds for one geometry column,
/// none where the row keeps its geometry.
pub(crate) fn staged_column(column: &[StagedGeometry<Vec<u8>>]) -> Vec<Option<&[u8]>> {
    column.iter().map(StagedGeometry::ewkb).collect()
}

/// Whether each row of an updating statement keeps one geometry column.
pub(crate) fn kept_column(column: &[StagedGeometry<Vec<u8>>]) -> Vec<bool> {
    column.iter().map(StagedGeometry::is_kept).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::staged_geometry::{StagedGeometry, kept_column, staged_column, written_column},
        store_invariant::StoreInvariant,
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, position::Position};

    fn point() -> GeometryForm {
        GeometryForm::Point(Position::try_new(13.4, 52.5, None).expect("a test position lies on the CRS84 axes"))
    }

    /// A kept geometry binds no EWKB, so the statement never parses or
    /// validates the positions, and is flagged as kept.
    #[test]
    fn a_kept_geometry_binds_no_ewkb_and_is_flagged() {
        let form = point();
        let column = vec![
            StagedGeometry::Kept,
            StagedGeometry::Written(Some(&form)).encoded().expect("a point encodes"),
            StagedGeometry::Written(None).encoded().expect("no geometry encodes"),
        ];
        pretty_assertions::assert_eq!(kept_column(&column), vec![true, false, false]);
        let staged = staged_column(&column);
        assert!(staged[0].is_none());
        assert!(staged[1].is_some_and(|ewkb| !ewkb.is_empty()));
        assert!(staged[2].is_none());
    }

    /// A row that creates its search row has no stored geometry to keep.
    #[test]
    fn an_inserting_statement_refuses_a_kept_geometry() {
        let column = vec![StagedGeometry::Written(None), StagedGeometry::Kept];
        assert!(matches!(written_column(&column), Err(StoreInvariant::KeptGeometryWithoutStoredRow)));
        assert!(written_column(&column[..1]).is_ok_and(|ewkb| ewkb == vec![None]));
    }
}
