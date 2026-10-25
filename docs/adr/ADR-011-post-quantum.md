# ADR-011: The post-quantum transition — what, in which order, and what we wait for upstream

**Status:** Accepted (decision recorded 2026-08-03). Item 1 implemented by
[ADR-012](ADR-012-dm-session-v2.md) (2026-10-25); item 2 revised by
[ADR-013](ADR-013-mls-ciphersuite.md) (2026-10-26) — MLS stays 128-bit, groups get PQ from the
1:1 sessions. 1.0 includes a hybrid KEM for personal
messages and raising the symmetric strength of MLS; replacing signatures comes together with
[ADR-010](ADR-010-identity-v2-multidevice.md) (a shared identity format).
**Date:** 2026-08-03
**Decided by:** project owner
**Related:** [ADR-007](ADR-007-identity-transport-key-split.md) (identity vs address),
[ADR-010](ADR-010-identity-v2-multidevice.md), [threat model](../architecture/threat-model.md),
[protocol-spec](../protocol-spec.md) (M5 padding buckets).

## Context: what we have now

| Layer | Algorithms | Survives a CRQC? |
|---|---|---|
| Identity, signatures | Ed25519 | **no** |
| Personal messages | X25519 + XSalsa20-Poly1305 (`crypto_box`) | **no** (key exchange) |
| Groups (MLS RFC 9420) | `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519` | **no** (HPKE on X25519) |
| Transport (Yggdrasil 0.5.13) | X25519 | **no** |
| Data backup | XChaCha20-Poly1305 + Argon2id | yes |
| Database (SQLCipher) | AES-256 | yes |

The symmetric part is fine; **all asymmetric crypto** is vulnerable. The practical threat is
not "it will be broken some day" but **"harvest now, decrypt later"**: traffic intercepted
today is stored until a quantum computer exists. For a messenger that promises privacy for
years ahead, this is the main argument for doing the KEM before signatures — a signature is
forged at the moment of attack, while encryption is broken retroactively.

## Decision (order of work)

**1. A hybrid KEM for personal messages — in 1.0, as a separate piece of work.**
X25519 + ML-KEM-768 following the X-Wing construction: shared secret = KDF(both). Hybrid, not
pure ML-KEM: lattices are younger than elliptic curves, and a flaw in the new scheme must not
wipe out what already works. No upstream is needed, the envelope is versioned, and
`Payload::Unknown` keeps old clients compatible.

**2. Symmetric strength of MLS — in 1.0, cheap.**
Move from `MLS_128_*` to the 256-bit suite. It relates to quantum only indirectly (Grover),
but it is a one-day change and settles the question "why 128 bits".
*Revised by [ADR-013](ADR-013-mls-ciphersuite.md):* it is not a one-day change — the only
256-bit suite that keeps Ed25519 is the draft X-Wing one, 7–10× larger group messages.

**3. Signatures (Ed25519 → ML-DSA) — together with ADR-010, after 1.0.**
Not because it is expensive in itself, but because of sizes: an ML-DSA-44 signature is 2420
bytes against 64. That breaks both the address (32 bytes) and the M5 padding buckets (CONTROL
256 B), i.e. the anti-traffic-analysis we already have. In the ADR-010 model the address is
derived from the DEVICE key, and the root key signs the list of devices — a PQ signature fits
there without breaking addressing. Doing it separately from ADR-010 would break the format
twice.

**An intermediate step, if the effect is needed earlier:** a hybrid signature as a separate
field. Ed25519 stays the address and signs everything, and ML-DSA rides alongside in the APP
bucket for the operations where forgery is most costly: a group invitation, key rotation, an
invite token.

**4. Groups (MLS) and the transport — waiting for upstream.**
PQ ciphersuites for MLS are IETF drafts, and `openmls` does not offer them yet; Yggdrasil has
not changed its transport either. Our own implementation of MLS crypto in a project this size
is not an option. What softens this: our E2E sits ON TOP of the transport, so breaking
Yggdrasil reveals metadata (who talks to whom), not content.

## What we pay

- Size: ML-KEM-768 adds ~1.1 KB to key establishment. It fits into the APP bucket (4 KiB), but
  this must be checked by a size-indistinguishability test — otherwise the PQ exchange becomes
  visible on the wire exactly where we hide everything else.
- Speed: a hybrid computes both exchanges. Unnoticeable on a phone, noticeable in tests.
- Dependence on young crates (ML-KEM): they have seen less auditing than dalek. Hence the
  hybrid — the classical part remains the safety net.
- Part of the picture is not closed by us (MLS, Yggdrasil). The proposal and the threat model
  must say this plainly, not promise "a fully post-quantum messenger".

## What this decision fixes

- The KEM before signatures — because of "harvest now, decrypt later".
- Hybrid, not pure PQ — at every level where we introduce it.
- Signatures are not touched until the ADR-010 identity format exists.
- We say honestly that MLS and the transport stay classical.
