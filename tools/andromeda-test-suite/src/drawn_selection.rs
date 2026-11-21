//! What the generated filters of a differential selected in each round, and the bounds a generated test holds them
//! to.
//!
//! Two executions that agree on a filter selecting nothing, or every Entity, or the same Entities as another filter,
//! say less about each other than two that agree on a filter selecting some Entities and leaving others. A generated
//! test sums what its filters selected over all of its seeds and expects, in each round, most filters to select at
//! least one Entity, the median filter that selects any to select several, few filters to select every Entity, and
//! few answers of several Entities to repeat an earlier answer of their case.

use googletest::prelude::*;
use ngsi_ld::model::EntityId;
use std::collections::HashSet;

/// The share of the filters, in percent, that select at least one Entity in a round, at the least.
const SELECTING_FLOOR_PERCENT: usize = 65;

/// The share of the filters, in percent, that select every Entity of their case in a round, at the most.
const EVERY_CEILING_PERCENT: usize = 10;

/// The share of the answers of two or more Entities, in percent, that name the same Entities as an earlier filter of
/// their case in the same round, at the most.
const REPEATED_CEILING_PERCENT: usize = 5;

/// The write after which the two executions are compared.
#[derive(Debug, Clone, Copy, strum::Display)]
pub(crate) enum Moment {
    /// Every Entity has been created.
    #[strum(serialize = "after every Entity was created")]
    Created,
    /// Every Entity has been changed by its second write.
    #[strum(serialize = "after every Entity was changed")]
    Changed,
}

/// The bounds a generated test holds what its filters selected to, beside the shares every test shares.
#[derive(Debug, Clone, Copy)]
pub struct SelectionBounds {
    /// The fewest Entities the median filter that selects any may select.
    median_floor: usize,
}

impl SelectionBounds {
    /// The bounds of the generated `q` filters, over cases of 20 Entities.
    pub const Q: SelectionBounds = SelectionBounds { median_floor: 3 };

    /// The bounds of the generated geoqueries, over cases of 12 Entities.
    pub const GEO: SelectionBounds = SelectionBounds { median_floor: 2 };
}

/// What the filters of one round selected.
#[derive(Debug, Clone, Default)]
pub struct RoundSelection {
    /// How many filters were asked.
    asked: usize,
    /// How many Entities each filter that selected any selected.
    sizes: Vec<usize>,
    /// How many filters selected every Entity of their case.
    every: usize,
    /// How many answers of two or more Entities named the same Entities as an earlier filter of their case.
    repeated: usize,
}

impl RoundSelection {
    /// What `answers`, the Entities each filter of one case selected in one round, in the order the filters were
    /// drawn, selected among the `entities` Entities of the case.
    pub(crate) fn of(answers: &[HashSet<EntityId>], entities: usize) -> RoundSelection {
        let mut selection = RoundSelection {
            asked: answers.len(),
            ..RoundSelection::default()
        };
        for (position, answer) in answers.iter().enumerate() {
            if !answer.is_empty() {
                selection.sizes.push(answer.len());
            }
            selection.every += usize::from(answer.len() == entities);
            let repeats = answer.len() >= 2 && answers.iter().take(position).any(|earlier| earlier == answer);
            selection.repeated += usize::from(repeats);
        }
        selection
    }

    /// The round of `other` added to this one.
    fn absorb(&mut self, other: RoundSelection) {
        self.asked += other.asked;
        self.sizes.extend(other.sizes);
        self.every += other.every;
        self.repeated += other.repeated;
    }

    /// How many answers named two or more Entities.
    fn several(&self) -> usize {
        self.sizes.iter().filter(|size| **size >= 2).count()
    }

    /// How many Entities the median filter that selected any selected, the lower of the two middle ones for an even
    /// count, None when no filter selected any.
    fn median(&self) -> Option<usize> {
        let mut sizes = self.sizes.clone();
        sizes.sort_unstable();
        sizes.get(sizes.len().checked_sub(1)? / 2).copied()
    }

    /// Expects the round to hold `bounds` and the shares every test shares, a failure naming `moment` and the figures.
    fn expect_within(&self, moment: Moment, bounds: SelectionBounds) {
        let selecting = self.sizes.len();
        expect_true!(
            selecting * 100 >= self.asked * SELECTING_FLOOR_PERCENT,
            "{moment}, {selecting} of {} filters selected at least one Entity, under the floor of {SELECTING_FLOOR_PERCENT}%",
            self.asked
        );
        expect_that!(
            self.median(),
            some(ge(bounds.median_floor)),
            "{moment}, the median filter selecting any selected under {} Entities",
            bounds.median_floor
        );
        expect_true!(
            self.every * 100 <= self.asked * EVERY_CEILING_PERCENT,
            "{moment}, {} of {} filters selected every Entity of their case, over the ceiling of {EVERY_CEILING_PERCENT}%",
            self.every,
            self.asked
        );
        expect_true!(
            self.repeated * 100 <= self.several() * REPEATED_CEILING_PERCENT,
            "{moment}, {} of {} answers of several Entities named the Entities of an earlier answer of their case, over the ceiling of {REPEATED_CEILING_PERCENT}%",
            self.repeated,
            self.several()
        );
    }
}

/// What the filters of both rounds selected.
#[derive(Debug, Clone, Default)]
pub struct Selections {
    /// After every Entity was created.
    created: RoundSelection,
    /// After every Entity was changed.
    changed: RoundSelection,
}

impl Selections {
    /// What a case selected `created` after its Entities were created and `changed` after they were changed.
    pub(crate) const fn new(created: RoundSelection, changed: RoundSelection) -> Selections {
        Selections { created, changed }
    }

    /// The selections of `other` added to these.
    pub fn absorb(&mut self, other: Selections) {
        self.created.absorb(other.created);
        self.changed.absorb(other.changed);
    }

    /// Expects both rounds to hold `bounds` and the shares every test shares, a failure naming the round and the
    /// figures.
    pub fn expect_within(&self, bounds: SelectionBounds) {
        self.created.expect_within(Moment::Created, bounds);
        self.changed.expect_within(Moment::Changed, bounds);
    }
}

#[cfg(test)]
mod tests {
    use crate::drawn_selection::RoundSelection;
    use googletest::prelude::*;
    use ngsi_ld::model::EntityId;
    use std::collections::HashSet;

    /// The answer naming the Entities `urn:x:<n>` for each of `numbers`.
    fn answer(numbers: &[u8]) -> Result<HashSet<EntityId>> {
        numbers.iter().map(|number| Ok(format!("urn:x:{number}").parse()?)).collect()
    }

    #[gtest]
    fn a_round_counts_selecting_complete_and_repeated_answers() -> Result<()> {
        let answers = [answer(&[])?, answer(&[1])?, answer(&[1, 2])?, answer(&[1, 2])?, answer(&[1, 2, 3])?];
        let round = RoundSelection::of(&answers, 3);
        expect_that!(round.asked, eq(5));
        expect_that!(round.sizes, elements_are![eq(&1), eq(&2), eq(&2), eq(&3)]);
        expect_that!(round.every, eq(1));
        expect_that!(round.repeated, eq(1));
        expect_that!(round.several(), eq(3));
        verify_that!(round.median(), some(eq(2)))
    }

    #[gtest]
    fn a_round_where_no_filter_selects_has_no_median() {
        expect_that!(RoundSelection::of(&[HashSet::new()], 3).median(), none());
    }
}
