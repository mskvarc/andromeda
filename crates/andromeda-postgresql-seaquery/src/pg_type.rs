//! PostgreSQL type names as identifiers.
//!
//! Every cast names a physical type, and naming it once here keeps two casts
//! from spelling the same type differently. The schema declares no enum type:
//! closed vocabularies are `SMALLINT` registry codes, which need no cast. The array
//! variants carry their `[]` so an `UNNEST` argument and an `::integer[]` cast
//! read the element type the same way a scalar column does.

use sea_query::Iden;

/// A PostgreSQL type, as it is spelled where a statement names one.
///
/// Backs [`crate::cast`]: a value cast to
/// [`PostgresType::Numeric`] and a column typed `numeric` refer to one type, so
/// the token is written in exactly one place.
#[derive(Iden, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostgresType {
    /// Arbitrary-precision `numeric`; the value column NGSI-LD numbers land in.
    #[iden = "numeric"]
    Numeric,
    /// `boolean`.
    #[iden = "boolean"]
    Boolean,
    /// `timestamptz`, the instant type every observed/modified time carries.
    #[iden = "timestamptz"]
    Timestamptz,
    /// `time`, the time-of-day type a clause 4.6.3 `Time` value casts to.
    #[iden = "time"]
    Time,
    /// `text`.
    #[iden = "text"]
    Text,
    /// `geography`, the spheroidal type `near`/distance operands cast to.
    #[iden = "geography"]
    Geography,
    /// `integer`.
    #[iden = "integer"]
    Integer,
    /// `bigint`.
    #[iden = "bigint"]
    Bigint,
    /// `smallint`.
    #[iden = "smallint"]
    Smallint,
    /// `bytea`, the wire type EWKB geometry binds as.
    #[iden = "bytea"]
    Bytea,
    /// `jsonb`.
    #[iden = "jsonb"]
    Jsonb,
    /// `interval`, the duration type an aggregation period binds as.
    #[iden = "interval"]
    Interval,
    /// `lquery`, the `ltree` match-pattern type (clause 4.18 scope matching).
    #[iden = "lquery"]
    Lquery,
    /// `ltree`, the label-path type a scope is stored as (clause 4.18).
    #[iden = "ltree"]
    Ltree,
    /// `uuid`, the identity type write and lease identifiers carry.
    #[iden = "uuid"]
    Uuid,
    /// `regclass`, the relation-identity type a catalogue row keys a table by.
    #[iden = "regclass"]
    Regclass,
    /// `xid8`, the 64-bit transaction identifier a write is stamped with.
    #[iden = "xid8"]
    Xid8,
    /// `integer[]`.
    #[iden = "integer[]"]
    IntegerArray,
    /// `bigint[]`.
    #[iden = "bigint[]"]
    BigintArray,
    /// `smallint[]`.
    #[iden = "smallint[]"]
    SmallintArray,
    /// `ltree[]`, the array type a registration's scopes are stored as.
    #[iden = "ltree[]"]
    LtreeArray,
    /// `boolean[]`.
    #[iden = "boolean[]"]
    BooleanArray,
    /// `numeric[]`.
    #[iden = "numeric[]"]
    NumericArray,
    /// `text[]`.
    #[iden = "text[]"]
    TextArray,
    /// `bytea[]`.
    #[iden = "bytea[]"]
    ByteaArray,
    /// `jsonb[]`.
    #[iden = "jsonb[]"]
    JsonbArray,
    /// `timestamptz[]`.
    #[iden = "timestamptz[]"]
    TimestamptzArray,
    /// `uuid[]`.
    #[iden = "uuid[]"]
    UuidArray,
}

#[cfg(test)]
mod tests {
    use crate::pg_type::PostgresType;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn scalar_type_names_render_bare() {
        assert_eq!(PostgresType::Numeric.to_string(), "numeric");
        assert_eq!(PostgresType::Boolean.to_string(), "boolean");
        assert_eq!(PostgresType::Timestamptz.to_string(), "timestamptz");
        assert_eq!(PostgresType::Time.to_string(), "time");
        assert_eq!(PostgresType::Text.to_string(), "text");
        assert_eq!(PostgresType::Geography.to_string(), "geography");
        assert_eq!(PostgresType::Interval.to_string(), "interval");
        assert_eq!(PostgresType::Lquery.to_string(), "lquery");
        assert_eq!(PostgresType::Jsonb.to_string(), "jsonb");
    }

    /// An array type carries its `[]`, so an `UNNEST` argument reads the element
    /// type exactly as the matching scalar column does.
    #[test]
    fn array_type_names_carry_their_brackets() {
        assert_eq!(PostgresType::IntegerArray.to_string(), "integer[]");
        assert_eq!(PostgresType::BigintArray.to_string(), "bigint[]");
        assert_eq!(PostgresType::NumericArray.to_string(), "numeric[]");
        assert_eq!(PostgresType::TextArray.to_string(), "text[]");
        assert_eq!(PostgresType::ByteaArray.to_string(), "bytea[]");
        assert_eq!(PostgresType::JsonbArray.to_string(), "jsonb[]");
        assert_eq!(PostgresType::TimestamptzArray.to_string(), "timestamptz[]");
    }
}
