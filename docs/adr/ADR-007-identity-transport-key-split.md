# ADR-007: Separating the long-term identity from the transport key; controlled address rotation

**Status:** Proposed 2026-07-29, **refined by experiments 2026-08-01**, implemented (see Action
items). Rotation works against a flood ONLY together with a short offline pause (otherwise the
new node inherits the routing position of the old key); with the pause it really stops the
stream. See "Experiment results" and "Check on the public mesh".
**Date:** 2026-07-29 (experiments and refinement — 2026-08-01)
**Decided by:** project owner
**Related:** [N8](../architecture/threat-model.md) (incoming traffic cannot be cut at the
application layer), [SEC-5](../architecture/security-audit-2026-07-02.md) (domain separation of
keys), [invariants.md](../architecture/invariants.md) §6 (new keys — only with domain
separation), `core/src/budget.rs` (M3, the ingress budget).

## Context (the problem)

Two scenarios lead to the same result — **a stream of bytes into the device that the victim
cannot stop**:

1. **A malicious contact.** The other person modifies their client and sends packets non-stop.
   The goal is not to read the conversation but to burn data and battery.
2. **An address leaked by carelessness.** A contact (without ill intent) discloses the address to
   a third party. Then the same thing, but the flooder is not even in the contact list.

What exists: the ingress budget `budget::Tier` drops packets **before decryption** — no CPU,
battery on crypto or storage is spent; `Tier::Blocked` = `(0.0, 0.0)`, i.e. everything is cut.
What does not: **the bytes have already reached the radio**, the mobile data is already paid for.
In a serverless model there is no upstream to filter the stream before us, and nothing to cut the
link with — the flooder is usually not our direct peer but a node behind the routing.

Hypothesis (July): the measure that really stops the stream is **to stop being addressable by
that key**, i.e. rotation. **The 2026-08-01 experiments REFINED it:** rotation "on the fly" does
not stop a flood (the new node inherits the routing position of the old key), but rotation + a
short offline pause does (the old key drops out of the tree and there is nowhere to send to).
Details in "Experiment results" and "Check on the public mesh". A separate question — why
rotation is EXPENSIVE and how to make it cheap — is the subject of this ADR. The cost of rotation
at the time:

```
one ed25519 seed  →  the Yggdrasil address (iwt.Addr(pub), ygg.go)
                  →  the fingerprint (then 8 BIP39 words; now a visual grid), QR verification
                  →  the MLS credential (group membership)
                  →  the contact identity in the contacts store
```

`Node::start(&identity.seed())` ([engine/src/lib.rs](../../engine/src/lib.rs)) — the address and
the identity were literally the same thing. So changing the address = changing the identity: all
verifications reset, membership in every MLS group breaks, and contacts see "a new unknown
person". An emergency exit, not a tool.

### A proposal examined: a separate address per contact

The owner proposed: since there are quadrillions of IPv6 addresses, give each contact its own
address — then exactly one channel is compromised, and only it needs changing.

The idea was examined and **rejected in its original form** — for three reasons:

- **It does not reduce the bytes.** All N addresses live on one device behind one radio. A flood
  to Bob's address crosses the mesh and wakes the same radio; we drop it more cheaply, but pay
  exactly the same for the traffic. The main pain of the scenario remains.
- **The cost multiplies by N.** An address is a function of a key (`iwt.Addr(ed25519.PublicKey)`),
  so N addresses = **N Yggdrasil nodes**: each with its own TLS sessions to public peers, its own
  participation in the tree, its own keepalives. 20 contacts = ×20 background traffic and wake-ups
  on a phone — more expensive than the attack itself.
- **The separation is partial.** The operator of a public peer sees all the identities from one
  IP: you are separated from contacts, not from the network.

But **there is a valuable seed in the proposal**: containing the damage. "One channel is
compromised — I fix one channel" is the right goal. It turns out it can be had without N nodes.

## Decision

Separate the **long-term identity** from the **transport key**.

### 1. Two keys instead of one

| | **IK — identity key** | **TK — transport key** |
|---|---|---|
| What | ed25519 from the seed/mnemonic | the Yggdrasil node key = the address |
| Lives | forever, never rotated | consumable, rotated on demand |
| Carries | fingerprint, QR verification, MLS credential, contact record, signatures | only addressing and routing |

TK is derived from the same seed with domain separation — by an epoch counter:

```
TK_n = Identity::derive_subkey("dev.p2p.transport.v1/" + n)     // BLAKE3 KDF
```

Determinism matters: restoring from the mnemonic keeps working (seed + epoch number = the same
address), and no new secret has to be stored. Consistent with invariant §6 "new keys — only with
domain separation".

**Epoch 0 is the current key** (`TK_0 == IK`). Existing installations do not migrate: until the
user rotates the address, everything is exactly as before.

### 2. The announcement of a new address is signed by the IK

```rust
Payload::TransportRotate { epoch: u32, transport_pub: [u8; 32], sig: [u8; 64] }
// sig = IK.sign(domain || epoch || transport_pub)
```

A contact accepts the announcement **only if the signature verifies against the IK it already
has**, and if `epoch > the known one`. That closes the obvious vector ("hi, I am Alice's new
key") and replay/downgrade. The contact record stores `(IK, TK, epoch)`; sending resolves
IK → TK.

Size — the **APP bucket of 4 KiB**, not CONTROL (as originally intended): a signature alone takes
64 B, plus the key — it does not fit under the 256-byte ceiling. It turned out even better: among
application messages the announcement is indistinguishable from ordinary text or an image, so the
flooder cannot see on the wire the moment the victim moves to a new address.

### 3. Where the owner's idea lives: a per-contact ACL for the announcement

Rotation is not a broadcast "to everyone". The user chooses **who gets the new address**. Bob
leaked it — we rotate and send it to everyone **except Bob**. The result is the same as with a
separate address per contact:

- Bob is left with a dead address and loses contact with us;
- the other contacts notice nothing — verifications, history and groups are intact;
- there is still one node: keepalives and battery do not multiply by the number of contacts.

The difference from the original idea: the compartmentalisation is not permanent (N channels
always) but **on demand** (one channel, cheaply re-created with selective notification). The price
— at each rotation the address changes for everyone, not for one; the gain — the cost of rotation
drops from "lose the identity" to "send a 200-byte announcement".

### 4. A transition period for offline contacts

A contact absent longer than the outbox lifetime would not see the announcement and would lose the
connection forever. The originally proposed mitigation: **keep the old node alive in parallel for
T days**, with a `Tier::Stranger` budget. Rejected later — see Action item 8a.

## Options considered

| Option | Why not |
|---|---|
| **A. A separate address per contact** (the original proposal) | N Yggdrasil nodes on a phone: ×N keepalives and wake-ups; the bytes still reach the radio; separation only from contacts, not from the network. **Not dropped completely:** after the IK/TK split it becomes a natural extension (several TKs at once) — reasonable for a desktop or an always-on node where a node is cheap. Recorded as a post-ADR option, "personas". |
| **B. Global rotation without separating keys** (the old N8) | Works, but resets verifications and breaks MLS group membership → fit as an emergency exit, not a tool. This ADR is exactly what makes it cheap. |
| **C. A personal relay in front of the device** | The only construction where filtering happens **before** our radio. Orthogonal to this ADR and compatible with it, but it needs an always-on node → cannot be the base solution in a serverless model. Remains an option for those to whom DoS resistance matters more than purity of the model. |
| **D. Change nothing, rely on `budget::Tier`** | The status quo: saves CPU, battery and storage, but not traffic. The cheap hardening (Action item 1) is done anyway; it does not compete with this ADR. |
| **E. Rotating a "channel" at the application layer (without changing the TK)** | Pointless: the flooder hits the node key, not an application identifier. Once the address is known, application-level rotation changes nothing. |

## The trade-off

**We get:** address rotation stops costing the identity; the damage from a leaked address is
contained to one contact; fingerprint, verifications and group membership survive; one source of
truth for addressing.

**We pay:**
- the assumption "peer key = address = identity" unravels across the code: addressing
  (`messenger`), the contacts store, the gate, the outbox, the MLS roster (membership by IK,
  sending by TK), room presence, blobs;
- the QR exchange carries two values (IK + the current TK) — the format changes;
- the ingress budget on input knows only the TK: before it is matched to an IK, a stranger is cut
  as `Stranger` (true before too, but now a conscious boundary);
- a simultaneous rotation by both sides while both are offline = a lost connection. Mitigated by
  how rare the operation is (manual, not routine) and by catching up via presence.

## Experiment results (2026-08-01) — a linear bench (reconnecting WITHOUT a pause)

> **Read together with the next section.** This bench put the victim back on the network almost
> immediately (gap ≈ 0). In that mode rotation does not stop a flood — as seen below. The public
> run (next section) showed that with an offline pause of ~30 s the conclusion flips. So "the
> flooder does not stop" below is about the gap ≈ 0 mode, not a general law.

Bench: `transport/examples/flood_rotation.rs` (`bash scripts/run-flood-rotation.sh`). Three real
Yggdrasil 0.5.13 nodes in a line **A (flooder) → R (transit) → B (victim)**, no direct A↔B link.
The measure is the cumulative `rx_bytes`/`tx_bytes` of the links (byte counters were added to
`ygg_peers_json` for this: smoothed `*_rate` values lie at phase boundaries). The baseline is
measured AFTER the handshake: an ironwood session completes only with a two-way exchange,
otherwise the "baseline" would be an undeliverable stream.

Stream 400 packets/s × 1 KB ≈ 423 KB/s, a 12 s measurement window.

| Measured | Baseline (victim with the old key) | After rotation (victim online with a new key) | After the victim left the network |
|---|---|---|---|
| A sends into the network | 5081 KB (423 KB/s) | **5081 KB (100 %)** | **5388 KB (106 %)** |
| arrives on the victim's link | 5081 KB | **5081 KB (100 %)** | — (no link) |
| reached the victim's application | 4620 packets | **0** | 0 |
| the victim sends back | — | **316 KB (≈6 % of the stream)** | — |
| transit R | forwards everything down | forwards everything down **to the new node** | absorbs everything itself |

**Conclusions:**

1. **The flooder does not stop.** Node A keeps sending 100 % of the bytes into the network both
   after the recipient key disappears and after the victim leaves entirely. `write()` accepts every
   packet. The transport gives no "destination unreachable" feedback that would make it go quiet.
   The expectation "the stream dies on the flooder's side" is **wrong**.
2. **Rotation does not save the victim's traffic.** While the victim is on the network (even under
   a new key), its mesh neighbour keeps sending it the stream addressed to the dead key: 100 % of
   the baseline arrives on the link. The radio and the mobile data are paid in full.
3. **Rotation guarantees exactly one thing:** application delivery stops (0 packets in the app
   instead of 4620). But the ingress budget `Tier::Blocked` gives the same, cheaper and without
   changing the address — it drops before decryption.
4. **The victim also pays in upload:** under someone else's stream its node sends ~6 % of the
   volume back (routing service traffic).
5. **The bytes are absorbed by whoever is further down the route.** When the victim is not on the
   network, transit R eats the whole stream — an argument for a "personal relay" (a node IN FRONT
   of the device). **But:** the public run showed a second way to kill the stream without a relay
   — going offline, after which the old key drops out of the tree and there is nowhere to send to
   (below).

**Check on the public mesh (2026-08-01) — the conclusion was REFINED, rotation partly
rehabilitated.** Tested on the REAL Yggdrasil: `transport/examples/flood_pubmesh.rs`, nodes A and
B join the global network through public peers, no direct link, the route is built by the real
tree. A gentle rate (50 packets/s) — through someone else's volunteer infrastructure. Control: the
new node's intake is measured with the flood OFF and ON (links also catch unrelated transit;
without a control the number means nothing). The decisive parameter was one the linear bench did
not isolate: **how long the victim's node stays COMPLETELY offline** between shutting down the old
key and coming back under the new one (`--gap`).

| Offline pause | flooder A sends into the network | reaches the new node | result |
|---|---|---|---|
| ~5 s (instant reconnect), shared peers | full volume | **89 % of the injected** | rotation does NOT help |
| ~5 s, disjoint peers | full volume | **88 %** | different peers do not help |
| **30 s** | **~0 (1 KB in 12 s)** | **0** | flood STOPPED |
| **60 s** | **~0** | **0** | flood STOPPED |
| **120 s** | **~0** | **0** | flood STOPPED |

Two modes:

- **Instant reconnect (gap ≈ 0–5 s).** Routing information about the old key is still alive in the
  tree; the new node, once connected, inherits its position — packets to the old key land on it
  (88–89 %). Different public peers do not help: the flood follows the destination key, not the
  entry point. This matches the linear bench (where the new node instantly reconnected to the same
  transit) — so the linear "100 %" was the gap ≈ 0 case, not a general rule.
- **Going offline for ~30 s or longer.** The old key **drops out of the routing tree**, and the
  flooder physically stops sending: its output falls from full volume to ~0 (`write()` goes nowhere
  — nobody has a route to the vanished key). The threshold is faster than ironwood's
  `sessionTimeout` (60 s): it is not the session timeout but the expiry of the key's announcement
  in the tree.

The key point: **Yggdrasil cannot route to a key that is not in the network** — a property of the
protocol, not the client's politeness. The attacker's modified client cannot get around it: the
mesh cannot be made to build a route to an absent key. So **rotation + a short offline pause + not
giving the new key to the flooder** (the per-contact ACL) really stop the stream — including an
already established session of a malicious contact.

### A side finding: a silent receiver blocks application delivery

Bench `transport/examples/silent_victim.rs`: the sender pours 5 packets/s, the receiving
application only reads and writes **not a single** packet. Result — **zero** delivered in 200
seconds. The mechanism (ironwood `encrypted/session.go`, analysed from the code):

1. A sender without a session **does not put the payload on the network at all**: it goes into a
   buffer (only the LAST message is kept), and a `sessionInit` of ≈200 B goes on the wire.
   Measured: 250 B per write attempt with a 500 B payload.
2. On the first init the receiver creates a session with `seq = init.seq` — and immediately
   discards that same init by the check `init.seq <= info.seq`, **without sending an ack**.
3. `init.seq` is unix time to the second, and `buf.init` is created once per buffer. The buffer
   lives `sessionTimeout = 1 minute`, but **the timer is reset on every write** — for a
   continuously sending peer it never expires. So the sender repeats the same init forever, and the
   receiver ignores it forever.
4. The deadlock is broken in two ways: the receiving application writes back (it already has a
   session → traffic → the sender replies with a fresh init → newer seq → ack), or the sender pauses
   for more than a minute.

**What it means.** A continuous flooder cannot deliver a single application packet to a silent
receiver — the contact gate is propped up by the transport. But:

- **it does not affect traffic**: 250 B per attempt still reach the victim; a DoS by bytes works
  here too;
- **it is an implementation detail, not a protocol guarantee.** The header of `session.go` has a
  TODO about exactly this area (keep the private keys of sent inits / answer an ack with an ack).
  Nothing may be built on it; re-check on every ironwood update
  (`cargo run -p p2p-ygg --example silent_victim`).

## Revision of the decision after the experiments

The overall picture after the public run: the IK/TK split is **both compartmentalisation and a
working anti-DoS measure, but with a mandatory condition — an offline pause**.

- ✓ **Rotation STOPS a flood — with a short offline pause.** Without the pause (reconnecting on the
  fly) the new node inherits the routing position of the old key and keeps receiving the flood; but
  after going offline for ~30 s the old key drops out of the tree and there is nowhere to send to —
  the flooder physically goes quiet. It works even against an already established session of a
  malicious contact, because a route to an absent key cannot be built. Conditions: (a) an offline
  pause; (b) the new key is not given to the flooder (the per-contact ACL).
- ✓ **Compartmentalisation.** A leaked address stops being a way to REACH us, while contacts are not
  lost, and verifications and groups survive. The answer to "a friend leaked my address".
- ✓ **Separating identity and address** as hygiene (SEC-5) and a prerequisite for future "personas"
  (several TKs on an always-on node).
- ⚠ **The cost to name honestly in the UX:** rotation against a flood = "go offline for about a
  minute, come back under a new address, send it to all contacts EXCEPT the source of the flood".
  Not instant and not free (under a persistent attack — round after round), but a WORKING way out
  with the device's own means, without an upstream. And during the pause the victim is offline
  anyway — it does not pay for the flood at that moment.
- ~~↑ **Raised in priority:** a personal relay (option C).~~ **NOT in 1.0 (owner's decision,
  2026-08-01).** The experiment showed where the bytes die — on the node BEFORE the victim, i.e.
  only a cooperative neighbour can cut the stream before the radio. But an intermediate filtering
  node is exactly the upstream the project exists to avoid; we will not bring it into 1.0. The idea
  stays recorded for the future (including the special case "a hint to trusted neighbours: this key
  is dead" — which works only with peers running our software), as a post-1.0 option, not a plan.
- ↑ **Raised:** an honest flood UX. The user must see that they are being flooded and have two ways
  out: "go offline" (stop paying right now) and "change address" (offline pause → new TK → send to
  everyone except the source of the flood).

In sum: the key split is not just a refactoring but the basis of a working answer to a flood with
the device's own means (rotation + an offline pause + selective notification). Priority: first the
cheap receive-path hardening and UX (item 1), then the split itself (items 2–9). A relay is
deliberately NOT in 1.0, and that is no longer a loss: the device can cut off the stream itself,
at the cost of a minute offline and re-sending the address.

## Action items

0. [x] **Experiments done 2026-08-01.** Result: rotation "on the fly" does not stop a flood;
       rotation + an offline pause of ~30 s does (the old key drops out of the tree, there is
       nowhere to send to). The benches are in the repository: `flood_rotation.rs` (a line,
       `bash scripts/run-flood-rotation.sh`), `flood_pubmesh.rs` (the real mesh,
       `bash scripts/run-flood-pubmesh.sh --gap N`), `silent_victim.rs` (session behaviour with a
       silent receiver).
1a. [x] **Taken into account in the rotation implementation (item 5):** rotation against a flood
       includes an offline pause (shut down the old TK → wait for the announcement in the tree to
       expire → bring up the new TK); otherwise the new node would inherit the stream to the old key.
1. [~] **Cheap hardening, independent of this ADR:**
       - drop by source key **inside libygg**, before crossing FFI — done
         (`transport/tests/blocklist.rs`);
       - remove the 64 KB allocation for every incoming datagram (`ygg_read`) — done (a
         `sync.Pool` of read buffers in `ygg.go`);
       - a flood detector + an honest screen "X is flooding you, N packets/s" with a choice: go
         offline (the only way to stop paying for the traffic) or rotate the address — done (per-
         source flood statistics in `messenger`, UI in item 8); blocking stays a user action, not
         automatic;
       - a guard on mobile (metered) networks.
2. [x] **`Identity` — DONE 2026-08-01.** `transport_seed(epoch)`/`transport_public` via
       `derive_subkey`; `TK_0 == IK` (no migration needed); `sign_transport_rotate` + a free
       `verify_transport_rotate` (the domain in the signed bytes, the epoch under the signature).
       3 unit tests.
3. [x] **Contacts store — DONE 2026-08-01.** Columns `transport_key`/`transport_epoch` (ALTER,
       empty = epoch 0), `apply_transport_rotate` (signature + strictly monotonic epoch + unknown
       contact = refusal), `transport_of`. 2 tests, including anti-replay and an attempt to hijack
       with someone else's identity.
4. [x] **Protocol — DONE 2026-08-01.** `Payload::TransportRotate {id, epoch, transport_pub, sig}`,
       reliable delivery (id → retry until ack + the outbox). **APP bucket, not CONTROL:** a single
       64-byte signature is incompatible with the 256-byte ceiling; among application messages the
       announcement is also indistinguishable from an ordinary message, so the flooder cannot see
       the moment of rotation. The size-indistinguishability test was extended.
4a. [x] **Receiving side — DONE 2026-08-01.** Anti-spoofing rewritten for the key split: a packet
       is accepted if `from == src` (epoch 0 / a stranger), OR `src` is a known address of that
       identity, OR it is the announcement itself whose signature binds `from → src` (otherwise the
       new address could not be learned — it already arrives from it). The peer is identified by
       the IDENTITY everywhere. `Shared.transports` (IK → TK) + `set_peer_transports`; `send`
       encrypts to the IDENTITY and writes to the ADDRESS. The engine applies the announcement
       through the store and updates the map (`refresh_transports`, including at start-up). Tests
       in `engine/tests/rotate.rs`.
5. [x] **Engine, sending side — DONE 2026-08-01.** The node starts on the TK of the current epoch
       (`transport.json`); `rotate_transport(recipients, gap_secs)` in a separate thread: shut down
       the node → **offline pause** → bring it up on the new TK → reconnect to public peers → commit
       the epoch → send the signed announcement to the SELECTED contacts. Those who were offline
       get it via presence (`announce_rotation_to`). The transport became replaceable on the fly
       (`Messenger::swap_transport` + the receive loop survives the swap); a transport factory was
       added for tests. The event `TransportRotated` for the UI.
5a. [x] **Found and fixed along the way 2026-08-01.** The receive dispatcher identified the peer by
       ADDRESS (`src`), not by identity: after a rotation, history was written under the transport
       key and the `Ack` was encrypted to the wrong key. Now, after authenticity is checked, the
       peer is the identity everywhere. Also: the trust tier and the blocklist were keyed by
       identity but looked up by address, so a contact who changed address would have been demoted
       to a stranger, and a blocked one would have escaped the block. A reverse address → identity
       map was added, the early libygg drop is moved to the current addresses, and an announcement
       from a blocked peer is not applied at all. Covered by the test
       `blocked_contact_cannot_escape_by_rotating`.
6. [x] **Groups — DONE 2026-08-01** (no changes needed, verified by the test
       `group_survives_member_rotation`): MLS membership is tracked by identities, and sending
       resolves the address in a single place (`Shared::send`), so group conversations survive a
       member's rotation without re-creating the group.
7. [x] **Exchange code — DONE 2026-08-01.** `my_contact_code()`: before a rotation, the old bare
       hex of the identity (old QR codes and scanners keep working); after — `identity:epoch:
       address:signature`. `add_contact_code()` accepts both forms and records the binding only
       with a valid signature: codes are often passed on through third parties, and without a
       signature an intermediary could substitute the address while keeping the identity.
8. [x] **UI — DONE 2026-08-01** following the mock-up
       [address-rotation.html](../design/mockups/address-rotation.html): a traffic card on the
       Network screen, a "Traffic stream" screen (sources + two actions), "Change address" with
       checkboxes (the source is unchecked by default, but the box is clickable), the progress of
       the operation with a countdown and a result. Notifications: a flood episode (once per
       episode) and a persistent "you are offline". The tone of the texts is direct: "the content
       does not get through, but the traffic is already paid for", without promising protection
       that does not exist.
8a. [x] **The grace period REJECTED** (owner's decision 2026-08-01). Keeping the old node alive =
       keeping the old key in the routing tree, i.e. cancelling the whole effect of rotation for
       the length of the grace period, plus ×2 keepalives. Instead — a persistent list of "who has
       not been told yet": a contact absent even for a month gets the new address when they first
       appear (catch-up via presence). The only uncovered case is when both sides changed address
       while they did not see each other — then an external re-exchange of codes is needed.
9. [x] Docs: N8 in the threat model rewritten; protocol-spec (the new Payload) and invariants (§7:
       "identity is the IK, addressing is the TK; do not mix them up") updated.
10. [~] **A personal relay (option C) — NOT in 1.0** (owner's decision 2026-08-01). The only place
       where the stream is killed before the radio, but it is exactly the upstream the project
       exists to avoid. Recorded for after 1.0; if ever done — a separate ADR (the trust model: who
       allows whom to filter).

**Order after the revision:** 1 (cheap receive-path hardening + flood UX — the only answer to a
flood by traffic available in 1.0) → 2–9 (the key split as compartmentalisation and hygiene). The
relay (10) — outside 1.0.
