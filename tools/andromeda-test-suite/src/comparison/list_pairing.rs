//! The pairing of the items of two lists regardless of order: by `id` when every item has one, by content, and
//! member by member for the few items content leaves unpaired.

use crate::comparison::{
    Expectation,
    Mode,
    difference::{Difference, Discrepancy},
    document_path::DocumentPath,
};
use serde_json::Value;
use std::collections::BTreeSet;

impl Expectation {
    /// Compares two lists regardless of order.
    pub(super) fn compare_lists(&self, path: &DocumentPath, actual: &[Value], expected: &[Value], mode: Mode, out: &mut Vec<Difference>) {
        if actual.len() != expected.len() {
            out.push(Difference::at(
                path,
                Discrepancy::Length {
                    actual: actual.len(),
                    expected: expected.len(),
                },
            ));
            return;
        }
        match (ids(actual), ids(expected), mode) {
            (Some(actual_ids), Some(expected_ids), _) => self.compare_by_id(path, (actual, &actual_ids), (expected, &expected_ids), mode, out),
            (Some(_) | None, Some(_) | None, Mode::Hash) => self.compare_by_hash(path, actual, expected, out),
            (Some(_) | None, Some(_) | None, Mode::Diff) => self.compare_by_pairing(path, actual, expected, out),
        }
    }

    /// Pairs the items of two lists by their `id` and compares each pair member by member.
    fn compare_by_id(
        &self,
        path: &DocumentPath,
        (actual, actual_ids): (&[Value], &[&str]),
        (expected, expected_ids): (&[Value], &[&str]),
        mode: Mode,
        out: &mut Vec<Difference>,
    ) {
        for (expected, id) in expected.iter().zip(expected_ids) {
            let item = path.item_with_id(id);
            match actual_ids.iter().position(|actual_id| actual_id == id) {
                Some(index) => self.compare(&item, None, &actual[index], expected, mode, out),
                None => out.push(Difference::at(&item, Discrepancy::MissingItem)),
            }
        }
    }

    /// Pairs the items of two lists inside a list item: every expected item with a distinct item equal by content.
    fn compare_by_hash(&self, path: &DocumentPath, actual: &[Value], expected: &[Value], out: &mut Vec<Difference>) {
        let item = path.any_item();
        let partner = self.pairing(&item, &actual.iter().collect::<Vec<_>>(), &expected.iter().collect::<Vec<_>>(), Mode::Hash);
        out.extend(
            expected
                .iter()
                .zip(&partner)
                .filter(|(_, paired)| paired.is_none())
                .map(|(value, _)| Difference::at(&item, Discrepancy::Unpaired((*value).clone()))), // clone: the difference owns the values it reports
        );
    }

    /// Pairs the items of two lists: items equal by content first; then, while few items are left (at most 0.7 of
    /// both lists), each left expected item with a distinct left actual item it agrees with member by member. More
    /// left items than that pair with nothing: pairing many items member by member costs quadratic work and matches
    /// unrelated items, so such a list is reported as differing, with what its closest items differ in.
    fn compare_by_pairing(&self, path: &DocumentPath, actual: &[Value], expected: &[Value], out: &mut Vec<Difference>) {
        let item = path.any_item();
        let hashed = self.pairing(&item, &actual.iter().collect::<Vec<_>>(), &expected.iter().collect::<Vec<_>>(), Mode::Hash);
        let left_expected = hashed
            .iter()
            .enumerate()
            .filter(|(_, paired)| paired.is_none())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let taken = hashed.iter().flatten().copied().collect::<BTreeSet<_>>();
        let left_actual = (0..actual.len()).filter(|index| !taken.contains(index)).collect::<Vec<_>>();
        if left_expected.is_empty() {
            return;
        }
        if 20 * left_expected.len() > 7 * (2 * expected.len() + 1) {
            out.push(Difference::at(path, Discrepancy::BeyondPairingCutoff(left_expected.len())));
            out.extend(
                left_expected
                    .iter()
                    .flat_map(|index| self.closest_differences(&item, &left_actual, actual, &expected[*index])),
            );
            return;
        }
        let actual_left = left_actual.iter().map(|index| &actual[*index]).collect::<Vec<_>>();
        let expected_left = left_expected.iter().map(|index| &expected[*index]).collect::<Vec<_>>();
        let diffed = self.pairing(&item, &actual_left, &expected_left, Mode::Diff);
        out.extend(
            expected_left
                .iter()
                .zip(&diffed)
                .filter(|(_, paired)| paired.is_none())
                .map(|(value, _)| Difference::at(&item, Discrepancy::Unpaired((*value).clone()))), // clone: the difference owns the values it reports
        );
    }

    /// The member differences between `expected` and the item among `candidates` (indexes into `actual`) it differs
    /// from least, to show what a list too different to pair differs in.
    fn closest_differences(&self, item: &DocumentPath, candidates: &[usize], actual: &[Value], expected: &Value) -> Vec<Difference> {
        candidates
            .iter()
            .map(|index| {
                let mut differences = Vec::new();
                self.compare(item, None, &actual[*index], expected, Mode::Diff, &mut differences);
                differences
            })
            .min_by_key(Vec::len)
            .unwrap_or_default()
    }

    /// The actual item paired with each expected item, under `mode`, along augmenting paths (Kuhn's algorithm).
    fn pairing(&self, item: &DocumentPath, actual: &[&Value], expected: &[&Value], mode: Mode) -> Vec<Option<usize>> {
        let agrees = expected
            .iter()
            .map(|expected| actual.iter().map(|actual| self.agrees(item, actual, expected, mode)).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mut partner = vec![None; actual.len()];
        for index in 0..expected.len() {
            let mut visited = vec![false; actual.len()];
            augment(index, &agrees, &mut partner, &mut visited);
        }
        let mut paired = vec![None; expected.len()];
        for (actual_index, expected_index) in partner.iter().enumerate() {
            if let Some(expected_index) = expected_index {
                paired[*expected_index] = Some(actual_index);
            }
        }
        paired
    }

    /// Whether `actual` agrees with `expected` under `mode`, as an item of a list at `item`.
    fn agrees(&self, item: &DocumentPath, actual: &Value, expected: &Value, mode: Mode) -> bool {
        let mut differences = Vec::new();
        self.compare(item, None, actual, expected, mode, &mut differences);
        differences.is_empty()
    }
}

/// The `id` of every item, when each is an object with a string `id`.
fn ids(items: &[Value]) -> Option<Vec<&str>> {
    items.iter().map(|item| item.get("id").and_then(Value::as_str)).collect()
}

/// Finds a partner for expected item `item` along an augmenting path (Kuhn's algorithm).
fn augment(item: usize, agrees: &[Vec<bool>], partner: &mut [Option<usize>], visited: &mut [bool]) -> bool {
    for candidate in 0..partner.len() {
        if agrees[item][candidate] && !visited[candidate] {
            visited[candidate] = true;
            if partner[candidate].is_none_or(|other| augment(other, agrees, partner, visited)) {
                partner[candidate] = Some(item);
                return true;
            }
        }
    }
    false
}
