# ADR-013: MLS ciphersuite — stay on the 128-bit X25519 suite, groups get PQ from the 1:1 sessions

**Status:** Accepted (2026-10-26). Revises [ADR-011](ADR-011-post-quantum.md) item 2.
**Date:** 2026-10-26
**Decided by:** implementer, on the measurements below; the owner may reverse it before the
format freeze (beta plan step 8) — the switch itself is small, see "If we reverse".
**Related:** [ADR-011](ADR-011-post-quantum.md) (post-quantum transition),
[ADR-012](ADR-012-dm-session-v2.md) (1:1 session v2), [plans/beta.md](../plans/beta.md) (step 4),
[threat model](../architecture/threat-model.md) (N11).

## Context

ADR-011 item 2 planned "move MLS from `MLS_128_*` to the 256-bit suite — a one-day change".
Checking it against the crates turned out differently:

- `openmls_rust_crypto` (our provider) implements only the 128-bit suites.
- The 256-bit suites of RFC 9420 (`MLS_256_DHKEMX448_*_Ed448`, `MLS_256_DHKEMP521_*_P521`)
  change the **signature** scheme. Our identity *is* an Ed25519 key (address, fingerprint,
  MLS credential), so they mean a new identity format — that is ADR-010 territory.
- The only 256-bit suite that keeps Ed25519 is `MLS_256_XWING_CHACHA20POLY1305_SHA256_Ed25519`
  (codepoint `0x004D`, from the IETF draft on PQ ciphersuites), provided by
  `openmls_libcrux_crypto` 0.3 (openmls feature `libcrux-provider`). It is hybrid
  post-quantum (X25519 + ML-KEM-768), so it would give groups more than "symmetric
  strength" — but at a price we measured.

## Measurement (openmls 0.8.1, ratchet-tree extension on, as in `group/src/lib.rs`)

Group grown one member at a time; sizes in bytes of the serialized MLS message.
"UpdAdmin" — the admin's self-Update (the daily PCS rotation of ADR-004), "UpdLast" — the
newest member's self-Update.

| N | KeyPackage | Add commit | Welcome | UpdAdmin | App msg (100 B) |
|---:|---:|---:|---:|---:|---:|
| **X25519, 128 (now)** | | | | | |
| 2 | 278 | 687 | 791 | 489 | 246 |
| 10 | 278 | 1 450 | 2 411 | 1 250 | 246 |
| 20 | 278 | 2 305 | 4 249 | 2 105 | 246 |
| 40 | 278 | 3 980 | 7 857 | 3 780 | 246 |
| 60 | 278 | 5 620 | 11 397 | 5 420 | 246 |
| **X-Wing, 256 (libcrux)** | | | | | |
| 2 | 2 648 | 5 428 | 5 435 | 3 948 | 246 |
| 10 | 2 648 | 18 459 | 20 098 | 16 978 | 246 |
| 20 | 2 648 | 31 391 | 34 971 | 29 910 | 246 |
| 40 | 2 648 | 56 033 | **63 464** | 54 552 | 246 |
| 60 | 2 648 | 79 455 | **90 704** | 77 974 | 246 |

CPU is not an issue for either (adding a member: 1–11 ms on the dev machine).

What the numbers mean for us:

- **Every control message of the group grows 7–10×.** A commit at 10 members is 18 KB
  instead of 1.5 KB, and it is sent to every member, retried until acked.
- **A Welcome stops fitting into one datagram (~64 KB) at about 40 members**, the admin's
  commit at about 45. We have no fragmentation for `Payload::Group`; it would have to be
  built (most likely through the blob machinery) before X-Wing could ship.
- **The codepoint is a draft.** If the IETF draft or the X-Wing KEM change before
  publication, our groups are tied to a pre-standard format right when we freeze it.

## Decision

**MLS stays on `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519` for the beta and 1.0.**
Post-quantum protection of groups against "harvest now, decrypt later" comes from the layer
below:

- After ADR-012 **every** packet between two peers — including every `Payload::Group`
  (commits, Welcomes, application messages) and every `GroupInvite` — travels inside the
  1:1 session, whose keys come from X25519 **and** ML-KEM-768 and are replaced every
  2 minutes. A passive recorder of the mesh sees only that layer.
- There are no relays or mailboxes: MLS ciphertext never sits with a third party that could
  store it and later attack the classical HPKE inside.
- The symmetric strength question (Grover against AES-128) is answered the same way: the
  outer layer is ChaCha20-Poly1305 with 256-bit keys; AES-128 itself remains NIST category 1.

What this does **not** give, stated plainly:

- **No defence in depth for PQ.** If the 1:1 session layer were broken (it is our own code
  — the main risk of the beta plan, hence fuzzing, the snow cross-check and the planned
  ProVerif model), group messages would be protected only classically. A quantum attacker
  would need *both* that break and the recording.
- A group member's device that is compromised still reveals the group's current epoch —
  that is PCS (ADR-004), unrelated to the suite.

## When to revisit (any one is enough)

1. MLS ciphertext starts to travel beyond a pairwise session: relays, store-and-forward
   mailboxes (channels, ADR-008), multi-device fan-out through another device (ADR-010).
2. The PQ ciphersuite draft is published with a final codepoint, or openmls makes a PQ suite
   its default.
3. Fragmentation of large group messages exists anyway (e.g. for big groups).

Identity 2.0 (ADR-010) breaks the format regardless — that is the natural moment to take
X-Wing together with ML-DSA signatures.

## If we reverse

The switch is mechanical: `openmls` with `libcrux-provider`, `openmls_libcrux_crypto::Provider`
instead of `OpenMlsRustCrypto` (same `MemoryStorage`, so the save/load format of group state
is unchanged), the ciphersuite constant, and `pm clear` on all devices (groups are not
migrated). Before it: fragmentation of `Payload::Group` or a hard member cap of ~30, and the
size buckets rechecked — a commit no longer fits the APP bucket even for two members.
