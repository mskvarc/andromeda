//! The refined value shape one attribute-history instance was recorded in, at
//! the grain Tables 4.5.19.1-1, 4.5.19.1-2 and 4.5.19.1-3 distinguish.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The value shape one history instance was recorded in.
    ///
    /// One member per column of Tables 4.5.19.1-1 and 4.5.19.1-2, plus the URI a
    /// Relationship object is held as, which Table 4.5.19.1-3 aggregates. It is
    /// finer than [`ValueKind`](crate::value_kind::ValueKind), which names the
    /// typed column a value sits in: one JSONB column holds both a JSON Object
    /// and a JSON Array, and one text column a JSON String, a `Date` and a
    /// `Time`, while the tables give each of them different methods.
    ///
    /// The stream catalogue persists it beside the family for every shape a
    /// stream holds, and a temporal aggregation projects it per row, so its
    /// numbers are a stored contract.
    ///
    /// Andromeda owns this vocabulary rather than the specification, so the
    /// labels are kebab-case.
    RecordedShape {
        frozen_at: (1, 0, 0),
        frozen_prefix: 10,
        frozen_digest: 0x8a8f_bc94_b079_29bb,
        codes: {
            /// A JSON number.
            Number = 1, "number";
            /// A JSON string that is neither of the two canonical temporal renderings.
            String = 2, "string";
            /// A JSON boolean.
            Boolean = 3, "boolean";
            /// A `GeoJSON` geometry.
            Geometry = 4, "geometry";
            /// A JSON object.
            Object = 5, "object";
            /// A JSON array, whose aggregates are taken over its size.
            Array = 6, "array";
            /// A clause 4.6.3 `DateTime`.
            DateTime = 7, "date-time";
            /// A clause 4.6.3 `Date`.
            Date = 8, "date";
            /// A clause 4.6.3 `Time`.
            Time = 9, "time";
            /// A URI, which is how a Relationship object is held.
            Uri = 10, "uri";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{compact_code_domain::CompactCodeDomain, recorded_shape::RecordedShape, registry::UnknownCompactCode};
    use pretty_assertions::assert_eq;

    /// One member per column of Tables 4.5.19.1-1 and 4.5.19.1-2 beside the
    /// URI of Table 4.5.19.1-3: a shared member would merge two columns of the
    /// tables back together.
    #[test]
    fn every_aggregation_table_column_has_exactly_one_member() {
        assert_eq!(RecordedShape::ALL.len(), 10);
    }

    #[test]
    fn a_code_no_shape_carries_names_the_recorded_shape_domain() {
        for code in [0, 11, -1] {
            assert_eq!(
                RecordedShape::try_from(code),
                Err(UnknownCompactCode::new(CompactCodeDomain::RecordedShape, code)),
                "{code}"
            );
        }
    }
}
