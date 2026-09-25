# ADR-006: Chat extensibility — features by reference, without rewrites

**Status:** Accepted 2026-07-05; the forward-compatibility lock (`Payload::Unknown`) is
implemented and tested (`unknown_payload_type_is_ignored_not_error`).
**Date:** 2026-07-05
**Decided by:** project owner
**Related:** [ADR-003](ADR-003-engine-api.md) (the engine ↔ client boundary — the foundation of
additivity), [v1.0-scope.md](../v1.0-scope.md) (what is in 1.0 vs deferred),
[app-ui.md](../design/app-ui.md) + [mockups/](../design/mockups/) (the north-star mock-up).

## Context (the problem)

The v1.0 chat was to be **minimal** (text / voice / image / file + status + reply), and the
social features of Telegram (reactions, pins, forwarding, edits) **deferred**. The reason is not
laziness but the architecture: **we have no server acting as arbiter**. Each such "UI feature" is
really a distributed problem (delivering, ordering and keeping shared state consistent over a
lossy mesh through MLS epochs), not a button. Building them right away would multiply the pain we
went through for plain text and pull focus from what sets us apart (serverless + privacy).

But deferring must not mean painting ourselves into a corner. The owner stated the requirement
plainly: **a clear 1.0 scope + code that lets us add the rest without rewriting thousands of
lines.** So we need a recorded **extensibility contract**: which seams guarantee that deferred
features land additively.

## Decision (the extensibility contract)

The model for ALL deferred chat features: **a feature by reference**. A reaction / reply / edit /
pin does not change the original message — it **references** it and lives next to it. The
reference is the stable pair `(author, id)` (every message already has an `id: u64`, seeded from
time and stored by both sides for dedup).

A new feature is added through **four additive seams**, none of which rewrites the core:

1. **Protocol — a new `Payload` variant, NOT a bump of the envelope version.** `Payload` is an
   internally tagged enum (`#[serde(tag="type")]`). A future feature = a new variant
   (`Reaction { target, emoji }`, `Edit { target, text }`, `Pin { target }`, …). **The
   forward-compatibility lock is already in place:** the `Payload::Unknown` variant
   (`#[serde(other)]`) — an old client that receives an unfamiliar type **ignores** it instead of
   failing in `decode`. Without it, the day reactions ship would crash every old client; with it,
   new types are additive (`core/src/protocol.rs`; test `unknown_payload_type_is_ignored_not_error`).

2. **Engine — new commands and events (ADR-003).** The UI is a projection of the snapshot and
   computes no domain logic. A feature adds its command and event; existing ones are not touched.

3. **Storage — a side table keyed by the reference, an idempotent migration.** Reactions, edits
   and pins are separate tables keyed by the reference. The message table is not migrated. (The
   pattern was already used: `outbox`.)

4. **UI — optional model fields + a slot in the row composable.** `Message` (Kotlin) gains
   `replyTo? / reactions? / edited? / pinned?`; the message row has slots for them. **The
   north-star mock-up** ([mockups/chat.html](../design/mockups/chat.html)) already shows the layout
   with these elements → the layout is not redone, only data rendering is added.

## Implementation layers (example: reactions, after 1.0)

- `Payload::Reaction { target, emoji, remove: bool }` + `Ack`.
- A command to react / an event "reaction changed"; in groups — through the same MLS envelope (no
  Commit needed, it is an application message of the epoch).
- A table `reactions(chat, target, author, emoji)`; an "emoji → count" aggregate in the snapshot.
- Chips rendered under the bubble (the mock-up has the space). Order and conflicts —
  last-writer-wins per `(author, target)`; divergence is not critical (a reaction does not affect
  decryption).

## Options considered

1. **Build the social features in 1.0.** Rejected — distributed problems without an arbiter, not
   what sets us apart; they pull focus and milestone budget.
2. **Defer WITHOUT a contract ("we will figure it out later").** Rejected — "later" turns into
   rewriting the protocol, storage and matching; exactly the cost this ADR avoids. And without
   `Unknown`, shipping a new type = a breaking migration of every old client.
3. **One extensible "super envelope" with a bag of fields (map<string, any>).** Rejected — it
   loses type safety and the ability to check the parser with Kani/proptest (important for the
   audit, M6); a tagged enum + `Unknown` gives both additivity and strict parsing.
4. **A tagged enum + an `Unknown` fallback + features by reference (chosen).** Additive,
   type-safe, forward-compatible, no core rewrite.

## The trade-off

- **We pay now** with one small lock (the `Unknown` variant, ~10 lines + a test) and the
  discipline "feature = variant + command/event + side table + slot", not changes to the core.
- **We get** every deferred feature as additive work and **backward compatibility on the wire**
  (old clients do not crash on new messages).
- An honest limit: `Unknown` saves from a *crash*, but an old client will not *show* the feature
  — that is expected graceful degradation, not a regression.

## Consequences

- Deferred features are outside 1.0 but **unblocked** architecturally; the order in which they are
  added is a product decision, not a rewrite.
- Any new `Payload` MUST stay compatible: a receiver that does not understand the type must not
  depend on it for the correctness of features it already supports.
- No `Ack` is sent for unknown types → the sender's retry expires by TTL (safe).

## State as of 2026-09

The contract was tested in practice sooner than planned: reply, retraction, forwarding (optionally
without the author), pins and edits were added in August for both 1:1 chats and groups, each as a
new `Payload`/`AppMsg` variant plus side data, without touching the core. Two refinements came out
of that work: the reference became the pair "author + id" in a single message table
([ADR-009](ADR-009-one-message-store.md)), and commands are `Engine` methods rather than
`enum Command` variants (see ADR-003). The forward-compatibility rule is described in
[protocol-spec §2](../protocol-spec.md). Reactions remain deferred.

## Action items

- [x] `Payload::Unknown` (`#[serde(other)]`) + an arm in the receive loop + a test. *(2026-07-05)*
- [x] Protocol spec: a section on forward compatibility and the rule for new types. *(2026-09)*
- [x] When adding the first feature by reference — fix the `target` format in the spec. *(done
      with replies and pins)*
