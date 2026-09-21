//! Write the standard ruleset out, so the published file and `Ruleset::default()` cannot drift.
use resolution_normalise::Ruleset;
fn main() {
    let rules = Ruleset::default();
    let json = serde_json::to_string_pretty(&rules).expect("serialises");
    println!("{json}");
}
