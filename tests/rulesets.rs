//! The published rulesets, and the manifest that indexes them.
//!
//! A vendor holding a claim record reads its `ruleset_sha256` and looks it up here to find which
//! rules were applied. That only works if the manifest's digests match the files it names, so
//! these tests are what keep the two from drifting apart — a manifest that disagrees with its own
//! files is worse than no manifest, because it looks authoritative.

use std::collections::BTreeMap;

use resolution_normalise::Ruleset;
use serde_json::Value;

fn manifest() -> Value {
    let text = std::fs::read_to_string("rulesets/manifest.json").expect("manifest is readable");
    serde_json::from_str(&text).expect("manifest is JSON")
}

fn entries() -> Vec<Value> {
    manifest()["rulesets"]
        .as_array()
        .expect("rulesets is an array")
        .clone()
}

#[test]
fn every_published_ruleset_matches_its_recorded_digest() {
    let entries = entries();
    assert!(!entries.is_empty(), "nothing is published");

    for entry in entries {
        let file = entry["file"].as_str().expect("file");
        let claimed = entry["digest"].as_str().expect("digest");
        let rules = Ruleset::from_path(&format!("rulesets/{file}")).expect("ruleset loads");
        let actual = rules.digest().expect("digests");

        assert_eq!(
            actual, claimed,
            "{file}: manifest says {claimed}, the file digests to {actual}. \
             A vendor looking up a claim's ruleset_sha256 here would be misled."
        );
        assert_eq!(
            rules.version,
            entry["version"].as_str().expect("version"),
            "{file}: the manifest's version must be the one inside the file"
        );
    }
}

#[test]
fn every_published_ruleset_is_usable() {
    // A published ruleset that fails validation would be a document nobody could actually judge
    // under, which makes it worse than not publishing it.
    for entry in entries() {
        let file = entry["file"].as_str().expect("file");
        let rules = Ruleset::from_path(&format!("rulesets/{file}")).expect("ruleset loads");
        rules.validate().unwrap_or_else(|e| panic!("{file} is not a usable ruleset: {e}"));
    }
}

#[test]
fn versions_and_digests_are_unique() {
    // Two entries sharing a version make "which rules applied" unanswerable, which is the one
    // question the manifest exists to answer.
    let mut by_version: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_digest: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries() {
        *by_version
            .entry(entry["version"].as_str().expect("version").to_owned())
            .or_default() += 1;
        *by_digest
            .entry(entry["digest"].as_str().expect("digest").to_owned())
            .or_default() += 1;
    }
    assert!(
        by_version.values().all(|n| *n == 1),
        "duplicate versions: {by_version:?}"
    );
    assert!(
        by_digest.values().all(|n| *n == 1),
        "duplicate digests: {by_digest:?}"
    );
}

#[test]
fn the_standard_ruleset_is_the_one_the_code_defaults_to() {
    // If the published file and Ruleset::default() diverge, a run with no --ruleset would be
    // judged under rules that appear nowhere in the manifest.
    let published =
        Ruleset::from_path("rulesets/kyvryn-res-1.0.json").expect("standard ruleset loads");
    assert_eq!(
        published.digest().expect("digests"),
        Ruleset::default().digest().expect("digests"),
        "the published standard ruleset has drifted from the built-in default"
    );
}

#[test]
fn the_manifest_warns_that_a_file_hash_is_not_the_digest() {
    // sha256sum on the file gives a different value from ruleset_sha256, because the digest is
    // over the canonical serialisation. Someone checking the obvious way and finding a mismatch
    // would reasonably conclude the numbers were invented, so the manifest has to say so.
    let note = manifest()["note"].as_str().expect("note").to_lowercase();
    assert!(note.contains("not of the file"), "got: {note}");

    let path = "rulesets/kyvryn-res-1.0.json";
    let bytes = std::fs::read(path).expect("readable");
    let file_hash = resolution_normalise::sha256_hex(&bytes);
    let canonical = Ruleset::from_path(path)
        .expect("loads")
        .digest()
        .expect("digests");
    assert_ne!(
        file_hash, canonical,
        "if these ever agree, the warning in the manifest should be revisited"
    );
}

/// A ruleset edited for one engagement must not keep a standard version string. To the party being
/// measured, a published version name carrying an unpublished digest is indistinguishable from
/// rules changed after the fact — and the manifest lookup we tell them to run is what surfaces it.
#[test]
fn an_edited_ruleset_may_not_keep_a_standard_version_string() {
    let mut edited = Ruleset::from_path("rulesets/kyvryn-res-1.0.json").expect("loads");
    assert!(
        edited.version_digest_conflict().expect("digests").is_none(),
        "the unmodified standard ruleset must not conflict with itself"
    );

    edited.staff_domains.push("acme-support.example".to_owned());
    let conflict = edited
        .version_digest_conflict()
        .expect("digests")
        .expect("an edited ruleset under a standard version must be caught");
    assert_eq!(
        conflict,
        Ruleset::standard_digest_for("kyvryn-res-1.0").expect("published"),
        "the conflict reports the digest the manifest actually publishes"
    );

    // Renaming resolves it: the version string is what makes the claim, so changing it withdraws
    // the claim.
    edited.version = "kyvryn-res-1.0-acme-2026-08".to_owned();
    assert!(edited.version_digest_conflict().expect("digests").is_none());
}
