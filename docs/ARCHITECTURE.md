# Architecture

One walk through the whole system: what it is made of and how data travels from a tap
in the UI to bytes on the mesh and back. Each topic links to a focused document
(invariants, protocol, ADRs). Entry point to the docs: [docs/README.md](README.md).

> Changing the system? Check [architecture/invariants.md](architecture/invariants.md)
> first — the cross-cutting rules whose violation causes silent or catastrophic bugs.

---

## 1. What it is, and the principles

A serverless end-to-end encrypted messenger with text, media, voice and video calls.
Peers talk over the **Yggdrasil** overlay mesh, where every node has a permanent IPv6
address derived from its ed25519 key. There is no server of ours in the data path.

Principles:
- **Fully peer-to-peer.** The transport is not trusted; it only routes datagrams by key.
- **One Rust core.** Protocol, crypto and networking have a single source of truth;
  clients are thin adapters.
- **Features are combinations of a few subsystems** (reliable delivery, MLS groups,
  content-addressed blobs, rooms), not separate columns of code — see
  [SUBSYSTEMS.md](SUBSYSTEMS.md).
- **A thin engine ↔ client boundary.** The client renders state and issues commands; it
  does not compute domain logic ([ADR-003](adr/ADR-003-engine-api.md),
  [protocol-spec.md](protocol-spec.md)).

Scope of v1.0 (Android first): [v1.0-scope.md](v1.0-scope.md). Current state, including
what is verified and what is broken: [project-state.md](project-state.md).

---

## 2. Crate map (Rust workspace) + Go + Android

```
core/        identity, crypto, wire protocol + padding, SQLCipher store, outbox,
             per-peer budgets, vault (at-rest), settings, crash-safe file writes
transport/   safe wrapper over libygg (FFI to yggdrasil-go): Node write/read by key
messenger/   session layer: handshake, presence, keepalive, reliable delivery
             (retry-until-ack), channel demux CHAN_CHAT/CHAN_MEDIA, room presence beacons
media/       media chain: Opus codec, jitter buffer, H.264 framing
group/       MLS primitive (RFC 9420, openmls): TreeKEM, forward secrecy, PCS
groupchat/   group chat over Yggdrasil (MLS + transport, serverless, admin sequencer)
blob/        content-addressed blobs: BLAKE3-verified chunks, resumable pull, limits,
             EXIF/GPS scrubbing
engine/      DOMAIN LOGIC for clients: the Engine facade, GroupManager (an actor for the
             !Send MLS state), BlobManager, RoomCallEngine (multi-party rooms), backup
androidjni/  THIN JNI bridge (cdylib libpeerage_android.so): marshals arguments to Engine calls
call/        desktop call engine (cpal audio + H.264); used only by the desktop client
desktop/     desktop client (Slint). Predates the engine and currently does not compile
ntfy/        client for ntfy.sh push wake-up. Written but not wired into any client yet
```

```
ygg/         Go: c-shared wrapper around yggdrasil-go (libygg.so) — a datagram API
             without TUN and without root
android/     Kotlin/Compose Android client (§9)
```

Dependencies, roughly: `core` ← everything; `transport` → libygg; `messenger` →
core + transport; `media`/`group`/`groupchat`/`blob` build on these; `engine` → all of
them. **`androidjni` → engine** (thin codec, logic in the engine). **`desktop` →
messenger + call + groupchat directly**, bypassing the engine — moving it onto the engine
is planned work.

---

## 3. The engine ↔ client boundary

Domain logic lives in `engine` (crate `peerage-engine`), not in JNI or the UI.
- **`Engine`** is the facade: it owns the messenger session, contacts, groups, blobs and
  rooms. The client calls its methods; the engine reports back through an **event queue**
  (JSON) that the client polls. Everything the UI shows can be rebuilt from the engine.
- **`GroupManager`** is an actor for `GroupChat`/openmls, which are `!Send`: it lives on
  one thread, receives commands over a channel, routes by `group_id` and persists state.
- **`BlobManager`** offers, fetches and serves blobs; **`RoomCallEngine`** runs
  multi-party rooms (mesh multicast plus a per-source mix).

Contract: [protocol-spec.md](protocol-spec.md); decision: [ADR-003](adr/ADR-003-engine-api.md).
`androidjni` decodes JNI arguments into an `Engine` call and encodes the result; every
entry point is wrapped in a panic guard (a unit test enforces this). **No logic in
JNI or UI.**

---

## 4. Transport: Yggdrasil and channels

- **libygg** (`ygg/libygg/ygg.go`) wraps `yggdrasil-go` as a c-shared library exposing a
  datagram API (`ygg_start/write/read/peers_json/...`). No TUN device, no root: it uses
  the `PacketConn` directly, addressing datagrams by the node's ed25519 key.
- **`transport`** is the safe Rust wrapper (`Node`): start/listen/add_peer/write/read.
- Nodes join the global mesh through public peers (`peers/public-peers.txt`).

**Channel discipline** (first byte of every datagram, `core/src/media.rs`) — an invariant:
- `CHAN_CHAT = 0x01` — reliable (id + ack + retry, `messenger::send_reliable`): chat,
  keys (MLS Welcome/KeyPackage), control (RoomInvite, acks, presence).
- `CHAN_MEDIA = 0x02` — fast, no ack, loss is acceptable: real-time audio/video only.
- Never send keys or chat over CHAN_MEDIA, and never push media through acks on CHAN_CHAT.

The channel byte stays in the clear on purpose: demultiplexing and the ingress budget
must work before decryption (threat model, N7). Details: [invariants.md §2](architecture/invariants.md).

---

## 5. Cryptography

- **Identity** (`core/src/identity.rs`): the root is an **ed25519** key (Yggdrasil v0.4+
  requires it). The Yggdrasil IPv6 address is derived from it (checked bit-for-bit
  against yggdrasil-go). Every other key is a sub-key derived with domain separation
  (`derive_subkey`, BLAKE3).
- **Verification:** a visual fingerprint — a 6×2 grid of shapes and colours, 96 bits —
  compared in person or by scanning a QR code. We draw the glyphs ourselves so the grid
  looks identical on every device.
- **1:1 messages** (`core/src/noise.rs`, `core/src/session.rs`): sessions from a hybrid
  **Noise XXhfs** handshake (X25519 + ML-KEM-768, ChaCha20-Poly1305, BLAKE2s) —
  [ADR-012](adr/ADR-012-dm-session-v2.md). Keys live in memory only and are replaced every
  2 minutes; a restart is just a new handshake. The static key is separate from the
  identity and signed by it.
- **Groups** (`group`/`groupchat`): **MLS (RFC 9420, openmls)** — TreeKEM, forward secrecy
  and post-compromise security. Serverless: messages spread over the mesh and the group
  admin sequences Commits. Members can be removed (revocation), and a daily self-update
  rotates keys ([ADR-004](adr/ADR-004-mls-membership-revocation.md)).
- **Padding** (`core/src/pad.rs`, `bucket_for` in `core/src/protocol.rs`): every payload
  is padded to a size bucket — control 256 B, application 4 KiB, blob chunk 25 KiB — so
  text, images, files and voice look the same on the wire, and a call invite looks like
  an ack.
- **At rest** (`core/src/vault.rs`): XChaCha20-Poly1305 for files, SQLCipher for the
  database. The key comes from the Android Keystore, from a password (Argon2id), or both.
- **Media** has no per-packet crypto of its own: real-time traffic rides the
  end-to-end encrypted Yggdrasil session, which also authenticates the source — known
  finding SEC-4.

Threat model and audit: [SECURITY.md](SECURITY.md),
[architecture/threat-model.md](architecture/threat-model.md).

---

## 6. Delivery

- **`send_reliable`** (`messenger/src/lib.rs`) is the single reliability primitive: it
  registers a pending entry and the maintenance loop resends it every ~3 s until an
  `Ack{id}` arrives or `message_ttl` (300 s) passes. It carries 1:1 text, group
  messages, MLS Commit/Welcome and invites. Anti-spoofing: an ack counts only if it comes
  from the peer the message was sent to.
- **Outbox** (`core/src/outbox.rs`, SQLite): anything not acknowledged survives restarts
  and is resent when the peer comes back online or the app starts. Size and lifetime
  are limited per peer by trust tier; contacts' messages do not expire (without a
  server, "expired" would mean "lost forever"), strangers' do.
- **Presence and keepalive:** ping/pong for online status; room presence beacons (TTL 5 s,
  instant leave).
- **Budgets** (`core/src/budget.rs`): one per-peer tier (blocked / stranger / contact /
  verified) sets the ingress rate limit and the storage cap. Packets over budget are
  dropped **before decryption**, so a flood does not cost CPU, battery or disk.
- **MLS invariant:** every operation that moves group state (send, receive, commit) must
  `save_group`. Otherwise, after a restart the sender uses an old generation, the
  receiver has already deleted that secret for forward secrecy, and messages stop
  arriving (invariants §4).

---

## 7. Persistence

- **SQLite (core)** — `core/src/db.rs`: SQLCipher (key from the vault), `journal_mode=WAL`,
  `busy_timeout=5000`, `foreign_keys=ON`. **One message table** (`chat_messages`) holds
  1:1 chats, groups and Saved messages ([ADR-009](adr/ADR-009-one-message-store.md)),
  with an FTS5 index for search; plus contacts, outbox, contact requests, blocklist, call
  log, pins, wanted downloads. Writes are serialised through one connection behind a
  mutex. Rules: [invariants.md §3](architecture/invariants.md).
- **Files (engine)** — vault-sealed: MLS group state (`.group`), the group index, admins,
  invites; blobs in `blobs/`. All of them are written crash-safely
  (`core::fsutil`: temp file → fsync → rename), so a killed process leaves either the old
  version or the new one.

---

## 8. Media pipeline

**Audio:** capture → **Opus** (`media/src/codec.rs`) → CHAN_MEDIA → jitter buffer
(`jitter.rs`: reorder by sequence number, prefetch, loss concealment) → decode → playback.
On Android, capture and playback are `CallAudio.kt` (AudioRecord/AudioTrack with the
platform echo canceller, noise suppressor and gain control); PCM crosses into Rust over JNI.

**Video (Android, hardware H.264):**
```
Camera2 → MediaCodec encoder (input surface, zero-copy) → H.264 access unit
  → Rust fragments it → CHAN_MEDIA → receiver reassembles → MediaCodec decoder → TextureView
```
- `VideoSender.kt`: Camera2 renders straight into the encoder's input surface and,
  optionally, a preview surface in the same session. The sensor angle and the stream's
  frame rate travel in a small header so the receiver can rotate and pace correctly.
- `VideoReceiver.kt`: pulls packets from Rust into an application-level jitter buffer
  that paces frames by the sender's frame rate.
- The input-surface size must be one the camera reports in
  `StreamConfigurationMap.getOutputSizes`, otherwise the capture session silently fails
  (`CameraCaps.kt`).

**Rooms:** a call is a persistent room. A presence beacon announces who is in it and what
they have on (audio/video); `RoomCallEngine` multicasts audio over the mesh and mixes per
source; video is received from live participants. In a group room, only members of the
current MLS roster are admitted.

Hardening media for real networks (bitrate adaptation, A/V sync):
[plans/media-webrtc-parity.md](plans/media-webrtc-parity.md).

---

## 9. Android client (Kotlin/Compose)

**Threading (invariant):** the UI never calls `Native.*` directly — any JNI call may block
and cause an ANR. Everything goes through background threads in `Model.kt`:
- `pollThread` — polls engine events;
- `tickerThread` — periodic status (peers, online, presence);
- `ops` — a single-thread executor that serialises operations (sending, contacts, calls).

State is **Compose snapshot state** (`mutableStateOf/List/Map`), safe to write from
background threads. StateFlow is deliberately not used.

**Files:**
- Root: `Native.kt` (JNI bindings), `Model.kt` (AppModel: state and threads) with its
  topic extensions (`ModelNotes`, `ModelSearch`, `ModelStorage`, `ModelCallLog`),
  `Types.kt` (data types), `MainActivity.kt`, `Theme.kt`/`Tokens.kt` (design tokens),
  media (`CallAudio`, `VideoSender`, `VideoReceiver`, `CameraPreview`, `CameraCaps`),
  call screens (`RoomCallScreen`, `ShareVideoScreen`), services (`ConnService` keeps the
  node online in the background, `CallService`, `IncomingCallReceiver`), `VaultKeyManager`.
- `ui/`: chat (one implementation for 1:1 and groups), contacts, groups, network,
  search, settings, storage, backup, onboarding, media viewer, QR scanner, shared
  components.

**UI system:** language and navigation — [design/app-ui.md](design/app-ui.md) (source of
truth), call screens — [design/call-ui.md](design/call-ui.md), colours/spacing/sizes —
[design/tokens.md](design/tokens.md) + `Tokens.kt`. New code uses `Tokens.*`, never inline
hex values. (The design documents are in Russian.)

---

## 10. Build and development

**Toolchain:** Rust 1.95+, Go 1.24+ (libygg, on Linux/WSL), Android NDK r28c + cargo-ndk,
Gradle 8.9 + JDK 21. The Rust and Go parts are built on Linux (WSL on Windows).

**Android:**
```sh
# Rust only (fast; when only Rust changed):
bash scripts/rebuild-peerage-android.sh <ABI>      # x86_64 (emulator) | arm64-v8a (phone)
# Full (when the ygg.go FFI changed): libygg + Rust
bash scripts/build-android-libs.sh <ABI>
./gradlew :app:assembleDebug                   # APK
./gradlew ktlintFormat                         # before committing Kotlin
```
The `.so` files go to `android/app/src/main/jniLibs/<ABI>/` (not in git).

**Tests (Linux/WSL):** `scripts/test-all.sh` runs the whole Rust suite, including
engine-level integration tests over an in-process transport double.
`scripts/live-two-node.sh` runs two headless nodes on the public mesh. A per-machine
`scripts/local.env` (not in git) can point `CARGO_TARGET_DIR` elsewhere.

---

## 11. Where things are documented

| Need | Document |
|---|---|
| Entry point | [docs/README.md](README.md) |
| This end-to-end picture | **ARCHITECTURE.md** (you are here) |
| What works, what is verified, what is broken | [project-state.md](project-state.md) |
| Cross-cutting rules (FFI/channels/SQLite/MLS/UI) | [architecture/invariants.md](architecture/invariants.md) |
| Engine ↔ client contract | [protocol-spec.md](protocol-spec.md), [adr/](adr/) |
| Subsystems → features | [SUBSYSTEMS.md](SUBSYSTEMS.md) |
| Security and threat model | [SECURITY.md](SECURITY.md), [architecture/threat-model.md](architecture/threat-model.md) |
| UI: language / calls / tokens | [design/app-ui.md](design/app-ui.md), [design/call-ui.md](design/call-ui.md), [design/tokens.md](design/tokens.md) |
| Scope and order of work | [v1.0-scope.md](v1.0-scope.md), [completion-plan.md](completion-plan.md), [ROADMAP.md](ROADMAP.md) |
| Plans (media, security) | [plans/](plans/) |
| Known bugs | [BUGS.md](BUGS.md) |
