//! Acceptance criteria, in the Judge's own wire form.
//!
//! The Judge evaluates five criterion types: whole-artifact hash, field hash, pattern match,
//! length bounds and exact equality. Everything below is exact equality over flags the normaliser
//! derived, plus a bound on turn count and a pattern on the scope tag — so a verdict never rests
//! on a judgment call.

use serde_json::{json, Value};

use crate::model::{ClaimRecord, RequesterClass};
use crate::ruleset::Ruleset;

/// The criteria a claim record must satisfy to be payable.
///
/// Fixed before evaluation and reproduced inside the attestation the Judge returns, so either party
/// can read exactly what was checked.
#[must_use]
pub fn for_claim(record: &ClaimRecord, rules: &Ruleset) -> Vec<Value> {
    vec![
        json!({"field_equals": {"pointer": "/reopened_within_window", "value": false}}),
        json!({"field_equals": {"pointer": "/repeat_contact_window", "value": false}}),
        json!({"field_equals": {"pointer": "/reached_human", "value": false}}),
        json!({"field_equals": {"pointer": "/downstream_reversal", "value": false}}),
        json!({"field_equals": {
            "pointer": "/requester_class",
            "value": RequesterClass::Customer.as_str()
        }}),
        json!({"field_matches": {"pointer": "/scope_tag", "pattern": rules.scope_tag_pattern}}),
        // The turn count is written as a string field as well, so the Judge's length bound can be
        // applied to the conversation size without a numeric comparison it does not implement.
        json!({"field_length_between": {
            "pointer": "/turn_marks",
            "min": rules.min_turns,
            "max": rules.max_turns
        }}),
        // Pins the derivation: a verdict is only about these inputs and these rules.
        json!({"field_equals": {"pointer": "/inputs_sha256", "value": record.inputs_sha256}}),
        json!({"field_equals": {"pointer": "/ruleset_sha256", "value": record.ruleset_sha256}}),
    ]
}

/// The structural schema the artifact must satisfy before any criterion is applied.
#[must_use]
pub fn schema() -> Value {
    json!({
        "required": [
            {"pointer": "/claim_id", "kind": "string"},
            {"pointer": "/period", "kind": "string"},
            {"pointer": "/resolution_type", "kind": "string"},
            {"pointer": "/reopened_within_window", "kind": "bool"},
            {"pointer": "/repeat_contact_window", "kind": "bool"},
            {"pointer": "/reached_human", "kind": "bool"},
            {"pointer": "/downstream_reversal", "kind": "bool"},
            {"pointer": "/requester_class", "kind": "string"},
            {"pointer": "/inputs_sha256", "kind": "string"},
            {"pointer": "/ruleset_sha256", "kind": "string"}
        ],
        "deny_unknown_fields": false
    })
}

/// The artifact body the Judge adjudicates: the claim record plus `turn_marks`.
///
/// `turn_marks` is one `x` per public turn. The Judge has no numeric comparison, so a count is
/// expressed as a length its `field_length_between` criterion can bound. The count itself stays in
/// the record for humans to read.
#[must_use]
pub fn artifact_body(record: &ClaimRecord) -> Value {
    let marks = "x".repeat(usize::try_from(record.turn_count).unwrap_or(usize::MAX));
    let mut value = serde_json::to_value(record).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut value {
        map.insert("turn_marks".to_owned(), Value::String(marks));
    }
    value
}
