# ADR-012: 1:1 session v2 — an interactive hybrid post-quantum handshake with rekeying

**Status:** Proposed (2026-10-25) — waiting for the owner's decision on the open questions
at the end. Nothing is implemented yet.
**Date:** 2026-10-25
**Decided by:** project owner (pending)
**Related:** [ADR-011](ADR-011-post-quantum.md) (post-quantum transition, item 1),
[ADR-007](ADR-007-identity-transport-key-split.md) (identity vs address),
[threat model](../architecture/threat-model.md), [security audit](../architecture/security-audit-2026-07-02.md)
(SEC-3, SEC-5), [protocol-spec](../protocol-spec.md) (M5 padding buckets),
[plans/beta.md](../plans/beta.md) (step 3).

## Context

Every 1:1 datagram today is `version || sender_ed25519_pub || nonce || crypto_box(...)`.
The Box key is the static X25519 pair converted from both identities, so **the key for a
pair of people never changes.** That gives us three problems at once:

1. **No forward secrecy in our layer (SEC-3).** Whoever records the traffic and later
   obtains one identity seed decrypts the whole history of that pair. Today we lean on the
   transport's ratchet (ironwood), which is outside our code and is re-verified by no one.
2. **No post-quantum protection (ADR-011, item 1).** X25519 falls to a quantum computer;
   recorded traffic can be decrypted later ("harvest now, decrypt later").
3. **The sender's identity travels in clear in every packet.** After ADR-007 the address
   and the identity are different keys — and the envelope header hands the identity to
   anyone on the path anyway.

### What our system already guarantees (and what that buys us)

- **Delivery happens only while both sides are online.** There is no server holding
  messages: the sender keeps them in its outbox (plaintext payload, SQLCipher at rest) and
  encrypts **at the moment of sending** (`Shared::send`). Retransmissions re-encrypt.
  So every encryption happens when the peer is reachable and can answer.
- This removes the reason asynchronous protocols exist. X3DH/PQXDH, one-time prekeys and
  the Double Ratchet's ability to encrypt to an offline peer solve the "recipient is
  offline, a server holds the message" problem. **We do not have that problem.** We can
  run an interactive handshake with fresh ephemeral keys, as WireGuard and TLS do.
- Session keys may live **in memory only**. Android kills the process at any moment; with
  in-memory sessions a restart simply means a new handshake. A Double Ratchet's state, on
  the contrary, must be persisted after every message, atomically, and a lost or stale
  state breaks the conversation.

## Options considered

| | Option | FS | PQ | New state at rest | Our own crypto code | Verdict |
|---|---|---|---|---|---|---|
| **A** | **Interactive Noise IK + hybrid KEM (HFS), WireGuard-style rekeying** | per session (minutes) | yes (KEM in handshake) | none | handshake ≈ one Noise pattern | **recommended** |
| B | `vodozemac` (Matrix Olm: 3DH + Double Ratchet) | per message | **no** — Olm has no PQ; would need our own wrapper around it | ratchet state per pair, every message | wrapper + glue | contradicts the PQ goal |
| C | Our own Double Ratchet + PQXDH-style start | per message | yes | ratchet state per pair, every message | the most: ratchet, skipped keys, PQXDH | largest risk, solves a problem we don't have |
| D | 1:1 as a two-member MLS group | per epoch | **no** until openmls ships a PQ ciphersuite | MLS state per pair | little | revisit when openmls has X-Wing |
| E | `libsignal` (PQXDH + SPQR) | per message | yes | ratchet state | none | AGPL, git-only, very heavy; async design we don't need |

**Why A.** It fits the delivery model exactly (both online), adds no state that can be lost
on a process kill, and is the smallest amount of new crypto code: one fixed Noise pattern
plus one KEM. Classical Noise patterns are formally analysed (Noise Explorer models),
KEM-based Noise was studied in the Post-Quantum Noise work (Angel et al., 2022), and
WireGuard — the same design family — has machine-checked proofs. The exact hybrid
pattern below is ours to model (verification plan, item 5). **What A does not give:** forward secrecy per
*message*. A compromise of the device during a session exposes that session's traffic (a
window of minutes), not earlier sessions. B/C/E give per-message FS at the price of
persisted ratchet state; with in-memory session keys and a short rekey interval the
practical difference is small, and the failure modes of A are far simpler.

## Decision (proposed)

### Handshake: `Noise_IKhfs_25519+MLKEM768_ChaChaPoly_BLAKE2s`

- **IK:** the initiator already knows the responder's static key — it is derived from the
  peer's identity, which is how a contact is added at all.
- **Static keys:** X25519, converted from the Ed25519 identity — the same key the Box uses
  today, so the number of protocols sharing the key does not grow (SEC-5 stays as it is;
  a signed, domain-separated static key is an open question below).
- **Hybrid (HFS tokens):** the first message carries an ephemeral ML-KEM-768 encapsulation
  key; the second carries the KEM ciphertext; the shared secret is mixed into the chaining
  key together with the X25519 results. Breaking the session needs breaking **both**
  X25519 and ML-KEM. Authentication stays classical — ADR-011: a signature is forged at
  the moment of an attack, encryption is broken retroactively; KEM first.

```
  -> e, es, s, ss, e1          (e1 = ephemeral ML-KEM-768 encapsulation key)
  <- e, ee, se, ekem1          (ekem1 = ML-KEM ciphertext to e1; its secret is mixed in)
```

- **No application data in the first message** (its payload would have weaker
  guarantees). Data flows only after the handshake completes: one extra round trip on a
  cold session, nothing on a warm one.
- **The initiator's identity is encrypted** inside message 1 (`s` token). Transport
  messages carry only a random session index — **the identity no longer travels in clear.**
- Sizes: message 1 ≈ 32 + 48 + 1 184 + 16 ≈ 1.3 KB, message 2 ≈ 32 + 1 088 + 16 ≈ 1.2 KB.
  Both are padded to the APP bucket (4 KiB) — the M5 size-indistinguishability test must
  cover them.

### Session lifetime (WireGuard model)

- A session is created by a handshake and lives **in memory only**.
- **Rekey:** when we send on a session older than `REKEY_AFTER` (default 2 min, open
  question) we start a new handshake; the old session keeps working until the new one is
  ready, and is kept for *receiving* for a short grace period (late and reordered packets).
- A session is **dropped** after `REJECT_AFTER` (e.g. 3 × `REKEY_AFTER`) or on restart.
- **Transport packets:** `type || receiver_index(4) || counter(8) || AEAD`. The counter is
  the nonce; a sliding replay window (as in WireGuard) rejects duplicates and very old
  packets. This replaces today's random 24-byte nonce.
- **Simultaneous initiation** (both sides start at once): both handshakes complete; each
  side sends on the newest session and accepts on any live one — no tie-break state.

### How it plugs into the messenger

- `Shared::send(peer, payload)` asks the session table for a live session. If there is
  none, the payload stays in `pending` (it already does for retries) and a handshake is
  started; when it completes, `pending` for that peer is flushed. The outbox and the
  retry-until-ack logic do not change — they already re-encrypt on every send.
- Pings, Acks, presence beacons, blob chunks and group (MLS) packets all ride on the
  session like any other payload.
- **Strangers:** a handshake from an unknown key is accepted (otherwise the contact
  request could not arrive), but rate-limited by the stranger tier **before** the
  expensive work (X25519 ×3 + ML-KEM encapsulation). A WireGuard-style cookie under load
  is a later hardening.
- **Media (step 4 of the beta plan):** the session exports a media key
  (HKDF over the handshake hash) — that is what SEC-4 for 1:1 calls will use. Group rooms
  take theirs from the MLS exporter.

### What this decision does NOT cover

- Post-quantum **authentication** (ADR-011: together with ADR-010, after 1.0).
- **Groups** stay classical MLS until openmls offers a PQ ciphersuite (ADR-011, item 4);
  group packets are, however, carried inside the PQ session between each pair of members.
- The transport (Yggdrasil) stays classical — breaking it reveals who talks to whom, not
  the content.

## Verification plan (part of the work, not after it)

1. **Classical core against official vectors:** the Noise state machine (MixKey/MixHash/
   Split) is checked against the published Noise test vectors for `Noise_IK_25519_ChaChaPoly_BLAKE2s`
   before the HFS tokens are added.
2. **Primitives from verified/audited crates:** ML-KEM from `libcrux-ml-kem` (formally
   verified with hax/F*, pre-1.0) or RustCrypto `ml-kem` (open question); X25519 and
   ChaCha20-Poly1305 from the crates we already use.
3. **Fuzz targets** for every new parser (handshake messages, transport header) — the rule
   from step 2.
4. **The whole loopback suite** (35 test binaries) runs over the new session layer
   unchanged — plus new tests: handshake under loss, restart of either side mid-session,
   simultaneous initiation, rekey during a burst, replay rejection, stranger rate limit.
5. **ProVerif model** of exactly this pattern (secrecy of session keys against an attacker
   who later learns the static keys, and with X25519 broken; mutual authentication) —
   the composition is ours, so it gets a model.

## Migration

The wire format breaks (envelope v1 is removed — no backward compatibility before the beta,
see the compatibility policy). **Stored data does not change** (the outbox holds payloads,
not ciphertext), so no `pm clear` is needed; both devices must simply run the new build.

## Open questions for the owner

1. **Approach:** A (recommended) vs B/C (per-message FS with persisted ratchet state).
2. **Rekey interval:** 2 min (WireGuard; more handshakes, ~2.5 KB each per active pair)
   or 5–10 min (fewer handshakes, a wider window if a device is compromised mid-session).
3. **ML-KEM crate:** `libcrux-ml-kem` (formally verified, pre-1.0, maintainers ask to be
   contacted for production) vs RustCrypto `ml-kem` (pure Rust, widely used, not
   independently audited). The hybrid keeps X25519 as the safety net either way.
4. **Noise implementation:** our own minimal implementation of this one pattern (≈300
   lines, checked against official vectors) vs `snow` — which, as far as I could find, has
   no ML-KEM support (only an old Kyber draft behind its HFS feature).
5. **Static key:** keep the converted identity key (as today) or a domain-separated
   static key signed by the identity (closes SEC-5 fully, costs a signature and 64 bytes
   in message 1).
