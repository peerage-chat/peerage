# ADR-017: Broadcast channels — signed post chains, sealed content-addressed blobs, explicit seeders

**Status:** Proposed 2026-11-24 (discussion with the owner). Part of the extended beta: the
project's own news live in a Peerage channel, not in someone else's messenger.
**Date:** 2026-11-24
**Decided by:** project owner
**Related:** [ADR-016](ADR-016-presence-and-mailboxes.md) (mailboxes, the Peerage peer
network, stage 1 / stage 2), [ADR-015](ADR-015-profile-per-network.md) (signed records,
hidden profiles), [ADR-012](ADR-012-dm-session-v2.md) (sessions), [ADR-003](ADR-003-engine-api.md)
(Command/Event), [ADR-009](ADR-009-one-message-store.md) (one message store), S5
content-addressed blobs (`blob`, `engine/blobs.rs`), `core::budget::Tier`, the channel sketch
in the project notes (2026-10).

## Context (the problem)

The owner wants project news inside Peerage. That needs one-to-many delivery: one publisher,
hundreds to thousands of readers, most of them phones that are online on and off. Nothing in
the engine does this today.

**What exists and what does not fit:**

- **Groups (MLS)** are many-to-many, every member is a cryptographic member, and every
  commit moves state on every device. That cost is wrong for a few thousand readers who never
  write, and a reader's list of other readers is not anyone's business.
- **Blobs (S5)** are content-addressed and verified chunk by chunk, but the id is
  `BLAKE3(plaintext)`, chunks travel inside a 1:1 session, and the source is the person who
  sent the message. A channel needs the opposite: any node that holds a copy is a source,
  and the node that holds it must not be able to read it.
- **Strangers** are cut off from chat by the contact-request gate. A reader and a caching
  node are not contacts. There is, however, a `Tier::Stranger` ingress budget and sessions
  work with any Peerage identity, so a narrow public request surface is possible.

**The earlier sketch** (2026-10, not an ADR) chose content addressing over relay nodes:
posts are encrypted with one channel key, the hash is taken over the ciphertext so every
holder has identical bytes, the publisher signs a small chain head, and heads spread cheaply.
This ADR fixes that sketch into a decision and cuts it to what the news channel needs first.

## Decision

### 1. What a channel is

- **A channel has its own key pair**, not the owner's identity key. The **channel id** is
  `BLAKE3(channel_pub)`.
  - Readers do not learn who runs the channel; the owner can move the key to another own
    device or hand it over.
  - Several admins come later as a signed admin list (§9), without a format change.
- **v1 channels are public**: the link carries the read key, so anyone with the link reads.
  Nodes without the link store ciphertext they cannot read.
- **Private channels** (invite-only, key rotation on removal) come later. The format already
  carries a `key_epoch` on every post, so rotation needs no protocol break.

### 2. Posts: a signed chain of sealed blobs

```
post (plaintext, inside the seal) = {
  channel_id, seq, prev, ts, key_epoch,
  body   (text, ≤ 16 KiB),
  attach [ {manifest, key} … ]   — sealed attachment blobs (§3)
}
signature = Sign(channel_key, post)                  — inside the seal
sealed    = XChaCha20-Poly1305(read_key[key_epoch], post ‖ signature)
post_id   = BLAKE3(sealed)
```

- **`seq` grows by one, `prev` is the id of the previous post.** The history is a hash chain,
  as in git: one verified head vouches for everything before it.
- **The signature is inside the seal.** A node without the read key cannot even confirm who
  signed, only store bytes by hash.
- **`post_id` is the hash of the ciphertext**, so every holder serves identical bytes, any
  holder is a valid source, and the existing chunk verification applies unchanged.
- **Editing and deleting** are later posts that refer to an earlier `seq` (the same idea as
  edits in chats, ADR-006). A chain is never rewritten.

### 3. Attachments: sealed content-addressed blobs

- Each attachment is encrypted with its **own random key**; the key and the manifest ride
  inside the (sealed, signed) post. Blob id = `BLAKE3(ciphertext)`.
- Chunking, manifests, limits and resumable download are the existing `blob` crate, applied
  to the ciphertext. Only the source changes: any holder instead of "the sender".
- Thumbnails stay separate small blobs (as in S5): a reader sees the feed without pulling
  full media.

### 4. The head: a public signed record

```
ChannelHead (SignedRecord, record type "channel head", signer = channel key) =
  { channel_id, seq, post_id, ts }
```

- About 100 bytes, **not encrypted**: it reveals only that a pseudonymous channel moved to
  `seq` at `ts`.
- Signed with the common record format of ADR-015 §3 (a new record type and a new signer
  kind, "channel key").
- **Fork detection:** two valid heads with the same `seq` and different `post_id` mean the
  channel key is compromised or misused. The client keeps the first, stops following, and
  shows a warning.

### 5. Where content lives: explicit seeders (stage 1)

- **The channel manifest** is a signed record (signer = channel key):
  `{ channel_id, name, about, avatar blob, seeders[] }`. Each seeder entry is a Peerage
  identity with its address record.
- **Seeders are chosen by the owner**: the owner's always-on node (`peerage-cli`, §8), a
  trusted contact's device in peer mode, or a volunteer who sent an offer that the owner
  accepted. Approval is mandatory: an unapproved seeder list invites Sybil nodes that
  collect reader metadata.
- **A seeder stores every post and attachment** of the channels it seeds (archive), within
  its own quotas, and does not need the read key.
- **No discovery network in v1.** Readers learn seeders from the manifest, exactly as
  ADR-016 stage 1 lists mailboxes explicitly. Gossip of heads between readers and finding
  holders through the Peerage peer network is stage 2 (ADR-016 §4).

### 6. Publishing and reading

**Publishing** (owner device):

1. Build and seal the post, store it locally.
2. Push the post blobs and the new head to every seeder (direct, reliable channel, retried
   like any outbox item).
3. The post counts as **published** once at least one seeder confirmed it. Until then the
   owner sees it as pending.

The owner device does **not** serve readers directly when seeders exist: serving would show
its network address to every reader. With no seeders it serves, and the UI says so.

**Reading** (subscriber):

- **Poll** a seeder for the head: on app start, on returning to the foreground, and on a
  background schedule with jitter (the same scheduler as mailbox fetches, ADR-016).
- **Push:** a seeder that holds a session with a subscriber may push a new head.
- On a newer head, walk `prev` back to the last known post and pull the missing posts by id,
  from any seeder. Attachments are pulled when shown (as in chats).
- **A new subscriber** gets the **last 50 posts**; older ones load on scroll from seeders
  that keep the archive.

### 7. The public request surface

Readers and seeders are usually not contacts, so a channel needs requests that the contact
gate lets through. They are **exactly these**, and nothing else is accepted from strangers:

| Request | Answer |
|---|---|
| `channel_head(channel_id)` | the latest head the node has, or "unknown" |
| `channel_manifest(channel_id)` | the latest manifest record |
| `blob_part(blob_id, chunk range)` | chunks of a sealed blob the node holds |
| `seed_push(head, blobs…)` | only from the channel key holder to its approved seeder |
| `seed_offer(channel_id)` | a volunteer asks the owner to become a seeder |

- **Budget:** strangers use `Tier::Stranger` ingress (cut before decryption, as now); a seeder
  adds a per-channel and per-requester quota.
- **Capability bit:** `Caps::CHANNELS` in the handshake (next to `MAILBOX`, E1), so a client
  asks only nodes that understand channels.
- **Format freeze:** everything here is new message types and new record types. Old clients
  ignore unknown types (E2 holds), nothing existing changes.

### 8. `peerage-cli`: the always-on node

A headless build of the same engine, run on any server or old computer:

- a regular Peerage identity, not a server role: it seeds channels it was approved for and,
  with ADR-016, holds mail for its owner and the owner's contacts;
- configured by a file and a few commands (create identity, show contact code, accept a
  seeding approval, quotas);
- **a separate ADR** covers the binary, its config, updates and limits; this ADR only needs
  that such a node exists.

### 9. Later, without a format change

- **Several admins:** a signed admin list (signer = channel key); posts signed by an admin key.
- **Private channels:** a new `key_epoch`, new read key handed to remaining readers.
- **Reactions and view counts:** need aggregation at seeders; not in v1.
- **Comments:** a linked group; not in v1.
- **Gossip and finding holders:** stage 2 with the Peerage peer network.

### 10. The official channel

- The **Peerage news channel** (id, read key, seeders) ships in the app as a default
  subscription; the reader can leave it.
- The id compiled into the app is the trust anchor: a lookalike channel named "Peerage"
  cannot pass for it.
- The same channel is the first content a new user sees, so the app is not empty before the
  first contact.

## What we pay

- **Seeders see who reads:** the reader's network address and which channel id it asks
  about (not the content, unless they also hold the link). Recorded in the threat model;
  hidden profiles reach seeders through their own network.
- **Public channels are public:** anyone with the link reads. That is the point for news, and
  the UI must say it when a channel is created.
- **Availability depends on seeders** until stage 2: if all seeders of a channel are down,
  new posts wait.
- **A new public request surface** for strangers: a fixed list, budgeted, fuzz-tested.

## Plan

- **C1 (`core`):** post seal/unseal and signature, `ChannelHead` and manifest records (new
  record type and signer kind), chain walk with fork detection; unit tests and property tests.
- **C2 (`engine`):** store (new tables with migration), publish, subscribe, poll, pull by id,
  the public request surface with `Tier::Stranger` quotas, `Caps::CHANNELS`; loopback tests
  owner → seeder → reader without the owner online.
- **C3 (`engine`):** the seeder role (accept approval, store, serve, quotas).
- **C4 (Android):** channels in the chat list, the channel screen, create/post for owners,
  subscribe by link or QR; spec in `app-ui.md` first, mockups by the owner.
- **C5 (`peerage-cli`):** separate ADR, then the binary; seeds the official channel.
- **C6:** the official channel as a default subscription.

## Open questions

1. **History for a new subscriber:** the last 50 posts (proposed) or only from the moment of
   subscribing (the 2026-10 sketch)?
2. **Where channels appear:** a "Channels" folder in the chat list (proposed) or mixed with
   chats?
3. **Owner without seeders:** allow publishing from the phone alone (it then serves readers
   and reveals its address), or require at least one seeder?

## How to check the decision is followed

- A loopback test: owner publishes, goes offline; a reader that never met the owner gets the
  posts from a seeder and verifies them.
- A test: a seeder without the read key cannot open a post or tell who signed it.
- A test: a forged post (bad signature, wrong `prev`, wrong `seq`) is rejected; two heads with
  one `seq` raise the fork warning.
- A test: a stranger's request outside the §7 list is dropped before decryption.
