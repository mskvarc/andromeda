//! Binding typed query values into PostgreSQL statements.
//!
//! The query layer decides *what* to bind and in which order, as a sequence of
//! [`BindValue`](crate::bind::BindValue)s; this macro drives them onto a
//! `sqlx` query builder in that same order.

/// Bind every value in a [`BindValue`](crate::bind::BindValue) slice to a
/// `sqlx` query, in placeholder order.
///
/// Expands to a loop that consumes the query builder and hands it back once
/// every value is bound, so the result is the same query type that was passed
/// in. That works with `sqlx::query()`, `sqlx::query_scalar()` and
/// `sqlx::query_as()` alike, since all three share the `.bind()` signature.
///
/// The arms name `BindValue` by its full path rather than importing it: a macro
/// expands at its call site, where no such import exists.
#[macro_export]
macro_rules! bind_values {
    ($query:expr, $binds:expr) => {{
        let mut q = $query;
        for bind in $binds {
            q = match bind {
                ::andromeda_postgresql_query::bind::BindValue::String(s) => q.bind(s.as_ref()),
                ::andromeda_postgresql_query::bind::BindValue::StringVec(v) => q.bind(v.as_ref()),
                ::andromeda_postgresql_query::bind::BindValue::I64(i) => q.bind(*i),
                ::andromeda_postgresql_query::bind::BindValue::I16(i) => q.bind(*i),
                ::andromeda_postgresql_query::bind::BindValue::F64(f) => q.bind(*f),
                ::andromeda_postgresql_query::bind::BindValue::Bool(b) => q.bind(*b),
                ::andromeda_postgresql_query::bind::BindValue::Timestamp(t) => q.bind(*t),
                ::andromeda_postgresql_query::bind::BindValue::Bytes(b) => q.bind(b.as_ref()),
            };
        }
        q
    }};
}
