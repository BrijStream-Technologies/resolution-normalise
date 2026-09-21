//! The rules a run is evaluated under. Fixed before the run, versioned, and hashed into every claim
//! record so a verdict can never be re-argued against different rules than the ones applied. Whether
//! both parties agreed them is a matter for the engagement; nothing in this type can know it.

use serde::{Deserialize, Serialize};

use crate::{sha256_hex, Error, Result};

/// Windows and classifications fixed before evaluation and, where an engagement exists, agreed by
/// both parties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ruleset {
    /// Human-readable version, carried in every claim record.
    pub version: String,
    /// A resolution is void if the ticket reopened within this many days of being solved.
    pub reopen_window_days: i64,
    /// A resolution is void if the same requester opened another ticket within this many days.
    pub repeat_contact_window_days: i64,
    /// A resolution is void if a refund, cancellation, chargeback or return followed within this
    /// many days.
    pub downstream_window_days: i64,
    /// When true, a later ticket only counts as repeat contact if it carries the same intent tag.
    /// When false, any later ticket from that requester counts.
    pub require_intent_match: bool,
    /// Prefix that marks the vendor's intent tags among all ticket tags.
    pub intent_tag_prefix: String,
    /// A claim is in contracted scope only if its intent tag matches this pattern.
    pub scope_tag_pattern: String,
    /// Email domains belonging to the buyer's own staff.
    pub staff_domains: Vec<String>,
    /// Substrings marking test or robot accounts.
    pub test_account_markers: Vec<String>,
    /// User ids belonging to the AI agent itself.
    ///
    /// Whether a reply was the agent or a human decides whether a resolution escalated, so it is
    /// an agreed rule, not a connector detail. Salesforce logs Agentforce actions under a service
    /// user; Zendesk and Intercom mark the author's type, and these ids override that when set.
    #[serde(default)]
    pub bot_user_ids: Vec<String>,
    /// Fewest public turns a billable conversation may have.
    pub min_turns: usize,
    /// Most public turns a billable conversation may have.
    pub max_turns: usize,
}

impl Default for Ruleset {
    fn default() -> Self {
        Self {
            version: "kyvryn-res-1.0".to_owned(),
            reopen_window_days: 7,
            repeat_contact_window_days: 7,
            downstream_window_days: 14,
            require_intent_match: true,
            intent_tag_prefix: "intent_".to_owned(),
            scope_tag_pattern: "^intent_[a-z0-9_]+$".to_owned(),
            staff_domains: Vec::new(),
            test_account_markers: vec!["+test@".to_owned(), "qa-".to_owned()],
            bot_user_ids: Vec::new(),
            min_turns: 2,
            max_turns: 200,
        }
    }
}

impl Ruleset {
    /// Load a ruleset from a JSON file.
    ///
    /// # Errors
    /// Returns [`Error::Io`] if the file cannot be read and [`Error::Parse`] if it is not a
    /// ruleset document.
    pub fn from_path(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        let parsed: Self = serde_json::from_str(&text).map_err(|source| Error::Parse {
            what: format!("ruleset {path}"),
            source,
        })?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Reject a ruleset that cannot be applied, rather than applying half of it.
    ///
    /// # Errors
    /// Returns [`Error::Ruleset`] for a negative window, an impossible turn range, or a scope
    /// pattern that is not a valid regular expression.
    pub fn validate(&self) -> Result<()> {
        if self.reopen_window_days < 0
            || self.repeat_contact_window_days < 0
            || self.downstream_window_days < 0
        {
            return Err(Error::Ruleset("windows cannot be negative".to_owned()));
        }
        if self.min_turns > self.max_turns {
            return Err(Error::Ruleset(
                "min_turns cannot exceed max_turns".to_owned(),
            ));
        }
        regex::Regex::new(&self.scope_tag_pattern)
            .map_err(|e| Error::Ruleset(format!("scope_tag_pattern is not a regex: {e}")))?;
        Ok(())
    }

    /// The digest of this ruleset, over its canonical JSON form.
    ///
    /// Travels with every claim record: a verdict that does not name the rules it applied is not
    /// evidence.
    ///
    /// # Errors
    /// Returns [`Error::Parse`] only if the ruleset cannot be serialised, which cannot happen for
    /// a value of this type.
    pub fn digest(&self) -> Result<String> {
        let canonical = serde_json::to_vec(self).map_err(|source| Error::Parse {
            what: "ruleset".to_owned(),
            source,
        })?;
        Ok(sha256_hex(&canonical))
    }

    /// The digest the published manifest records for a standard ruleset version, if any.
    ///
    /// The manifest ships inside the crate so this check needs no network and works for a vendor
    /// holding nothing but the pack. Without it, a ruleset edited for one engagement can keep a
    /// standard version string while carrying a different digest — which is indistinguishable, to
    /// the party being measured, from rules quietly changed after the fact.
    #[must_use]
    pub fn standard_digest_for(version: &str) -> Option<String> {
        #[derive(serde::Deserialize)]
        struct Entry {
            version: String,
            digest: String,
        }
        #[derive(serde::Deserialize)]
        struct Manifest {
            rulesets: Vec<Entry>,
        }
        let manifest: Manifest =
            serde_json::from_str(include_str!("../rulesets/manifest.json")).ok()?;
        manifest
            .rulesets
            .into_iter()
            .find(|e| e.version == version)
            .map(|e| e.digest)
    }

    /// Whether this ruleset's version claims to be a standard one while its content is not.
    ///
    /// `Some(expected_digest)` means the version string is published and the content does not
    /// match it. That must be surfaced before a pack goes out, not discovered by the vendor.
    ///
    /// # Errors
    /// Returns [`Error::Parse`] if this ruleset cannot be serialised for hashing.
    pub fn version_digest_conflict(&self) -> Result<Option<String>> {
        let Some(expected) = Self::standard_digest_for(&self.version) else {
            return Ok(None);
        };
        if expected == self.digest()? {
            return Ok(None);
        }
        Ok(Some(expected))
    }

    /// The longest window in the ruleset, in days. A claim cannot be adjudicated until every one
    /// of its windows has closed.
    #[must_use]
    pub fn longest_window_days(&self) -> i64 {
        self.reopen_window_days
            .max(self.repeat_contact_window_days)
            .max(self.downstream_window_days)
    }
}
