//! The SQL test of a clause 4.10 `near` relation, measured on the WGS 84
//! spheroid over `geography`.
//!
//! `PostGIS` measures two `geography` values two ways. The tree-based measure,
//! which `_ST_DWithin` uses once its cache holds a repeated operand and
//! `ST_Distance` always uses, is fast and right for every pair but one kind: it
//! reports two edges as touching, and so the two geometries as zero apart,
//! whenever an end of one lies on the great circle of the other beyond it. Such
//! an end lies at the antipode of a point of the other edge. The verdict then
//! depends on whether the statement happened to warm the cache, and disagrees
//! with the in-memory evaluator, which reads the pair as apart. The error only
//! ever brings two geometries together: a pair the tree finds apart, or at a
//! positive distance, is apart or at that distance.
//!
//! So the tree measure decides every pair it finds apart, and a pair it finds
//! within the distance is measured again edge against edge
//! ([`uncached_dwithin`], [`uncached_distance`]). That measure tests edges for
//! crossing only when the two geometries' boxes overlap, which two geometries
//! holding an antipodal pair do only when they span thousands of kilometres. It
//! costs more than the tree, but runs only on the pairs the tree finds close,
//! and stops at the first pair of edges within the distance. A point has no
//! edge, so a pair with a Point or `MultiPoint` on either side is decided by the
//! tree alone: the reference's form is known when the statement is built, and
//! the subject's dimension is read per row.

use crate::{error::RenderError, stored_geometry::StoredGeometry};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_query::{geo::geo_query_error::GeoQueryError, query_error::QueryError};
use andromeda_postgresql_seaquery::{
    literal::{false_literal, int_literal, true_literal},
    postgis::{exact_dwithin, st_dimension, st_distance, uncached_distance, uncached_dwithin},
};
use sea_query::{Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Whether a reference geometry has edges the tree measure can misread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceShape {
    /// A Point or a `MultiPoint`: no edge, so the tree measure is right
    /// against every subject.
    Points,
    /// A line or a polygon.
    Edged,
}

impl ReferenceShape {
    /// The shape of `form`.
    const fn of(form: &GeometryForm) -> ReferenceShape {
        match form {
            GeometryForm::Point(_) | GeometryForm::MultiPoint(_) => ReferenceShape::Points,
            GeometryForm::LineString(_) | GeometryForm::MultiLineString(_) | GeometryForm::Polygon(_) | GeometryForm::MultiPolygon(_) => ReferenceShape::Edged,
        }
    }
}

/// The `near` test of `subject` against the reference whose form is
/// `reference_form` and whose `geography` `reference` renders afresh for each
/// operand: within `max_distance` metres, inclusively, and at least
/// `min_distance` metres away.
///
/// # Errors
///
/// Returns [`RenderError::Query`] carrying [`GeoQueryError::NearWithoutDistance`]
/// when neither distance is given, which clause 4.10 requires at least one of.
pub(crate) fn near_expression(
    subject: &StoredGeometry,
    reference: impl Fn() -> SimpleExpr,
    reference_form: &GeometryForm,
    max_distance: Option<f64>,
    min_distance: Option<f64>,
) -> Result<SimpleExpr, RenderError> {
    let shape = ReferenceShape::of(reference_form);
    let within = max_distance.map(|distance| within(subject, &reference, shape, distance));
    let beyond = min_distance.map(|distance| beyond(subject, &reference, shape, distance));
    match (within, beyond) {
        (Some(within), Some(beyond)) => Ok(within.and(beyond)),
        (Some(half), None) | (None, Some(half)) => Ok(half),
        (None, None) => Err(RenderError::Query(QueryError::GeoQuery(GeoQueryError::NearWithoutDistance))),
    }
}

/// `subject` comes within `distance` metres of the reference, the distance
/// itself included.
fn within(subject: &StoredGeometry, reference: &impl Fn() -> SimpleExpr, shape: ReferenceShape, distance: f64) -> SimpleExpr {
    let tree = exact_dwithin(subject.geographic(), reference(), distance);
    match shape {
        ReferenceShape::Points => tree,
        ReferenceShape::Edged => SeaExpr::case(tree.not(), false_literal())
            .case(is_points(subject), true_literal())
            .finally(uncached_dwithin(subject.geographic(), reference(), distance))
            .into(),
    }
}

/// `subject` stays at least `distance` metres from the reference.
fn beyond(subject: &StoredGeometry, reference: &impl Fn() -> SimpleExpr, shape: ReferenceShape, distance: f64) -> SimpleExpr {
    let tree = SeaExpr::expr(st_distance(subject.geographic(), reference())).gte(distance);
    match shape {
        ReferenceShape::Points => tree,
        ReferenceShape::Edged => SeaExpr::case(tree, true_literal())
            .case(is_points(subject), false_literal())
            .finally(SeaExpr::expr(uncached_distance(subject.geographic(), reference(), distance)).gte(distance))
            .into(),
    }
}

/// The subject is a Point or a `MultiPoint`, which has no edge.
fn is_points(subject: &StoredGeometry) -> SimpleExpr {
    SeaExpr::expr(st_dimension(subject.planar())).eq(int_literal(0))
}

#[cfg(test)]
mod tests {
    use crate::{near_measure::near_expression, stored_geometry::StoredGeometry};
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr, Query, SelectStatement, SimpleExpr};

    fn geometry(value: serde_json::Value) -> Geometry {
        serde_json::from_value::<Geometry>(value).expect("a test geometry parses")
    }

    fn subject() -> StoredGeometry {
        let column = |name: &str| Expr::col((Alias::new("g"), Alias::new(name)));
        StoredGeometry::written(column("value_geometry_reach"), column("value_geometry_valid"), column("value_geometry"))
    }

    fn reference() -> SimpleExpr {
        Expr::col((Alias::new("r"), Alias::new("reference")))
    }

    fn render(expression: SimpleExpr) -> SelectStatement {
        Query::select().expr(expression).to_owned()
    }

    fn square() -> Geometry {
        geometry(serde_json::json!({ "type": "Polygon", "coordinates": [[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]] }))
    }

    /// A Point has no edge to misread, so the tree decides alone.
    #[test]
    fn a_point_reference_is_measured_by_the_tree_alone() {
        let point = geometry(serde_json::json!({ "type": "Point", "coordinates": [180, 0] }));
        assert_statement_snapshot!(
            "near-measure-a-point-reference-is-measured-by-the-tree-alone",
            render(near_expression(&subject(), reference, point.form(), Some(25_000.0), Some(10.0)).expect("renders"))
        );
    }

    /// The tree's verdict that an edged pair comes within the distance is
    /// measured again edge against edge, unless the subject is points.
    #[test]
    fn an_edged_reference_confirms_the_tree_within_the_distance() {
        assert_statement_snapshot!(
            "near-measure-an-edged-reference-confirms-the-tree-within-the-distance",
            render(near_expression(&subject(), reference, square().form(), Some(1000.0), None).expect("renders"))
        );
    }

    /// A distance the tree finds at least the minimum stands; a smaller one
    /// is measured again edge against edge, unless the subject is points.
    #[test]
    fn an_edged_reference_confirms_the_tree_short_of_the_minimum() {
        assert_statement_snapshot!(
            "near-measure-an-edged-reference-confirms-the-tree-short-of-the-minimum",
            render(near_expression(&subject(), reference, square().form(), None, Some(1000.0)).expect("renders"))
        );
    }
}
