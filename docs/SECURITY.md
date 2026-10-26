# Security

This page summarises the security model, the audits done so far and the status of every
finding. The detailed documents:

- [architecture/threat-model.md](architecture/threat-model.md) — assets, adversaries,
  trust boundaries, what we guarantee (G1–G6) and what we explicitly do not (N1–N12);
- [architecture/security-audit-2026-07-02.md](architecture/security-audit-2026-07-02.md) —
  the second, protocol-level pass (MLS membership, call membership, FS layering);
- [plans/security-hardening.md](plans/security-hardening.md) — the hardening plan.

All audits so far are internal. There has been no external audit.

## Security goals

- **End-to-end confidentiality.** Message content and media are readable only by the
  participants. The transport (Yggdrasil/ironwood) is not trusted; it only routes
  datagrams addressed by ed25519 key.
- **Forward secrecy and post-compromise security.** In groups this comes from MLS. For
  1:1 messages it comes from our own session layer (ADR-012): fresh ephemeral keys every
  2 minutes, kept in memory only.
- **Post-quantum confidentiality on the wire.** The 1:1 key exchange is hybrid
  (X25519 + ML-KEM-768): recorded traffic stays secret unless both are broken. All group
  (MLS) traffic and call media ride these sessions too; MLS itself stays on its classical
  128-bit suite ([ADR-013](adr/ADR-013-mls-ciphersuite.md)). Authentication is classical
  (ADR-011).
- **Protection from a man in the middle at first contact.** Users can compare a visual
  fingerprint out of band (in person or by QR) and mark the contact verified.
- **At rest.** A seized or stolen device does not give away the identity key or the
  history without the key from the Android Keystore and/or the user's password.
- **Group integrity (MLS).** Only members read group messages; membership is bound
  to authenticated identities, and removed members lose access to new messages.

Out of scope: hiding who talks to whom (Yggdrasil routes by public key, and presence
reveals activity), a compromised operating system, physical access to an unlocked device.

## Cryptographic building blocks

- **Identity:** ed25519. Yggdrasil address = `AddrForKey(pub)`. Every other key is a
  domain-separated sub-key of the seed (`Identity::derive_subkey`, BLAKE3).
- **1:1:** sessions from a `Noise_XXhfs_25519+MLKEM768_ChaChaPoly_BLAKE2s` handshake
  ([ADR-012](adr/ADR-012-dm-session-v2.md)): own Noise state machine checked byte for byte
  against `snow` for classic XX, ML-KEM from `libcrux-ml-kem` (formally verified),
  ChaCha20-Poly1305 with a counter nonce and a replay window. The static key is separate
  from the identity and signed by it; the identity travels only encrypted. Rekey every
  2 minutes.
- **Groups:** MLS (RFC 9420, openmls 0.8), `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`
  ([ADR-013](adr/ADR-013-mls-ciphersuite.md)) — TreeKEM, FS/PCS, signatures bound to ed25519.
- **Transport:** the ironwood `iwe` session layer (ratchet, FS, anti-replay) over the mesh.
- **Wire padding:** every payload is padded to one of three size buckets (256 B / 4 KiB /
  25 KiB), so message classes are indistinguishable by size (threat model G6/N7).
- **At rest:** XChaCha20-Poly1305 for files, SQLCipher for the database. On Android the
  key is wrapped by the Keystore, derived from a password with Argon2id, or both. State
  files are written crash-safely (temp → fsync → rename); key and seed files are 0600.

## First audit (core and desktop, before the Android client)

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| H1 | High | Adding a member to an MLS group did not check that the identity and signature key in the KeyPackage belong to the expected peer (member substitution). | **Fixed** — `Member::add` compares both with `expected_identity`, otherwise `IdentityMismatch`. |
| H2 | High | Forward secrecy / PCS for 1:1. | **Delegated to the transport** (ironwood `iwe` ratchet, checked against its source). The second audit reopened this as SEC-3: FS for 1:1 depends on one external component. |
| M1 | Medium | History, profile and groups were stored in plaintext. | **Fixed** — at-rest encryption (`core/src/vault.rs`); every save/load goes through `vault::seal/open`. |
| M2 | Medium | Calls accepted media packets from any source. | **Fixed** — calls filter by the allowed peer / participant list. |
| M3 | Medium | No replay protection at the messenger layer. | **Fixed** — a dedup ring (4096 entries) in the receive loop. |
| L1 | Low | Parsing a group blob could read past the buffer. | **Fixed** — bounds check `4 + glen > data.len()`. |
| L2 | Low | Secret files were written without restricting permissions. | **Fixed** — 0600 on unix. The write was not actually atomic until 2026-09-25; now it is (`core::fsutil`). |
| L3 | Low | `unsafe` in the FFI transport. | **Accepted** — all `unsafe` is contained in `transport/src/lib.rs` behind a safe API; lengths are clamped on the Go side. |
| L4 | Low | No indication that a contact's fingerprint was verified. | **Fixed** — a `verified` flag, shown in the UI. |
| INFO | Info | No automated dependency scan. | **Added** — `deny.toml` + `scripts/audit-deps.sh` (cargo-deny). |
| DEP1 | High | hpke-rs 0.2 under MLS: nonce reuse, all-zero X25519 shared secret, KDF length bug, panic (RUSTSEC-2026-0069/0070/0071/0072). | **Fixed** — openmls 0.6 → 0.8, hpke-rs 0.6.1 (libcrux backend). |
| DEP2 | High | openh264-sys2 0.6: heap overflow while decoding untrusted video (RUSTSEC-2025-0008). | **Fixed** — openh264 0.9 (openh264-sys2 0.9.6). Android decodes with the platform MediaCodec. |
| DEP3 | Info | Unmaintained: `audiopus_sys`, `paste`, `proc-macro-error2`. | **Accepted** — not vulnerabilities; no safe upgrade path (deep transitive); listed in `deny.toml` with a reason. |

## Second audit (2026-07-02, protocol level)

| ID | Severity | Finding | Status |
|----|----------|---------|--------|
| SEC-1 | High | No MLS Remove/Update: nobody could be revoked, and PCS never recovered. | **Fixed** ([ADR-004](adr/ADR-004-mls-membership-revocation.md)): Remove, daily self-update, resync of lagging members. Tested on devices. |
| SEC-2 | Medium | Group call media was not bound to the MLS roster. | **Fixed** — only roster members are admitted. Fails open while the roster is not cached (N6). |
| SEC-3 | Medium | FS for 1:1 relies only on the transport ratchet; the application layer is a static Box. | **Fixed** ([ADR-012](adr/ADR-012-dm-session-v2.md), 2026-10-25): 1:1 sessions with ephemeral hybrid (X25519 + ML-KEM-768) keys, in memory only, replaced every 2 minutes. FS is per session (minutes), not per message. |
| SEC-4 | Medium | Call media confidentiality relies only on the transport. | **Fixed** (2026-10-26) — every media frame is sealed with the media key of the 1:1 session to its recipient (ChaCha20-Poly1305, own counter and replay window); group calls are a full mesh of pairs, so the same holds there and each frame is authenticated to its sender. Hybrid post-quantum like the session. |
| SEC-5 | Medium | One key reused across protocols. | **Mitigated** — domain-separated derivation for all new keys ([invariants §6](architecture/invariants.md)); the identity/transport split ([ADR-007](adr/ADR-007-identity-transport-key-split.md)); the identity-converted Box key is gone — 1:1 uses a separate signed static key (ADR-012). The identity still signs both the transport (epoch 0) and MLS credentials. |
| SEC-L1…L4 | Low | Bounded replay ring; unauthenticated envelope time; plaintext fallback on the desktop when the vault is unavailable; a wipe footgun on vault mismatch. | Open, low. |

## Surface added after the audits (not yet re-audited)

- **Room presence.** Beacons travel on the reliable, encrypted channel and the source is
  checked against the envelope, but their content (flags, room id) is not separately
  signed, and there is no per-source rate limit on beacons. Presence reveals who is in a
  room and when — by design, and outside the threat model.
- **JNI bridge.** All input from Kotlin is parsed in Rust; bad hex keys become `None`.
  Every entry point is wrapped in a panic guard, enforced by a unit test. The engine
  lives behind a session pointer; there is no global mutable state in the bridge.
- **Hardware H.264 (MediaCodec).** Vendor decoders may have their own vulnerabilities
  (especially on some low-cost chipsets). The decoder parses untrusted video from peers;
  Android's media sandboxing limits the blast radius.
- **Blobs.** Chunks are verified with BLAKE3 one by one and as a whole; size and chunk
  count are limited; downloads above a threshold need the user's consent. Property tests
  cover the parsers; `cargo-fuzz` targets cover wire, media, blob manifest, scrub,
  session and mentions (`scripts/fuzz.sh`). Kani proves for all bounded inputs that
  `unpad` never panics and inverts `pad`, that the session replay window accepts each
  packet number at most once and never rejects a fresh one in the window, and that an
  accepted mention span is safe to slice (`scripts/kani.sh`, see plans/beta.md step 6).

## Dependency scan (cargo-deny)

`cargo deny check` (advisories, bans, licenses, sources) passes. Real advisories (DEP1,
DEP2) were fixed by upgrades; the remaining `unmaintained` warnings are documented as
accepted. The project license is **GPL-3.0-only**; the allowed dependency licenses are
listed in `deny.toml`.

## Running the checks

```bash
scripts/test-all.sh             # the whole Rust suite (Linux/WSL)
bash scripts/audit-deps.sh      # dependency vulnerabilities and licenses (cargo-deny)
```

## Reporting a vulnerability

Please report privately through GitHub's "Report a vulnerability" (Security tab of the
repository) rather than opening a public issue, and allow time for a fix before
disclosure.
