//! Derivation of billed-resolution claims from vendor-neutral helpdesk records.
//!
//! This crate is published so that the party being measured can check the measurement. A vendor
//! billing per resolution can take the buyer's source bundle and ruleset, run this code, and
//! recompute every claim record byte for byte — including `inputs_sha256` and `ruleset_sha256`.
//! If the digests do not reproduce from the bundle, ruleset and `evaluated_at` supplied, the claim
//! has not been substantiated as stated. Non-reproduction is not by itself proof of bad faith: a
//! different `evaluated_at` changes deferral decisions, and an engagement-specific ruleset will not
//! be in the published manifest.
//!
//! That is the whole trust model: **falsifiability, not vouching**. A signed verdict proves that
//! given inputs under given rules produced a decision. It proves nothing about whether the inputs
//! were faithful. Re-running this code against the same bundle establishes that the records follow
//! from it; comparing the bundle against the vendor's own records is what tests whether the inputs
//! were faithful. Neither step requires trusting the buyer or Kyvryn.
//!
//! # What is here, and what is not
//!
//! Here: the vendor-neutral [`source`] records, the [`ruleset`], the [`normalise`] derivation, the
//! [`criteria`] it produces for the Judge, and the [`model`] those share.
//!
//! Not here: connectors that fetch records from Zendesk, Intercom or Salesforce, and the tooling
//! that submits claims and builds evidence packs. Reproducing a result needs none of them — a
//! bundle is a JSON file ([`source::SourceBundle::from_path`]), and the buyer exports it. The
//! connectors only decide how that file gets filled.
//!
//! # Deliberate constraints
//!
//! Every flag here is arithmetic over timestamps and state transitions, because the Judge has no
//! numeric comparison — its criteria are exact equality, regular expressions, lengths and hashes.
//! The derivation does the counting; the Judge does the checking. That division is what keeps a
//! verdict re-checkable by anyone holding the public key.
//!
//! Nothing here assesses answer quality. Substituting our model's judgement for the vendor's would
//! be the same error the product exists to point at.

pub mod criteria;
pub mod model;
pub mod normalise;
pub mod ruleset;
pub mod source;

pub use model::{ClaimRecord, Outcome, RequesterClass};
pub use ruleset::Ruleset;
pub use source::SourceBundle;

/// Errors surfaced by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A required configuration value was absent. Configuration is fail-closed: there are no
    /// defaults for credentials or endpoints.
    #[error("missing required configuration: {0}")]
    MissingConfig(String),
    /// A file could not be read or written.
    #[error("io error on {path}: {source}")]
    Io {
        /// The path being read or written.
        path: String,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A document did not parse.
    #[error("could not parse {what}: {source}")]
    Parse {
        /// What was being parsed.
        what: String,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// An upstream HTTP call failed.
    #[error("{service} request failed: {detail}")]
    Http {
        /// Which service.
        service: String,
        /// What went wrong.
        detail: String,
    },
    /// A signing or key error.
    #[error("key error: {0}")]
    Key(String),
    /// A ruleset was not usable.
    #[error("invalid ruleset: {0}")]
    Ruleset(String),
    /// Inputs that are individually valid but cannot be used together.
    #[error("inputs do not fit together: {0}")]
    Conflict(String),
}

/// This crate's version, so a document produced with it can name the derivation that made it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// SHA-256 as lowercase hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
