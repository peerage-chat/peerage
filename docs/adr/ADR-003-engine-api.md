# ADR-003: The engine API (a Command/Event facade) and the engine ↔ client boundary

**Status:** Proposed 2026-06-27; implemented in part (see "State as of 2026-09" at the end).
**Date:** 2026-06-27
**Decided by:** project owner

## Context (the problem)

The principle "logic in Rust crates, UI and JNI are thin adapters" was written down in
[SUBSYSTEMS.md](../SUBSYSTEMS.md), but **in practice the boundary drifted**. Symptoms:

1. **The engine ↔ client interface was ad-hoc JSON over JNI**
   ([P1](../archive/architecture-review-2026-06-16.md)). There was no typed, versioned
   contract: every new field = a change to a JSON event in Rust + parsing in Kotlin + UI.
   Example (2026-06-27): adding the sender to a group message touched 5 files in 3 crates +
   Kotlin.
2. **Domain logic leaked into the client.** The offline queue TTL was hard-coded in
   `Model.kt`; "N members · M online" was computed in Kotlin (`groupMembers ∩ online`); the
   network statistics (ping, traffic, peer list) were not provided by the engine, so the
   client drew "—".
3. **Two call/media engines** (desktop `call` vs Android `androidjni/room_call`). For 1.0 the
   release client is **Android only**, so full unification is postponed; the release media
   engine is one (Android).
4. **No versioning** of the wire (P2) → it could not evolve without breaking.

Consequence: features went "every which way" — each one reached into internals, and there
was no single place for them.

## Decision

Introduce **one engine facade with a typed contract** and make it the ONLY surface between
the engine and any client.

### The boundary (one line per rule)
- **The engine owns all domain logic and state** (identity, contacts, chats, groups, rooms,
  presence, offline queue, blobs, network statistics, settings).
- **The client is an adapter:** it renders state and sends intents. It **does not compute**
  domain logic (no TTLs, no presence intersections, no protocol parsing in Kotlin).
- **The contract is an API, not scattered JSON strings.** One channel of commands in, one
  stream of events out, plus a state snapshot query.

### Shape of the API
- `enum Command` — client intents (`SendDm`, `JoinRoom`, `SetRelay`, `RequestBlob`,
  `OpenChat`, `SetSetting`…). Exhaustive and versioned.
- `enum Event` — notifications and state changes going out (`Message`, `Presence`,
  `RoomMembers`, `NetStats`, `BlobReady`, `DeliveryStatus`…).
- **Snapshots / queries** — current state as lists (contacts, history, group members with
  presence, network metrics), so the client does not rebuild it.
- **`protocol_version`** in the contract; evolution is additive (new variants and fields do
  not break old clients).

### Where it lives
A platform-independent crate **`engine`**, into which the orchestration moves (it was spread
over `androidjni/lib.rs` + `groups.rs`, plus what leaked into `Model.kt`). `androidjni`
becomes a **thin JNI codec** over `engine`. The desktop can later sit on the same `engine`
(not required for 1.0).

### What to bring back into the engine (from the client)
- **Network statistics** — peers, ping, traffic, the list of peer addresses.
- **Offline queue:** TTL and policy → an engine setting; the queue counter comes from the engine.
- **Group presence:** the engine computes "N members · M online" and hands it over ready.
- **Message model:** ids, ordering, dedup — the engine (S7). Time formatting and date chips
  stay in the client (that is UI, not domain).
- **Settings** (energy, media, storage, TTL) — one store in the engine (F3).

## Options (what we rejected and why)
- **Keep JSON per JNI call as it was** — cheap now, but exactly what produces the chaos and
  blocks versioning (P1/P2). Rejected.
- **Formalise the API inside `androidjni`** (no separate crate) — less moving, but ties the
  contract to Android and gets in the way of the desktop and of tests without JNI.
  Compromise: start there and move to `engine` once the shape settles.
- **Binary IPC right away (flatbuffers/protobuf over JNI)** — best performance, but
  premature; start with a typed Rust API, and the JNI codec can stay JSON for now (it is not
  the bottleneck).

## Consequences
**Pros:** one place per feature; the engine is tested without a device; the desktop can reuse
it; versioning unblocks evolution; the network screen stubs get honest data.
**Cons / cost:** a one-off refactor (extracting `engine`, a JNI codec); some boilerplate for
serialisation; discipline NOT to push logic into Kotlin.

### Migration (the app keeps working)
Incrementally, subsystem by subsystem, not "rewrite everything at once":
1. Declare `Command`/`Event`/`Snapshot` (types) — first mirroring the current behaviour.
2. Move orchestration from `androidjni` into `engine`; JNI → a thin codec.
3. Bring back one subsystem at a time (start with **network statistics** and **offline TTL** —
   they clearly leaked and give a quick visible result), deleting logic from `Model.kt`.
4. Every step — build the `.so` + check on the emulator, keeping features alive.

## Open questions
- A separate `engine` crate right away vs temporarily in `androidjni`.
- FFI encoding: JSON (now) → a typed binary format later, if needed.
- How deep to go before 1.0: what is mandatory (statistics, TTL, presence, settings) and what
  can wait (the full message model S7).

## State as of 2026-09

- **Done:** the `peerage-engine` crate holds all device-independent domain logic (`Engine`,
  the event pump, `GroupManager`, `BlobManager`, `RoomCallEngine`, aggregates); `androidjni`
  is a thin codec with a panic guard on every entry point. Network statistics, group
  presence, the offline queue and its policy (now per trust tier), settings and the message
  model live in the engine. The engine is tested without a device (engine-level integration
  tests over an in-process transport double).
- **Done differently:** the event and snapshot shapes are Rust types in `engine/src/dto.rs`,
  pinned by tests; Kotlin writes only through `WireJson`. Commands are methods of `Engine`,
  not variants of an `enum Command` — the typed enum was never introduced, and there is no
  `protocol_version` field in the client contract yet.
- **Not done:** the desktop client still bypasses the engine.
