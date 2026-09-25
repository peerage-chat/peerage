# ADR-009: One message store for all chats

**Status:** Accepted 2026-08-03; implemented (table `chat_messages`, migrations of the old
table and of the group JSON files, tests in `core/src/store.rs` and
`engine/tests/group_flow.rs`).
**Date:** 2026-08-03
**Decided by:** project owner
**Related:** [ADR-006](ADR-006-chat-extensibility.md) (features by reference: the pair
"author + id"), [ADR-003](ADR-003-engine-api.md) (engine ↔ client boundary),
[v1.0-scope.md](../v1.0-scope.md) (milestone M5.5 — everyday usability).

## Context (the problem)

1:1 chats lived in SQLite, while groups lived in a `Vec<GMsg>` in memory that was rewritten
in full to a JSON file on every message. Two branches of one entity, "a message", diverged
through the whole stack: protocol, storage, engine API, JNI, client model, screens. The cost
showed up on message actions (reply, retract, selection): every feature had to be written
**twice**, and the second copy debugged from scratch.

Besides the duplication, the file-based variant had direct defects:

- O(n) writing on every message, and the whole group history in memory;
- no pagination: opening a chat loaded the entire conversation and pushed it as one JSON
  through JNI;
- no search over messages — nothing to search a file with;
- **group conversations did not make it into the backup at all** (the backup knew only
  about the table), i.e. "restored — the group chats are gone";
- the row key in SQLite was `(chat, id, mine)`, but in a group the author is not "me / not
  me": two members with the same id would overwrite each other.

## Decision

**One table `chat_messages` for 1:1 chats, Saved messages and groups.**

- `chat` — with whom / where: the peer's key (32 B) or the group id;
- `scope` — 0 a 1:1 chat (including notes to self), 1 a group. A separate column, not "a
  guess by length": the rule "32 bytes means a peer" would one day turn into a group
  pretending to be a chat;
- `sender` — hex of the author. A message reference is the pair "author + id" (ADR-006), and
  now it reads the same in a 1:1 chat and in a group. `mine` stays as a label for the UI, but
  **is no longer part of the key**;
- the row key: `(chat, scope, sender, id)`;
- the pagination cursor is `(time, rowid)`: time is not unique, and with time alone messages
  of the same second would either be lost or come twice.

The group layer no longer keeps its own history: `GroupManager` gets the same store as 1:1
chats and writes there. There is one row type for the client (`StoredMessage`), so `GMsg` is
gone.

## Migrations (user data)

Both one-off, each covered by a test:

1. **The old `messages` table** → `chat_messages`: the author is restored from the `mine`
   flag (mine → us, not mine → the peer), and `reply_mine` becomes `reply_sender`. After the
   move the old table is dropped, so a second run does not duplicate anything.
2. **The file `groups/<gid>.msgs.json`** → the store; the file is renamed to `.bak` (not
   deleted: if parsing misunderstood something, the data is still on disk).

## What we pay

- A wider schema: `sender`/`reply_sender` are stored as hex text (64 bytes instead of 32).
  Deliberately: the same form as in the DTO and the group protocol, and the SQL migration of
  the old table is a single `INSERT ... SELECT` without walking rows in Rust.
- `GroupManager` now depends on the message store (a constructor argument) — group tests have
  to open a database. In exchange a group stopped being "special".
- The group history is no longer a human-readable file next to the MLS state; diagnostics go
  through the shared database.

## What it enables

Forwarding, pins, search (FTS5), group chats in the backup and pagination are done **once** —
on the shared path, not once in each branch. That was exactly the goal of milestone M5.5: make
the messenger fit for daily use without doubling the work on every feature.
