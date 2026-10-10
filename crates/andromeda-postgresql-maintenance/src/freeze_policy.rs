//! Classification of a database's anti-wraparound freeze ages.

/// Age at and past which a database is closer to wraparound than to its
/// freeze maximum: about half of the 2^31 transaction IDs a database may span.
const CRITICAL_FREEZE_AGE: u64 = 1_000_000_000;

/// One ID space's age in a database beside the server's freeze maximum for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FreezeAge {
    /// Age of the database's oldest unfrozen ID (`datfrozenxid` or
    /// `datminmxid`).
    pub(crate) age: u64,
    /// The age at which autovacuum forces an anti-wraparound vacuum
    /// (`autovacuum_freeze_max_age` or `autovacuum_multixact_freeze_max_age`).
    pub(crate) freeze_max_age: u64,
}

/// How far a database's anti-wraparound vacuum is behind.
///
/// Ordered from least to most urgent, so the more urgent of two pressures is
/// their maximum.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum FreezePressure {
    /// Below 125% of the freeze maximum: autovacuum is still within its window.
    Normal,
    /// At or past 125% of the freeze maximum: anti-wraparound vacuum has had a
    /// quarter of its window and not completed.
    Overdue,
    /// At or past one billion: about half of the wraparound limit is spent.
    Critical,
}

/// Classify a database by the more urgent of its transaction ID and multixact
/// ages. Both bounds are inclusive.
pub(crate) fn freeze_pressure(transaction: FreezeAge, multixact: FreezeAge) -> FreezePressure {
    Ord::max(id_space_pressure(transaction), id_space_pressure(multixact))
}

/// Classify one ID space's age against its freeze maximum.
const fn id_space_pressure(age: FreezeAge) -> FreezePressure {
    if age.age >= CRITICAL_FREEZE_AGE {
        return FreezePressure::Critical;
    }
    if age.age >= age.freeze_max_age.saturating_mul(5) / 4 {
        return FreezePressure::Overdue;
    }
    FreezePressure::Normal
}

#[cfg(test)]
mod tests {
    use crate::freeze_policy::{FreezeAge, FreezePressure, freeze_pressure};
    use pretty_assertions::assert_eq;

    #[test]
    fn freeze_pressure_boundaries_are_inclusive() {
        let transaction = |age| FreezeAge {
            age,
            freeze_max_age: 200_000_000,
        };
        let multixact = FreezeAge {
            age: 0,
            freeze_max_age: 400_000_000,
        };
        for (age, pressure) in [
            (249_999_999, FreezePressure::Normal),
            (250_000_000, FreezePressure::Overdue),
            (999_999_999, FreezePressure::Overdue),
            (1_000_000_000, FreezePressure::Critical),
        ] {
            assert_eq!(freeze_pressure(transaction(age), multixact), pressure, "age {age}");
        }
    }

    #[test]
    fn multixact_pressure_wins_when_higher() {
        let transaction = FreezeAge {
            age: 1_000,
            freeze_max_age: 200_000_000,
        };
        let multixact = FreezeAge {
            age: 500_000_000,
            freeze_max_age: 400_000_000,
        };
        assert_eq!(freeze_pressure(transaction, multixact), FreezePressure::Overdue);
    }
}
