# Subsystem map

The conceptual index of the project. The idea: **features are combinations of a small set
of subsystems (primitives), not separate columns of code.** A new wish is decomposed onto
existing subsystems. If it fits none of them, that is a signal a genuinely new primitive
has appeared (rare, and it needs a discussion).

This map is the common spine of three planes:
- **Code** — crate and module boundaries (where logic lives; UI and JNI are thin adapters).
- **Plan** — the order of work: foundation → features → polish
  ([v1.0-scope](v1.0-scope.md)).
- **Interface** — UI surfaces grow out of subsystems, not the other way round.

Code principle: feature logic is platform-independent and lives in Rust crates (`core`,
`messenger`, `litertc`, `media`, `group`, `groupchat`, `blob`, `engine`). `androidjni` and the
Compose UI are thin adapters.

---

## Subsystems (bottom up)

### F1. Identity and crypto ✅
- **What:** ed25519 identity, the 24-word recovery phrase, 1:1 sessions (hybrid Noise,
  ADR-012), the at-rest
  vault, the visual fingerprint. Plus **an address separate from the identity** (ADR-007):
  the transport key is derived per epoch with domain separation, epoch 0 is the identity
  itself, and an address change is announced with the identity's signature.
- **Where:** `core` (`identity.rs`, `crypto.rs`, `vault.rs`, `kdf.rs`).
- **Enables:** everything else; contact verification, secure storage, cutting off a flood
  by changing the address without losing contacts.

### F2. Transport (mesh) ✅
- **What:** Yggdrasil over FFI (no TUN, no root), joining the global mesh; demultiplexing
  of the reliable and media channels; QoS queues (audio/chat blocking, video
  drop-on-full); size padding.
- **Where:** `transport` (libygg.so), `core/protocol.rs`, `core/pad.rs`.
- **Also:** a blocklist of sources inside libygg — packets from a blocked source are
  dropped before crossing FFI; the node can be replaced on the fly (address change).
- **Enables:** any delivery of bytes between peers.

### F3. Storage ✅
- **What:** contacts, message history, profile, settings; SQLCipher; crash-safe state
  files (`fsutil`).
- **Where:** `core` (`store.rs`, `db.rs`, `settings.rs`, `fsutil.rs`). All conversations
  are one table, `chat_messages` (1:1 chats, Saved, groups) — [ADR-009](adr/ADR-009-one-message-store.md).
- **Also:** **a key from a password** (`kdf.rs`, Argon2id with the profile number in every
  file) and **the data backup** — the container `core/backup.rs` (frames,
  XChaCha20-Poly1305, frame number in the AAD) plus contents and merging in
  `engine/backup.rs`. A data backup ≠ a key backup: the key restores the identity, the
  data backup restores the conversations.
- **Enables:** persistence of everything; moving conversations to a new device; at-rest
  levels (Keystore / password / both).

### S1. Messaging (reliable channel) ✅
- **What:** sessions, events, presence/keepalive, reliable delivery (retry until ack),
  deduplication of repeated deliveries by message id (replay protection itself is the
  ironwood session's job).
- **Where:** `messenger`.
- **Enables:** 1:1 chat, call signalling (invite/decline), presence.

### S2. Groups (MLS) ✅
- **What:** cryptographic membership (openmls), serverless ordering (admin sequencer),
  onboarding by KeyPackage, removal and key rotation (ADR-004), persisted state.
- **Where:** `group` (crypto), `groupchat` (ordering), `engine/groups.rs` (the actor).
- **Enables:** group chat, group rooms, channels later.

### S3. Media engine ✅ (polish ongoing)
- **What:** Opus (audio), H.264 (video), adaptive jitter buffer, frame fragmentation and
  reassembly, call orchestration.
- **Where:** `litertc` — the media plane per [ADR-014](adr/ADR-014-litertc.md) (packet
  format, frame reassembly, audio jitter buffer, network simulator); `media` (codecs:
  `codec.rs`, `video.rs`), `engine/room_call.rs`; on Android the capture and codecs are
  platform APIs (`CallAudio`, `VideoSender`, `VideoReceiver`).
- **Debt:** the desktop has its own call engine (`call`); unifying it with the engine is
  part of the desktop work.
- **Enables:** 1:1 and group calls; later screen sharing.

### S4. Rooms and presence ✅
- **What:** a room = a group (or a DM pair) + a presence beacon, Discord-style: join any
  time, no host, instant leave, TTL. A room is STABLE (deterministic room id). In a group
  room, only the current MLS roster is admitted.
- **Where:** `engine` (rooms, presence), `messenger`.
- **Enables:** call rooms, the participant grid, "who is online now".

### S5. Content-addressed blobs ✅
- **What:** chunks addressed by hash (BLAKE3), pull ("give me blob X"), resumable, size and
  type limits (anti-DoS), fetched directly from the other person's device; EXIF/GPS
  scrubbing; streaming on both sides; thumbnails as separate small blobs; downloads above a
  threshold need consent.
- **Where:** crate `blob`, `engine/blobs.rs`.
- **Enables:** avatars, images, video, files, voice messages — one subsystem, many
  features.

### S6. Offline delivery ✅
- **What:** a persistent outbox in SQLite, flushed when the peer comes online and at
  start-up; size and lifetime per trust tier (contacts' messages do not expire).
- **Where:** `core/outbox.rs`, `messenger`, `engine`.
- **Enables:** serverless messaging without both sides being online at once.

### S7. Message model ✅ partly
- **What:** the schema and semantics of a message, not the transport (that is S1).
  References are the pair "author + id" (ADR-006); unknown payload types are ignored, so
  extensions are additive.
- **Done:** replies, edits, retraction ("delete for everyone"), forwarding (optionally
  without the author), pins, captions, mentions in groups (a span + identity key, the name
  is drawn by the receiver; `core/mention.rs`).
- **Not yet:** reactions, read receipts, typing indicator.
- **Where:** `core/protocol.rs`, `groupchat` (`AppMsg`), `core/store.rs`.

### P1. Platform shell ✅
- **What:** a foreground service that keeps the node online, the JNI bridge, the Compose
  UI, notifications.
- **Where:** `androidjni`, `android/`. Push wake-up (`ntfy`) is written but not wired in.
- **Enables:** all screens; staying reachable in the background.

---

## Feature → subsystems

| Feature | Subsystems | Status |
|---|---|---|
| 1:1 chat | S1 + F3 + S6 | ✅ |
| Groups | S2 (+ S4 for presence) | ✅ |
| 1:1 and group calls | S3 + S4 | ✅ |
| Participant grid, "you are alone" | S4 + P1 | ✅ |
| Avatars | S5 + S4 (hash check) | ✅ |
| Images / video / files / voice messages | S5 + S7 | ✅ |
| Replies / edits / forwarding / pins | S7 | ✅ |
| Mentions in groups | S7 | ✅ |
| Reactions / read receipts / typing | S7 | ⏳ |
| Search | F3 (FTS5) | ✅ |
| Data backup | F3 | ✅ |
| Settings (media/camera/energy/storage/at rest) | P1 + F3 | ✅ |
| Multiple devices per identity | F1 + S2 + F3 (ADR-010) | ⏳ after 1.0 |
| Screen sharing (Android) | S3 + P1 | ⏳ after 1.0 |
| Stickers / link previews | S5 + S7 + UI | ⏳ after 1.0 |
| Relay incentives | F2 + S6 (ADR-008) | ⏳ after 1.0 |

---

## Consistency and failures (the distributed model)

This is a distributed system — partitions, Byzantine failures and clock skew are real. The
principle: **do NOT try to "solve all of distributed systems up front"** (that is
paralysis), but EXPLICITLY state which guarantees we give or do not give and which
failures we assume. That turns "a pile of problems" into a bounded list of decisions.

**Model: AP + eventual consistency.** For a messenger that is the right choice — messages
arrive and converge over time rather than blocking for strict consistency. The priority is
**safety** (never distort or confuse authorship); liveness is weak (a message may be
delayed during a partition — fine, it waits in the queue). FLP: in an asynchronous network
with failures you cannot guarantee both at once.

**Already handled by design (not by accident):**
- **Partitions / offline** → the outbox (S6) + presence TTL + delivery on return. A
  partition is the norm, not a failure.
- **Clock skew** → wall-clock time is NOT used for CORRECTNESS. Local timing is monotonic
  (`Instant`: jitter, presence TTL). Wall-clock time is only cosmetic (time in the UI) and
  a coarse outbox lifetime for strangers (one day, tolerant of skew). Order is logical, not
  by clock.
- **Byzantine / malicious peers** (partly) → signatures and AEAD (E2E, MLS) give
  authenticity and integrity; anti-replay; parser validation (property tests). Full BFT
  consensus is NOT needed — it is for N mutually distrustful parties agreeing on one value
  (blockchains), not our case.
- **Duplicates / retries** → every reliable message has a per-sender id; the receiver
  deduplicates (a replay ring in the messenger, the MLS generation in groups).

**Genuinely open questions (each gets its own ADR, not "everything at once"):**
- **Group message order under a partition, and a missing or faulty admin sequencer** →
  [ADR-002](adr/ADR-002-group-message-ordering.md). Today the admin sets the order (a soft
  leader): simple, but what if the admin is offline, or on one side of a partition? The
  chosen direction is decided; implementation is after 1.0.
- **Equivocation** — a malicious member sends different versions to different people.
  Detectable with a hash chain or signatures (the divergence becomes visible), but the
  policy for reacting to it is an open question.

---

## Cross-cutting UI surfaces (design them as layers, not screens)

- **The call window** — a persistent layer above the rest, not tied to one screen.
- **Settings** — one architecture with sections (media quality, camera, energy, storage,
  at-rest level).
- **The navigation skeleton** — chats / chat / call rooms / contacts / settings. Decided
  early, so features slot in. Details: [design/app-ui.md](design/app-ui.md) (Russian).
