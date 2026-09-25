# ADR-010: Identity 2.0 — several devices per identity

**Status:** Accepted (decision recorded 2026-08-03) — **implementation after 1.0**. In 1.0
only what keeps us from breaking the format twice changes: the identity ↔ address split is
already done ([ADR-007](ADR-007-identity-transport-key-split.md)), and a message reference
no longer depends on "mine / theirs" ([ADR-006](ADR-006-chat-extensibility.md),
[ADR-009](ADR-009-one-message-store.md)).
**Date:** 2026-08-03
**Decided by:** project owner
**Related:** [ADR-004](ADR-004-mls-membership-revocation.md) (MLS membership and
revocation), [ADR-009](ADR-009-one-message-store.md) (one message table),
[ADR-011](ADR-011-post-quantum.md) (post-quantum transition — the same identity format),
[v1.0-scope.md](../v1.0-scope.md).

## Context (the problem)

Today an identity = one key pair = one device. Conversations live only on the phone; a
data backup brings back the history but not group membership, and a second phone cannot be
set up at all. For a messenger used every day this is the most painful limitation — and it
is not about convenience: a lost phone means lost access, even if the 24 words are written
down.

The temptation is to solve it head-on — copy the key to the second device. **That must not
be done.** In MLS a member is a leaf with its own ratchet state. Two devices under one leaf
diverge across epochs, break forward secrecy and hang the group for ALL members, not only
for the owner of two phones. It is not "works badly" — it destroys the group cryptography.

## Decision (model B: devices are independent keys under a shared identity)

**Three levels instead of one:**

1. **The root identity key** — the same 24 words. Kept offline and NOT used in day-to-day
   operation: it only signs the list of devices.
2. **A device key** — one per phone or desktop. This is what takes part in MLS (its own
   leaf), signs messages and lives on the network.
3. **The address** — still derived from the transport key (ADR-007); each device has its own.

**The device list** is a document signed by the root key: `{version, [device keys + names +
time added], signature}`. It spreads the same way the profile does today: to a contact on
connect, to group members inside MLS. A contact accepts the list only if the signature
verifies against the root key they already know by its fingerprint. The version is
monotonic: an old list cannot replace a newer one.

**Consequences for groups.** Each device is a separate MLS member. "Adding a person to a
group" means adding all of their devices; a new device = Add, a revocation = Remove with an
epoch change (the ADR-004 machinery already exists). A 1:1 chat becomes a small group —
which also removes today's asymmetry "DM ≠ group".

**History sync is separate from membership.** Our messages are immutable and addressed by
the pair "author + id" (ADR-006), they live in one table (ADR-009), and edits and deletions
are tombstone records. So merging two devices needs no arbiter: it is a set union with
conflicts resolved by (time, author, id). The first exchange between one's own devices
happens over QR when pairing, and afterwards whenever they meet on the network.

**Revoking a device** (lost, stolen): a new version of the list without it + Remove from all
groups + rotation. Until the revocation has spread, the device keeps full rights — an honest
limit, and the UI has to say so plainly.

## What we pay

- The number of MLS members multiplies by the number of devices: 10 people with 3 devices
  each is 30 leaves, not 10. Acceptable for our group sizes; not for "channels with a million
  members" (we will not have those).
- The root key becomes truly critical: it signs the set of devices. Onboarding has to
  explain this at least as well as today's "24 words = your identity".
- Sync is possible only when both devices are online: we have no server to hold the history.
  A second device that was away for a long time does not catch up instantly.
- The fingerprint for verification becomes the fingerprint of the ROOT, not of the key on the
  network. The verification screen has to be rewritten (and the difference explained to
  people).

## What it enables besides multiple devices

- **Post-quantum signatures** ([ADR-011](ADR-011-post-quantum.md)): the root key can become
  hybrid (Ed25519 + ML-DSA) without touching addressing, because the address is derived from
  the device key, not from the root.
- **Recovery after losing a phone**: a new device is added by the root and catches up on
  history from the others — without a cloud.
- **The desktop** as a full device, not "a second account".

## Why decide now if we build it after 1.0

The identity format breaks once or twice. Multiple devices and PQ signatures both pull the
same thread — breaking the link "identity key = address". The decision is recorded now so
that 1.0 does not lay down a format we would have to break twice.
