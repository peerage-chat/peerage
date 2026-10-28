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
| 0 | 1 | version/type | `0x20`: version 2 (high nibble), type 0 = media (low nibble). `0x24`: type 4 = media retransmitted after a NACK (below). Anything else: drop. |
| 1 | 1 | track | `0` audio, `1` camera video, `2` screen. Unknown: drop. |
| 2 | 4 | stream_id | Random, chosen by the sender for each call. A new value means the sender started over. |
| 6 | 2 | seq | Per-track packet counter, wraps at 2¹⁶. |
| 8 | 2 | tw_seq | Transport-wide counter: one per recipient, across all tracks, in the order packets leave the sender (after pacing), wraps at 2¹⁶. |
| 10 | 4 | ts | Capture time. Audio: 48 kHz sample counter (+960 per 20 ms frame). Video: 90 kHz clock from the sender's first video frame. Wraps. |
| 14 | … | payload | Audio: one Opus frame. Video: one fragment (below). |

A packet shorter than 14 bytes is dropped. The whole packet is at most 14 + 60 000 bytes.

**Sender rules.**
- `seq` counts packets actually sent. Audio `ts` advances by 960 for every 20 ms frame,
  including frames not sent (muted microphone, encoder failure): silence is a jump in
  `ts` with no gap in `seq`, so receivers don't count it as loss.
- One media packet goes to each call member. `stream_id`, `seq` and `ts` are the same for
  every member; `tw_seq` is that member's own counter.
- `stream_id` must be drawn from a cryptographic RNG for every call, so a rejoin is never
  mistaken for the old stream.
- Audio packets of a stream all have the same size (Opus in constant-bitrate mode). With
  variable bitrate the size of an encrypted packet follows the sound, and phrases can be
  recognised from sizes alone. Discontinuous transmission (no packets in silence) stays
  off for the same reason.

`tw_seq` numbers what actually left for this recipient, retransmissions included; the
recipient reports when each one arrived (transport feedback, below), and the sender's
bandwidth estimate compares that with when it left.

**Retransmitted media** (`0x24`) is a video packet sent again because the receiver asked
for it (NACK, below). Everything after byte 0 is the original packet's, except `tw_seq`,
which is the recipient's next one. A separate type keeps retransmissions out of the
receiver's loss statistics: reports describe loss on first transmission, which is what
the bandwidth estimate needs. Audio is never retransmitted.

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
| 10 | 1 | fps | The frame rate the sender's encoder was configured for, informational: receivers play frames by their capture timestamps, since a camera often delivers fewer (darkness, emulators). `0` = unknown. |
| 11 | … | data | The chunk. An empty frame is one fragment with empty data. |

Every fragment of a frame carries the same flags, rotation and fps. Video `seq` counts
fragments, not frames; all fragments of a frame share its `ts`.

## Receiver report

Every call member sends every other member a report about once a second, on the same
channel. It says how the reporter receives the addressee's streams, and echoes the
addressee's last report for the round-trip time.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | version/type | `0x21`: version 2, type 1 = receiver report. |
| 1 | 4 | ts_ms | The reporter's clock, milliseconds, arbitrary start, wraps. |
| 5 | 4 | echo_ts_ms | `ts_ms` of the last report received from the addressee. |
| 9 | 4 | echo_delay_ms | Milliseconds since that report arrived. `0xFFFFFFFF`: no echo (and `echo_ts_ms` is ignored). |
| 13 | 1 | n | Report blocks, `0 ≤ n ≤ 3`. |
| 14 | 18·n | blocks | One per received track (below). |

A report whose length is not exactly `14 + 18·n`, or with `n > 3`, is dropped.

Report block — how the reporter receives one stream of the addressee:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | track | As in the media packet. |
| 1 | 1 | fraction_lost | Share of packets lost since the previous report, × 256 (RFC 3550 §6.4.1). |
| 2 | 4 | stream_id | The stream the block is about. Blocks about another stream are ignored. |
| 6 | 4 | highest_seq | Extended highest `seq` received: wraps × 2¹⁶ + `seq`. |
| 10 | 4 | cumulative_lost | Packets expected minus received since the stream started, never below 0. Retransmissions (`0x24`) don't count as received. |
| 14 | 4 | jitter | Interarrival jitter (RFC 3550 §6.4.1) in the track's clock units. |

**Round-trip time.** On a report with an echo, the addressee computes
`rtt = now_ms − echo_ts_ms − echo_delay_ms` on its own clock (mod 2³²). Values outside
0…60 000 ms are ignored; the rest are smoothed (7/8 old + 1/8 new).

A hostile member can lie in its reports. Until bandwidth estimation (ADR-014 step 7),
reports only feed statistics; after it, a lie can lower only what we send to everyone,
which a member can already cause by dropping our packets.

## Transport feedback

Every member tells every other member, every 100 ms while packets are coming, when each
of that member's packets arrived. The sender knows when it sent each one; growing gaps
between arrival and departure mean a queue is building somewhere on the path.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | version/type | `0x25`: version 2, type 5 = transport feedback. |
| 1 | 2 | base_seq | `tw_seq` of the first packet covered. |
| 3 | 2 | count | Packets covered: `base_seq … base_seq + count − 1`, `1 ≤ count ≤ 1024`. |
| 5 | 4 | ref_time | Arrival of the earliest received packet among them, in 250 µs units on the reporter's clock (arbitrary start, wraps). |
| 9 | 2·count | deltas | Per packet: arrival after `ref_time` in 250 µs units (capped at `0xFFFE`, 16 s); `0xFFFF` = not received. |

A feedback whose length is not exactly `9 + 2·count`, or with `count = 0` or
`count > 1024`, is dropped.

**Reporter.** Each feedback covers, without gaps, from the first `tw_seq` not yet
reported to the newest received; a packet arriving after its range was reported is
not reported again. A range longer than 1024 keeps its newest 1024.

**Sender — bandwidth estimate** (per member, after GCC, draft-ietf-rmcat-gcc-02):

- *Delay.* Packets sent within 5 ms of each other form a group. For consecutive groups,
  `(arrival gap − departure gap)` is accumulated, smoothed (0.9), and the slope over
  the last 20 groups (least squares) is compared with an adaptive threshold (starts at
  12.5, follows |trend| up slowly and down fast, 6…600). Above it for more than 10 ms
  and still rising: overuse. Below its negative: underuse.
- *Delay-based rate.* Overuse: down to 85 % of the bitrate that actually arrived in the
  last 0.5 s, at most once per RTT + 100 ms. Underuse: hold. Otherwise grow by 8 % a
  second, or by one 1200-byte packet per RTT + 100 ms near the bitrate of the last
  overuse.
- *Loss-based rate.* Loss is counted over windows of at least a second, minus the
  link's inherent loss: a level that drops at once to any lower measurement and rises
  by a tenth of the difference per window (≈ 10 s), capped at 10 %. Constant loss that
  does not depend on our rate (radio, a starved CPU on a node) thus fades into the
  background, while a burst from congestion counts in full. Above 10 % the rate drops
  by half that excess (from the current estimate); below 2 % it grows by 8 % a second;
  in between it holds.
- *Estimate* = the lower of the two. They are kept apart (as in WebRTC) so that
  delay-triggered cuts cannot ratchet the estimate down while moderate loss forbids
  growth. Both start at the upper bound (set by the caller from the codec settings)
  and stay between the bounds.

The encoders get the lowest estimate across members (one encoder serves everyone). A
member who lies in its feedback can lower only what we send, which it could already
cause by dropping our packets.

**Sender — pacing.** Video and retransmissions leave at 2.5 × the estimate × the number
of members, with at most 5 ms of unused rate saved up, so a keyframe spreads over
milliseconds instead of overflowing a queue on the path; no video packet waits longer
than 0.5 s (the pace rises instead). Control packets (reports, feedback, NACK, keyframe
requests) and audio are never delayed; retransmissions go before new video.

## NACK

A receiver that sees a gap in a video track's `seq` asks the sender to repeat the missing
packets (RFC 4585 Generic NACK semantics).

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | version/type | `0x22`: version 2, type 2 = NACK. |
| 1 | 1 | track | The addressee's track the packets belong to. |
| 2 | 4 | stream_id | The addressee's stream. A NACK about another stream is ignored. |
| 6 | 1 | n | Pairs, `1 ≤ n ≤ 128`. |
| 7 | 4·n | pairs | `pid` (u16): a lost `seq`; `blp` (u16): bit *i* set means `pid + i + 1` is lost too. |

A NACK whose length is not exactly `7 + 4·n`, or with `n = 0` or `n > 128`, is dropped.

**Receiver.** A missing `seq` is first requested 20 ms after the gap is seen (fragments of
one frame leave together and jitter reorders them; asking at once would repeat packets
that are merely late), then again every RTT + 30 ms, at most three times, and forgotten
once the frame is no longer awaited. More than 128 missing at once is not worth repeating:
the list is dropped and the frame logic below asks for a keyframe instead.

**Sender.** Keeps the video packets of the last 1.5 s (at most 4 MiB) and repeats a
requested one to the member who asked, as `0x24`, if it is still there. Retransmissions
to a member take at most a quarter of the bytes first sent to that member, plus a reserve
of 256 KiB that starts full and refills from that quarter; a request that does not fit
is dropped. A member who NACKs everything therefore costs at most 25 % more traffic to
itself and cannot make us flood the others.

## Keyframe request

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 1 | version/type | `0x23`: version 2, type 3 = keyframe request (like a PLI). |
| 1 | 1 | track | The addressee's video track. |
| 2 | 4 | stream_id | The addressee's stream. A request about another stream is ignored. |

Exactly 6 bytes, otherwise dropped.

**Receiver.** Asks when the decoder has nothing to build on: a complete frame had to be
thrown away for lack of its reference, a missing frame was not recovered in time, or the
decoder was restarted (the tile appeared on screen, the decoder failed). It repeats the
request once a second until a keyframe arrives. A stream that starts with a keyframe
still on its way asks for nothing.

**Sender.** Honours one request per member at most every 900 ms (so a once-a-second
retry is never refused for timing noise) by making the next video frame a keyframe.
Without requests, the sender still sends a keyframe every 10 s as a safety net.

## Receiver rules

These are the limits that keep a hostile call member from exhausting memory. They are
part of the contract: a receiver that accepts more is not wrong on the wire, but it is
unsafe.

- **New stream.** Receive state is kept per (sender, track) for one `stream_id`. A packet
  with a different `stream_id` replaces that state: the jitter buffer or frame
  reassembler starts empty.
- **Reassembly.** A frame is delivered when all its fragments have arrived, in any order.
  A fragment whose `frag_count` disagrees with an earlier fragment of the same frame is
  dropped. At most 64 frames and 16 MiB of frame data (incomplete, or complete and
  waiting for an earlier one) are kept; beyond that the oldest frame is dropped.
- **Order.** Frames go to the decoder strictly in `frame_id` order, because each P-frame
  refers to the one before it. When the next frame is missing and newer ones are
  complete, the receiver waits for it (its fragments come back after the NACK) for
  2.5 × RTT + 50 ms, between 100 and 800 ms. A complete keyframe newer than the missing
  frame ends the wait at once: everything before it is dropped.
- **No reference.** If the wait runs out, or the stream starts with P-frames, the
  receiver delivers nothing until a keyframe: P-frames are dropped and a keyframe is
  requested. Frames after a keyframe that is still being completed are kept for it.
- **Late frames.** Frame ids compare in serial-number arithmetic (half the circle ahead
  is newer). A frame older than the next one expected is dropped.
- **Playout.** Each frame gets a time to show it: its capture timestamp plus an offset
  that covers 99 % of the recent frames (arrival − timestamp over the last 10 s), so
  frames are shown at the sender camera's pace whatever its real frame rate, and the
  clock offset and drift between the two phones are absorbed by the window. The offset
  rises at once and falls by at most 50 ms per second (100 ms per second while it is
  more than 250 ms above what is needed): playback runs at most 5–10 % fast. A frame
  is never scheduled before it is ready nor before the previous one, and never more
  than a second out. The frame goes to the decoder at that time and is shown as soon as
  it is decoded: the device's decoder delay is constant and only shifts the whole
  stream.
- **Decoder queue.** When the oldest queued frame was due more than 0.5 s ago — nobody
  is decoding (the member's tile is off screen, the decoder stalled) — the queue jumps
  to its newest keyframe; P-frames are never dropped one by one. If there is no
  keyframe to jump to and the consumer is still taking frames (it pulled one within the
  last second — a slow decoder, not a hidden tile), a keyframe request (`0x23`) is sent,
  under the usual retry limit. A queue nobody drains
  is thrown away at 150 frames or 32 MiB, and the receiver then quietly waits for the
  next keyframe.
- **Audio.** Packets are reordered by `seq` in a jitter buffer. A packet older than the
  next one to play (serial arithmetic on 2¹⁶) is dropped. A missing packet is declared
  lost once two later packets are waiting; the decoder then recovers it from the in-band
  FEC copy in the packet after it, or conceals it if that packet is missing too. Audio is
  never retransmitted.
- **Members only.** Packets from someone who is not a current call member are dropped,
  and a member who leaves loses all receive state.
