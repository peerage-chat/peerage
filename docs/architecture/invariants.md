# Architecture invariants

Cross-cutting rules whose violation causes *silent* or *catastrophic* bugs (a native crash
of the whole process, encryption state falling out of sync, lost messages). Keep them in
mind for any change in the affected areas. Every contributor — human or AI-assisted
session — must follow them.

Status of each: ✅ holds in the code / ⚠️ partly / 📋 a rule for the future.

---

## 1. The FFI/JNI boundary (Rust ↔ Kotlin) — the most fragile area

A Rust panic at the JNI boundary = **a native crash of the whole Android process** (without
a Java stack trace).

- ✅ **`catch_unwind` on EVERY JNI entry point.** The macro `jni_safe!` in
  `androidjni/src/lib.rs` wraps the body and turns a panic into a log line plus a safe
  default. New JNI functions **must** go through it; the unit test
  `every_jni_entry_point_is_guarded` fails the build otherwise.
- ✅ **No `unwrap()`/`expect()` on fallible operations in FFI code.** Return a C-compatible
  result (a code, a status, an empty array — `empty_bytes()`), never panic.
- 📋 **Memory ownership: whoever allocated it frees it.** A buffer created in Rust is not
  freed by Kotlin directly — only through a destructor or a call back into Rust. And vice
  versa.

## 2. Transport channel discipline (Yggdrasil datagrams) — ✅

Two channels, by the first byte of the datagram (`core/src/media.rs`):
`CHAN_CHAT = 0x01`, `CHAN_MEDIA = 0x02`.

- **CHAN_CHAT** — reliable: id + ack + retry until confirmed (`send_reliable` in
  `messenger/src/lib.rs`). For: chat messages, keys (MLS Welcome/KeyPackage), control
  (RoomInvite, Ack, presence control).
- **CHAN_MEDIA** — fast: light datagrams **without acks**, loss is acceptable. For:
  real-time audio/video only.

**Forbidden:** sending keys, chat or files over `CHAN_MEDIA`; pushing media through the
reliable ack transport on `CHAN_CHAT` (acks on video = an avalanche of confirmations and
dead latency).

## 3. SQLite (core: contacts / messages / outbox) — ✅/⚠️

`core/src/db.rs`, SQLCipher. **All conversations live here**, in one table
`chat_messages`: 1:1 chats, Saved messages and groups ([ADR-009](../adr/ADR-009-one-message-store.md)).
Group history used to live separately (a list in memory + a JSON file), and every chat
feature was written twice; `engine/groups` keeps only MLS state and service files.

- ✅ The row key is `(chat, scope, sender, id)`: the author is part of the key, because in
  a group "mine / not mine" does not replace it (two members' ids can coincide).
- ✅ Pagination by the cursor `(time, rowid)`, not by time alone: time is not unique.
- ⚠️ A new history column also means `MSG_COLS`/`read_msg` in `store.rs`: the column order
  there is shared by all queries, and if they diverge, fields get silently mixed up.
- ✅ Search goes through the FTS5 index `messages_fts` (external content over
  `chat_messages`, kept in sync by triggers); on a SQLite build without FTS5 it quietly
  degrades to `LIKE` — search that "sometimes does not build" is worse than slow search.
  Old databases are re-indexed once, by `PRAGMA user_version`; you cannot count rows in the
  index itself (with external content, `COUNT(*)` reads the main table and is never
  "empty" — which is why search over old conversations used to stay silent).
- ✅ The encryption key comes from the `vault`; `PRAGMA journal_mode = WAL`;
  `foreign_keys = ON`.
- ✅ **`PRAGMA busy_timeout = 5000`** — wait up to 5 s for a lock instead of an immediate
  `SQLITE_BUSY` (WAL checkpoint contention / a possible second connection).
- ✅ **Writes are serialised through `Arc<Mutex<Connection>>`** (one connection). That is
  the *main* protection against concurrent writes; `busy_timeout` is the safety net.
- 📋 If a second connection ever appears: writes in transactions; on `SQLITE_BUSY` —
  exponential back-off and retry.

## 4. Persisting MLS state — ✅ (an iron rule)

> **Every operation that moves MLS state (send / receive / commit) MUST save the group
> (`save_group`).**

The MLS ratchet moves on sending too. If the sending ratchet is not saved, after a restart
you send from an "old" generation → the receiver has already deleted that secret for
forward secrecy → `"secret deleted to preserve forward secrecy"` → messages stop arriving.
See `engine/src/groups.rs` (`send_text`/`on_group`/`on_key_reply` all call `save_group`)
and the fixes 7e5fc2a/a86248c. Since ADR-004 a lagging member is healed by resync
(Remove + Add); a group that diverged before that has to be re-created.

The save itself must be crash-safe — see §11.

## 5. UI / Android lifecycle — ✅/📋

- ✅ **The UI NEVER calls `Native.*` directly.** Any JNI call may block (network, FFI,
  disk, mutexes) → ANR. Everything goes through background threads: `pollThread` (events),
  `tickerThread` (status), `ops` (a single-thread executor that serialises operations). See
  the doc block in `Model.kt`.
- ✅ **State is Compose snapshot state** (`mutableStateOf/List/Map`), safe to write from
  background threads (recomposition is scheduled by itself). StateFlow is deliberately not
  used.
- 📋 **The node is down or off → the UI goes Offline immediately**, and input is disabled
  (do not pile up operations into nowhere). Coverage to be checked.

## 6. Domain separation of keys (SEC-5) — ✅ (a rule for NEW keys)

> **Every new symmetric key is derived with `Identity::derive_subkey("dev.p2p.<domain>.vN")`
> (BLAKE3 KDF), NOT by reusing the seed, the identity or the Box key.**

Each domain has its own unique label → an independent key. This prevents cross-protocol
reuse (a signature or key from one context being valid in another).

**Existing keys are deliberately NOT touched** (changing them = breaking the identity or
compatibility):
- **The ed25519 identity** = the MLS credential + the basis of the contact record and the
  fingerprint. It is the node's identity — shared by definition.
  *ADR-007 refinement:* the transport key (the Yggdrasil address) is **no longer** the
  identity; it is derived per epoch with domain separation; epoch 0 equals the identity, so
  nothing changed for those who never changed their address. See §7.
- **The X25519 Box secret** = `SHA-512(seed)[..32]` — the standard libsodium construction
  (`crypto_sign_ed25519_sk_to_curve25519`), analysed by the crypto community.
- **The MLS signing key** = the identity: the binding `sig_key == pubkey` is the H1
  authentication (the leaf is signed by the contact's key). Separating it → after 1.0 (it
  needs a certificate chain).

## 7. Identity and address are different keys (ADR-007) — ✅

> **A peer is identified by its IDENTITY (IK), not by its address (TK).** Encrypt to the
> identity, send to the address. Everything keyed to a peer — history, contacts, trust
> tier, blocklist, MLS roster, events — is keyed by the identity.

A Yggdrasil address is derived from the node key, so "change the address" = "change the
key". So that this does not cost the identity, the transport key is separate:
`TK_n = derive_subkey("dev.p2p.transport.v1/" + n)`, and **epoch 0 is the identity key
itself** (installations that never changed their address work as before).

Practical consequences, each paid for by a bug found:
- **Receiving.** The rule "`from == src`" is replaced by "the binding is confirmed":
  `from == src`, or `src` is a known address of that identity, or it is a signed change
  announcement binding `from → src`. After that, the code sees only the identity.
- **Sending.** Resolving "identity → address" lives in ONE place (`Shared::send`). Do not
  add a second sending path: the single point is exactly what let groups survive rotation
  without a single change.
- **Budget and blocklist.** They are keyed by identity, but the receive layer sees the
  address → resolving address → identity BEFORE the tier check is mandatory. Otherwise a
  contact is demoted to a stranger after changing address, and a blocked peer escapes the
  block.
- **Early drop in libygg** cuts by address, so on an address change the block must be moved
  to the current key (and removed from the abandoned one).

Checked by `engine/tests/rotate.rs` (6 scenarios, including an attempt to escape a block by
rotating).

## 8. Long-lived formats: protection parameters live in the file itself

Any on-disk format whose decryption depends on **parameters** (KDF cost, algorithm, salt
length) must store those parameters (or a profile number) inside. Keeping them only in code
means losing data at the first update: a key derived with other parameters is a DIFFERENT
key, and the user sees "wrong password" for the right password and blames themselves.

Applies to `vault.meta` (the third line is the profile number) and the `.p2pbak` header
(the profile byte). The profile is stored as a NUMBER from the table `core::kdf::profile`,
not as numbers: numbers from a file are input, and "memory 4 GiB" would take the app down
before any password check. The meaning of an existing number must never change — only new
ones may be added (test `profile_1_parameters_are_frozen`).

## 9. Kotlin → engine: serialisation only through `WireJson`

`kotlinx.serialization` by default **does not write fields equal to their default value**.
Because of this the backup selection reached the core as an empty `{}` ("include nothing"),
and it was caught by a live run, not by a test: each side worked correctly on its own. Every
DTO that goes out is serialised through `WireJson` (`encodeDefaults = true`); `EngineJson`
is only for reading incoming data.

## 10. Waiting for a peer's answer needs ITS OWN retry

`send_reliable` resends an unconfirmed packet only for `message_ttl` = **5 minutes**, after
which it is quietly dropped. That is enough for "the other side's connection blinked" and
not enough for anything that waits for a human on the other side: the inviter to a group,
someone who accepted a contact request and so on may come back a day later.

Rule: every protocol step that WAITS for the other side's action must
1) survive a restart (state on disk, not only in memory),
2) have its own retry on the beacon (every 2 s) with a sensible interval,
3) repeat THE SAME cryptographic material if the answer depends on it (a fresh KeyPackage
   would make a Welcome for the previous one undecryptable).

The rule is symmetric: it also applies to the side that WAITS for someone else's answer. If
we invited someone, "whom and where" must be on disk (table `group_invites_sent`) —
otherwise after our restart an incoming answer has nothing to match and silently vanishes.
And the other way round: a repeated invitation must not overwrite consent already given.

Presence is only an accelerator: it does not arrive for everyone (a non-contact does not
send it), and it must not be the only trigger. A real example and analysis: `BUGS.md`,
"a group invitation was lost if the inviter went offline".

## 11. State files are written crash-safely — ✅

Android kills the process at any moment. A plain `fs::write` truncates the file first and
writes afterwards; being killed in between leaves an empty or cut file — for MLS state that
is a group that can no longer be loaded, for the profile a lost identity.

> **Every state file is written through `p2p_core::fsutil::write_atomic`** (or
> `write_atomic_private` for keys and seeds, 0600): a temp file next to the target →
> `sync_all` → `rename` over it.

The reader sees either the old version or the new one in full. The same applies to blobs
(small ones through `write_atomic`, large ones are assembled in a `.part` file and renamed).

## 12. Hanging up is never queued behind the peer — ✅

A call peer controls how much traffic reaches us. If our own "hang up" waits in a queue
that the peer's traffic also feeds, the peer can keep our microphone live after we hang up
(Signal RingRTC CVE-2026-113380: a flooded call event queue).

> **Leaving a call stops sending at once and on its own path.** In the engine
> `RoomCallEngine::halt` (no recipients, running flag down) runs before
> `room_audio_stop` returns; the threads are joined in the background. In the client the
> teardown runs on its own thread, not on `ops`. Because the call START lives on `ops`,
> start and teardown share a lock and a call generation: a start queued before the
> hang-up sees the new generation and does nothing.

Guard test: `engine/tests/gate_rooms.rs::hangup_stops_our_audio_at_once_under_peer_flood`.

---
Related: [tokens.md](../design/tokens.md), [ADR-003](../adr/ADR-003-engine-api.md) (engine
↔ client boundary), [ADR-007](../adr/ADR-007-identity-transport-key-split.md) (identity vs
address), [ADR-008](../adr/ADR-008-proof-of-transit.md) (transit incentives).
