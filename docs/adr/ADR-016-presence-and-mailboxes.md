# ADR-016: Every beacon is optional; asynchronous delivery through mailboxes at Peerage peers

**Status:** Proposed 2026-11-19 (discussion with the owner). The extension points (E1–E5) go
in **before step 8** (format freeze); the mechanism itself comes **after the beta**.
**Date:** 2026-11-19
**Decided by:** project owner
**Related:** [ADR-015](ADR-015-profile-per-network.md) (profiles, hidden profiles, signed
records), [ADR-012](ADR-012-dm-session-v2.md) (1:1 sessions, signed static key),
[ADR-011](ADR-011-post-quantum.md) (hybrid PQ), [ADR-008](ADR-008-proof-of-transit.md)
(transit receipts), [ADR-002](ADR-002-group-message-ordering.md) (group ordering),
[threat model](../architecture/threat-model.md) N3, N4, N7, N9; step 7c of `docs/plans/beta.md`.

## Context (the problem)

**How delivery works now** (`messenger/src/lib.rs`). "Online" is not a separate beacon. It is
a side effect of delivery:

- offline contacts are probed with a growing interval (`PROBE_MIN` 2 s up to `PROBE_MAX`
  300 s), and online contacts are kept with keepalives;
- any authentic packet from a contact counts as "online" (`peer_alive`): it emits
  `Event::Presence` and starts the resend (`pump`);
- the new-address announcement (`announce_later`) also waits for that meeting.

`docs/design/app-ui.md` records the same: «Статус „в сети“ контакты видят всегда — он
следует из доставки (keepalive), отдельной настройки нет».

**The owner's requirement:** every beacon is optional and has its own switch. Hidden
profiles (ADR-015 §5) send none at all.

**Why a switch alone does not work.** While delivery is direct only, hiding "online" is a
lie. If a sender reached the recipient and got an answer or an ACK, the sender knows the
recipient is online. Both sides must also be online at the same moment, so phones that are
online on and off cannot really chat asynchronously.

**The owner's second idea:** a message to an offline contact goes to other Peerage peers,
which keep it for a while. It was mentioned once as part of the peer/leech mode ("peers keep
other people's messages 14 days, leeches 7"), but it was never designed.

**These are one problem.** An honest presence switch needs a way to deliver that is not
direct, and that way is the mailbox.

## Decision

### 1. Beacons and their switches

| Beacon | What it reveals | When it is off |
|---|---|---|
| Online status (answers to probes, keepalives) | when the user is online | No keepalives and no answers to probes from contacts. Incoming messages arrive through mailboxes. **Lands together with mailboxes (§3); before that, the switch would cut off incoming messages** |
| Typing | activity in a chat | already optional (7c) |
| Read receipts | when a message was read | already optional (7c) |
| Delivery receipts (✓✓) | the moment the user came online | Not sent end-to-end. A mailbox deletes a message on the recipient's fetch acknowledgement, which goes to the mailbox, not to the sender. When the switch is on but online status is hidden, receipts are sent in batches with a random delay |
| Call-room presence | being in a call | functionally needed while in the room; nothing is sent outside it |

**Not beacons:**

- **The address announcement** is always delivered, through a mailbox or together with the
  next own message.
- **Profile updates** are not presence either.

**What cannot be hidden:** sending a message directly shows the other person that the sender
was online at that moment. Telegram's "hide last seen" behaves the same way.

**Defaults:**

- **regular profile:** online status is on (today's behaviour); typing and read receipts are
  off (7c);
- **hidden profile:** everything is off.

### 2. Delivery does not depend on presence

- **Retries run on their own schedule.** Delivery must work even if the contact never
  answers a probe.
- **"Meeting" only speeds things up.** Today it is what triggers sending.
- **Order:**
  1. a direct attempt;
  2. if it is not acknowledged in time, or the contact is not reachable, deposit in the
     recipient's mailboxes;
  3. direct retries continue with backoff.

  The existing message-id dedup makes double delivery harmless.

### 3. Mailboxes

**Who stores mail:**

- **Stage 1** — only places the recipient chose:
  - the recipient's own always-on device (an old phone or a PC in "mailbox" mode);
  - contacts who agreed to hold mail for them.

  Trust already exists, so Sybil attacks do not matter.
- **Stage 2** — volunteers in peer mode, with transit receipts as the incentive (ADR-008).
  Volunteers are strangers, so this stage needs blinded tokens and limits.
- **Open storage at arbitrary peers through a DHT** is not in 1.x.

**How the sender finds the mailboxes:** the recipient publishes a **signed mailbox list**.
It uses the common signed-record format from ADR-015 §3 and reaches contacts in two ways:

- through the contact code (TLV);
- through an announcement when the list changes.

**Encryption.** A message for a mailbox cannot use the current session: the Noise XX
handshake (ADR-012) is interactive, and the session lives only in memory for minutes.
Instead:

- **one-way encryption to the recipient's mailbox prekey:** HPKE, hybrid X25519 + ML-KEM as
  in ADR-011;
- the recipient publishes **medium-term signed prekeys** in the mailbox record and deletes
  old ones, which gives forward secrecy per prekey period;
- the sender is authenticated **inside** the envelope (the sender's static key, signed by
  the identity), so the mailbox does not learn who sent the message;
- **group messages** are MLS ciphertexts, which are asynchronous by design, and are
  deposited as they are.

**Addressing without revealing the recipient:**

- A message is filed under a **blinded tag**, `H(pair_secret, period)`. The `pair_secret`
  comes from the static keys of the pair.
- Tags of different periods cannot be linked to each other.
- The recipient fetches by its own tags. The mailbox does not know who a message is for or
  who it is from.

**What a mailbox sees:**

- the tag;
- the size bucket (M5);
- the time;
- **the depositor's network address**.

The last one is unavoidable in a regular profile: the mailbox learns "X deposits something".
In a hidden profile the address hides the IP. This goes into the threat model next to N4.

**Limits:**

- message lifetime (default 7 days, the mailbox owner can change it);
- maximum size;
- a quota per depositor;
- `budget::Tier`.

**Who may deposit:**

- **Stage 1:** the recipient gives contacts a deposit token for each mailbox. The mailbox
  then knows the deposit is for its owner's friend, which is acceptable for one's own
  device or a trusted contact.
- **Stage 2** (strangers' mailboxes) needs blinded tokens.

**Fetching and acknowledgement:**

- The recipient queries its mailboxes on its own schedule with a random delay (one scheduler
  per profile, ADR-015 §6) and proves ownership.
- After taking a message it sends the mailbox an acknowledgement, and the copy is deleted.
- While the recipient keeps a session with the mailbox, the mailbox may push new mail.
- **Copies:** a deposit goes to up to 3 mailboxes, and dedup by id already exists.

**Group order.** Through mailboxes, MLS commits arrive out of order. An epoch buffer is
needed (the current ADR-002 ordering assumes delivery is mostly direct).

**Ticks.** A new state, "in the mailbox", sits between ✓ and ✓✓. The message `status` is a
string in the database, so a new value needs no migration. What it looks like is decided in
`app-ui.md` when it is implemented.

### 4. The Peerage peer network as a common base

Three features need to find **Peerage nodes**, not just Yggdrasil peers:

- stage-2 mailboxes;
- the rendezvous for NAT traversal (ADR-015 P6);
- gossip of channel chain heads (channel architecture).

That base is designed once, as a separate ADR. Stage 1 does not need it: the recipient lists
its mailboxes explicitly.

### 5. Hidden profiles

All beacons are off. Mailboxes are the main delivery path, and mailboxes live in the same
network as the profile (an onion service, an I2P destination).

## What we pay

- **Weaker forward secrecy for mail:** it is per prekey period, not per session, and is
  recorded in the threat model.
- **Metadata at the mailbox:** tag, bucket, time, depositor address.
- **A delay** of up to one fetch interval for anyone who hides presence.
- **Work in the group layer:** the epoch buffer.

## Plan

**Before step 8 (extension points; without them the mechanism would need a protocol break
later):**

- **E1.** A capability bitset in the handshake, next to the protocol version (step 8), with a
  "mailbox" bit.
- **E2.** Unknown types are already ignored (`#[serde(other)] Unknown` in
  `core/src/protocol.rs` plus a test). Check the same holds for unknown fields and unknown
  TLVs in the contact code.
- **E3.** One versioned signed-record format: the address record, the mailbox list and the
  device list (ADR-015 F2).
- **E4.** A slot for the mailbox list in the contact code v2 (ADR-015 F3).
- **E5.** Delivery does not wait for a meeting. The resend runs on its own schedule
  (behaviour, not format; cheap now). Before beta only the code moves; nothing new is sent.

**After the beta:**

- **M1.** Stage-1 mailboxes (own device, contacts) together with the online-status switch,
  plus the "in the mailbox" tick.
- **M2.** Hidden-profile defaults (with ADR-015).
- **M3.** Epoch buffer for groups.
- **M4.** Volunteers: blinded tokens, ADR-008 receipts, the Peerage peer network.

## Open questions (owner)

1. **Is the online-status switch mutual** (hide own status, then do not see others'), like
   read receipts in 7c?
2. **Default mail lifetime** and whether a contact has to agree before becoming a mailbox.
3. **How the "in the mailbox" state is shown.**

## How to check the decision is followed

- A loopback test: a recipient that never answers probes receives through a mailbox; the
  mailbox cannot tell recipient or sender from what it stores.
- A test: a recipient with "online" off sends no keepalives and answers no probes.
- A test: tags of two periods cannot be linked without the pair secret.
