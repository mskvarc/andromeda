//! Where one position sits inside a `GeoJSON` geometry.
//!
//! IETF RFC 7946 section 3.1.1 nests positions in the `coordinates` member one
//! array level per structural layer: none for a Point, one for the positions of
//! a `LineString`, two for the vertices of a Polygon's rings, three for a
//! `MultiPolygon`. A refused position is named by its index at each of those
//! levels, so a client sending a thousand-vertex boundary is told which vertex
//! to fix, not only that one of them is wrong.

use std::fmt;

/// The index of a position at each nesting level of a geometry's
/// `coordinates` member, outermost first.
///
/// Empty for the single position of a Point, `[2]` for the third position of a
/// `LineString`, `[0, 3]` for the fourth vertex of a Polygon's exterior ring,
/// `[1, 0, 3]` for that vertex in the second polygon of a `MultiPolygon`.
/// Renders as the JSON path a client reads it as: `coordinates[0][3]`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoordinatePath(Vec<usize>);

impl CoordinatePath {
    /// The path through the given indices, outermost first.
    #[must_use]
    pub const fn new(indices: Vec<usize>) -> CoordinatePath {
        CoordinatePath(indices)
    }

    /// The path of the position at `index` within the list that `enclosing`
    /// leads to.
    #[must_use]
    pub fn nested(enclosing: &[usize], index: usize) -> CoordinatePath {
        CoordinatePath(enclosing.iter().copied().chain([index]).collect())
    }

    /// The index at each nesting level, outermost first.
    #[must_use]
    pub fn indices(&self) -> &[usize] {
        &self.0
    }
}

// Hand-written because the rendering is a loop, one `[index]` per level, which
// no derive's format string can express.
impl fmt::Display for CoordinatePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("coordinates")?;
        self.0.iter().try_for_each(|index| write!(formatter, "[{index}]"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_position_is_the_coordinates_member_itself() {
        pretty_assertions::assert_eq!(CoordinatePath::default().to_string(), "coordinates");
    }

    #[test]
    fn a_nested_position_renders_one_index_per_level_outermost_first() {
        pretty_assertions::assert_eq!(CoordinatePath::nested(&[1, 0], 3).to_string(), "coordinates[1][0][3]");
        pretty_assertions::assert_eq!(CoordinatePath::nested(&[1, 0], 3).indices(), &[1, 0, 3]);
    }

    #[test]
    fn a_position_of_a_flat_list_carries_its_own_index_alone() {
        pretty_assertions::assert_eq!(CoordinatePath::nested(&[], 2), CoordinatePath::new(vec![2]));
    }
}
