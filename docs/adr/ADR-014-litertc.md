# ADR-014: LiteRTC — the call media plane as one internal, sans-IO crate

**Status:** Accepted (2026-10-28). Scope checked feature by feature against WebRTC in
[litertc-vs-webrtc.md](../architecture/litertc-vs-webrtc.md) (in Russian); its P1 list is
this ADR's scope, P2/P3 come later.
**Date:** 2026-10-28
**Decided by:** the owner set the direction (our own minimal analogue of WebRTC, no
libwebrtc, security first, not published separately until the messenger ships); the
implementer proposes the boundary, the format and the order below.
**Related:** [SUBSYSTEMS](../SUBSYSTEMS.md) (S4 media), [invariants](../architecture/invariants.md)
(2 — channels, 12 — hanging up), [ADR-012](ADR-012-dm-session-v2.md) (the session that seals
media, SEC-4), [threat model](../architecture/threat-model.md) (N6, N7),
[plans/beta.md](../plans/beta.md) ("4K falls apart").

## Context

A call today works end to end — 1:1 and group rooms, audio and video, encrypted per pair
(SEC-4) — but the media code has no boundary of its own. It is spread over five places:

| Where | What it does |
|---|---|
| `core/src/media.rs` | the 8-byte media header (track, seq u16, ts u32), video fragmentation (8 KB chunks) and reassembly with memory caps |
| `media/` | Opus encoder/decoder, the audio jitter buffer, an openh264 path for the desktop |
| `engine/src/room_call.rs` | per-peer audio jitter + decode + mix, speaking levels, video fan-out and per-peer video queues, traffic counters, the send/receive threads |
| `messenger` | sealing each frame with the 1:1 session's media key, the `CHAN_MEDIA` byte, the ingress budget before decryption, the bounded media channel |
| Android `VideoSender` / `VideoReceiver` | MediaCodec H.264, a 2-byte side header inside the payload (rotation, fps), the video jitter buffer and paced playout |

What that costs us:

1. **No feedback from receiver to sender.** Nothing travels back: no loss report, no
   retransmission request, no keyframe request, no bandwidth estimate. The sender encodes
   at a fixed bitrate with a keyframe every second (`KEY_I_FRAME_INTERVAL = 1`), so a lost
   fragment smears the picture until the next keyframe, and keyframes eat bandwidth all
   the time. `VideoSender.requestKeyframe()` exists, but only the local camera calls it.
   This is most of "4K falls apart".
2. **No real timing.** Video goes out with `ts = 0`; the receiver paces by the fps byte
   of the side header. Audio and video are not synchronised to each other.
3. **Protocol data hides inside the payload.** Rotation and fps travel as two opaque
   bytes that only Kotlin understands; the next client would have to reverse-engineer them.
4. **Nothing can be tested without two devices.** There is no simulated network, so loss,
   jitter, reordering and bandwidth drops are tested by hand on phones, or not at all.
5. **Duplicated engines.** The desktop's `call` crate (`GroupCall`) re-implements the room
   engine and no longer builds.

What already works and must not regress:

- **Encryption and sender authentication** come from the 1:1 session (SEC-4,
  `Sessions::seal_media` / `open_media`): one key schedule, bound to the identity proven
  by the handshake, with a replay window.
- **Flood protection before crypto**: the `CHAN_MEDIA` byte and the per-source ingress
  budget let us drop a flood without spending crypto on it (threat model N7 explains why
  that byte stays visible).
- **Hanging up is never queued behind the peer** (invariant 12).
- **Hard memory caps** on everything a peer can make us allocate (fragments per frame,
  pending bytes, per-peer video queue).
- **The roster gate** (N6): media is accepted only from current room members.

## Options considered

1. **libwebrtc.** Rejected by the owner: a huge foreign dependency we cannot audit, and
   most of it (ICE, DTLS-SRTP, SDP) solves problems Yggdrasil and our sessions already solve.
2. **A Rust WebRTC stack (webrtc-rs, str0m).** Same objection at a smaller scale: the
   value is in ICE/DTLS/SRTP/SDP, which we do not need, and the surface stays large.
   **We borrow str0m's sans-IO design as an idea**, not the crate.
3. **Standard RTP/RTCP packets (RFC 3550/4585) over our datagrams.** Familiar and well
   specified, but 12+ bytes of header per packet, extension machinery we would not use,
   and every field becomes something we promise to parse. We take the **semantics** of
   RTCP feedback (NACK, PLI, receiver reports) and of congestion control (Google
   Congestion Control, draft-ietf-rmcat-gcc), not the byte layout.
4. **Keep the code where it is and add features in place.** Cheapest today, but every new
   feature would be another cross-cut through engine, core, media and Kotlin, still with
   no way to test it without devices.
5. **One internal crate with a sans-IO core — chosen.**

## Decision

### 1. The crate

A new workspace crate **`litertc`** (directory `litertc/`). It depends on nothing from
Peerage — no `peerage-*` crate, no Yggdrasil, no MLS, no Android. Allowed dependencies:
`std`, `thiserror`; anything else needs a reason in review. `#![forbid(unsafe_code)]`.

It is not published separately (owner's decision); being dependency-free is about a clean
boundary and testability, not about crates.io.

### 2. Sans-IO: no threads, no sockets, no clock inside

LiteRTC is a state machine. The caller feeds it inputs with the current time and pulls
outputs; it never sleeps, never spawns, never reads a clock:

```rust
pub struct Session<P> { … }              // one call, from our side; P = peer id
impl<P: Copy + Eq + Hash> Session<P> {
    pub fn new(cfg: Config) -> Self;
    pub fn add_peer(&mut self, peer: P);
    pub fn remove_peer(&mut self, peer: P);

    // our media → network
    pub fn send_audio(&mut self, now: Instant, payload: &[u8], samples: u32);
    pub fn send_video(&mut self, now: Instant, frame: &[u8], info: VideoInfo); // keyframe, rotation, capture time
    pub fn poll_transmit(&mut self, now: Instant) -> Option<Transmit<P>>;    // (to, bytes) — the caller seals and sends

    // network → their media
    pub fn handle_datagram(&mut self, now: Instant, from: P, bytes: &[u8]); // `from` is already authenticated
    pub fn poll_event(&mut self, now: Instant) -> Option<Event<P>>;

    // timers (feedback, pacing, retransmission)
    pub fn poll_timeout(&self) -> Option<Instant>;
    pub fn handle_timeout(&mut self, now: Instant);

    pub fn stats(&self, peer: P) -> Option<PeerStats>;
}

pub enum Event<P> {
    Audio { from: P, payload: Vec<u8>, lost_before: u32 },   // decode, or conceal the loss
    VideoFrame { from: P, data: Vec<u8>, info: VideoInfo, play_at: Instant },
    KeyframeRequested,                                        // tell the encoder
    TargetBitrate { audio_bps: u32, video_bps: u32 },         // tell the encoders
}
```

Why sans-IO:

- **Deterministic tests.** A virtual clock and a simulated network drive the same code
  that runs in production (section 7).
- **Invariant 12 stays easy to keep.** LiteRTC has no threads and no queue of its own
  that a hang-up could wait behind; the engine keeps owning the threads and the halt flag,
  exactly as now. Hanging up = the engine stops calling `Session` and drops it.
- **One caller per platform.** The engine drives it on Android today; the desktop can
  drive the same `Session` later instead of the stale `call` crate.

### 3. What stays outside LiteRTC

| Concern | Stays in | Why |
|---|---|---|
| Encryption, sender authentication, replay | `messenger` + `core::session` (SEC-4) | one key schedule bound to the identity; LiteRTC never sees a key |
| `CHAN_MEDIA` byte, ingress budget | `messenger` | must run before decryption (N7) |
| Rooms, invites, presence beacons, roster gate | `engine` | signalling rides the reliable chat channel |
| Codecs (Opus in Rust, H.264 in MediaCodec / openh264), capture, render | `media`, platform | LiteRTC is codec-agnostic: frames in, frames out, plus `KeyframeRequested` / `TargetBitrate` for the encoder |
| Mixing, speaking levels | `engine` | application policy, not transport |

`peerage-media` keeps the codecs only; its jitter buffer moves into LiteRTC.

### 4. Wire format v2 (inside the sealed payload)

Encryption is unchanged: the messenger seals whatever `poll_transmit` returns, and
`handle_datagram` receives what `open_media` decrypted. Inside, the redundant channel
byte of today's inner header becomes a **version + packet-type byte**, so the format can
evolve. Two packet types:

- **Media:** track, a random **stream id** chosen each time a track starts sending (a
  restarted sender is recognised as a new stream instead of guessing from `seq` jumps),
  `seq` (per track), a **transport-wide seq** (one counter across all tracks to a
  receiver, for the delay-based estimate), `ts` = **capture time** in the codec clock
  (48 kHz audio, 90 kHz video), and for video: frame id, fragment index/count, flags (keyframe,
  carries metadata). **Rotation and fps move into the protocol** as metadata on keyframes
  and on change, so the 2-byte side header disappears.
- **Feedback** (receiver → sender, sent periodically and on loss): NACK ranges for video
  (RFC 4585 semantics), keyframe request (PLI), a receiver report (highest seq, loss
  fraction, jitter, arrival-time samples for the delay estimate, an echo for RTT).

Feedback travels on `CHAN_MEDIA`, not on the reliable chat channel: it is real-time and
loss-tolerant, and acks on it would be the avalanche invariant 2 forbids. **Invariant 2
is amended** to read "real-time audio/video and its feedback".

The exact byte layout is written as a separate spec,
`docs/architecture/litertc-wire.md`, in step 3 — the spec, not the code, is the contract.
Before the beta, the compat policy allows breaking the media format (old and new clients
simply won't hear each other); the version byte is there so that after the beta we
don't have to.

### 5. Recovery from loss

- **Video:** the sender keeps a bounded history of recent fragments (by bytes and by
  age, about one RTT plus margin). A receiver that sees a gap sends a NACK; the sender
  retransmits to that one receiver. If the gap can't be filled in time, the receiver
  sends a keyframe request and **skips** P-frames until the next keyframe instead of
  feeding the decoder a broken reference. Once that works, the periodic keyframe moves
  from 1 s to on-demand plus a long safety interval (about 10 s), which frees bandwidth.
- **Audio:** no retransmission (it would arrive too late). Opus in-band FEC is already
  encoded but never decoded (`AudioDecoder::decode` always passes `fec = false`): the
  jitter buffer reports "packet N lost, N+1 is here", and the decoder recovers N from the
  FEC data in N+1. The FEC share then follows the loss fraction from receiver reports;
  concealment (PLC) stays as it is.

### 6. Bandwidth: estimate, adapt, pace

- **Estimate** per receiver from the reports: loss-based (back off above ~10 % loss) and
  delay-gradient-based (back off when one-way delay keeps growing), after GCC. Delay
  matters more for us than for plain UDP: Yggdrasil peer links are TCP/TLS/QUIC, so
  congestion shows up first as growing queues and latency, and only later as drops.
- **Adapt**: `TargetBitrate` goes to the encoders. Kotlin maps the video target to an
  encoder bitrate and a resolution step; the audio target goes to Opus.
- **Pace**: fragments of a large frame go out spread over a few milliseconds instead of
  one burst, which is where queue drops come from today. The pacer has priorities:
  audio, then retransmissions, then video, so a keyframe never delays speech.
- **Mesh**: a group call sends a separate copy to each member (each is sealed with that
  member's key). With one encoder, the target is the minimum across receivers. Simulcast
  or a relay would lift that; both are out of scope, but the API (per-recipient
  `Transmit`, a reserved layer field) does not rule them out.

### 7. Testing is part of the crate

- **Network simulator** (`litertc::sim`): virtual clock; links with random and burst loss
  (Gilbert–Elliott), delay and jitter, reordering, a bandwidth cap with a queue; several
  peers.
- **Scenario tests that act as acceptance criteria**, for example: after a single lost
  fragment at 5 % loss and 200 ms RTT the video recovers without waiting for the next
  periodic keyframe; when bandwidth drops from 4 to 1 Mbit/s, the target bitrate follows
  within seconds and loss settles; NACK and keyframe-request floods from a hostile member
  stay within their budgets (section 8).
- **Fuzzing:** the existing `fuzz/fuzz_targets/media.rs` moves to the new parser, plus
  targets for feedback parsing and reassembly.
- **Live check** on the two emulators after each step, as before.

### 8. Security rules for LiteRTC

A room member is authenticated but may be hostile (threat model N5). Everything LiteRTC
parses comes from such a member, so:

- **Every allocation is bounded** (the existing caps carry over): fragments per frame,
  pending bytes, queued frames, the retransmission history, NACK ranges per packet.
- **No amplification:** retransmissions take at most a fixed share of the send rate;
  keyframe requests are honoured at most about once per second per peer. Otherwise one
  member could make us flood everyone, or keep the encoder producing only keyframes.
- **No panics on input**; parsing lives in one module and is fuzzed.
- **Audio packet size must not leak speech.** With variable bitrate, an Opus packet's size
  follows the sound, and encrypted VBR voice is known to leak phrases through packet
  sizes alone (Wright et al., IEEE S&P 2008); WebRTC does not hide this. We send audio at
  a constant packet size (Opus CBR, or padding to a fixed size). DTX stays **off** on
  purpose: skipping silent frames would reveal the rhythm of speech. The cost is a little
  bandwidth and ~50 packets/s during silence.
- **LiteRTC does not authenticate.** It trusts `from` because the messenger has already
  proven it (SEC-4). That boundary is written into the crate docs.

## Order of work — calls keep working after every step

1. **This ADR.**
2. ✅ (2026-10-28) **Move without changes.** Create `litertc`; move the fragmentation/reassembly from
   `core/media.rs` and the jitter buffer from `media/`; the format on the wire stays
   byte-identical. Add the simulator and tests that pin today's behaviour.
3. ✅ (2026-10-28) **Sans-IO `Session`.** `room_call.rs` drives it (it keeps its threads,
   mixing and halt); the JNI surface does not change. Write the wire spec
   ([litertc-wire.md](../architecture/litertc-wire.md)). As built, received media is
   *pulled* per peer at the consumer's pace (`pull_audio` per 20 ms tick, `pull_video`
   by the decoder) rather than pushed as events — playout is clocked by the consumer
   anyway; events stay for control (`KeyframeRequested`, `TargetBitrate`) from step 5.
   `messenger` now only seals and opens bytes (`send_media_packet`, raw `MediaPacket`);
   the format is LiteRTC's alone. A peer that leaves the call loses its receive state,
   so a rejoin starts from an empty buffer.
4. ✅ (2026-10-28) **Wire v2.** Version byte, stream id, transport-wide seq, capture timestamps,
   rotation/fps in the protocol; Kotlin drops the side header. The stream id replaced
   both restart heuristics (the video `STALE_WINDOW` and the audio restart window of
   R-108): frame and packet numbers now compare in plain serial arithmetic. Media between old and new builds breaks here (pre-beta policy).
5. ✅ (2026-10-28) **Feedback and stats:** receiver reports, RTT, per-peer stats (they also replace the
   ad-hoc traffic counters). Reports go out once a second from the engine's receive
   thread (`poll_timeout`/`handle_timeout`); stats are logged every 5 s. Audio `seq` now
   counts sent packets only, so a muted microphone is not loss.
6. **Recovery:** NACK, retransmission, keyframe request, skip-to-keyframe; then the
   longer keyframe interval. Audio: decode Opus FEC, constant packet size, DTX off.
7. **Bandwidth:** estimator, prioritised pacing, `TargetBitrate` → MediaCodec bitrate and resolution
   steps, Opus FEC. Revisit the 600 packets/s media ingress budget against real bitrates.
8. **Playout and A/V sync:** the video jitter buffer moves from `VideoReceiver` into
   LiteRTC (`play_at`); Kotlin only decodes and renders.
9. **Desktop:** when the desktop client comes back, it drives `litertc::Session`; the
   `call` crate is deleted.

Steps 2–3 change no behaviour and can land quickly. Steps 6–7 are where the quality
comes from.

## What this decision does NOT cover

- A relay/SFU, simulcast or SVC — mesh stays; groups are small.
- New codecs (VP9, AV1), FEC for video, RED for audio, bandwidth probing, time-stretching
  in the audio jitter buffer, a capability exchange in signalling — P2/P3 of the review,
  each needs its own decision.
- Any change to media encryption: SEC-4 keys stay in the 1:1 sessions.
- Screen-sharing UX (the track already exists).
- A public release of the crate (owner's decision: not before the messenger ships).

## Consequences

- One place for the media plane, testable without devices; the engine shrinks to call
  orchestration plus mixing.
- Kotlin loses protocol knowledge (side header, then the video jitter buffer).
- `docs/architecture/invariants.md` §2 is amended (feedback on `CHAN_MEDIA`); a new
  invariant records that LiteRTC is sans-IO and does no authentication.
- `docs/SUBSYSTEMS.md` S4 gets the new crate and loses the desktop-engine debt at step 9.
