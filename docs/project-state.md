# interlace — project state

**As of:** 2026-09-25 · **Purpose:** an honest baseline for the NLnet (Restack) proposal —
what exists, how each part was verified, and what is broken or missing. Written to be
read by a reviewer who will open the code afterwards.

---

## 1. What interlace is

A serverless end-to-end encrypted messenger. There is no server of ours anywhere in the
data path: peers talk directly over the [Yggdrasil](https://yggdrasil-network.github.io/)
mesh (an end-to-end encrypted IPv6 overlay routed by public keys). Groups use MLS
(RFC 9420, via `openmls`). The domain logic lives in a Rust engine; the Android client
(Jetpack Compose) only renders state and sends commands. License: GPL-3.0-only.

On Android a foreground service keeps the node online in the background. A client for
push wake-up through ntfy.sh exists as a crate (`ntfy/`) but is not wired into any
client yet, so there is currently no external service involved at all.

## 2. Numbers

| | |
|---|---|
| Active development | 2026-06-13 → 2026-08-24 (≈10 weeks), 387 commits, one maintainer |
| Rust (production code, excl. tests) | ≈17.4k lines in 12 crates (engine 6.0k, core 3.9k, androidjni 2.1k, messenger 1.5k, desktop 1.1k, blob 0.8k, …) |
| Kotlin (Android client) | ≈18.6k lines |
| Go (Yggdrasil wrapper, `libygg.so`) | ≈0.5k lines |
| Rust tests | 236 passing tests: unit, property tests on parsers, and engine-level integration tests over an in-process transport double |
| Android unit tests | 33 |
| Design docs | 10 ADRs, protocol specification, threat model, internal crypto audit (2026-07-02), architecture invariants |

## 3. How it was built (GenAI disclosure)

380 of 387 commits carry a `Co-Authored-By: Claude` trailer: the code was written with
Anthropic's Claude models through Claude Code, under the direction of the maintainer.
The maintainer made the architecture and product decisions (every ADR ends with
"decided by: project owner"), ran all on-device tests, and reviewed the results. Full
session transcripts (prompts, model, timestamps, unedited output) are kept locally and
can be provided as a provenance log per the
[NLnet GenAI policy](https://nlnet.nl/foundation/policies/generativeAI/).

## 4. Architecture in one paragraph

`core` (identity, wire protocol, padding, SQLCipher store, budgets) ← `transport`
(datagrams over `libygg`) ← `messenger` (reliable delivery: retry-until-ack, outbox)
← `group`/`groupchat` (MLS) + `blob` (content-addressed chunks) + `media` (audio/video
framing) ← `engine` (the single Command/Event/Snapshot facade, [ADR-003](adr/ADR-003-engine-api.md))
← `androidjni` (a thin codec; no domain logic). Details: [ARCHITECTURE.md](ARCHITECTURE.md),
[SUBSYSTEMS.md](SUBSYSTEMS.md), [protocol-spec.md](protocol-spec.md).

## 5. Feature inventory and verification level

Verification levels, weakest to strongest:

- **emu** — checked by hand on the Android emulator only;
- **loop** — automated engine-level test over the in-process transport double (packet
  loss and restarts are simulated where noted);
- **mesh** — two headless nodes on the public Yggdrasil mesh (`scripts/live-two-node.sh`);
- **device** — a real phone (Redmi Note 8 Pro, Android 11) talking to the emulator over
  the public mesh.

There has been no test with two physical phones, and none on iOS or desktop (there is
no iOS client, and the desktop client does not build — §7).

| Area | Feature | Level |
|---|---|---|
| 1:1 chat | text, delivery status (clock/✓/✓✓), reply, edit, delete for me / for both | device (text, status); loop + emu (edit, delete) |
| | offline delivery: persistent outbox, resend on reconnect, order, dedup | device (2026-07-04) |
| | contact request gate (strangers cannot land in the chat list) | loop + mesh |
| Groups | MLS groups, invite card (join/decline), group name propagation | loop + mesh |
| | kick/leave, self-update (PCS) rotation, resync of a lagging member | loop (incl. packet loss) + device (2026-07-03) |
| | reliable group delivery (retry-until-ack per recipient) | device |
| Media | images, files (up to 64 MiB, resumable chunked pull), voice messages, avatars by hash | device (images/voice), loop (blob flow incl. loss) |
| | EXIF/GPS stripping before sending | unit |
| | full-screen viewer (swipe, zoom, video timeline) | emu |
| Chat store | one message table for DMs, groups and Saved; paged history; forwarding (optionally without author); pins; full-text search (FTS5) | loop + emu |
| Calls | 1:1 and group rooms, audio + H.264 video, jitter buffer, camera switch, call roster bound to the MLS roster | device (phone ↔ emulator, so never more than two participants) |
| Onboarding & keys | key generation, 24-word recovery phrase, visual fingerprint (6×2 shape/colour grid, 96 bits) + QR verification | emu |
| At rest | SQLCipher; key in Android Keystore, or password (Argon2id), or both; app lock | emu + unit |
| Backup | data backup (Argon2id + XChaCha20-Poly1305), selectable contents, merge on restore; group history restored read-only | emu + loop |
| Anti-abuse | per-peer budgets from one tier (`budget::Tier`): ingress rate limit and storage cap, enforced before decryption; local reputation (served blobs → up to ×2 budget) | loop |
| Anti-traffic-analysis | size buckets: text ≡ media on the wire, call start ≡ ack/ping | unit + mesh |
| Address rotation | change own transport address (identity stays), ADR-007 | mesh (experiment) |

## 6. Security posture

What we claim and what we explicitly don't is written down in the
[threat model](architecture/threat-model.md). In short:

**Guaranteed (G1–G6):** content confidentiality and sender authenticity; group
membership bound to authenticated identities; at-rest encryption; message classes
indistinguishable by datagram size.

**Internal crypto audit 2026-07-02** ([report](architecture/security-audit-2026-07-02.md)):

| Finding | Status |
|---|---|
| SEC-1 (High) — no MLS Remove/Update, so no revocation and no PCS | **fixed** ([ADR-004](adr/ADR-004-mls-membership-revocation.md)), tested on device |
| SEC-2 — group call media not bound to the MLS roster | **fixed** (fail-open while the roster is not cached, see N6) |
| SEC-5 — one key reused across protocols | **fixed** for new keys (domain-separated KDF); the identity/transport split itself is designed in ADR-007 |
| SEC-3 — forward secrecy of 1:1 messages relies only on the transport ratchet (Yggdrasil/ironwood); the application layer is a static NaCl box | **open** |
| SEC-4 — call media confidentiality relies only on the transport | **open** |
| SEC-L1…L4 — replay ring bound, unauthenticated envelope time, desktop plaintext fallback, wipe footgun | open, low |

**Explicit non-goals of v1.0:** hiding who talks to whom (presence beacons and
keepalives reveal the social graph to an on-path observer; no onion routing or
cover traffic); timing and volume analysis; quantum adversaries ("harvest now,
decrypt later" is planned, [ADR-011](adr/ADR-011-post-quantum.md)); multiple devices
per identity ([ADR-010](adr/ADR-010-identity-v2-multidevice.md)).

No external audit has been done.

## 7. Known defects and gaps

Code-level:

- **JNI panic guard — fixed 2026-09-25.** Five of 114 entry points (including `start`)
  ran without `jni_safe!`, breaking our own invariant #1. All are guarded now, and a
  unit test fails the build if a new entry point is added without the guard.
- **77 `unwrap()` in production Rust** outside the desktop crate. Most are mutex locks
  (they panic only if another thread panicked while holding the lock); the rest need
  review. The desktop crate has another 62.
- **The desktop client does not compile.** It predates the engine and still calls an
  old call API; `examples/call_signaling` has the same problem. Tests exclude it.
- **No CI.** Tests are run by hand (`scripts/test-all.sh` in WSL).
- **Test suite:** the full run on 2026-09-25 is green — 236 passed, 1 ignored (a test
  that needs the public ntfy server). The same day it had been red: attachments
  below 8 MiB were written with a plain `fs::write`, so a reader could see an empty or
  truncated file, and a crash mid-write left a stub that counted as "downloaded" and
  was never fetched again. Fixed by write-to-temp + rename.
- **Crash-safe state files — fixed 2026-09-25.** The same review found that MLS group
  state, the group index, invites, settings and even the identity file were written by
  truncate-then-write. Android can kill the app between the two steps; that would have
  left a group that can no longer be loaded, or a lost identity. Every state file now
  goes through one helper (`core::fsutil`: temp file → fsync → rename).
- **Padding leak for edits — fixed 2026-09-25.** Message editing was added after the
  anti-traffic-analysis milestone and fell into the 256-byte CONTROL bucket, so an edit's
  size revealed the length of the new text in 256-byte steps. Edits are now in the same
  bucket as text, and the size-indistinguishability test covers them.
- **Fuzzing is partial:** property tests (`proptest`) cover the wire, media and blob
  parsers; there is no `cargo-fuzz` and no Kani harnesses yet.
- **No reproducible build, no F-Droid package.**
- **Some documentation is in Russian.** The README, architecture, protocol
  specification, security documents, invariants and all ADRs are in English; the design
  documents, plans, bug notes, roadmap and the July audit report are still in Russian.

Behaviour:

- Group message ordering depends on the admin acting as sequencer. Ordering without an
  admin (tolerant to partitions) is designed in [ADR-002](adr/ADR-002-group-message-ordering.md)
  but not implemented.
- Calls: in rare cases one side's video stops mid-call (cause unknown, a redial fixes
  it); video latency is ≈2 s on a cold start; the "Invite" button inside a call does
  nothing yet; there is no active-speaker highlight.
- The first message of a stranger is not replayed into the chat after their contact
  request is accepted.
- One identity = one device. A second device cannot be added (N12).

## 8. What is left

**To finish v1.0 (Android):** the remaining everyday chat features (mentions, invite
links, read receipts and typing indicators behind one symmetric switch; message
editing is done); hybrid post-quantum key exchange for 1:1 (ADR-011); closing
SEC-3/SEC-4; the hardening items in §7; reproducible build and F-Droid.

**After v1.0, decided but not started:** multi-device identity and history sync without
a server (ADR-010); a desktop client on the same engine; admin-free group ordering
(ADR-002).
