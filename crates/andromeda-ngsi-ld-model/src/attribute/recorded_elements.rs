//! The elements of an array, recorded against the budget of values the entity
//! they belong to may still hold.

use andromeda_ngsi_ld_representation::value_budget::ValueBudget;
use serde::{
    Deserialize,
    de::{IgnoredAny, SeqAccess},
};

/// Read the elements of an array, one value each, no further than `budget`
/// admits.
///
/// Once the budget is spent the rest of the array is skipped without being
/// recorded, so an array of millions of elements costs what the budget costs
/// whatever its length. The array is cut only where the entity it belongs to
/// is certain to be over its value limit when it is counted, and that count is
/// what refuses it. Without a budget every element is recorded.
///
/// # Errors
///
/// Returns the failure of the first element that does not read as a `T`, and of
/// the array itself.
pub(crate) fn recorded_elements<'de, T: Deserialize<'de>, A: SeqAccess<'de>>(mut elements: A, budget: Option<&ValueBudget>) -> Result<Vec<T>, A::Error> {
    let mut recorded = Vec::new();
    while budget.is_none_or(|budget| !budget.is_spent()) {
        let Some(element) = elements.next_element::<T>()? else {
            return Ok(recorded);
        };
        if let Some(budget) = budget {
            budget.spend();
        }
        recorded.push(element);
    }
    while elements.next_element::<IgnoredAny>()?.is_some() {}
    Ok(recorded)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::attribute::recorded_elements::recorded_elements;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::value_budget::ValueBudget;
    use serde::{
        Deserializer,
        de::{SeqAccess, Visitor},
    };
    use std::{fmt, num::NonZeroUsize};

    /// Reads an array of numbers into the elements recorded against `budget`.
    struct Numbers<'b>(Option<&'b ValueBudget>);

    impl<'de> Visitor<'de> for Numbers<'_> {
        type Value = Vec<u32>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an array of numbers")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, elements: A) -> Result<Vec<u32>, A::Error> {
            recorded_elements(elements, self.0)
        }
    }

    fn read(text: &str, budget: Option<&ValueBudget>) -> Vec<u32> {
        let mut reader = serde_json::Deserializer::from_str(text);
        let read = (&mut reader).deserialize_seq(Numbers(budget)).expect("an array of numbers");
        reader.end().expect("nothing follows the array");
        read
    }

    fn budget(limit: usize) -> ValueBudget {
        ValueBudget::new(JsonValueLimit::new(NonZeroUsize::new(limit).expect("a positive limit")))
    }

    #[test]
    fn every_element_is_recorded_without_a_budget() {
        pretty_assertions::assert_eq!(read("[1,2,3,4]", None), vec![1, 2, 3, 4]);
    }

    #[test]
    fn an_array_within_the_limit_is_recorded_whole() {
        pretty_assertions::assert_eq!(read("[1,2,3]", Some(&budget(3))), vec![1, 2, 3]);
    }

    #[test]
    fn an_array_past_the_limit_is_cut_one_element_past_it() {
        pretty_assertions::assert_eq!(read("[1,2,3,4,5,6]", Some(&budget(3))), vec![1, 2, 3, 4]);
    }

    #[test]
    fn the_budget_is_shared_by_the_arrays_read_against_it() {
        let budget = budget(3);
        pretty_assertions::assert_eq!(read("[1,2]", Some(&budget)), vec![1, 2]);
        pretty_assertions::assert_eq!(read("[3,4,5,6]", Some(&budget)), vec![3, 4]);
    }

    #[test]
    fn the_elements_past_the_cut_are_skipped_not_read() {
        pretty_assertions::assert_eq!(read(r#"[1,2,"not a number",{"a":[]}]"#, Some(&budget(1))), vec![1, 2]);
    }
}
