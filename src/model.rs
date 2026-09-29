//! The canonical claim record: the only document the Judge ever sees.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Who was actually on the other end of the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequesterClass {
    /// A real customer. The only class a resolution may be billed for.
    Customer,
    /// The buyer's own staff.
    Staff,
    /// A test or QA account.
    Test,
    /// An automated sender.
    Bot,
    /// The requester was not in the supplied directory, so the claim cannot be classified.
    Unknown,
}

impl RequesterClass {
    /// The tag used in the claim record and in criteria.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Customer => "customer",
            Self::Staff => "staff",
            Self::Test => "test",
            Self::Bot => "bot",
            Self::Unknown => "unknown",
        }
    }
}

/// One billed resolution, recomputed from the buyer's records.
///
/// Field order is the serialisation order and must not be reordered: the record is hashed and
/// signed, and a reordering would change bytes that a verdict already covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    /// The vendor's identifier for the billed resolution.
    pub claim_id: String,
    /// The ticket behind it.
    pub ticket_id: String,
    /// Billing period, `YYYY-MM`.
    pub period: String,
    /// `confirmed` or `assumed`, as the vendor classified it. Other vendors use other categories.
    pub resolution_type: String,
    /// When the vendor says it was resolved, RFC 3339.
    pub resolved_at: String,
    /// True when the ticket returned to an open state inside the ruleset's window.
    pub reopened_within_window: bool,
    /// True when the same requester opened another ticket inside the ruleset's window. Intercom
    /// reverses a charge when the customer returns to the same conversation; a new ticket falls
    /// outside that rule, so the original stays billed.
    pub repeat_contact_window: bool,
    /// True when a human agent ever took the ticket, including after the invoice was issued.
    pub reached_human: bool,
    /// True when a refund, cancellation, chargeback or return followed inside the ruleset's window.
    ///
    /// Tested only against the downstream export supplied for the period. False therefore means no
    /// matching event was *in that export*, which is not a finding that none occurred -- and where
    /// no downstream export was supplied at all, this can never be true. Nothing in a claim record
    /// distinguishes the two, so a document reporting this field has to say so.
    pub downstream_reversal: bool,
    /// Who was asking.
    pub requester_class: String,
    /// Public turns in the conversation.
    pub turn_count: u64,
    /// The vendor's own intent tag, or the empty string when it set none.
    pub scope_tag: String,
    /// Amount billed, micros of USD.
    pub amount_usd_micros: u64,
    /// Digest over every source record this claim was derived from.
    pub inputs_sha256: String,
    /// Version of the ruleset applied.
    pub ruleset_version: String,
    /// Digest of the ruleset applied.
    pub ruleset_sha256: String,
}

impl ClaimRecord {
    /// True when no criterion in the ruleset is violated.
    ///
    /// Advisory only, for local reporting. The Judge's signed verdict is the authority, and it
    /// evaluates the criteria independently of this method.
    #[must_use]
    pub fn passes_locally(&self, min_turns: u64, max_turns: u64, scope_ok: bool) -> bool {
        !self.reopened_within_window
            && !self.repeat_contact_window
            && !self.reached_human
            && !self.downstream_reversal
            && self.requester_class == RequesterClass::Customer.as_str()
            && self.turn_count >= min_turns
            && self.turn_count <= max_turns
            && scope_ok
    }
}

/// What normalisation concluded about one billed claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// A claim record ready for adjudication.
    Claim(Box<ClaimRecord>),
    /// The windows have not closed yet, so the claim cannot be evaluated without pre-judging it.
    Deferred {
        /// The vendor's claim id.
        claim_id: String,
        /// The earliest time this claim can be adjudicated, RFC 3339.
        adjudicable_at: String,
        /// What the vendor billed for it. It counts toward neither the billed total nor the amount
        /// at stake, because nothing evaluated it -- but a reader needs it to see how much of the
        /// invoice this period's figures leave out. Defaults to zero when reading records written
        /// before this field existed.
        #[serde(default)]
        amount_usd_micros: u64,
    },
    /// The billed ticket id is not in the supplied helpdesk export. That is a strong exception if
    /// the export is complete for the period -- the vendor billed for a ticket the buyer has no
    /// record of -- but an export cut to the wrong dates produces the same result, so check
    /// coverage before disputing one.
    Unmatched {
        /// The vendor's claim id.
        claim_id: String,
        /// The ticket the vendor named.
        ticket_id: String,
        /// Why it could not be matched.
        reason: String,
        /// What the vendor billed for it. It counts toward both the billed total and the amount at
        /// stake, since the vendor did bill it. Defaults to zero when reading records written before this field existed.
        #[serde(default)]
        amount_usd_micros: u64,
    },
}

/// Counts for the period, for the evidence pack's first page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    /// Claims in scope: what the vendor billed for the period, less any claim whose windows had not
    /// closed when the claims were derived.
    ///
    /// Includes unmatched claims, which are in scope but were evaluated against nothing -- subtract
    /// `unmatched` for the count something actually tested.
    ///
    /// Add `deferred` for the vendor's own count. Under deferral this is a subset of the invoice,
    /// and every rate and total in the same document is over it; `EvidencePack::deferred_note` says
    /// so in words, in every format.
    pub billed: u64,
    /// Claims that failed no criterion locally.
    ///
    /// Serialised as `unflagged`: these claims were checked locally against the ruleset and did
    /// not trip a rule, but were not adjudicated. Calling them "verified" in a machine-readable file that also carries
    /// `judge_key_hex: null` invites exactly the reading it should not.
    #[serde(rename = "unflagged")]
    pub verified: u64,
    /// Claims that failed at least one criterion.
    pub exceptions: u64,
    /// Claims whose windows have not closed, and so nothing evaluated them.
    pub deferred: u64,
    /// What the vendor billed for those deferred claims, micros of USD.
    ///
    /// Carried so a consumer of the JSON can reach the invoice total -- `billed_usd_micros` plus
    /// this -- without parsing the prose that states it.
    #[serde(default)]
    pub deferred_usd_micros: u64,
    /// Claims with no matching ticket.
    pub unmatched: u64,
    /// Amount billed for the claims in scope here, micros of USD. Add `deferred_usd_micros` for the
    /// period's invoice.
    pub billed_usd_micros: u64,
    /// Amount attached to exceptions and unmatched claims, micros of USD.
    pub at_stake_usd_micros: u64,
    /// Exception counts by criterion.
    pub by_reason: std::collections::BTreeMap<String, u64>,
}

/// Format micros of USD as a plain decimal string, without floating point.
///
/// Rounded half-up, not truncated, for the reason the rate helpers beside it give: a file whose
/// purpose is to be added up by the other side should not have its rows sum to less than its own
/// total. Truncating drops up to a cent per row, and a dispute is lost more easily on a figure that
/// does not foot than on the figure itself.
#[must_use]
pub fn usd(micros: u64) -> String {
    // Half-up on the sub-cent remainder, in integers: no float ever touches an invoice.
    let cents_total = micros.saturating_add(5_000) / 10_000;
    let dollars = cents_total / 100;
    let cents = cents_total % 100;
    format!("{dollars}.{cents:02}")
}

/// Parse an RFC 3339 timestamp, returning `None` rather than panicking.
#[must_use]
pub fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod usd_tests {
    use super::usd;

    /// A file meant to be added up should foot.
    ///
    /// Truncation drops up to a cent per row, so a total computed in micros and printed once can
    /// come out above the sum of the rows printed beside it — in the one document whose purpose is
    /// for the other side to check the arithmetic.
    #[test]
    fn rows_sum_to_their_total() {
        // Three claims a vendor priced in thirds of a cent.
        let rows = [3_333_333_u64, 3_333_333, 3_333_334];
        let total: u64 = rows.iter().sum();
        assert_eq!(usd(total), "10.00");
        for row in rows {
            assert_eq!(usd(row), "3.33");
        }
    }

    #[test]
    fn the_half_cent_goes_up() {
        assert_eq!(usd(1_005_000), "1.01");
        assert_eq!(usd(1_004_999), "1.00");
        assert_eq!(usd(990_000), "0.99");
        assert_eq!(usd(0), "0.00");
    }
}
