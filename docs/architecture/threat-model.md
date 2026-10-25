# Threat model

Status: draft v1 (2026-07-02, revised through 2026-09). Scope: v1.0 (Android client +
Rust core + Yggdrasil/Ironwood transport). The detailed findings are in
[security-audit-2026-07-02.md](security-audit-2026-07-02.md) (Russian); a summary with
statuses is in [SECURITY.md](../SECURITY.md).

This document says **what** we protect, **from whom**, which guarantees we give — and,
just as important, **what we explicitly do not promise**. An honest boundary of
guarantees is a requirement for a serverless messenger.

## 1. Assets

| Asset | Where it lives | Worst outcome if compromised |
|---|---|---|
| Identity seed (ed25519, 32 bytes) | `profile.json`, sealed by the vault | Full impersonation of the node; decryption of all past and future DMs (see below) |
| Vault key (at rest) | Android Keystore and/or the user's password (Argon2id) | Decryption of the whole database and group files on the device |
| Message history (DMs, groups, Saved) | SQLCipher `messenger.db` | The conversations leak |
| MLS group state (epoch secrets) | `groups/*.group`, sealed by the vault | Decryption of group messages of those epochs |
| Call content (audio/video) | in flight only (never stored) | Eavesdropping on the call |
| Social graph / presence | in flight (beacons, keepalive) | Disclosure of "who talks to whom, and when" |

## 2. Adversaries

- **A1 — passive observer on the mesh path.** An intermediate Yggdrasil node or a network
  observer. Sees transport-encrypted traffic, but knows routing metadata (the public keys
  of source and destination).
- **A2 — active network attacker.** Can drop, delay, replay and inject datagrams. Can NOT
  forge the Ironwood transport source key (see the root assumption).
- **A3 — malicious or compromised group member.** Is (or was) legitimately in the group
  and holds its epoch secrets.
- **A4 — local attacker on the device.** Has access to the app's file system (a stolen or
  rooted phone, a backup), but NOT to the Android Keystore / OS keyring.
- **A5 — operator of a third-party service** (ntfy.sh as a push/delivery relay, if
  enabled; not wired in today). Sees what passes through the relay.

## 3. Trust boundaries and the root assumption

All source authenticity rests on **one root assumption**:

> **The Ironwood/Yggdrasil transport guarantees that the `src` key of a received
> datagram is the sender's real public key, and that the content is encrypted between
> the two end nodes.**

Resting on it:
- anti-spoofing on the reliable channel (`from == src`, `messenger/src/lib.rs`);
- **all** confidentiality and authenticity of the media channel (`CHAN_MEDIA` has no
  application-layer encryption — finding SEC-4).

If this assumption breaks (a regression in Ironwood, a switch to a transport without
end-to-end encryption), calls lose confidentiality silently. That is an invariant, not a
detail: any replacement of the transport must keep the property or add an application layer.

## 4. Guarantees we give

- **G1.** DM and group *text* messages are end-to-end encrypted; an intermediary (A1, A5)
  cannot read the content.
- **G2.** Forgery or corruption of a message is detected (AEAD tag of the 1:1 session /
  MLS), and a replayed 1:1 packet is rejected (the session's replay window).
- **G3.** The sender is authentic: a message cannot be attributed to someone else's key
  (G3 relies on the root assumption).
- **G4.** At rest: without the key from the Keystore/keyring (A4), the database and group
  files are unreadable.
- **G5.** Group membership on *add* is bound to the peer's authenticated identity (a
  KeyPackage cannot be substituted, `group/src/lib.rs::add`).
- **G6 (M5, anti-traffic-analysis).** An on-path observer (A1) cannot tell message classes
  apart **by datagram size**: text, image, file, voice and group messages have the same
  size (4 KiB bucket); a call start (`RoomInvite`) is indistinguishable from an
  ack/ping/presence beacon (256 B bucket); the short last chunk of a blob is padded to a
  full one, so the exact file size does not leak through the remainder. Implementation:
  `core/src/pad.rs` + `bucket_for` in `core/src/protocol.rs`; test
  `text_media_and_call_are_indistinguishable_by_size`. Session handshake packets have the
  same sizes (msg1/msg2 = an APP packet, msg3 = a CONTROL packet, random padding).
  **Limits of G6 — see N7.**
- **G7 (ADR-012).** 1:1 messages have forward secrecy per session: session keys come from
  fresh ephemeral X25519 **and** ML-KEM-768 keys, live in memory only and are replaced
  every 2 minutes. Recording traffic today and later stealing the device, or later
  breaking X25519 with a quantum computer, does not reveal past sessions. Neither identity
  travels in the clear; a stranger who learns our new address cannot make us answer
  without already knowing our identity (`mac1`).

## 5. What we do NOT promise (explicit non-goals of v1.0)

These are deliberate limitations of the current version, not bugs. Users should
understand them before trusting the system with sensitive conversations.

- **N1. 1:1 forward secrecy is per session, not per message, and authentication is not
  post-quantum.** (SEC-3 itself is resolved by ADR-012, see G7.) Whoever compromises a
  device *during* a session reads that session's traffic — a window of up to 3 minutes —
  but not earlier sessions. Signatures and the handshake's static keys are classical: a
  future quantum attacker could impersonate, not decrypt the past (ADR-011). A handshake
  is recognisable by its structure (no AEAD counter, X25519 keys are not uniform random) —
  G6 hides its size, not its existence.
- **~~N2. No member revocation and no post-compromise recovery in groups.~~**
  **RESOLVED** (SEC-1, [ADR-004](../adr/ADR-004-mls-membership-revocation.md), implemented
  2026-07-02, tested on devices 2026-07-03): the admin sequencer performs Remove
  (revocation), self-Update (PCS, a rolling rotation once a day) and resync of a lagging
  member (Remove + Add). Covered by `engine/tests/group_flow.rs` (kick/leave, including
  under packet loss).
- **N3. Metadata is not hidden.** Presence beacons and keepalives (every ~15 s) reveal the
  social graph and activity patterns to A1. Yggdrasil routes by public keys: source and
  destination are visible on the path. There is no anonymisation (onion/mixnet).
  Anyone who knows our identity can learn whether we are online: we answer their 1:1
  handshake (ADR-012), as we must for a contact request to arrive at all — the same as a
  stranger's message getting an Ack before. Knowing only our address is not enough
  (`mac1`).
- **N4. A relay's operator sees relay metadata.** If `ntfy.sh` is used (A5), its operator
  sees the topic name, size and time of messages (not the content).
- **N5. No protection from a malicious member inside a group.** A3 reads all group content
  within their epochs — a property of any group, not a defect.
- **N6. Call media is bound to membership by the gate, not by cryptography.** SEC-2 is
  implemented (`Engine::sync_room_members`, `room_peer_allowed`): a GROUP call admits only
  peers from the current MLS roster (an empty or not yet cached roster admits nobody —
  fail-closed since the 2026-10 review), a removed member drops out on Remove, and a DM
  room admits only its one contact (the room id is checked against `dm_room_id`). The
  recipient list of our voice is built only by this filter, from the first frame on.
  Residual limitation: media itself is not encrypted to the members (SEC-4), so the
  guarantee holds only as far as the transport does.
- **N7. Padding (G6) hides SIZE, but not timing, volume or channel.** What A1 still sees
  despite the buckets:
  * **timing and rate** — a conversation is recognisable by its packet rhythm; constant
    cover traffic is deliberately NOT done in 1.0 (mobile battery);
  * **the channel byte** — the first byte of a datagram openly distinguishes `CHAN_CHAT`
    from `CHAN_MEDIA`, so **the fact of a call is visible** regardless of the padding of
    `RoomInvite`. A conscious trade-off: demultiplexing and the ingress budget
    (anti-abuse, M1/M3) must work BEFORE decryption, otherwise a flood makes the victim
    spend crypto on every packet. Removing the byte would give up cheap flood protection
    for protection that timing analysis bypasses anyway (a call is ~50 packets/s);
  * **volume** — the number and total size of packets reveal "sent a large file"; padding
    hides only the remainder of the last chunk, not the number of chunks;
  * **the bucket as a class** — the bucket itself (256 B vs 4 KiB vs 25 KiB) separates
    control / application / blob body. Full uniformity at one size was rejected: padding
    a presence beacon (every 2 s) to 4 KiB is ×40 traffic and battery.
- **N8. Incoming traffic cannot be cut at the application layer.** The ingress budget
  (`p2p_core::budget::Tier`) drops packets of a blocked or flooding peer BEFORE
  decryption, so no CPU, battery or storage is spent. But **the bytes have already reached
  the radio** — the mobile data is already paid for. This is a property of any open
  addressable network: whoever knows the address can send (like mail reaching a server
  despite the spam filter), and in a serverless model there is no upstream to filter
  before us. There is also nothing to cut the link with: libygg has `ygg_add_peer` but no
  `remove_peer`, and even that would help only if the flooder were our DIRECT peer, not a
  node behind mesh routing.

  Two scenarios lead here: **a malicious contact** (a modified client sending a stream
  non-stop) and **an address leaked by carelessness** (a contact disclosed the address to
  a third party — the flooder is not even in the contact list).

  **Address rotation stops a flood — but ONLY with an offline pause** (2026-08-01, tested
  on the line `flood_rotation.rs` and on the real public mesh `flood_pubmesh.rs`, analysis
  in [ADR-007](../adr/ADR-007-identity-transport-key-split.md)). Two modes:
  * **Rotation "on the fly" (reconnecting within seconds) does NOT help.** Routing
    information about the old key is still in the tree, the new node inherits its
    position and keeps receiving the stream: on the real mesh it receives **88–89 %** of
    the injected traffic (control "flood off" = 0 KB background). Different public peers
    do not help — the flood follows the destination key.
  * **Rotation + going offline for ~30 s or longer STOPS the stream.** The old key drops
    out of the routing tree and the flooder physically stops sending: its output falls
    from full volume to ~0 (`write()` goes nowhere). This is a property of the protocol:
    **Yggdrasil cannot route to a key that is not in the network**, and the attacker's
    modified client cannot get around it. It works even against an already established
    session of a malicious contact.

  Conditions: an offline pause + the new key is not given to the source of the flood
  (per-contact ACL). The cost is a minute offline and re-sending the address to contacts;
  under a persistent attack, round after round. Application-level delivery of the
  foreign stream is already cut by `Tier::Blocked` before decryption; rotation is needed
  precisely to stop the traffic itself.

  **What remains of rotation:** a leaked address stops being a way to REACH us, and with
  separate identity and transport keys this does not cost contacts. Today rotation costs
  the identity: one ed25519 seed is at once the address, the fingerprint, the MLS
  credential and the contact record, so changing the address resets all verifications and
  breaks membership in all groups. Hence the plan:
  [ADR-007](../adr/ADR-007-identity-transport-key-split.md) separates the **long-term
  identity (IK)** from the **transport key (TK = address)**. Then rotation is a 200-byte
  announcement signed by the IK: fingerprint, verifications and groups survive, and **the
  user chooses the list of recipients** — Bob leaked it, we rotate and do not tell Bob.
  This contains the damage of a leaked address — but, again, it is about access, not
  about traffic.

  A separate **address per contact** (N addresses = N keys) was considered and rejected in
  its original form: there are plenty of addresses, but N addresses = N Yggdrasil nodes on
  one device (×N keepalives and wake-ups), and the bytes still reach the same radio —
  analysis in ADR-007.

  Residual limitations of rotation: a contact absent for longer than the grace period
  loses the connection; a flooding contact left in the recipient list receives the new
  address along with everyone else.

  **Against traffic with the device's own means (no upstream) there are two moves:** go
  offline (drop the peerings — stop paying right away, but connectivity is gone too) and
  change the address with an offline pause (stop the stream for good, without losing
  contacts). The app must show the flood honestly and offer both. **A personal relay** (a
  cooperative node in front of the device) would cut the stream without going offline —
  but that is exactly the upstream the project exists to avoid, so **1.0 has no relay**
  (owner's decision, 2026-08-01); it is recorded in ADR-007 as post-1.0. 1.0 does not need
  it: rotation + an offline pause is a working way out.

  Everything else lowers the cost of a flood, not its volume: drop by source key inside
  libygg (before FFI), no per-packet allocations, the ingress budget before decryption,
  a guard on mobile data.

  **A side finding of the experiment (not a guarantee!).** A continuous sender delivers
  NOT ONE application packet to a silent receiver: tested for 200 s at 5 packets/s, zero
  delivered (`transport/examples/silent_victim.rs`). The cause is in ironwood: a sender
  without a session sends not the payload but `sessionInit` (~200 B; measured 250 B per
  attempt instead of 500 B of payload); the receiver absorbs the first init without an ack
  (`init.seq <= info.seq` right after the session is created), and the sender's buffer
  timer is reset by every write, so the init keeps the same timestamp forever. It is
  broken either by a reply from the receiving application or by the sender pausing for
  more than a minute. Result: the contact gate is propped up by the transport, but **this
  does not affect the traffic volume** (the bytes arrive), and it is an **implementation
  detail** — there is a TODO in `session.go` about exactly this spot. Re-check on every
  ironwood update; nothing may be built on it.

- **N9. A client does not prove that it does transit work.** The code is open (GPL-3.0)
  and runs on someone else's device, so any self-report from a client ("I relay
  traffic") is a claim, not proof: relaying can be cut out to save battery while the flag
  stays on. In 1.0 this is harmless **only because there are no privileges for transit**:
  `budget::Tier` tiers are assigned by trust (contact/verified), queue priority and TTL
  too, and there is nothing to lie about yet. As soon as there is a single privilege for
  transit, the flag becomes a free reward for the cheater at the expense of the honest.
  The decision is made in advance — [ADR-008](../adr/ADR-008-proof-of-transit.md):
  privileges are granted only against receipts signed by the recipient (someone else's
  signature cannot be forged, so a receipt cannot be obtained without delivering the
  packet). Implementation is post-1.0; 1.0 only reserves a field in the presence beacon,
  because later it would not fit into the CONTROL bucket of 256 B (N7) without losing
  size indistinguishability.

- **N10. A data backup is an asset that lives OFF the device.** A `.p2pbak` file by its
  nature goes to the cloud, to an SD card, to someone else's computer, and is protected
  only by a password (Argon2id + XChaCha20-Poly1305; the Keystore key is not available
  there and would not help — the backup has to open on another device). Honestly, that
  means:
  * **whoever finds the file gets nothing without the password** — and we give no hints
    either: no date, no contents in the plaintext header (only magic, version, KDF
    profile, salt);
  * **a weak password = a weak backup.** Guessing happens offline, without our limits;
    the only protection is the cost of an attempt (~1 s on a phone);
  * **the file contains neither the identity key nor MLS group state** — a leaked backup
    means "they read the conversations", not "they became you" or "they keep reading your
    groups";
  * **the reverse direction is dangerous too:** a file can be PLANTED ("here is your
    backup, here is the password"). So restoring does not carry over the verification
    flag: "verified" means "the fingerprint was compared in person", and such a claim is
    not restored from a file (owner's decision, 2026-08-01). Everything else is merged
    rather than overwritten: local records newer than the archive win.
  * **the KDF parameters are recorded in every file** (profile number). Otherwise raising
    the cost in a future version would make old backups unreadable with a "wrong
    password" message — and a person would delete their data thinking they mistyped.
- **N11. A quantum adversary is outside the v1.0 model.** All asymmetric crypto is
  classical (Ed25519, X25519 — in our packets, in MLS and in the Yggdrasil transport). The
  symmetric part (XChaCha20-Poly1305, AES-256 in the database) holds. The practical threat
  is not "broken tomorrow" but **"harvest now, decrypt later"**: traffic intercepted today
  is kept until a cryptographically relevant quantum computer exists. Order of work and
  what depends on upstream (`openmls`, Yggdrasil) — [ADR-011](../adr/ADR-011-post-quantum.md);
  1.0 closes it with a hybrid KEM for personal messages, signatures come with identity 2.0
  ([ADR-010](../adr/ADR-010-identity-v2-multidevice.md)).
- **N12. One identity — one device.** A key cannot be copied to a second phone (two devices
  under one MLS leaf break the group for all members), and a data backup brings back the
  history but not the membership. The solution is recorded in
  [ADR-010](../adr/ADR-010-identity-v2-multidevice.md) and will be implemented after 1.0.

## 6. Hardening priorities (security roadmap)

In order of return on risk (details in the [audit document](security-audit-2026-07-02.md)):

1. ✅ **SEC-1** — MLS Remove + periodic self-Update → revocation and PCS
   ([ADR-004](../adr/ADR-004-mls-membership-revocation.md)). Done 2026-07-02/03.
2. ✅ **SEC-2** — call membership from the MLS roster. Done (fail-open, see N6).
3. ✅ **SEC-3** — 1:1 sessions with a hybrid post-quantum handshake and rekeying
   ([ADR-012](../adr/ADR-012-dm-session-v2.md)). Done 2026-10-25.
4. **SEC-5** — domain separation of keys (a separate MLS signing key). The derivation
   primitive and the policy for new keys are done.
5. **SEC-4** — encryption parity for media (a key from the MLS/Box exporter); metadata in the UI.
6. **N8, flooding** — after the 2026-08-01 experiments (ADR-007) the order is:
   (a) cheap hardening of the receive path + an honest flood UX: "go offline" and "change
   address" buttons; (b) separation of identity and transport keys — both containment of
   a leaked address AND a working way to stop a flood (rotation + an offline pause, the
   old key drops out of the tree). A relay (post-1.0, not planned) would cut the stream
   without going offline, but we do not bring an upstream into a serverless design.
7. **N11, post-quantum** ([ADR-011](../adr/ADR-011-post-quantum.md)): a hybrid KEM
   (X25519 + ML-KEM-768) for personal messages and the 256-bit MLS suite — in 1.0;
   signatures (ML-DSA) together with identity 2.0, otherwise the format breaks twice. MLS
   and the transport stay classical until upstream moves, and we say so plainly.
8. **N12, multi-device** ([ADR-010](../adr/ADR-010-identity-v2-multidevice.md)): the root
   signs the list of devices, each device is its own leaf in MLS. After 1.0.
9. **N9, transit incentives** (post-1.0, [ADR-008](../adr/ADR-008-proof-of-transit.md)):
   proof-of-transit receipts. To be done together with the first privilege for transit —
   earlier there is nothing to distribute, later is too late (the flag would already be
   the norm). The place in the protocol is reserved in 1.0.

---
Revised whenever the crypto layer, the transport or the membership model changes.
