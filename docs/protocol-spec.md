# Protocol and engine contract

**Status:** describes the current protocol (revised 2026-09). Related:
[ADR-003](adr/ADR-003-engine-api.md) (the engine boundary),
[ADR-006](adr/ADR-006-chat-extensibility.md) (message references and forward
compatibility), [threat model](architecture/threat-model.md).

Two contracts live here: the **wire protocol** between peers (payloads, padding,
long-lived formats) and the **engine ↔ client contract** inside one device.

---

## 1. Engine ↔ client contract

The client (Android over JNI; later the desktop) renders state and sends commands; the
engine computes all domain logic.

- **Commands** are methods of the `Engine` facade (`engine/src/lib.rs`); JNI exposes each
  as `Native.*` (`androidjni/`). A command may return a result synchronously
  (e.g. an id, or `false` if the peer is unknown).
- **Snapshots** are synchronous reads of ready-made state: contacts, a page of history,
  chat summaries, group presence ("N members · M online"), network statistics. The
  engine computes aggregates; the client does not.
- **Events** are asynchronous notifications (incoming message, delivery status, presence,
  room changes, blob received, …) placed on a queue the client polls (`poll_event`).

**Where the schema lives.** The shapes of snapshots and events are Rust types in
`engine/src/dto.rs`, not `json!` literals at call sites. Earlier the schema existed
twice — JSON built in Rust and parsed by string keys in the client — and a renamed field
was discovered only on a device. Now `push_event` takes a `dto::UiEvent`, snapshots are
built from structs, and tests in `dto.rs` pin the wire shape literally, so an
incompatible change breaks the build, not the user. In the other direction, Kotlin
serialises only through `WireJson` ([invariants §9](architecture/invariants.md)).

**Media is a separate hot path**, outside the typed contract: PCM and H.264 cross the
boundary as raw short/byte buffers (`room_push_audio`/`room_pull_audio`,
`room_push_video`/`room_pull_video_packet`). Fragmentation, jitter buffering and mixing
happen inside the engine.

**Identifiers:** keys, group ids and room ids are hex of 32-byte values; timestamps are
unix time (cosmetic); correctness timing uses monotonic clocks inside the engine.

---

## 2. Wire protocol overview

Every datagram starts with a channel byte (`CHAN_CHAT = 0x01`, reliable;
`CHAN_MEDIA = 0x02`, real-time). On `CHAN_CHAT` the body is a binary envelope
`[v:1][from:32][nonce:24][ciphertext]` (version 2), where the ciphertext is a NaCl Box of
the padded `Payload` serialised as MessagePack with field names (`core/src/protocol.rs`).
No send time travels in the clear, and bytes travel as bytes — no hex and no JSON arrays
of numbers, so a 12 KiB blob chunk costs about 12.3 KiB on the wire. The same MessagePack
form is what the offline queue stores. Shared Box keys are cached per peer (bounded), so
the X25519 exchange is not redone for every packet.

| Group | Payloads |
|---|---|
| Session | `Hello`, `Ping`, `Pong`, `Ack` |
| 1:1 messages | `Text`, `Media`, `Edit`, `Retract`, `Pin` |
| Groups (MLS) | `Group` (MLS message), `GroupInvite`, `GroupKeyRequest`, `GroupKeyReply`, `GroupResyncRequest`, `GroupLeaveRequest` |
| Rooms (calls) | `RoomInvite`, `RoomDecline`, `RoomPresence` |
| Blobs | `BlobManifest`, `BlobManifestPart`, `BlobManifestRequest`, `BlobRequest`, `BlobChunk` |
| Profile / address | `ProfileUpdate`, `TransportRotate` |

**Forward compatibility** (ADR-006): an unknown payload type deserialises to `Unknown`
and is ignored, not treated as an error. The receiver sends no `Ack` for it; the sender's
retry simply expires. New fields are optional and omitted when empty, so older clients
see the same bytes.

Inside an MLS group, the application body is `AppMsg` (`groupchat/src/lib.rs`): `Text`,
`Media`, `Retract`, `Edit`, `Pin`, plus service messages such as the group name.

---

## 3. Anti-abuse (ingress)

- **Mechanism (M1):** a receiver-enforced token bucket per source
  (`p2p_core::ratelimit::RateLimiter`) in the messenger's receive loop. It is applied by
  the Yggdrasil source key **before decryption**, so a flood is dropped before expensive
  crypto. Separate budgets: strict for CHAN_CHAT (the spam vector), generous for
  CHAN_MEDIA (a call is legitimately high-rate). The receiver enforces it, so modifying
  the sender does not get around it. For a mesh source that cannot be disconnected at the
  link level, an exhausted budget means "the session is cut" on input.
- **One budget per peer (M3):** `p2p_core::budget::Tier` is the ONE place from which the
  ingress limit, the outbox cap and the outbox lifetime are derived (earlier these
  policies lived in five places with their own constants):

  | Tier | Ingress (burst / per second) | Outbox lifetime |
  |---|---|---|
  | `Blocked` | rejected **before decryption** | not stored |
  | `Stranger` | 30 / 10 — **one shared bucket for the whole class** | 1 day |
  | `Contact` | 60 / 30 per peer | **unlimited** |
  | `Verified` | 120 / 60 per peer | **unlimited** |

  Strangers share one bucket for the whole class, so a Sybil with 1000 keys does not
  widen its budget. The engine computes the tier (`refresh_budgets`) from contacts
  (`verified`) and the gate's blocklist; a block beats a contact.

  **The outbox is not trimmed by volume, nor (for contacts) by age.** It consists of the
  user's OWN messages: there is no server to keep them for us, so both "expired" and
  "evicted" mean "lost forever" (a contact who comes back after a month must still get
  them). There is no automatic eviction at all; when the queue grows, the client
  **warns** (`OutboxStats::should_warn`, threshold 1000), and the queue can be cleared only
  by an explicit user action (`clear_peer`).

  **The one exception is a download the user asked for.** By tapping "Download" the user
  requested a stream of bytes from the peer; cutting it with the anti-flood budget makes
  no sense, since the budget protects against UNEXPECTED traffic. `Messenger::boost_peer`
  lifts the ingress limit for that peer only and for a limited time (`BLOB_BOOST_FOR`,
  2 minutes, extended by every new request). A block beats the boost: `Blocked` is cut
  before the check.

  The intent to download is itself persistent (table `wanted_blobs`): the peer may be
  offline or leave mid-download — the engine continues by itself when it reappears
  (`resume_wanted`) and survives a restart. The user should not have to watch for the
  other person coming online.
- **Reputation (method 1):** a "useful deed" is a peer fully serving us a blob, i.e.
  spending its own traffic on us. The `helpful` counter in contacts raises the budget
  smoothly up to ×2 (20 deeds). The bonus goes **only to those who passed the gate**
  (`Contact`/`Verified`); otherwise a Sybil would bypass the shared bucket by "helping"
  with a thousand throwaway keys.

### Address separate from identity; changing the address (ADR-007)

**Identity (IK)** — ed25519 from the seed/mnemonic: the fingerprint, QR verification, the
MLS credential and the contact record hang on it. It never changes.
**Address (TK)** — the Yggdrasil node key from which the IPv6 address is derived. It is
consumable: it is changed to cut off someone's flood.

```
TK_n = Identity::derive_subkey("dev.p2p.transport.v1/" + n)     // BLAKE3 KDF
TK_0 = IK                                                       // epoch 0
```

Epoch 0 is the identity key itself, so an installation that never changed its address
behaves exactly as before the split; there is no migration. Epoch keys are deterministic
from the seed — restoring from the mnemonic gives the same addresses, and no separate
secret is stored.

**The announcement.** The new address is sent to contacts as

```rust
Payload::TransportRotate { id, epoch, transport_pub, sig }
// sig = IK.sign("dev.p2p.transport-rotate.v1" || epoch_be || transport_pub)
```

The receiver applies it only if ALL of these hold: the sender is a known contact; the
signature verifies against their **identity**; the epoch is strictly newer than the known
one. Without the first condition the announcement is backed by nothing; without the
second it is a contact-hijacking vector ("I am Alice's new address"); without the third
an intercepted old announcement would send the victim back to an abandoned (possibly
flooded) address. For a blocked sender the announcement is not applied at all.

Delivery is reliable (`id` → retry until ack): a contact who was away at the moment of
the change must learn the new address when they return, otherwise the connection is lost
for good. The engine catches up with those it missed via presence.

**Bucket — APP (4 KiB), not CONTROL.** A signature alone is 64 bytes and does not fit
under the 256-byte ceiling. It turned out better this way: among application messages the
announcement is indistinguishable from ordinary text or an image, so an observer (and the
flooder) cannot see on the wire the moment the victim moves to a new address.

**Anti-spoofing with two keys.** The old receive rule "`from` in the envelope must equal
the transport `src`" stops working once the address ≠ the identity. A packet is accepted
if any of these holds:

1. `from == src` — epoch 0 or a stranger (exactly the old rule);
2. `src` is a known address of that identity (the binding was confirmed by an announcement
   accepted earlier);
3. it is the announcement itself, and its signature binds `from → src` — the
   self-certifying case, without which the new address could not be learned at all: it
   already arrives FROM the new address.

After the check the peer is identified by the identity **everywhere**: history, contacts,
trust tier, blocklist and events are keyed by it. Otherwise changing the address would
split the conversation (incoming written under the transport key), and a blocked peer
could escape the block by simply changing keys.

**Where to encrypt and where to send are different keys.** The body is encrypted to the
recipient's IDENTITY (their X25519 is derived from it), and the datagram is sent to their
current ADDRESS. Neither can be mixed up: encrypt to the address and the recipient cannot
decrypt; send to the identity and the packet goes to an abandoned key.

**Exchange code (QR / "copy key").** Before a rotation — the old bare hex of the identity,
so old codes and scanners keep working. After — an extended form:

```
<identity-hex>:<epoch>:<address-hex>:<signature-hex>
```

The signature is the same as in the announcement, and it is mandatory: codes are often
passed on through third parties (chats, forums), and without a signature an intermediary
could substitute the address while keeping the identity genuine.

**Groups survive it unchanged:** MLS membership is tracked by identities, and the address
is resolved at the single point of sending, so a member can change address without
re-creating the group.

**What changing the address does NOT give.** It cuts off a flooder's stream only together
with an offline pause (measured: with an instant reconnect the new node inherits the
routing position of the old key and receives ~88 % of the stream; after ~30 s offline the
old key drops out of the tree and there is nowhere to send to). Limits:
[threat model](architecture/threat-model.md) N8 and
[ADR-007](adr/ADR-007-identity-transport-key-split.md).

---

## 4. Messages

### Caption on an attachment

The caption travels **inside** `Payload::Media` (field `caption`), not as a separate text
message. Otherwise "a photo with a caption" is two packets in a row, and an observer
recognises the characteristic pair even though each alone is indistinguishable from text.
The length is cut at the sending layer to `MAX_CAPTION` (1 KiB), so that the media message
stays in the 4 KiB APP bucket; cutting in the UI is not enough — another client might not.

In groups it is the same: the `caption` field of `AppMsg::Media` (the body is encrypted by
MLS and travels on the same `CHAN_CHAT`), with the same ceiling.

### Sending large attachments as a stream

The manifest of a large file is computed **as a stream** (`p2p_blob::manifest_of_file`):
the file is read one chunk at a time, and the hashes and the overall id accumulate on the
fly. When serving, chunks are read from disk by index (`read_file_chunk`), and our own copy
is written in the same chunked, encrypted format as received blobs. Result: sending a
gigabyte file needs a gigabyte of RAM on neither side.

In groups it is the same: `group_send_media_file` computes the manifest as a stream and
stores our copy in chunks. The bytes go out to members ONLY if the attachment is smaller
than `AUTO_DOWNLOAD_LIMIT`; each member requests a large one themselves with a button.
Otherwise one person with an 800 MB video would decide for the whole group whether
everyone spends their data.

To make this possible, `AppMsg::Media` carries card metadata — `size`, `duration_ms`,
`width`/`height` and `thumb` (hex id of a separate thumbnail blob). The preview goes to
everyone at once: it is small, and without it an offer to "download 800 MB" is blind.

The exception is images: they need `scrub` (EXIF/GPS), which needs the whole file in
memory. Images are small, so they take the old path.

### Editing a message

`Payload::Edit { id, target, text }` replaces the text of the sender's OWN earlier message
`target` (for an attachment, its caption). "Whose" is not transmitted: the target is always
the sender's message, because rewriting someone else's text would be putting words in their
mouth, not editing. Only the text changes; type, blob and time stay. **No edit history is
kept:** otherwise "deleted and rewrote" becomes a lie in both directions. Delivery is
reliable (`id`). The bucket is APP, like `Text`: in the CONTROL bucket an edit was rounded
up to a multiple of 256 bytes and gave away the length of the new text (fixed 2026-09-25,
covered by `text_media_and_call_are_indistinguishable_by_size`).

In groups: `AppMsg::Edit { target, text }` inside MLS; the author is taken from MLS, not
from the body.

### Forwarding

`Payload::Text` and `Payload::Media` carry `fwd_from` — **the hex key of the author of the
ORIGINAL message** (empty — the message is our own; an empty field is not serialised, so
the bytes do not change). In groups `AppMsg::Text` and `MediaBody` have the same field.

It is the key that travels, not a ready string "forwarded from Anna": the client draws the
name from ITS OWN contacts, so the label cannot lie — an unknown key shows as truncated
hex, not as someone else's name.

Forwarding a forwarded message keeps the FIRST author, not the intermediary (test
`forwarding_keeps_the_original_author`). An attachment is forwarded only if the forwarder
has its bytes: otherwise the recipient would have nowhere to fetch the content from.

**Privacy is the forwarder's choice.** By default a forward reveals the original author to
the recipient: without a source it becomes hearsay that cannot be trusted. But the
forwarder has a "hide author" switch — then `fwd_anon` goes on the wire without `fwd_from`,
i.e. **the author's key is not sent at all**. The recipient sees "forwarded" without a
name: the message does not pose as the forwarder's own, but the source is not named. The
forwarder's own copy is kept exactly as it went to the recipient — showing oneself a name
the other side never received would diverge from reality (test
`hidden_forward_sends_no_author_at_all`).

### Pinned message

`Payload::Pin { id, target, target_mine, on }` — pin (`on = true`) or unpin a message in a
1:1 chat. A message reference is the pair "author + id" (ADR-006); "whose" is sent from the
SENDER's point of view and flipped on receipt, as for a reply. CONTROL bucket: the same size
as an ack. Delivery is reliable — a pin made while the other person is offline must arrive
later, otherwise the two would see different headers for the same chat.

**A pin is a property of the CHAT, not of whoever set it.** Otherwise "pinned something
important" would mean "pinned it for myself", and the other side would never know.

In groups it is the same — `AppMsg::Pin { target, sender, on }` inside MLS. Only the **admin**
may pin (ADR-004), and this is checked on RECEIPT, against the admin key from local state:
the sender's flag cannot be trusted, since a client can be patched. So a member remembers not
only "am I the admin" but also WHO the admin is (`group_meta.admin_key`): the creator of
the group, and for those who joined — the inviter.

Pins are stored in a separate table `pinned` (chat, kind, id, author, time, by whom), not as
a flag in the history row: another person sets and removes it, and it must survive deleting
the message locally.

### Reply and retraction (ADR-006)

`Payload::Text` carries `reply_to` (id of the quoted message; 0 — an ordinary message) and
`reply_mine` — **whose it is from the SENDER's point of view**. The receiver flips the flag:
their "mine" is "theirs" for us. The reference is the pair "author + id", because the id is
seeded by the sender's clock and may coincide on both sides; without the flag a reply to
one's own and a reply to the other's message would be indistinguishable. The quoted text
does NOT travel on the wire: it is taken from local history, so deleting the original leaves
no diverging copy.

`Payload::Retract { id, target }` — a request to delete OUR message `target` ("delete for
everyone"); `id` gives reliable delivery, so the request arrives even after being offline.
The receiver deletes the row `(peer = sender, id = target, mine = false)` — i.e. someone
else's message cannot be retracted (test `retract_cannot_touch_someone_elses_message`).
Bucket — CONTROL: a retraction is indistinguishable by size from an ack. **The boundary is
honest:** this is a request, not a guarantee — the other person may have read the message
already, taken a screenshot or used a patched client, and the UI must say so plainly.

**In groups it is the same, but the reference is the pair "author + id"** (there are many
participants; a mine/theirs flag is not enough). The group application body (`AppMsg`,
encrypted by MLS) got `id`/`reply_to`/`reply_sender` on text, `id` on media and a new
variant `Retract{target}`. The id is seeded by time in MILLIseconds: uniqueness is needed
only per sender, and seconds would not suffice — two messages in a row would get the same id.

The author of a retraction is taken FROM MLS, not from the packet body: otherwise a member
could erase other people's messages by substituting their key (test
`group_retract_ignores_foreign_messages`). Messages from old clients arrive with `id = 0` —
they can be neither replied to nor retracted: there is nothing to reference, and the UI does
not let you select them.

---

## 5. Anti-traffic-analysis: size buckets (M5)

An on-path observer (A1) sees the size of the encrypted datagram. So that size does not
reveal the TYPE of a message, the plaintext is padded to a bucket **before** encryption
(`core/src/pad.rs`, format `[len: u32 BE][content][zeros]`; `bucket_for` in
`core/src/protocol.rs`). Content larger than a bucket is rounded up to a multiple of it.

| Bucket | Size | What goes in | What it hides |
|---|---|---|---|
| CONTROL | 256 B | `Ack`/`Ping`/`Pong`/`RoomPresence`/`Hello`, **`RoomInvite`/`RoomDecline`**, `Retract`, `Pin`, key request, resync/leave, blob request | a call start is indistinguishable from an ack/ping |
| APP | 4 KiB | `Text`, **`Edit`**, **`Media`**, `Group`, `GroupInvite`, `GroupKeyReply`, `ProfileUpdate`, `TransportRotate`, `BlobManifest` | an image/voice/file is indistinguishable from text; text length and file name are hidden |
| CHUNK | 12 KiB + 256 B | `BlobChunk` | the remainder of the last chunk (the exact file size) |

`unpad` reads the length from the prefix and does not depend on the bucket, so changing
buckets is **compatible in both directions** with old clients. A full chunk fits its bucket
and the envelope stays far below the 65535-byte datagram (test
`padded_blob_chunk_fits_datagram`).

**Limits (honestly, see [threat model N7](architecture/threat-model.md)):** only size is
hidden. Timing and rate of packets, volume (the number of chunks) and **the open first
channel byte** (`CHAN_CHAT` / `CHAN_MEDIA`) are visible. The channel byte is kept on
purpose: demultiplexing and the ingress budget must work before decryption (otherwise a
flood makes the victim spend crypto on every packet), and timing gives away a call anyway
(~50 packets/s). Cover traffic (a constant background) is not done in 1.0 — battery cost.

## 6. Transit claim in the presence beacon (reserved, ADR-008)

`Ping`/`Pong` carry an optional field `transit` of type `TransitClaim` (`digest` — BLAKE3
of a batch of receipts, `count`, `epoch`). **In 1.0 it is always empty, not serialised and
not read by the receiver.**

Why it exists already: privileges for relaying (queue priority, longer TTL) cannot be
granted on self-report — the code is open, and relaying is easy to cut out while keeping
the flag. The honest scheme — receipts signed by the recipient
([ADR-008](adr/ADR-008-proof-of-transit.md)) — comes after 1.0, but the SPACE for it must
be ready now: the presence beacon has to stay in the 256-byte CONTROL bucket, and a
receipt (a 64-byte signature plus a key) is about a hundred bytes, so two of them already
do not fit. So the beacon carries only the digest of
a batch, and the receipts themselves are requested separately.

Tests that hold this property (`core/src/protocol.rs`):
`presence_with_transit_claim_stays_in_control_bucket` — a beacon with a FILLED claim is the
same size as an `Ack`; `empty_transit_field_is_absent_on_the_wire` — an empty field does not
change the bytes on the wire.

---

## 7. Data backup file (`.p2pbak`)

Not a network format, but just as long-lived a contract: a file created today must open in
future versions. Implementation — `core/src/backup.rs`, contents — `engine/src/backup.rs`.

```text
"P2PBAK" | version u8 | kdf_profile u8 | salt[16]
frame*:   len u32 LE | nonce[24] | ciphertext(XChaCha20-Poly1305)
```

* **Frames, not one blob** — attachments can be gigabytes, and a phone cannot hold the
  backup in memory. A frame's plaintext is `[kind u8][payload]`, where `0` is a JSON record
  (manifest / contacts / messages / calls), `1` is a blob (`[id_len][id][bytes]`), `2` is
  the end.
* **Frame number in the AAD + a mandatory end frame.** Each frame is authentic by itself,
  but their ORDER and COMPLETENESS are not: without this, whoever holds the file could
  reorder or cut pieces (for example, remove part of a conversation) without knowing the
  password.
* **`kdf_profile`** — the number of the Argon2id cost profile (`core/src/kdf.rs`). It lives
  IN THE FILE because the cost must grow with hardware, and a key with other parameters is a
  different key: without the recorded profile, the first increase would make every earlier
  backup unreadable, showing "wrong password" for the right password. The NUMBER is stored,
  not the numbers themselves: numbers from a file are input, and "memory 4 GiB" would take
  the app down before the password is even checked.
* **The key comes from the password, not from the keystore.** The file by its nature leaves
  the device and must open where this phone's Keystore is unavailable.
* **Frame ceiling 16 MiB** (attachments above 15 MB are not included and are counted
  separately) — protection against a file that claims a gigabyte frame.

What is NOT in the file: the identity key (a separate backup — 24 words), MLS group state
(a stale ratchet breaks the group), contacts' transport keys (they go stale), the
verification flag (see threat model N10).
