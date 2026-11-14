//! The clause 4.9 conformance cases: for each operator, a filter, an Entity it
//! selects and an Entity it rejects.
//!
//! An Entity that does not carry the addressed element never satisfies a
//! predicate over it, in either polarity, so every case also expects the
//! rejection of an Entity with none of the three Attributes. Each execution of
//! `q` reads these cases: the in-memory evaluator selects by them and the
//! statements over stored rows conjoin the presence test they imply.

use iri_rs::{Iri, iri};

/// The expanded name of the Number Attribute the cases read.
pub const SPEED: Iri<&str> = iri!("https://example.com/speed");
/// The expanded name of the String Attribute the cases compare with values.
pub const COLOR: Iri<&str> = iri!("https://example.com/color");
/// The expanded name of the String Attribute the cases match with patterns.
pub const NAME: Iri<&str> = iri!("https://example.com/name");

/// An Entity carrying one Property value for each of [`SPEED`], [`COLOR`] and
/// [`NAME`].
#[derive(Clone, Copy, Debug)]
pub struct ConformanceVehicle {
    /// The Number value of [`SPEED`].
    pub speed: i64,
    /// The String value of [`COLOR`].
    pub color: &'static str,
    /// The String value of [`NAME`].
    pub name: &'static str,
}

/// One filter with an Entity it selects and, unless the filter is a bare
/// existence test, one it rejects.
#[derive(Clone, Copy, Debug)]
pub struct ConformanceCase {
    /// The filter, naming the Attributes by expanded name.
    pub filter: &'static str,
    /// An Entity the filter selects.
    pub selected: ConformanceVehicle,
    /// An Entity the filter rejects, none for existence, which rejects only an
    /// Entity without the Attribute.
    pub rejected: Option<ConformanceVehicle>,
}

/// A vehicle of the given values.
const fn vehicle(speed: i64, color: &'static str, name: &'static str) -> ConformanceVehicle {
    ConformanceVehicle { speed, color, name }
}

/// The conformance cases.
pub const CONFORMANCE: &[ConformanceCase] = &[
    ConformanceCase {
        filter: "\"https://example.com/speed\">50",
        selected: vehicle(80, "red", "Alpha"),
        rejected: Some(vehicle(10, "red", "Alpha")),
    },
    ConformanceCase {
        filter: "\"https://example.com/speed\"==10..20",
        selected: vehicle(15, "red", "Alpha"),
        rejected: Some(vehicle(25, "red", "Alpha")),
    },
    ConformanceCase {
        filter: "\"https://example.com/speed\"!=10..20",
        selected: vehicle(25, "red", "Alpha"),
        rejected: Some(vehicle(15, "red", "Alpha")),
    },
    ConformanceCase {
        filter: "\"https://example.com/color\"==\"red\",\"blue\"",
        selected: vehicle(80, "red", "Alpha"),
        rejected: Some(vehicle(80, "green", "Alpha")),
    },
    ConformanceCase {
        filter: "\"https://example.com/color\"!=\"red\",\"blue\"",
        selected: vehicle(80, "green", "Alpha"),
        rejected: Some(vehicle(80, "red", "Alpha")),
    },
    ConformanceCase {
        filter: "\"https://example.com/name\"~=\"^A.*\"",
        selected: vehicle(80, "red", "Alpha"),
        rejected: Some(vehicle(80, "red", "Beta")),
    },
    ConformanceCase {
        filter: "\"https://example.com/name\"!~=\"^A.*\"",
        selected: vehicle(80, "red", "Beta"),
        rejected: Some(vehicle(80, "red", "Alpha")),
    },
    ConformanceCase {
        filter: "\"https://example.com/speed\"",
        selected: vehicle(80, "red", "Alpha"),
        rejected: None,
    },
];

#[cfg(test)]
mod tests {
    use crate::conformance::{COLOR, CONFORMANCE, NAME, SPEED};

    #[test]
    fn every_filter_names_an_attribute_the_vehicles_carry() {
        for case in CONFORMANCE {
            assert!(
                [SPEED, COLOR, NAME]
                    .iter()
                    .any(|attribute| case.filter.contains(&format!("\"{}\"", attribute.as_str()))),
                "{} names no conformance attribute",
                case.filter
            );
        }
    }
}
