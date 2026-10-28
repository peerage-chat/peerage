# LiteRTC wire format

The contract for call media between Peerage clients ([ADR-014](../adr/ADR-014-litertc.md)).
The spec, not the code, is the contract: a second implementation must be writable from
this page alone. The code that implements it is `litertc/src/wire.rs`; its `golden_*`
tests pin the bytes below.

Current version: **v2** (since 2026-10-28, ADR-014 step 4). v1 (an 8-byte header starting
with `0x02`, no stream id, video timestamp always 0, rotation and fps hidden inside the
video payload) is not accepted any more: v1 and v2 clients don't hear each other.

## Where the packet lives

```text
datagram on Yggdrasil:  [ 0x02 = CHAN_MEDIA ][ sealed media packet ]
sealed by:              the 1:1 session's media key (ADR-012, SEC-4)
after opening:          [ LiteRTC packet — this page ]
```

The channel byte and the sealing are outside LiteRTC. By the time LiteRTC sees a packet,
the sender's identity has been proven by the session handshake and replays have been
rejected. LiteRTC does not authenticate anything itself.

All integers are big-endian.

## Media packet

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | version/type | `0x20`: version 2 (high nibble), type 0 = media (low nibble). Anything else: drop. |
| 1 | 1 | track | `0` audio, `1` camera video, `2` screen. Unknown: drop. |
| 2 | 4 | stream_id | Random, chosen by the sender for each call. A new value means the sender started over. |
| 6 | 2 | seq | Per-track packet counter, wraps at 2¹⁶. |
| 8 | 2 | tw_seq | Transport-wide counter: one per recipient, across all tracks, wraps at 2¹⁶. |
| 10 | 4 | ts | Capture time. Audio: 48 kHz sample counter (+960 per 20 ms frame). Video: 90 kHz clock from the sender's first video frame. Wraps. |
| 14 | … | payload | Audio: one Opus frame. Video: one fragment (below). |

A packet shorter than 14 bytes is dropped. The whole packet is at most 14 + 60 000 bytes.

**Sender rules.**
- Audio `seq` and `ts` advance for every 20 ms frame, including frames not sent (muted
  microphone, encoder failure), so the receiver sees a gap rather than a time shift.
- One media packet goes to each call member. `stream_id`, `seq` and `ts` are the same for
  every member; `tw_seq` is that member's own counter.
- `stream_id` must be drawn from a cryptographic RNG for every call, so a rejoin is never
  mistaken for the old stream.

`tw_seq` is not used by receivers yet: it is there for the delay-based bandwidth estimate
(ADR-014 step 7), so that adding the estimate does not need another format change.

## Video fragment (payload of a video packet)

A video frame (an H.264 access unit in Annex B form, SPS/PPS in front of keyframes) is cut
into chunks of at most 8 000 bytes. Each chunk travels in its own media packet:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | frame_id | Per-sender frame counter, wraps at 2³². |
| 4 | 2 | frag_idx | Index of this chunk, `0 ≤ frag_idx < frag_count`. |
| 6 | 2 | frag_count | Chunks in the frame, `1 ≤ frag_count ≤ 1024`. |
| 8 | 1 | flags | Bit 0: keyframe. Other bits: zero, ignored. |
| 9 | 1 | rotation | Quarter turns clockwise the receiver applies (the camera captures in sensor orientation). Taken modulo 4. |
| 10 | 1 | fps | The sender's frame rate; the receiver paces output by it. `0` = unknown. |
| 11 | … | data | The chunk. An empty frame is one fragment with empty data. |

Every fragment of a frame carries the same flags, rotation and fps. Video `seq` counts
fragments, not frames; all fragments of a frame share its `ts`.

## Receiver rules

These are the limits that keep a hostile call member from exhausting memory. They are
part of the contract: a receiver that accepts more is not wrong on the wire, but it is
unsafe.

- **New stream.** Receive state is kept per (sender, track) for one `stream_id`. A packet
  with a different `stream_id` replaces that state: the jitter buffer or frame
  reassembler starts empty.
- **Reassembly.** A frame is delivered when all its fragments have arrived, in any order.
  A fragment whose `frag_count` disagrees with an earlier fragment of the same frame is
  dropped. At most 16 incomplete frames and 16 MiB of incomplete data are kept; beyond
  that the oldest incomplete frame is dropped.
- **Late frames.** Frame ids compare in serial-number arithmetic (half the circle ahead
  is newer). A frame not newer than the last delivered one is dropped, and so are
  incomplete frames older than a delivered one.
- **Audio.** Packets are reordered by `seq` in a jitter buffer. A packet older than the
  next one to play (serial arithmetic on 2¹⁶) is dropped. A missing packet is declared
  lost (the decoder conceals it) once two later packets are waiting.
- **Members only.** Packets from someone who is not a current call member are dropped,
  and a member who leaves loses all receive state.
