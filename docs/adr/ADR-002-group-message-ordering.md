# ADR-002: Group message order under partition and when the admin fails

**Status:** Accepted — direction recorded (2026-07-02); **implementation after 1.0** (1.0 keeps
the admin sequencer, see [v1.0-scope](../v1.0-scope.md) "Deferred past 1.0").
**Date:** 2026-06-24 · open questions settled 2026-07-02
**Decided by:** project owner

## Context (the problem)

In a group, messages from different people must be shown **in the same order for everyone**.
Otherwise Anna sees the conversation as "question → answer" and Boris as "answer → question".

Today the order is set by an **admin sequencer**: one designated member (the admin) numbers
every message in turn, and the others follow those numbers. Simple, and it works **as long as
the admin is online and the network is not split**. The weak spots are exactly the ones a
friend pointed out:

1. **The admin is offline** → nobody numbers new messages → the order "hangs".
2. **A network partition** (some members cut off, the admin on one side) → the other side
   cannot get numbers → it cannot agree on an order.
3. **A malicious or buggy admin** sends **different orders to different people**
   ("equivocation") → members' versions of history diverge.

On a P2P network of phones, partitions and being offline are **the norm**, not a rare failure.
Tying correctness to "the admin is always reachable and honest" is fragile.

## Decision (the direction)

Keep the admin sequencer as the **fast path** for the ordinary case, but add a
**self-healing fallback** so that the group keeps working and converges to a common view
**without the admin**. Concretely — a **signed hash chain (causal order):**

- Every message **references the hash(es) of the messages its author saw before it**
  ("parents"). That forms a graph (a DAG) of what follows what.
- **Causal order without a leader:** if message B saw A, B is shown after A. Everyone
  computes this locally, without the admin.
- **Tamper protection:** history cannot be rewritten without breaking hashes. Equivocation
  becomes **visible** — a "fork" in the chain (two messages from one author with the same
  parent) — and is detected automatically.
- The admin sequencer becomes an **optional "finaliser"**: when two messages are causally
  concurrent (neither saw the other), the display order is decided either by the admin or by
  a deterministic rule (e.g. by hash) — no longer a critical dependency, just "how to show it
  neatly".

Result: **the hash chain gives safety and convergence; the admin or a rule gives a neat final
order on top.** The idea matches the "signed chain head" already planned for channels — so it
is not foreign to the project.

## Options considered

### A. Admin sequencer only (as now)
| Parameter | Assessment |
|---|---|
| Complexity | Low |
| Under partition | Hangs / diverges |
| Trust in the admin | Full (blind) |

**Pros:** simple, already works. **Cons:** fragile exactly against our failure modes.

### B. Pure causal order (vector clocks / hash DAG), no leader
| Parameter | Assessment |
|---|---|
| Complexity | Medium |
| Under partition | Converges by itself |
| Trust in the admin | Not needed |

**Pros:** leaderless, survives partitions, equivocation is visible. **Cons:** the display
order of **concurrent** messages is ambiguous → a deterministic rule is needed; the UI must be
able to "complete" the order when messages arrive out of line.

### C. Hybrid (recommended)
A hash chain (safety + convergence) **+** the admin or a deterministic rule for a neat final
order.

**Pros:** the best of A and B; survives the admin being offline and partitions; gives
equivocation detection and dedup for free. **Cons:** more moving parts than A.

### D. Full BFT consensus (as in blockchains)
**Rejected.** The strongest guarantees, but enormous complexity. We do **not** need N mutually
distrustful parties to agree on one value — we need convergence + tamper protection. Overkill.

## The trade-off

A is the simplest but breaks exactly where it matters to us. C adds a hash chain (each message
carries its parents' hashes; the client builds the DAG and applies a deterministic display
rule) — moderate complexity, **but it is exactly what keeps a group alive through partitions
and loss of the admin, and it detects equivocation along the way.** Since the hash chain was
already planned for channels, the entry cost is lower.

## Consequences

**Simpler / more reliable:**
- A group survives the admin being offline and partitions (it converges by itself).
- Equivocation is detected; history is protected from tampering.
- Deduplication (content addressing) comes with it.

**Harder:**
- The message schema grows (parent hashes) — subsystem S7.
- The client must build the DAG and apply a deterministic display rule.
- The UI must show a **stable** order even while concurrent messages are still being filled in.

**To revisit later:**
- How much history to carry and sync (related to blobs and sync).
- The policy for reacting to detected equivocation.

## Open questions settled (2026-07-02)

### 1. The order of concurrent messages = deterministic, WITHOUT a leader
**Chosen: `(lamport, blake3_hash)` — a Lamport timestamp, ties broken by hash.** NOT an admin
finaliser.

- Every message carries a **Lamport clock** `lamport = 1 + max(lamport of parents)` (0 if
  there are no parents). The final display order = sort by `lamport`, ties by
  `blake3(content)`. All clients compute the same thing, locally.
- **Why not an admin finaliser:** it would bring back exactly the dependency on the admin's
  availability that this whole effort removes. With the admin offline, concurrent messages
  would "hang" again. A deterministic rule removes the dependency entirely — **the admin stops
  being critical for order** (it remains only a fast path for the causally linear ordinary
  case).
- **Why Lamport rather than a pure hash:** a pure hash gives a consistent but "random" order
  that ignores logical causality. Lamport respects "logically later" in a total order; the
  hash only breaks genuine ties (truly concurrent messages — nobody saw one before the other
  anyway).
- BLAKE3 is chosen on purpose — the same hash as content-addressed blobs; we reuse the
  primitive and get dedup "for free".

### 2. Message schema: MLS already authenticates → no separate signature
Inside an MLS group, application messages are **already** authenticated by the sender and
integrity-protected (by MLS). So a DAG node needs only **`parents: Vec<Hash>` (BLAKE3 of the
parents seen) + `lamport: u64`**, carried INSIDE the MLS-encrypted payload. A separate author
signature (as originally assumed) is redundant — removed from the scope of S7, saving bytes
and complexity.

### 3. Equivocation: detect → warn (in 1.0 style), automatic removal — after 1.0
Two messages from one author with the same parent = a visible "fork" in the DAG → detected.
Policy: **warn in the UI** (mark the branch), do NOT exclude automatically (auto-wiping a
member is a footgun). After 1.0: a detected equivocator can be **revoked through
[ADR-004](ADR-004-mls-membership-revocation.md) Remove** — the mechanism is already designed;
it is the natural link between DAG detection and revocation.

### 4. Relation to ADR-004 (separation of responsibility)
ADR-002 orders **application** messages (the causality DAG). ADR-004 is responsible for
delivering and recovering **Commits** (membership and epochs) and healing laggards. Both sit
on the same reliable group channel. In the long term the DAG could cover Commit order too,
but in v1.0 they are separate: membership — admin sequencer + resync (ADR-004); application
order — the admin fast path now, a Lamport DAG after 1.0.

## Action items

1. [x] **Rule for concurrent messages — DECIDED:** `(lamport, blake3_hash)`, deterministic,
       without a leader.
2. [x] **Schema — DECIDED:** `parents: Vec<blake3::Hash>` + `lamport: u64` inside the MLS
       payload; no separate signature (MLS authenticates).
3. [ ] A prototype of DAG assembly + the deterministic final order in the `groupchat` crate
       (after 1.0; a unit test: diverging histories converge to one order).
4. [x] **Equivocation — DECIDED:** detect → warn; automatic revocation through ADR-004 Remove
       — after 1.0.
5. [ ] History depth / sync strategy — together with blobs: nodes are addressed by BLAKE3 →
       sync through the same pull mechanism as blobs.

---

*ADR-001 (reserved): unifying the desktop and Android call/media engine — to be written
separately.*
