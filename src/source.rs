//! Vendor-neutral source records.
//!
//! Connectors translate a helpdesk's own shapes into these. The normaliser sees nothing else, so
//! adding Intercom or Service Cloud later means writing a translation, not a second rulebook.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// A support ticket, reduced to the fields any resolution claim depends on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    /// Helpdesk ticket or conversation id, as that helpdesk writes it.
    pub id: String,
    /// The person who asked.
    pub requester_id: String,
    /// When the ticket was opened.
    pub created_at: DateTime<Utc>,
    /// Every tag on the ticket; the intent tag is selected from these by prefix.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Something that happened to a ticket, in order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketEvent {
    /// Ticket this event belongs to.
    pub ticket_id: String,
    /// When it happened.
    pub at: DateTime<Utc>,
    /// What happened.
    pub kind: EventKind,
}

/// The event kinds that bear on whether a resolution was real.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Ticket status moved to this value, lowercased (`solved`, `open`, `pending`, `closed`).
    StatusChanged {
        /// The new status.
        to: String,
    },
    /// Ticket was assigned to a user.
    AssigneeChanged {
        /// The assignee, or `None` when unassigned.
        to_user_id: Option<String>,
    },
    /// A public comment was added by this author.
    PublicComment {
        /// Author of the comment.
        author_id: String,
    },
}

/// A helpdesk user, enough to classify who was actually asking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// User id.
    pub id: String,
    /// Helpdesk role: `end-user`, `agent`, `admin` or `bot`.
    pub role: String,
    /// Email address, when the helpdesk exposes one.
    #[serde(default)]
    pub email: Option<String>,
}

impl User {
    /// True when this user is helpdesk staff rather than a customer.
    #[must_use]
    pub fn is_staff_role(&self) -> bool {
        matches!(self.role.as_str(), "agent" | "admin")
    }
}

/// One resolution the vendor billed for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilledClaim {
    /// The vendor's identifier for this billed resolution.
    pub claim_id: String,
    /// The ticket it refers to.
    pub ticket_id: String,
    /// `confirmed` or `assumed`, as the vendor classified it. Intercom defines an assumed
    /// resolution as the customer exiting "without requesting further assistance" and bills both at
    /// the same rate (fin.ai, retrieved 2026-09-20). Other vendors use other categories.
    pub resolution_type: String,
    /// When the vendor says the resolution occurred.
    pub resolved_at: DateTime<Utc>,
    /// Amount billed, in micros of USD, so no float ever touches an invoice.
    pub amount_usd_micros: u64,
}

/// Something in the buyer's own systems that contradicts a resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownstreamEvent {
    /// The customer it concerns.
    pub requester_id: String,
    /// When it happened.
    pub at: DateTime<Utc>,
    /// `refund`, `cancellation`, `chargeback` or `return`.
    pub kind: String,
}

/// Everything one period's attestation runs on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceBundle {
    /// Tickets referenced by the billed claims.
    pub tickets: Vec<Ticket>,
    /// Events for those tickets.
    pub events: Vec<TicketEvent>,
    /// Users referenced as requesters or comment authors.
    pub users: Vec<User>,
    /// The vendor's billed resolutions for the period.
    pub claims: Vec<BilledClaim>,
    /// Contradicting events from the buyer's billing or order systems.
    #[serde(default)]
    pub downstream: Vec<DownstreamEvent>,
}

impl SourceBundle {
    /// Reads a bundle from canonical JSON.
    ///
    /// This is the vendor's entry point. Reproducing an attestation requires the bundle, the
    /// ruleset and the normaliser — not a connector and not credentials for the helpdesk it came
    /// from. A vendor checking a claim against it is reading the same bytes the buyer's run read,
    /// which is what makes `inputs_sha256` meaningful rather than decorative.
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be read, [`Error::Parse`] if it is not a bundle.
    pub fn from_path(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| Error::Parse {
            what: path.to_owned(),
            source,
        })
    }

    /// Writes the bundle as canonical JSON, so it can be handed to the other side of a dispute.
    ///
    /// # Errors
    /// [`Error::Parse`] if the bundle cannot be serialised, [`Error::Io`] if the write fails.
    pub fn to_path(&self, path: &str) -> Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Parse {
            what: "source bundle".to_owned(),
            source,
        })?;
        std::fs::write(path, text).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })
    }
}
