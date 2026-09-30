//! How the inputs of one aggregation group disagree in meaning.

use derive_more::Display;

/// The facet on which the instances one aggregation group spans disagree, so
/// that a value-sensitive aggregate over them would compare incomparable values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum AggregationConflict {
    /// Two different unit codes, which the broker does not convert between.
    #[display("unit codes the broker does not convert between")]
    Units,
    /// Values carrying a unit code beside values carrying none.
    #[display("values carrying a unit code and values carrying no unit at all")]
    UnitAndUnitless,
    /// Instances written as two different attribute families.
    #[display("instances written as different attribute families")]
    Families,
    /// Values recorded in two shapes with no common ordering or scale.
    #[display("values of different shapes, which have no common ordering or scale")]
    ValueShapes,
}
