# ADR-004: MLS member revocation, key rotation (PCS) and healing laggards

**Status:** Accepted — implemented 2026-07-02, **tested on two devices 2026-07-03** (invite /
kick / leave work phone ↔ emulator)
**Date:** 2026-07-02
**Decided by:** project owner
**Closes findings:** [SEC-1](../architecture/security-audit-2026-07-02.md) (High — no
Remove/Update), [SEC-2](../architecture/security-audit-2026-07-02.md) (call membership from the
roster). Related to [ADR-002](ADR-002-group-message-ordering.md) (order and delivery) and the
symptom "old groups do not heal".

## Context (the problem)

MLS was chosen for two properties: **forward secrecy** and **post-compromise security**
(recovering security AFTER a member's state leaks), plus the ability to **revoke a member**.
At the time the code (`group/src/lib.rs`) had `add`/`create`/`process`, but **no
`remove_member` and no self-`Update`**. The consequences were three intertwined defects:

1. **Nobody could be revoked (SEC-1).** A member who left or was compromised stayed in the
   group forever. The membership only grew.
2. **PCS never recovered (SEC-1).** The epoch advanced only when a member was added; there was
   no key rotation on demand. A leaked epoch secret stayed readable for the attacker
   indefinitely.
3. **A group could not "heal" from desync.** MLS requires Commits to be processed **strictly in
   order**. Whoever missed a Commit (was offline longer than the offline queue TTL, reinstalled
   the app) got stuck on an old epoch, hit "secret deleted" and could no longer decrypt new
   messages. The only workaround was to **re-create the group**. That is the complaint "old
   groups do not heal".

These three are **one root problem**: the MLS membership/epoch lifecycle was implemented only
"forward, for adding", without revocation, rotation or recovery. On top of that, membership in
a **call** was taken from presence beacons rather than from the MLS roster (SEC-2) — which
would become a hole exactly when revocation starts working (a revoked member knowing the `gid`
would keep joining calls).

A constraint of the environment (as in ADR-002): P2P on phones = **being offline and partitions
are the norm**. So correctness must not depend on "the admin is always online", and stragglers
must be **healed**, not abandoned.

## Decision

Introduce the **full membership lifecycle through one sequencer-admin** (the group creator — as
already established for adding and for order in ADR-002), plus **self-healing of laggards**.
Three primitives, all on top of the existing reliable channel `Payload::Group` (retry until ack):

### 1. Remove (revocation) — admin only
The admin builds an MLS Commit removing the member's leaf (`remove_members` in openmls) and
sends it **to all current members, including the one being removed**. The removed member,
having processed the Commit, finds they are no longer there → wipes the local group state and
shows "You were removed". Admin only = no concurrent Commits (consistent with the ADR-002
sequencer).

### 2. Update (key rotation, PCS) — proposed by anyone, committed by the admin
Any member who suspects compromise sends the admin an **Update proposal** (a new leaf key); the
admin includes it in a Commit → the epoch changes, the old secret is useless → PCS is restored
for that member. Plus a **periodic admin rekey** (a self-Update of the admin on schedule / at
start-up) gives the whole group a "rolling" PCS. A forced rotation of a specific member by a
button — after 1.0.

### 3. Resync (healing a laggard) = Remove + Add in one Commit
The key new mechanism. A member who hits an epoch desync error in `process_message` sends the
admin **`GroupResyncRequest{gid}`**. The admin:
- checks that the requester is **in the current roster** (otherwise refuses: a revoked member
  must not "heal" their way back in);
- asks them for a fresh KeyPackage (reusing `GroupKeyRequest/Reply`);
- makes a **Commit `remove(old leaf) + add(new KeyPackage)`** — the member re-enters from the
  current epoch through a Welcome. The group is "healed" without being re-created.

So `Remove` is the basic primitive, `Update` gives PCS, and `Resync` (Remove + Add) reuses
`Remove` for healing. All admin-sequenced, all over the reliable channel.

**Leave (leaving voluntarily)** is a special case: the member asks the admin to leave → the
admin does `Remove`. If the admin is offline — a **soft leave** (forget the group locally, stop
sending beacons), marked "not cryptographically revoked until the admin confirms" (it completes
when the admin is next reachable).

## Implementation layers (through the whole stack)

| Layer | What to add |
|---|---|
| `core/protocol.rs` | A new `Payload::GroupResyncRequest { gid: Vec<u8> }` (member → admin). Removal/rotation Commits go over the existing `Payload::Group{id,data}` — no new types needed. `GroupKeyRequest/Reply` are reused for the KeyPackage on resync. |
| `group/lib.rs` (`Member`) | `remove(&mut, leaf_identity: &[u8;32]) -> commit`; `update_self(&mut) -> commit`; `leaf_index_of(identity)`; `process` distinguishes **"I was removed"** → `Incoming::SelfRemoved` and **epoch desync** → `GroupError::EpochMismatch` (so the upper layer starts a resync instead of swallowing it). A mandatory `save` after every mutating commit (the MLS invariant). |
| `groupchat/lib.rs` (`GroupChat`) | `remove_member(peer)` (admin: commit → to all remaining); `rekey_self()`; `request_resync()` (sends `GroupResyncRequest` to the admin); incoming: `SelfRemoved` → report "drop group" upwards; `EpochMismatch` in `handle` → automatic `request_resync()`. Admin side: on `GroupResyncRequest` from a peer in the roster → start the KeyPackage exchange → `remove+add` commit. |
| `engine/groups.rs` (`GroupManager`) | Store the **group admin** (creator pubkey) + an `am_admin` flag. Commands `remove_member(gid, peer)`, `rekey(gid)`, `leave(gid)`. Events: `on_resync_request(peer, gid)`; `Incoming::SelfRemoved` → delete the group from disk + `groups_changed` + a system message. **Rate-limit resync** per (peer, gid). |
| `engine/lib.rs` | **SEC-2:** in `sync_room_members`, filter presence peers for a group `gid` through `groups.members(gid)` (DMs as before). Plumb the new commands and events. |
| `androidjni` | `groupRemoveMember(gid, peerHex)`, `groupRekey(gid)`, `groupLeave(gid)`; new events in the `pollEvent` JSON (`group_member_removed`, `you_were_removed`, `group_resyncing`). |
| Android UI (Compose) | **Group admin screen**: the member list + "remove" (visible to the admin only). System lines "X was removed", "You were removed from the group" (screen → back to the list). Resync is **silent** (at most a "syncing…" hint). A "Leave group" button. |

## Options considered

### A. Leave as is (Add only), heal by re-creating
| Parameter | Assessment |
|---|---|
| Complexity | None |
| Revocation / PCS | No |
| Healing | Re-create the group (history and membership lost) |

**Rejected:** exactly the current pain; MLS would have been chosen for nothing.

### B. Leaderless revocation (any member commits Remove/Update)
| Parameter | Assessment |
|---|---|
| Availability | Does not depend on the admin |
| Concurrent Commits | Frequent → diverging epochs, races |
| Complexity | High (needs the ADR-002 DAG order of Commits **now**) |

**Rejected for v1.0:** removal especially needs a single authority — otherwise a "split roster"
(X removed for half of the members, not for the other half). Combining it with the ADR-002 DAG
— after 1.0.

### C. Admin sequencer + resync healing (recommended)
Removal and rekeying are committed only by the admin (no races); laggards heal through
Remove + Add; the removed member is checked against the roster. Reuses existing primitives
(Add, the reliable group channel, the KeyPackage exchange).

**Pros:** closes SEC-1, heals "old groups", unblocks SEC-2, a minimum of new protocol types.
**Cons:** revocation and rotation are unavailable while the admin is offline (the same
availability limit as ADR-002); "I was removed" and desync need careful handling.

### D. Full BFT / consensus on the roster
**Rejected:** overkill (as in ADR-002) — we need not an N-party agreement but a single
sequencer + convergence.

## The trade-off

Option C adds ~1 protocol type (`GroupResyncRequest`), three methods in `Member`, their
plumbing through the layers and an admin screen in the UI — **moderate complexity**. In return:
real member revocation, restored PCS, and — which fixes a live pain — **self-healing of laggards
without re-creating the group**. Centralising revocation in the admin is deliberate: it removes
Commit races at the cost of depending on the admin's availability (accepted until the post-1.0
DAG).

## Consequences

**More reliable / more secure:**
- A revoked member loses access to new messages AND (through SEC-2) to the group's calls.
- PCS recovers (on demand + periodically).
- A group heals itself — "re-create it" is no longer needed.
- The roster becomes the source of truth for call membership (SEC-2).

**Harder:**
- Handling "I was removed" and "I fell behind" — new states in `groupchat` and the UI.
- "Who is the group admin" must be stored and checked.
- Resync amplification → rate-limiting requests is mandatory.

**Risks and how we contain them:**
- *The admin is offline* → revocation and rotation wait (documented; post-1.0 DAG).
- *A Commit race* on a member Update → the admin commits proposals serially; an out-of-epoch
  Commit is rejected by the receiver → automatic resync.
- *A revoked member tries to heal back in* → resync is served only to peers in the current roster.
- *FS/PCS of call media* → separately (SEC-4 / security-hardening S5): ideally the A/V key comes
  from the MLS exporter secret, and then revocation automatically covers media too.

## Action items

1. [x] **`Member` (crypto core) — DONE 2026-07-02.** `remove`, `update_self`, `leaf_index_of`;
       `process` distinguishes `SelfRemoved` (via `staged.self_removed()`) / `Rekeyed` (Update
       only, membership unchanged) / `MembershipChanged` and returns `EpochMismatch` on
       `WrongEpoch`/`NoPastEpochData`. 3 unit tests (revoke; rekey; missed commit →
       EpochMismatch).
2. [x] **`GroupChat` — DONE 2026-07-02.** `remove_member` (Commit to all, including the one
       being removed — the list is taken BEFORE removal), `rekey_self`; `handle` maps the new
       variants exhaustively → `Inbound::{SelfRemoved, NeedsResync, None}`; `request_resync`.
3. [x] **Resync protocol — DONE 2026-07-02.** `Payload::GroupResyncRequest` + event + dispatch;
       `request_resync` (broadcast to the roster), `on_resync_request` (admin gate + "in the
       roster" + a 10 s throttle → KeyPackage exchange marked as re-add); `on_key_reply` for a
       resync does Remove + Add; a Welcome for a known group = a silent merge (keeps the name
       and history).
4. [x] **`GroupManager` — DONE 2026-07-02.** `remove_member` (admin only, the flag from
       `admins.json`), `rekey`, `is_admin`, `leave` (GroupLeaveRequest → the admin revokes;
       otherwise a soft local leave) + `on_leave_request`; on `Inbound::SelfRemoved` —
       `delete_group` + the event `group_removed`; `NeedsResync` → `request_resync`.
5. [x] **SEC-2 — DONE 2026-07-02.** `sync_room_members` intersects presence with the group
       roster (DMs as before), fail-open on a cold cache.
6. [x] **JNI + Compose admin screen — DONE 2026-07-02.** `groupRemoveMember`/`groupRekey`; a
       management screen (MEMBERS with kick for the admin + ADD); `group_removed` → a toast +
       leaving the group.
7. [x] **Periodic admin rekey — DONE 2026-07-02.** At start-up the admin rotates the key of its
       groups where more than 24 h have passed (`rekey.json`) — a rolling PCS without a timer
       thread; laggards heal through resync.

**Status 2026-07-03: ADR-004 IMPLEMENTED AND VERIFIED** — items 1–7 closed; unit tests
(`peerage-group`, `peerage-core` roundtrip, loopback invite/kick/leave, `engine/tests/group_flow.rs`
including packet loss) + **a live run phone ↔ emulator**: a group invitation (the former
blocker) works, kick → "you were removed" + leaving, leave → revocation by the admin. Groups
created before `admins.json` existed show up as non-admin (not migratable).
