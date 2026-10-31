//! Every european letter folds alike in both engines part 3.

use crate::triggering::letter_fold_corpus::notifies_each_fold_set;
use googletest::prelude::*;

/// Pins: For each of the 320 letters U+0402 to U+1E11 of the Latin, Greek and Cyrillic blocks and the Ohm, Kelvin
/// and Angstrom signs, a Subscription with q name~="(?i)^<letter>$" and the `entityCreated` trigger notifies
/// exactly the letters, of all 1201 created as subjects, that a query carrying the same q selects: the letter's
/// case fold set
/// Clause: 4.9, 5.8.6, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_ntf_trg_0017_every_european_letter_folds_alike_in_both_engines_part_3() -> Result<()> {
    notifies_each_fold_set(640..960).await
}
