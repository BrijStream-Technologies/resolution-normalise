//! Derivation: raw helpdesk records in, canonical claim records out.
//!
//! Every flag here is arithmetic over timestamps and state transitions — deliberately the part the
//! Judge does not do, so that the Judge's rules stay expressible as exact equality checks. The
//! ruleset digest and an input digest travel with each record, so a third party holding the same
//! exports can recompute this byte for byte.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Duration, Utc};

use crate::model::{ClaimRecord, Outcome, RequesterClass, Summary};
use crate::ruleset::Ruleset;
use crate::source::{EventKind, SourceBundle, Ticket, TicketEvent, User};
use crate::{sha256_hex, Error, Result};

/// Normalise one period's records into claim records.
///
/// `evaluated_at` is when the attestation is being run: claims whose windows have not yet
/// closed are deferred rather than judged early.
///
/// # Errors
/// Returns [`Error::Ruleset`] if the ruleset is unusable, and [`Error::Parse`] if a source record
/// cannot be canonicalised for hashing.
pub fn normalise(
    bundle: &SourceBundle,
    rules: &Ruleset,
    period: &str,
    evaluated_at: DateTime<Utc>,
) -> Result<Vec<Outcome>> {
    rules.validate()?;
    let ruleset_sha256 = rules.digest()?;

    let tickets: HashMap<&str, &Ticket> = bundle.tickets.iter().map(|t| (t.id.as_str(), t)).collect();
    let users: HashMap<&str, &User> = bundle.users.iter().map(|u| (u.id.as_str(), u)).collect();

    let mut events_by_ticket: HashMap<&str, Vec<&TicketEvent>> = HashMap::new();
    for event in &bundle.events {
        events_by_ticket
            .entry(event.ticket_id.as_str())
            .or_default()
            .push(event);
    }
    for list in events_by_ticket.values_mut() {
        list.sort_by_key(|e| e.at);
    }

    let mut tickets_by_requester: HashMap<&str, Vec<&Ticket>> = HashMap::new();
    for ticket in &bundle.tickets {
        tickets_by_requester
            .entry(ticket.requester_id.as_str())
            .or_default()
            .push(ticket);
    }

    let mut outcomes = Vec::with_capacity(bundle.claims.len());
    for claim in &bundle.claims {
        let Some(ticket) = tickets.get(claim.ticket_id.as_str()) else {
            outcomes.push(Outcome::Unmatched {
                claim_id: claim.claim_id.clone(),
                ticket_id: claim.ticket_id.clone(),
                reason: "no ticket with this id in the buyer's helpdesk export".to_owned(),
                amount_usd_micros: claim.amount_usd_micros,
            });
            continue;
        };

        let adjudicable_at = claim.resolved_at + Duration::days(rules.longest_window_days());
        if adjudicable_at > evaluated_at {
            outcomes.push(Outcome::Deferred {
                claim_id: claim.claim_id.clone(),
                adjudicable_at: adjudicable_at.to_rfc3339(),
            });
            continue;
        }

        let empty = Vec::new();
        let events = events_by_ticket.get(claim.ticket_id.as_str()).unwrap_or(&empty);

        let reopened_within_window = reopened(
            events,
            claim.resolved_at,
            Duration::days(rules.reopen_window_days),
        );
        let reached_human = reached_human(events, &users, rules);
        let no_siblings: Vec<&Ticket> = Vec::new();
        let siblings = tickets_by_requester
            .get(ticket.requester_id.as_str())
            .unwrap_or(&no_siblings);
        let repeat_contact_window = repeat_contact(
            ticket,
            siblings,
            claim.resolved_at,
            Duration::days(rules.repeat_contact_window_days),
            rules,
        );
        let downstream_reversal = bundle.downstream.iter().any(|d| {
            d.requester_id == ticket.requester_id
                && d.at >= claim.resolved_at
                && d.at <= claim.resolved_at + Duration::days(rules.downstream_window_days)
        });
        let requester_class = classify(
            users.get(ticket.requester_id.as_str()).copied(),
            &ticket.requester_id,
            rules,
        );
        let turn_count = u64::try_from(
            events
                .iter()
                .filter(|e| matches!(e.kind, EventKind::PublicComment { .. }))
                .count(),
        )
        .unwrap_or(u64::MAX);
        let scope_tag = ticket
            .tags
            .iter()
            .find(|t| t.starts_with(&rules.intent_tag_prefix))
            .cloned()
            .unwrap_or_default();

        let inputs_sha256 = input_digest(ticket, events, claim, &bundle.downstream)?;

        outcomes.push(Outcome::Claim(Box::new(ClaimRecord {
            claim_id: claim.claim_id.clone(),
            ticket_id: claim.ticket_id.clone(),
            period: period.to_owned(),
            resolution_type: claim.resolution_type.clone(),
            resolved_at: claim.resolved_at.to_rfc3339(),
            reopened_within_window,
            repeat_contact_window,
            reached_human,
            downstream_reversal,
            requester_class: requester_class.as_str().to_owned(),
            turn_count,
            scope_tag,
            amount_usd_micros: claim.amount_usd_micros,
            inputs_sha256,
            ruleset_version: rules.version.clone(),
            ruleset_sha256: ruleset_sha256.clone(),
        })));
    }
    Ok(outcomes)
}

/// A ticket reopened when it left `solved`/`closed` for an open state inside the window.
fn reopened(events: &[&TicketEvent], resolved_at: DateTime<Utc>, window: Duration) -> bool {
    events.iter().any(|e| {
        matches!(&e.kind, EventKind::StatusChanged { to }
            if (to == "open" || to == "pending" || to == "hold"))
            && e.at > resolved_at
            && e.at <= resolved_at + window
    })
}

/// A human took the ticket if staff were assigned, or a staff member posted publicly — at any
/// time, including after the invoice went out. Late escalation is still escalation.
fn reached_human(events: &[&TicketEvent], users: &HashMap<&str, &User>, rules: &Ruleset) -> bool {
    let is_human = |id: &str| -> bool {
        if rules.bot_user_ids.iter().any(|bot| bot == id) {
            return false;
        }
        users.get(id).is_some_and(|u| u.is_staff_role())
    };
    events.iter().any(|e| match &e.kind {
        EventKind::AssigneeChanged { to_user_id } => {
            to_user_id.as_deref().is_some_and(is_human)
        }
        EventKind::PublicComment { author_id } => is_human(author_id),
        EventKind::StatusChanged { .. } => false,
    })
}

/// A customer who gave up and opened a *new* ticket about the same thing.
///
/// Intercom reverses a charge when the customer returns to the same conversation (fin.ai, retrieved
/// 2026-09-20). A new ticket falls outside that rule, so the original stays billed unless someone
/// looks across tickets.
fn repeat_contact(
    original: &Ticket,
    requester_tickets: &[&Ticket],
    resolved_at: DateTime<Utc>,
    window: Duration,
    rules: &Ruleset,
) -> bool {
    let intent = |t: &Ticket| -> Option<String> {
        t.tags
            .iter()
            .find(|tag| tag.starts_with(&rules.intent_tag_prefix))
            .cloned()
    };
    let original_intent = intent(original);
    requester_tickets.iter().any(|t| {
        if t.id == original.id {
            return false;
        }
        if t.created_at <= resolved_at || t.created_at > resolved_at + window {
            return false;
        }
        if rules.require_intent_match {
            match (&original_intent, intent(t)) {
                (Some(a), Some(b)) => a == &b,
                _ => false,
            }
        } else {
            true
        }
    })
}

fn classify(user: Option<&User>, id: &str, rules: &Ruleset) -> RequesterClass {
    if rules.bot_user_ids.iter().any(|bot| bot == id) {
        return RequesterClass::Bot;
    }
    let Some(user) = user else {
        return RequesterClass::Unknown;
    };
    if user.is_staff_role() {
        return RequesterClass::Staff;
    }
    if user.role == "bot" {
        return RequesterClass::Bot;
    }
    let email = user.email.as_deref().unwrap_or("").to_ascii_lowercase();
    if email.is_empty() {
        return RequesterClass::Customer;
    }
    if rules
        .test_account_markers
        .iter()
        .any(|m| email.contains(&m.to_ascii_lowercase()))
    {
        return RequesterClass::Test;
    }
    if rules
        .staff_domains
        .iter()
        .any(|d| email.ends_with(&d.to_ascii_lowercase()))
    {
        return RequesterClass::Staff;
    }
    if email.starts_with("noreply@") || email.starts_with("no-reply@") {
        return RequesterClass::Bot;
    }
    RequesterClass::Customer
}

/// Digest over exactly the records this claim was derived from, in a fixed order.
fn input_digest(
    ticket: &Ticket,
    events: &[&TicketEvent],
    claim: &crate::source::BilledClaim,
    downstream: &[crate::source::DownstreamEvent],
) -> Result<String> {
    let relevant: Vec<&crate::source::DownstreamEvent> = downstream
        .iter()
        .filter(|d| d.requester_id == ticket.requester_id)
        .collect();
    let canonical = serde_json::json!({
        "ticket": ticket,
        "events": events,
        "claim": claim,
        "downstream": relevant,
    });
    let bytes = serde_json::to_vec(&canonical).map_err(|source| Error::Parse {
        what: format!("inputs for claim {}", claim.claim_id),
        source,
    })?;
    Ok(sha256_hex(&bytes))
}

/// Count outcomes for the evidence pack.
#[must_use]
pub fn summarise(outcomes: &[Outcome], rules: &Ruleset) -> Summary {
    let mut summary = Summary::default();
    let scope = regex::Regex::new(&rules.scope_tag_pattern).ok();
    let min = u64::try_from(rules.min_turns).unwrap_or(0);
    let max = u64::try_from(rules.max_turns).unwrap_or(u64::MAX);
    for outcome in outcomes {
        match outcome {
            Outcome::Claim(record) => {
                summary.billed = summary.billed.saturating_add(1);
                summary.billed_usd_micros = summary
                    .billed_usd_micros
                    .saturating_add(record.amount_usd_micros);
                let scope_ok = scope
                    .as_ref()
                    .is_some_and(|re| re.is_match(&record.scope_tag));
                let reasons = failed_reasons(record, min, max, scope_ok);
                if reasons.is_empty() {
                    summary.verified = summary.verified.saturating_add(1);
                } else {
                    summary.exceptions = summary.exceptions.saturating_add(1);
                    summary.at_stake_usd_micros = summary
                        .at_stake_usd_micros
                        .saturating_add(record.amount_usd_micros);
                    for reason in reasons {
                        let entry = summary.by_reason.entry(reason).or_insert(0);
                        *entry = entry.saturating_add(1);
                    }
                }
            }
            Outcome::Deferred { .. } => summary.deferred = summary.deferred.saturating_add(1),
            Outcome::Unmatched {
                amount_usd_micros, ..
            } => {
                summary.billed = summary.billed.saturating_add(1);
                summary.billed_usd_micros = summary.billed_usd_micros.saturating_add(*amount_usd_micros);
                summary.at_stake_usd_micros =
                    summary.at_stake_usd_micros.saturating_add(*amount_usd_micros);
                summary.unmatched = summary.unmatched.saturating_add(1);
                let entry = summary
                    .by_reason
                    .entry("unmatched_ticket".to_owned())
                    .or_insert(0);
                *entry = entry.saturating_add(1);
            }
        }
    }
    summary
}

/// Which criteria a record fails, named the way the dispute file names them.
#[must_use]
pub fn failed_reasons(record: &ClaimRecord, min: u64, max: u64, scope_ok: bool) -> BTreeSet<String> {
    let mut reasons = BTreeSet::new();
    if record.reopened_within_window {
        reasons.insert("reopened_within_window".to_owned());
    }
    if record.repeat_contact_window {
        reasons.insert("repeat_contact_window".to_owned());
    }
    if record.reached_human {
        reasons.insert("reached_human".to_owned());
    }
    if record.downstream_reversal {
        reasons.insert("downstream_reversal".to_owned());
    }
    if record.requester_class != RequesterClass::Customer.as_str() {
        reasons.insert("requester_not_customer".to_owned());
    }
    if record.turn_count < min || record.turn_count > max {
        reasons.insert("turn_count_out_of_range".to_owned());
    }
    if !scope_ok {
        reasons.insert("scope_tag_invalid".to_owned());
    }
    reasons
}

/// Exception counts as a plain map, for callers that want them without the rest of the summary.
#[must_use]
pub fn reason_counts(summary: &Summary) -> BTreeMap<String, u64> {
    summary.by_reason.clone()
}
