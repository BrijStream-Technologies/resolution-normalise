//! Print the canonical digest of a ruleset file.
//!
//! ```text
//! cargo run --example ruleset_digest -- rulesets/kyvryn-res-1.0.json
//! ```
//!
//! This exists because the obvious check gives the wrong answer. `ruleset_sha256` in a claim
//! record is **not** a hash of the ruleset file's bytes: it is a hash of the ruleset's canonical
//! serialisation, so whitespace, key order and formatting in the file do not affect it. Running
//! `sha256sum` on the file produces a different value, and anyone comparing that against a claim
//! record would reasonably conclude the numbers had been made up.
//!
//! Hashing the canonical form rather than the file is deliberate: two parties who agreed the same
//! rules should arrive at the same digest even if one of them reformatted the document.

use resolution_normalise::Ruleset;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: ruleset_digest <ruleset.json>");
        std::process::exit(2);
    };

    let rules = match Ruleset::from_path(&path) {
        Ok(rules) => rules,
        Err(e) => {
            eprintln!("could not read {path}: {e}");
            std::process::exit(1);
        }
    };
    match rules.digest() {
        Ok(digest) => {
            println!("version {}", rules.version);
            println!("digest  {digest}");
        }
        Err(e) => {
            eprintln!("could not digest {path}: {e}");
            std::process::exit(1);
        }
    }
}
