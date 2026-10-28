# LiteRTC wire format

The contract for call media between Peerage clients ([ADR-014](../adr/ADR-014-litertc.md)).
The spec, not the code, is the contract: a second implementation must be writable from
this page alone. The code that implements it is `litertc/src/wire.rs`; its `golden_*`
tests pin the bytes below.

Current version: **v1** (the format calls have used since before LiteRTC). v2 (step 4 of
ADR-014) adds a version byte, a stream id, a transport-wide sequence number, capture
timestamps for video and video metadata.

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

## Media packet (v1)

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | marker | `0x02`. Any other value: drop the packet. |
| 1 | 1 | track | `0` audio, `1` camera video, `2` screen. Unknown: drop. |
| 2 | 2 | seq | Per-track packet counter, wraps at 2¹⁶. |
| 4 | 4 | ts | Audio: sample counter in the 48 kHz clock (+960 per 20 ms frame, wraps). Video: always `0` in v1. |
| 8 | … | payload | Audio: one Opus frame. Video: one fragment (below). |

A packet shorter than 8 bytes is dropped. The whole packet is at most 8 + 60 000 bytes.

**Sender rules.** Audio `seq` and `ts` advance for every 20 ms frame, including frames not
sent (muted microphone, encoder failure), so the receiver sees a gap rather than a time
shift. One media packet goes to each call member; the same `seq` is used for every
member.

## Video fragment (payload of a video packet)

A video frame (an H.264 access unit in Annex B form, SPS/PPS in front of keyframes) is cut
into chunks of at most 8 000 bytes. Each chunk travels in its own media packet:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | frame_id | Per-sender frame counter, wraps at 2³². |
| 4 | 2 | frag_idx | Index of this chunk, `0 ≤ frag_idx < frag_count`. |
| 6 | 2 | frag_count | Chunks in the frame, `1 ≤ frag_count ≤ 1024`. |
| 8 | 1 | flags | Bit 0: keyframe. Other bits: zero, ignored. |
| 9 | … | data | The chunk. An empty frame is one fragment with empty data. |

Video `seq` counts fragments, not frames.

**v1 carries no rotation or frame rate in the protocol.** The Android client puts two
bytes in front of the frame data (`rotation / 90`, `fps`); that is an Android-to-Android
convention inside `data`, not part of this format, and v2 replaces it.

## Receiver rules

These are the limits that keep a hostile call member from exhausting memory. They are
part of the contract: a receiver that accepts more is not wrong on the wire, but it is
unsafe.

- **Reassembly.** A frame is delivered when all its fragments have arrived, in any order.
  A fragment whose `frag_count` disagrees with an earlier fragment of the same frame is
  dropped. At most 16 incomplete frames and 16 MiB of incomplete data are kept; beyond
  that the oldest incomplete frame is dropped.
- **Late frames.** A frame up to 1 024 ids behind the last delivered one is late and
  dropped; incomplete frames older than a delivered one are dropped. A frame further
  behind is a restarted sender (it rejoined the room) or a wrapped counter, and is
  accepted as a new stream.
- **Audio.** Packets are reordered by `seq` in a jitter buffer. A packet up to 64 behind
  the next one to play is late and dropped; further behind is a restarted sender and
  resets the buffer. A missing packet is declared lost (the decoder conceals
  it) once two later packets are waiting.
- **Members only.** Packets from someone who is not a current call member are dropped.
