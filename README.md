# resolution-normalise

Derives billed-resolution claim records from vendor-neutral helpdesk records.

**This crate is published so that the party being measured can check the measurement.**

If someone has disputed an invoice using a claim record produced by this code, you do not have to
take their word for any of it. Take their source bundle and their ruleset, run this crate, and
compare. The derivation is deterministic: same bundle plus same ruleset yields byte-identical
records, including the `inputs_sha256` and `ruleset_sha256` digests each record carries.

If the digests do not reproduce from the bundle, ruleset and `evaluated_at` supplied, the claim has not been substantiated as stated, and you can show exactly that.

## Reproducing a run

You need two files from whoever made the claim, and nothing else:

| File | What it is |
|---|---|
| `bundle.json` | The source records the derivation read — tickets, events, users, the billed claims, and any downstream refunds or cancellations. It contains requester ids and email addresses, which are needed to classify staff and test accounts: treat it as personal data when you share it |
| `ruleset.json` | The criteria those records were judged against, fixed before the run |

```rust
use resolution_normalise::{normalise, Ruleset, SourceBundle};

let bundle = SourceBundle::from_path("bundle.json")?;
let rules = Ruleset::from_path("ruleset.json")?;
let outcomes = normalise(&bundle, &rules, "2026-08", evaluated_at)?;
```

`evaluated_at` decides only which claims are deferred: a claim whose windows had not closed by then
is not judged. The claim records do not carry it. Any value after every window in the period has
closed produces the same records, so to reproduce a closed period, pass a time after the period end
plus the ruleset's longest window.

`cargo test` runs the reproduction suite against the fixtures in `fixtures/`.

## Rulesets

`rulesets/` holds Kyvryn's standard rulesets, and `rulesets/manifest.json` maps each version to
its digest. A claim record names the rules it was judged under in `ruleset_sha256`; look that
digest up in the manifest to see which document it was.

**A hash of the file is not that digest.** `ruleset_sha256` is taken over the ruleset's canonical
serialisation, not the bytes on disk, so whitespace and key order in the file do not change it:

```
$ sha256sum rulesets/kyvryn-res-1.0.json      # NOT the value in a claim record
$ cargo run --example ruleset_digest -- rulesets/kyvryn-res-1.0.json
version kyvryn-res-1.0
digest  83fd376e34bcc1b7bbbcfef9a2a6bc6bfe033cea0ab7a7fdb8c64100182edf0c
```

Hashing the canonical form is deliberate — two parties who agreed the same rules arrive at the
same digest even if one reformatted the document — but it means the obvious check gives an
unrelated number, and we would rather say so than have you conclude the figures were invented.

**A digest that is not in the manifest is not necessarily wrong.** Rules may be fixed per
engagement, so a buyer, or a buyer and vendor together, may use terms that differ from the standard set.
In that case the ruleset travels in full inside the evidence pack, and the pack's copy is what
the digest refers to. The manifest covers the standard rulesets only.

## What this does not do

**It does not assess answer quality.** Every flag here is arithmetic over timestamps and state
transitions: did the ticket reopen inside the ruleset's window, did the same requester come back, did
a refund follow, was the requester a member of staff or a test account. Whether the agent's reply
was any *good* is not evaluated, because substituting one model's judgement for another's would be
the same error this exists to point at.

**It is not an audit.** It is a comparison against criteria fixed in advance.

**It does not decide anything.** This crate derives boolean facts. Those facts are checked against
the criteria by a separate adjudicator, which signs a verdict anyone can verify offline against a
published key. The split is deliberate: the adjudicator has no numeric comparison at all, so the
counting has to happen here, in the open, where it can be re-run.

## What is deliberately not here

Connectors that fetch records from Zendesk, Intercom or Salesforce, and the tooling that submits
claims and builds evidence packs. None of that is needed to reproduce a result. A bundle is a JSON
file, and whoever ran the original derivation can export it; the connectors only decide how that
file gets filled.

## Determinism

The derivation is a pure function of `(bundle, ruleset, period, evaluated_at)`. It reads no
environment, opens no sockets, and consults no clock of its own. That is what makes a digest worth
computing, and it is asserted directly by the test suite rather than left as a claim in a README.

## Licence and scope

This crate is Apache-2.0. It is the derivation only. The `attest` command-line tool that feeds it —
the helpdesk connectors, the submission client and the evidence-pack renderer — is proprietary and
distributed as binaries at
[attest-cli](https://github.com/BrijStream-Technologies/attest-cli), under its own licence. None of
that is needed to reproduce a result: a source bundle is a JSON file, and everything the
measurement depends on happens here, in the open.

Zendesk, Intercom, Fin, Salesforce and Agentforce are trademarks of their respective owners. Kyvryn
and BrijStream Technologies are not affiliated with, endorsed by, or sponsored by any of them.
