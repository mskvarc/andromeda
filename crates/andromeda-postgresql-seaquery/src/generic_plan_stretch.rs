//! A stretch of a transaction whose statements PostgreSQL plans once per
//! connection, without their values.
//!
//! A statement reading a bound array through `UNNEST($n)` is costed by a
//! custom plan from the array it is given, one element mostly, and by a
//! generic plan as ten elements. Under `plan_cache_mode = auto` the generic
//! plan then never looks as cheap as the custom plans before it, and the
//! statement is planned again on every execution although its one access path
//! serves every array it is given. `force_generic_plan` plans it once.
//!
//! `SET LOCAL` lasts to the end of the transaction, so a stretch reads the mode
//! in force when it begins and puts it back when it ends, and the statements
//! after it are planned as before. A stretch cut short by an error leaves the
//! mode set: the error aborts the transaction, which runs no other statement.

use crate::{
    introspection::current_setting,
    setting::{Setting, SettingScope, set_config},
    statement::Statement,
};
use sea_query::{Iden, Query, SelectStatement};
use sqlx::PgConnection;
use strum::{EnumString, IntoStaticStr};

/// A value of `plan_cache_mode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, EnumString, IntoStaticStr)]
enum PlanCacheMode {
    /// A generic plan once its estimated cost is not above the custom plans'
    /// average.
    #[strum(serialize = "auto")]
    Auto,
    /// A custom plan on every execution.
    #[strum(serialize = "force_custom_plan")]
    ForceCustomPlan,
    /// The generic plan on every execution.
    #[strum(serialize = "force_generic_plan")]
    ForceGenericPlan,
}

/// The fenced subquery reading the mode in force, `prior`.
#[derive(Iden, Clone, Copy)]
#[iden = "prior"]
struct Prior;

/// The mode the fenced subquery read, `mode`.
#[derive(Iden, Clone, Copy)]
#[iden = "mode"]
struct Mode;

/// A stretch of the current transaction planned with generic plans, holding
/// the mode it puts back when it ends.
#[derive(Debug)]
#[must_use = "the transaction keeps generic plans until the stretch ends"]
pub struct GenericPlanStretch {
    /// The mode in force when the stretch began.
    prior: PlanCacheMode,
}

impl GenericPlanStretch {
    /// Plan the statements the transaction on `connection` runs next with
    /// generic plans, until [`GenericPlanStretch::end`].
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the mode cannot be read or set, and
    /// [`sqlx::Error::Decode`] when the server reports a mode PostgreSQL does
    /// not define.
    pub async fn begin(connection: &mut PgConnection) -> Result<GenericPlanStretch, sqlx::Error> {
        let prior: String = begin_statement().to_query_scalar().fetch_one(connection).await?;
        let prior = prior.parse::<PlanCacheMode>().map_err(|unknown| sqlx::Error::Decode(Box::new(unknown)))?;
        Ok(GenericPlanStretch { prior })
    }

    /// Put back the mode in force before the stretch.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the mode cannot be set.
    pub async fn end(self, connection: &mut PgConnection) -> Result<(), sqlx::Error> {
        mode_statement(self.prior).to_query().execute(connection).await?;
        Ok(())
    }
}

/// The mode in force, read before the stretch's own is set:
/// `SELECT "prior"."mode", set_config('plan_cache_mode', 'force_generic_plan', true)
/// FROM (SELECT current_setting('plan_cache_mode') AS "mode" OFFSET 0) AS "prior"`.
///
/// The fenced subquery is a node of its own, so its row, and the mode in it,
/// is read before the outer projection sets the new mode.
fn begin_statement() -> SelectStatement {
    let read = Query::select().expr_as(current_setting(Setting::PlanCacheMode), Mode).offset(0).take();
    Query::select()
        .column((Prior, Mode))
        .expr(set_config(
            Setting::PlanCacheMode,
            PlanCacheMode::ForceGenericPlan.into(),
            SettingScope::Transaction,
        ))
        .from_subquery(read, Prior)
        .take()
}

/// `SELECT set_config('plan_cache_mode', <mode>, true)`.
fn mode_statement(mode: PlanCacheMode) -> SelectStatement {
    Query::select()
        .expr(set_config(Setting::PlanCacheMode, mode.into(), SettingScope::Transaction))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::{
        generic_plan_stretch::{PlanCacheMode, begin_statement, mode_statement},
        statement::rendering::Rendered,
    };
    use pretty_assertions::assert_eq;
    use sea_query::Value;

    #[test]
    fn the_stretch_reads_the_mode_in_force_before_setting_its_own() {
        let (sql, values) = begin_statement().rendered();
        assert_eq!(
            sql,
            r#"SELECT "prior"."mode", set_config($1, $2, TRUE) FROM (SELECT current_setting($3) AS "mode" OFFSET 0) AS "prior""#
        );
        assert_eq!(
            values.0,
            vec![
                Value::from("plan_cache_mode"),
                Value::from("force_generic_plan"),
                Value::from("plan_cache_mode")
            ]
        );
    }

    #[test]
    fn the_stretch_puts_back_the_mode_it_read() {
        let (sql, values) = mode_statement(PlanCacheMode::Auto).rendered();
        assert_eq!(sql, "SELECT set_config($1, $2, TRUE)");
        assert_eq!(values.0, vec![Value::from("plan_cache_mode"), Value::from("auto")]);
    }

    #[test]
    fn every_mode_postgresql_defines_is_read_back() {
        for mode in ["auto", "force_custom_plan", "force_generic_plan"] {
            assert_eq!(mode.parse::<PlanCacheMode>().map(<&'static str>::from), Ok(mode));
        }
    }
}
