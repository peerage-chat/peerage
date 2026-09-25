# ADR-008: Transit incentives — receipts (proof of transit), not an "I am a relay" flag

**Status:** Accepted (decision made), **implementation after 1.0.** 1.0 only reserves a field in
the protocol (`Payload::Ping/Pong.transit`) so that the presence beacon format does not have to
break later.
**Date:** 2026-08-01
**Decided by:** project owner
**Related:** [threat model](../architecture/threat-model.md) N8 (incoming traffic),
`core/src/budget.rs` (M3, tiers and limits), `core/src/outbox.rs` (outbox lifetime),
[ADR-007](ADR-007-identity-transport-key-split.md) (identity ≠ address), `core/src/pad.rs` + M5
(size buckets).

## Context (the problem)

The network stands on nodes that relay other people's traffic. That costs battery and mobile
data, so it makes sense to grant privileges for transit: priority in the delivery queue, a longer
outbox lifetime, a higher ingress budget.

**The trap is that the client is open (GPL-3.0) and runs on the user's device.** Anyone can make
a build where relaying is cut out and the "I am a relay" flag is kept — and get the privileges
while spending nothing. This is not a hypothesis about a villain: saving battery motivates
exactly this, and the first such build would spread across forums. Worse, the incentive works
backwards: the honest node pays with battery, the cheater does not, and their privileges are the
same.

The general rule everything follows from: **a client's self-report is not proof.** We can neither
trust the flag nor "check the build" — code on someone else's device can always be changed, and
attempts to prevent it (attestation, obfuscation) come down to fighting one's own user, which
contradicts both the license and the point of the project.

## Options

### 1. Local reputation in SQLite

Each node counts how much useful work a neighbour did and raises its tier. The mechanics partly
exist: `contacts.helpful` (M3) counts blobs served to us.

*Pro:* simple, works offline, nothing to coordinate.
*Con:* **selective defection.** A cheater only needs to serve those who are watching and ignore
the rest. Reputation is local — everyone has their own, and nobody has the full picture of "he
helps everyone".

### 2. Active probing

Periodically send a test packet through the candidate and see whether it arrived.

*Pro:* a direct check of the work being done.
*Con:* **it works only while a probe is indistinguishable from ordinary traffic.** A cheater who
can tell a probe apart (by size, rate, source, pattern) serves probes and drops the rest — the
same selective defection, only cheaper. And we cannot make a probe guaranteed indistinguishable:
by definition we generate it. On top of that the scheme adds traffic paid for by the verifier.

### 3. Proof of transit: receipts signed by the recipient

When node A relays a packet from Alice to Bob, **Bob** signs with his key a tiny receipt "the
packet with such-and-such hash came through A" and gives it to A. A collects receipts and
presents them as proof of work.

*Pro:* **it cannot be forged.** A cannot forge Bob's signature, and there is no way to get it
other than delivering the packet to Bob. This is the only one of the three options where the
privilege is tied to an event that would not happen without real work, rather than to the
client's claim about itself.
*Con:* more expensive — a signature per transit (or per batch), a settlement period, a protocol
for presenting and storing receipts. Some attacks remain — collusion (two cheaters signing
receipts for each other) and inflation with one's own "Bobs" — and they are handled by weighting
by the signer's tier: a receipt from a verified contact weighs more than one from a fresh key.

## Decision

**Option 3.** It is the only mathematically honest one: the other two rely on the good faith of
code we do not control. Options 1 and 2 are not thrown away — they are fine as a cheap heuristic
*on top of* receipts, but not as a basis for privileges.

**Implementation is deferred past 1.0.** Reason: before 1.0 we have no privilege worth
protecting — tiers (`budget::Tier`) are assigned by trust (contact / verified), not by transit,
and there is nothing to cheat. Building a settlement system before there is anything for it to
distribute is working ahead of the problem.

**But one thing goes into 1.0 now: reserving space in the protocol.**

## What exactly is reserved (and why it could not wait)

`Payload::Ping` and `Payload::Pong` (the presence beacon) got a field:

```rust
pub struct TransitClaim {
    digest: Vec<u8>,  // BLAKE3 of the batch of receipts for the period
    count:  u32,      // how many receipts are in the batch
    epoch:  u32,      // settlement period number
}
```

Three reasons to do this in advance rather than later:

1. **The CONTROL bucket of 256 B (M5).** A presence beacon must be indistinguishable by size from
   an `Ack` — otherwise a mesh observer starts seeing who is online, and the property closed in M5
   breaks retroactively. Adding the field later, we would find it does not fit. The test
   `presence_with_transit_claim_stays_in_control_bucket` pins this already: a beacon with a
   FILLED claim is the same size as an `Ack`.
2. **A separate "I am a relay" packet would be a new distinguishable class of traffic** —
   regular, distinguishable by size and addressable. The claim has to ride inside the existing
   beacon.
3. **The beacon carries a digest, not the receipts.** One receipt is a 64-byte signature plus a
   key, ~200 B in hex JSON: not even one fits into 256 B. So the beacon carries only a commitment
   (the hash of the batch and its size), and whoever has reason to check requests the receipts.
   As a side effect this also limits how much one can lie: the claimed number is confirmed only
   by a presented batch.

The field is optional and **not serialised while empty** (`skip_serializing_if`), so the bytes on
the wire do not change in 1.0 — there is a test for that too.

## What we pay

* The reservation lives in the protocol without an implementation — a reader sees a field that
  nobody fills. Mitigated: the type and both fields reference this ADR.
* In 1.0 the receiver **must ignore** the claim: an unverified claim is worth zero, and "trust it
  a little" is exactly the hole this whole decision is about.
* The full scheme will need receipt storage and a settlement period — separate work with its own
  limits (otherwise receipts themselves become a way to bloat the database).

## How to check the decision is followed

* `core/src/protocol.rs`: `presence_with_transit_claim_stays_in_control_bucket`,
  `empty_transit_field_is_absent_on_the_wire`.
* In 1.0 there must be no code that reads `transit` and changes a tier, priority or TTL because of
  it. Grep for `transit` in `messenger/`, `engine/`, `core/budget`.
