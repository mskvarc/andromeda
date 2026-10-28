//! The data type a column, a composite type field or a domain is declared with,
//! and what PostgreSQL makes of the built-in ones.

use crate::declared_schema::qualified_name::QualifiedName;
use pg_query::protobuf::TypeName;

/// The schema the parser qualifies a type written as an SQL keyword with
/// (`integer` reads as `pg_catalog.int4`).
const CATALOG_SCHEMA: &str = "pg_catalog";

/// A data type as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredType {
    /// The type's name, as the parser reports it.
    name: QualifiedName,
    /// Whether the type is an array of the named one.
    array: bool,
}

impl DeclaredType {
    /// The type `type_name` declares; `None` for a type name the parser
    /// reports without a name.
    #[must_use]
    pub fn of(type_name: &TypeName) -> Option<DeclaredType> {
        Some(DeclaredType {
            name: QualifiedName::of_nodes(&type_name.names)?,
            array: !type_name.array_bounds.is_empty(),
        })
    }

    /// The type's name, as the parser reports it.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// Whether the type is an array of the named one.
    #[must_use]
    pub const fn is_array(&self) -> bool {
        self.array
    }

    /// The type as a message names it: its name, `[]` after an array.
    #[must_use]
    pub fn written(&self) -> String {
        format!("{}{}", self.name.name(), if self.array { "[]" } else { "" })
    }

    /// The built-in type the name denotes, unqualified or in `pg_catalog`.
    #[must_use]
    pub fn builtin(&self) -> Option<BuiltinType> {
        match self.name.schema() {
            None | Some(CATALOG_SCHEMA) => self.name.name().parse().ok(),
            Some(_) => None,
        }
    }
}

/// How PostgreSQL stores a value of a type in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    /// A fixed number of bytes, its offset in the row cacheable.
    FixedWidth,
    /// A length header and the bytes, compressible and movable out of line.
    VariableLength,
}

/// The width of an integer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum IntegerWidth {
    /// 16 bits.
    Smallint,
    /// 32 bits.
    Integer,
    /// 64 bits.
    Bigint,
}

/// A built-in or extension type the migrations may use, named as the parser
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum BuiltinType {
    /// `smallint`.
    Int2,
    /// `integer`.
    Int4,
    /// `bigint`.
    Int8,
    /// `real`.
    Float4,
    /// `double precision`.
    Float8,
    /// `boolean`.
    Bool,
    /// `timestamp with time zone`.
    Timestamptz,
    /// `timestamp without time zone`.
    Timestamp,
    /// `date`.
    Date,
    /// `time without time zone`.
    Time,
    /// `time with time zone`.
    Timetz,
    /// `interval`.
    Interval,
    /// `uuid`.
    Uuid,
    /// A full transaction identifier.
    Xid8,
    /// An object identifier.
    Oid,
    /// `money`.
    Money,
    /// `text`.
    Text,
    /// `character varying`.
    Varchar,
    /// `character`.
    Bpchar,
    /// `json`.
    Json,
    /// `jsonb`.
    Jsonb,
    /// `bytea`.
    Bytea,
    /// `numeric`.
    Numeric,
    /// `bit(n)`.
    Bit,
    /// `bit varying`.
    Varbit,
    /// A text search document.
    Tsvector,
    /// `PostGIS` planar geometry.
    Geometry,
    /// `PostGIS` geodetic geometry.
    Geography,
    /// A label path.
    Ltree,
    /// A label path pattern.
    Lquery,
}

impl BuiltinType {
    /// How a value of the type is stored.
    #[must_use]
    pub const fn storage(self) -> Storage {
        match self {
            BuiltinType::Int2
            | BuiltinType::Int4
            | BuiltinType::Int8
            | BuiltinType::Float4
            | BuiltinType::Float8
            | BuiltinType::Bool
            | BuiltinType::Timestamptz
            | BuiltinType::Timestamp
            | BuiltinType::Date
            | BuiltinType::Time
            | BuiltinType::Timetz
            | BuiltinType::Interval
            | BuiltinType::Uuid
            | BuiltinType::Xid8
            | BuiltinType::Oid
            | BuiltinType::Money => Storage::FixedWidth,
            BuiltinType::Text
            | BuiltinType::Varchar
            | BuiltinType::Bpchar
            | BuiltinType::Json
            | BuiltinType::Jsonb
            | BuiltinType::Bytea
            | BuiltinType::Numeric
            | BuiltinType::Bit
            | BuiltinType::Varbit
            | BuiltinType::Tsvector
            | BuiltinType::Geometry
            | BuiltinType::Geography
            | BuiltinType::Ltree
            | BuiltinType::Lquery => Storage::VariableLength,
        }
    }

    /// Whether a value of the type carries a collation.
    #[must_use]
    pub const fn is_collatable(self) -> bool {
        matches!(self, BuiltinType::Text | BuiltinType::Varchar | BuiltinType::Bpchar)
    }

    /// The width of the type, when it is an integer.
    #[must_use]
    pub const fn integer_width(self) -> Option<IntegerWidth> {
        match self {
            BuiltinType::Int2 => Some(IntegerWidth::Smallint),
            BuiltinType::Int4 => Some(IntegerWidth::Integer),
            BuiltinType::Int8 => Some(IntegerWidth::Bigint),
            BuiltinType::Float4
            | BuiltinType::Float8
            | BuiltinType::Bool
            | BuiltinType::Timestamptz
            | BuiltinType::Timestamp
            | BuiltinType::Date
            | BuiltinType::Time
            | BuiltinType::Timetz
            | BuiltinType::Interval
            | BuiltinType::Uuid
            | BuiltinType::Xid8
            | BuiltinType::Oid
            | BuiltinType::Money
            | BuiltinType::Text
            | BuiltinType::Varchar
            | BuiltinType::Bpchar
            | BuiltinType::Json
            | BuiltinType::Jsonb
            | BuiltinType::Bytea
            | BuiltinType::Numeric
            | BuiltinType::Bit
            | BuiltinType::Varbit
            | BuiltinType::Tsvector
            | BuiltinType::Geometry
            | BuiltinType::Geography
            | BuiltinType::Ltree
            | BuiltinType::Lquery => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::declared_schema::declared_type::{BuiltinType, DeclaredType, IntegerWidth, Storage};
    use pretty_assertions::assert_eq;

    /// The type of a column declared `written`.
    fn declared(written: &str) -> Option<DeclaredType> {
        let parsed = pg_query::parse(&format!("create table t (c {written})"), 0).ok()?;
        let statement = parsed.protobuf.stmts.into_iter().next()?.stmt?.node?;
        let pg_query::NodeEnum::CreateStmt(create) = statement else {
            return None;
        };
        let pg_query::NodeEnum::ColumnDef(column) = create.table_elts.into_iter().next()?.node? else {
            return None;
        };
        DeclaredType::of(column.type_name.as_ref()?)
    }

    #[test]
    fn a_keyword_type_reads_as_its_catalog_type() {
        let integer = declared("integer");
        assert_eq!(integer.as_ref().and_then(DeclaredType::builtin), Some(BuiltinType::Int4));
        assert_eq!(
            integer.and_then(|integer| integer.builtin()).and_then(BuiltinType::integer_width),
            Some(IntegerWidth::Integer)
        );
        assert_eq!(
            declared("timestamp with time zone").and_then(|written| written.builtin()),
            Some(BuiltinType::Timestamptz)
        );
    }

    #[test]
    fn an_array_keeps_its_element_type() {
        let array = declared("text[]");
        assert_eq!(
            array.as_ref().map(|array| (array.is_array(), array.builtin())),
            Some((true, Some(BuiltinType::Text)))
        );
    }

    #[test]
    fn a_bit_string_is_variable_length() {
        assert_eq!(
            declared("bit(80)").and_then(|bit| bit.builtin()).map(BuiltinType::storage),
            Some(Storage::VariableLength)
        );
    }

    #[test]
    fn a_type_of_another_schema_is_not_built_in() {
        assert_eq!(declared("public.text").and_then(|other| other.builtin()), None);
    }
}
