//! The arcs of a geometry in an R-tree, keyed by the longitude/latitude
//! rectangle each covers, so a search for the arcs near a point or another arc
//! measures only those whose rectangle it reaches.

use crate::great_circle::{arc_bounds, arcs};
use geo::{Line, Rect};
use rstar::{AABB, RTree, RTreeObject};

/// One great-circle arc of a geometry, under the box that covers it.
#[derive(Debug)]
pub(crate) struct IndexedArc {
    /// The arc.
    pub(crate) arc: Line<f64>,
    /// Its great-circle bounds.
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for IndexedArc {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        self.envelope
    }
}

/// Every arc of a geometry, indexed by its great-circle bounds.
pub(crate) fn arc_tree(geometry: &geo::Geometry<f64>) -> RTree<IndexedArc> {
    RTree::bulk_load(
        arcs(geometry)
            .into_iter()
            .map(|arc| IndexedArc {
                arc,
                envelope: aabb(&arc_bounds(arc)),
            })
            .collect(),
    )
}

/// The R-tree box of a longitude/latitude rectangle.
pub(crate) fn aabb(rect: &Rect<f64>) -> AABB<[f64; 2]> {
    AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])
}
