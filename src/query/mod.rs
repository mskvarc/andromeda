//! NGSI-LD query compilation, SQL generation, and query language.

/// Push a SQL condition with a bind parameter, incrementing the parameter index.
///
/// This replaces the repeated three-line pattern of pushing a formatted condition,
/// pushing a bind value, and incrementing the parameter index.
macro_rules! bind_condition {
    ($conditions:expr, $binds:expr, $idx:expr, $fmt:expr, $bind_val:expr) => {{
        $conditions.push(format!($fmt, *$idx));
        $binds.push($bind_val);
        *$idx += 1;
    }};
}

pub mod compile;
pub mod compiled;
pub mod error;
pub mod q;
pub mod sql;
