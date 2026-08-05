//! Binding typed query values into PostgreSQL statements.
//!
//! The query layer decides *what* to bind and in which order; translating that
//! decision into `sqlx` calls is a store concern, which is why this lives here
//! and not beside [`BindValue`](andromeda_query::sql::bind::BindValue). Keeping it
//! on this side of the boundary is also what keeps `sqlx` out of the query
//! layer's dependency list, for the same reason it stays out of
//! `andromeda-geometry`.

/// Bind every value in a [`BindValue`](andromeda_query::sql::bind::BindValue)
/// slice to a `sqlx` query, in placeholder order.
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
                ::andromeda_query::sql::bind::BindValue::String(s) => q.bind(s.as_ref()),
                ::andromeda_query::sql::bind::BindValue::StringVec(v) => q.bind(v.as_ref()),
                ::andromeda_query::sql::bind::BindValue::I64(i) => q.bind(*i),
                ::andromeda_query::sql::bind::BindValue::I16(i) => q.bind(*i),
                ::andromeda_query::sql::bind::BindValue::F64(f) => q.bind(*f),
                ::andromeda_query::sql::bind::BindValue::Bool(b) => q.bind(*b),
                ::andromeda_query::sql::bind::BindValue::Timestamp(t) => q.bind(*t),
                ::andromeda_query::sql::bind::BindValue::Bytes(b) => q.bind(b.as_ref()),
            };
        }
        q
    }};
}
