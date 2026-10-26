# ADR-012: 1:1 session v2 — an interactive hybrid post-quantum handshake with rekeying

**Status:** Accepted (2026-10-25). Implemented 2026-10-25: `core/src/noise.rs`,
`core/src/session.rs`, wired into `messenger` (wire format in
[protocol-spec §2.1](../protocol-spec.md)). Open from the verification plan: the ProVerif
model (item 5).
**Date:** 2026-10-25
**Decided by:** project owner — approach A, rekey every 2 minutes, a separate static key
signed by the identity (which turns the pattern from IK into XX, see below); the ML-KEM
crate was left to the implementer (`libcrux-ml-kem`).
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
| **A** | **Interactive Noise handshake + hybrid KEM (HFS), WireGuard-style rekeying** | per session (minutes) | yes (KEM in handshake) | none | handshake ≈ one Noise pattern | **chosen** |
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

## Decision

### Handshake: `Noise_XXhfs_25519+MLKEM768_ChaChaPoly_BLAKE2s` + `mac1`

- **Static keys are separate from the identity (closes SEC-5 for 1:1).** Each side has a
  static X25519 key derived from its seed with its own domain label
  (`peerage:session-static:v1`) and **signed by the Ed25519 identity**. The signature
  travels in the handshake payload next to the identity key; the receiver checks it and
  that the identity is the one it expects. Precedent: libp2p's Noise handshake signs its
  static key with the peer's identity key the same way.
- **XX, not IK.** IK requires the initiator to know the responder's static key in advance,
  and a separate static key cannot be derived from the identity — that is its point. In XX
  the statics are exchanged inside the handshake.
- **Hybrid (HFS tokens):** message 1 carries an ephemeral ML-KEM-768 encapsulation key,
  message 2 the KEM ciphertext; the KEM secret is mixed into the chaining key together
  with the X25519 results. Breaking a session needs breaking **both** X25519 and ML-KEM.
  Authentication stays classical — ADR-011: a signature is forged at the moment of an
  attack, encryption is broken retroactively; KEM first.

```
  -> e, e1                        + mac1   (e1 = ephemeral ML-KEM-768 encapsulation key)
  <- e, ee, ekem1, s, es          payload: responder identity + signature over its static
  -> s, se                        payload: initiator identity + signature, then data
```

- **Probe resistance (`mac1`, as in WireGuard).** In XX the responder reveals its identity
  in message 2 — so anyone who learned a new address could ask "whose is it?", which would
  undo address rotation (ADR-007). Message 1 therefore carries
  `mac1 = MAC(HASH("mac1" || responder_identity_pub), message1)`: only someone who already
  knows our identity can produce it, and without it we stay silent.
- **Round trips:** the initiator's first data rides in message 3 — one round trip before
  the first message on a cold session, as with IK without 0-RTT data. Nothing on a warm
  session.
- **Both identities are encrypted** inside the handshake. Transport messages carry only a
  random session index — **the identity no longer travels in clear.**
- Sizes: message 1 ≈ 32 + 1 184 + 16 ≈ 1.2 KB, message 2 ≈ 32 + 1 104 + 48 + 112 ≈ 1.3 KB,
  message 3 ≈ 48 + 112 + data. All padded to the APP bucket (4 KiB) — the M5
  size-indistinguishability test must cover them.

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

1. **Classical core against a reference:** the Noise state machine (MixKey/MixHash/
   Split) runs `Noise_XX_25519_ChaChaPoly_BLAKE2s` against `snow` with fixed keys and
   must produce byte-identical messages before the HFS tokens are added.
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

## Decisions taken (2026-10-25)

1. **Approach A.**
2. **Rekey every 2 minutes** (WireGuard's value).
3. **ML-KEM: `libcrux-ml-kem`** — formally verified (hax/F*); it is pre-1.0, and the
   hybrid keeps X25519 as the safety net if the young implementation turns out wrong.
4. **Noise: our own minimal implementation of exactly this pattern.** `snow` has no
   ML-KEM; it is used in tests only, as a reference for the classical XX part (same
   inputs → byte-identical messages).
5. **A separate static key signed by the identity** → pattern XX + `mac1` (above).
