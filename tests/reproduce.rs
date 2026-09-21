//! What a vendor does when it disputes a claim made against it.
//!
//! These tests take the same two files a buyer would hand over — a source bundle and a ruleset —
//! and recompute the derivation from them. No connector, no credentials, no network. If this
//! crate could not reproduce results from those two inputs alone, the product's central claim
//! would be false, so this is the suite that matters most to the party being measured.

use chrono::{DateTime, Utc};
use resolution_normalise::{
    model::Outcome,
    normalise::{normalise, summarise},
    ruleset::Ruleset,
    SourceBundle,
};

/// Fixed, because a derivation that depended on the wall clock could not be reproduced tomorrow.
fn evaluated_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-16T00:00:00Z")
        .expect("fixed timestamp parses")
        .with_timezone(&Utc)
}

fn bundle() -> SourceBundle {
    SourceBundle::from_path("fixtures/bundle.json").expect("bundle loads")
}

fn rules() -> Ruleset {
    Ruleset::from_path("fixtures/ruleset.json").expect("ruleset loads")
}

fn outcomes() -> Vec<Outcome> {
    normalise(&bundle(), &rules(), "2026-08", evaluated_at()).expect("normalise runs")
}

#[test]
fn a_bundle_and_a_ruleset_are_enough_to_reproduce_a_run() {
    let outcomes = outcomes();
    assert!(
        !outcomes.is_empty(),
        "the published crate must derive claim records from exported files alone"
    );
    assert!(
        outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Claim(_))),
        "at least one claim must be judgeable, or the fixture proves nothing"
    );
}

#[test]
fn the_same_inputs_give_byte_identical_records() {
    // The whole dispute rests on this. If two runs over the same inputs disagreed, neither party
    // could rely on a digest, and every claim would come down to whose run to believe.
    let first = serde_json::to_string(&outcomes()).expect("serialises");
    let second = serde_json::to_string(&outcomes()).expect("serialises");
    assert_eq!(first, second);
}

#[test]
fn the_input_digest_is_stable_across_runs() {
    let digests = |set: &[Outcome]| -> Vec<String> {
        set.iter()
            .filter_map(|o| match o {
                Outcome::Claim(record) => Some(record.inputs_sha256.clone()),
                _ => None,
            })
            .collect()
    };
    let first = digests(&outcomes());
    let second = digests(&outcomes());

    assert!(!first.is_empty(), "no digests to compare");
    assert_eq!(first, second, "inputs_sha256 must not move between runs");
    assert!(
        first.iter().all(|d| d.len() == 64),
        "each digest is a hex SHA-256"
    );
}

#[test]
fn a_changed_ruleset_changes_the_ruleset_digest() {
    // Periods judged under different rules are not comparable, so the digest has to move when the
    // rules do. A vendor checks this before accepting that two periods show a trend.
    let mut altered = rules();
    altered.reopen_window_days += 1;

    let first_digest = |set: &[Outcome]| -> Option<String> {
        set.iter().find_map(|o| match o {
            Outcome::Claim(record) => Some(record.ruleset_sha256.clone()),
            _ => None,
        })
    };

    let original = normalise(&bundle(), &rules(), "2026-08", evaluated_at()).expect("runs");
    let changed = normalise(&bundle(), &altered, "2026-08", evaluated_at()).expect("runs");

    let (before, after) = (first_digest(&original), first_digest(&changed));
    assert!(before.is_some() && after.is_some());
    assert_ne!(before, after);
}

#[test]
fn the_summary_counts_what_the_records_say() {
    // A vendor reads the summary first and the records second; they have to agree.
    let outcomes = outcomes();
    let summary = summarise(&outcomes, &rules());

    let claims = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Claim(_)))
        .count() as u64;
    let deferred = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Deferred { .. }))
        .count() as u64;
    let unmatched = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Unmatched { .. }))
        .count() as u64;

    assert_eq!(summary.billed, outcomes.len() as u64);
    assert_eq!(summary.verified + summary.exceptions, claims);
    assert_eq!(summary.deferred, deferred);
    assert_eq!(summary.unmatched, unmatched);
}

#[test]
fn a_bundle_survives_a_round_trip_through_a_file() {
    // The bundle is how the inputs travel between the two parties. If writing and reading it back
    // changed anything, the digests either side computes would diverge for no reason.
    let dir = std::env::temp_dir().join("resolution-normalise-roundtrip");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("bundle.json");
    let path = path.to_str().expect("utf-8 path");

    bundle().to_path(path).expect("writes");
    let reloaded = SourceBundle::from_path(path).expect("reads back");

    let before = normalise(&bundle(), &rules(), "2026-08", evaluated_at()).expect("runs");
    let after = normalise(&reloaded, &rules(), "2026-08", evaluated_at()).expect("runs");
    assert_eq!(
        serde_json::to_string(&before).expect("serialises"),
        serde_json::to_string(&after).expect("serialises")
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn the_billed_total_is_every_claim_the_vendor_billed() {
    // An unmatched claim -- billed for a ticket the buyer has no record of -- used to count toward
    // the number billed but not the dollars billed, and never toward the amount at stake. The
    // strongest exception there is dropped out of both totals.
    let bundle = bundle();
    let outcomes = outcomes();
    let summary = summarise(&outcomes, &rules());

    let deferred = outcomes
        .iter()
        .filter(|o| matches!(o, Outcome::Deferred { .. }))
        .count();
    assert_eq!(deferred, 0, "the invariant below holds for a fully closed period");

    let invoiced: u64 = bundle.claims.iter().map(|c| c.amount_usd_micros).sum();
    assert_eq!(
        summary.billed_usd_micros, invoiced,
        "the billed total must equal the sum of every claim the vendor billed"
    );

    let unmatched_amount: u64 = outcomes
        .iter()
        .filter_map(|o| match o {
            Outcome::Unmatched {
                amount_usd_micros, ..
            } => Some(*amount_usd_micros),
            _ => None,
        })
        .sum();
    assert!(unmatched_amount > 0, "the fixture carries an unmatched claim");
    assert!(
        summary.at_stake_usd_micros >= unmatched_amount,
        "an unmatched claim's amount is at stake"
    );
}
