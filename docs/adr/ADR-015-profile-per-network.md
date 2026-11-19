# ADR-015: A profile is one identity on one network; one transport contract for every network

**Status:** Proposed 2026-11-19 (discussion with the owner). The format part (F1–F5) goes in
**before step 8** (format freeze, `docs/plans/beta.md`); the networks themselves come **after
the beta**.
**Date:** 2026-11-19
**Decided by:** project owner (the profile idea came from an outside suggestion and was
accepted in discussion)
**Related:** [ADR-007](ADR-007-identity-transport-key-split.md) (identity ≠ address, rotation),
[ADR-010](ADR-010-identity-v2-multidevice.md) (several devices per identity),
[ADR-012](ADR-012-dm-session-v2.md) (1:1 sessions authenticate by identity, not by address),
[ADR-016](ADR-016-presence-and-mailboxes.md) (optional beacons, asynchronous delivery),
`docs/plans/censorship-transport.md` (getting past proxies and DPI),
[threat model](../architecture/threat-model.md) N3, N7, N8, N12.

## Context (the problem)

The goal is to support several networks besides Yggdrasil:

- **Tor** (onion services);
- **I2P**, which is required: it is heavy, but it is the strongest fully peer-to-peer
  anonymous network;
- in the future, **a Peerage-native transport** (peer-to-peer with NAT traversal through
  other peers of the network).

Today the code is written for one network, and the identity is tied to its address:

- **The seed is the identity.** 24 words = the 32-byte ed25519 seed = the identity key (IK).
- **At epoch 0 the address is the identity.** `transport_seed(0) == seed` (`core/src/identity.rs`),
  so the Yggdrasil address of an install that never rotated is the IK itself. The contact
  code at epoch 0 is just `hex(IK)`. In the contacts table an empty `transport_key` means
  "address = identity".
- **The signed address statement has no network in it.** `rotate_msg` signs
  `domain ‖ epoch ‖ tk` (32 bytes). A signed address can therefore be replayed on another
  network.
- **An address is always 32 bytes.** `PeerKey = [u8; 32]` everywhere. That is enough for
  Yggdrasil and for Tor v3 (an ed25519 key). A full I2P destination is about 390 bytes.
- **`Transport` (`messenger/src/lib.rs`) is shaped by Yggdrasil:** `peer_count`,
  `tree_count`, `peers_json`, `add_peer(uri)`. The engine also hands it `public_peer_uris`.
- **One engine drives the app.** Android has a single `pollThread` and `tickerThread`.
  Both JNI (`engine(session_ptr)`) and libygg (`nodes map[uint64]*node`) already work by
  handle, so several engines in one process are possible.

There are two ways to support several networks.

**(A) One identity reachable over several networks.** Each contact gets several addresses,
the engine needs a state machine for "which network reaches this contact now" with fallback,
and the own/stranger split has to be redone per network. Above all, **the same IK on two
networks links them by construction**: an onion identity next to a Yggdrasil identity with
the same key is deanonymized.

**(B) One profile per network.** Profile A lives on Yggdrasil only. Profile B lives on Tor
only and has a different identity. A user who wants both runs two profiles.

## Decision: (B)

### 1. What a profile is

A profile is:

- its own root identity;
- **exactly one network kind**;
- a "hidden" flag (§5);
- its own data directory, database, blobs and vault key.

The same identity is never used on two networks. Everything that exists today stays
profile-local without change: the contact-request gate (7i), `budget::Tier`, MLS groups and
backups. A group cannot mix networks.

### 2. Network and carrier are different things

- **A network (overlay)** decides addressing and routing: Yggdrasil, Tor onion services,
  I2P, a future Peerage-native network. One profile = one network.
- **A carrier** is how the bytes reach the first hop:
  - Yggdrasil over tcp/tls/quic/wss, through an HTTP proxy, or through SOCKS (Tor as a carrier);
  - Tor bridges (obfs4, snowflake, webtunnel);
  - I2P reseed sources and transports.

Carriers are **settings inside a profile**, never a new profile. Getting past censorship
(`docs/plans/censorship-transport.md`) is a carrier problem: when Yggdrasil's usual entry
points are blocked, the user changes the carrier and keeps the identity and every contact.
In Russia both Tor (needs bridges) and I2P (reseed is blocked) need carriers too. So the
entry-point and carrier layer is one common mechanism under all networks.

### 3. Key hierarchy (consistent with ADR-010)

```
24 words
 └─ profile root            (profile 0 = today's seed, so existing installs keep their identity)
     └─ device key          (one device until ADR-010 ships; until then the device secret = the root seed)
         └─ network address (network kind, epoch), rotated as in ADR-007
```

- **Every address is derived from the device secret, with the network kind in the label:**
  `derive_subkey("peerage:transport:v2/{kind}/{epoch}")`. **`TK_0 == IK` is removed.**
  - The identity key no longer shows up on any network as an address.
  - Two devices of one identity (ADR-010) get different addresses, because the device
    secret differs.
- **Existing installs at epoch 0** do one forced rotation on upgrade, with the usual signed
  announcement to contacts (the 7g mechanism, `announce_later`). Data is not cleared.
- **The signed address record v2** replaces `rotate_msg`:

  ```
  "peerage:addr:v2" ‖ kind(1) ‖ epoch(4) ‖ len(2) ‖ addr ‖ signer(1)
  ```

  - The network kind and the address length are part of what is signed.
  - `signer` says what signed it: the identity now, a device key under a device list later
    (ADR-010). The record does not need to break again.
  - The address record, the mailbox list (ADR-016) and the device list (ADR-010) all use
    **one** versioned signed-record format.
- **Profile n from the same 24 words** is allowed by the rule
  `derive("peerage:profile:v1/{n}")`, with profile 0 left unchanged. **The default is a
  separate phrase per profile**, because a shared phrase links the profiles for whoever holds
  it (§7). Only the rule is fixed now; implementing it can wait.

### 4. One transport contract; the engine does not know the network kind

| Today (Yggdrasil-shaped) | Contract |
|---|---|
| `PeerKey = [u8; 32]` | `Addr { kind, bytes }`: opaque and variable-length; the engine only stores and compares it |
| The sender is known because Yggdrasil checks it | `read()` returns an **authenticated** sender address, and that is the adapter's job. Yggdrasil: native. I2P: signed repliable datagrams. Tor: the caller is anonymous to an onion service, so the first frame of a connection proves possession of the caller's onion key |
| `write`, `write_media(urgent)` | The same for every network. Over stream networks (Tor, I2P streaming) the adapter frames and pools connections itself, and the media class is dropped when the queue is full. A frame above the network's practical datagram size is split inside the adapter (I2P datagrams above ~10 KiB are often lost). The M5 buckets do not change |
| `peer_count`, `tree_count`, `peers_json` | `status()`: state, number of nodes, and details for that network's settings screen |
| `add_peer(uri)`, `public_peer_uris` | `entry_points(list)`: Yggdrasil peer URIs, Tor bridges, I2P reseed sources; carriers (§2) |
| Checks by network name | `caps()`: `hides_ip`, latency class for real-time media, `max_frame`, `lan_discovery`, `direct_path`. Features check capabilities, never the kind |
| `block` / `unblock` by key | The same, by `Addr`; packets are still dropped before decryption |
| Rotation (ADR-007) | The same for every network: a new key, onion address or I2P destination from the epoch subkey, signed by the identity |

Everything above the contract — 1:1 sessions (ADR-012), MLS and the store — authenticates by
the identity and does not change. **The network kind appears in only three places:**

- creating a profile;
- the adapter factory;
- that adapter's settings screen.

### 5. Hidden profiles and the honest limit of what they promise

A profile is **hidden** when its network has `hides_ip`, so that an IP address cannot be
tied to the identity. Within such a profile, anything that would open a direct IP-level
connection is off, and it is switched off by the capability check, not by "if Tor":

- discovery on the local network;
- the direct path for calls;
- NAT traversal.

On the same basis, NAT traversal **reveals the IP to the other person even in a Yggdrasil
profile** (today only the mesh neighbours see it). That goes into the threat model as a cost
of the direct path.

**Timing correlation is the limit.** Two profiles on one phone with one uplink give each
other away by acting at the same moment:

- both drop and reconnect together when the phone moves between Wi-Fi and mobile data;
- Android Doze wakes them in the same batch;
- presence, typing and read times line up;
- the clock offset in message timestamps is the same.

Running the profiles one at a time does not help: "Yggdrasil left, Tor came up a second
later" links them just as well. A Yggdrasil public peer sees both the key and the IP. If the
same IP is also seen entering Tor, the link exists at the network level.

What the profile promises:

- **linking by identifiers** (keys, addresses, signatures, data): ruled out;
- **linking by timing**: made harder, **not** ruled out;
- **full separation**: a separate device. The interface and the threat model (new item N13)
  say this plainly.

Measures, driven by the "hidden" property:

- **No presence.** All beacons are off by default (ADR-016), and delivery goes through
  mailboxes.
- **No shared timers.** Each profile has its own scheduler; retries and reconnecting after a
  network change get their own random delay of minutes, not milliseconds.
- **Probing the profile is closed.** Onion client authorization and an encrypted LeaseSet in
  I2P: only holders of the key from the contact code can fetch the address descriptor, so a
  stranger cannot test whether the profile is online.
- **Coarse timestamps** in messages: no clock fingerprint.
- **Optionally, Yggdrasil over Tor as a carrier** for the regular profile on a phone that
  also has a hidden profile: slower, but the IP is not tied to the Yggdrasil key.

### 6. One engine per profile

- Each profile has its own engine, scheduler and threads. **Profiles share no tick.**
- On Android, `pollThread` and `tickerThread` become per-profile.
- **Stage 1:** only one profile is active at a time, and switching restarts the engine.
  **Stage 2:** all profiles receive at once.

### 7. Codes, backups and layout

- **Contact code v2:** a `peerage:` prefix, a version, the network kind, then TLV fields:
  - the identity;
  - the signed address record;
  - the name;
  - network extras such as the onion client-auth key;
  - the mailbox list (ADR-016).

  A code for another network kind is rejected with an explicit error.
- **Backup:** the network kind goes in the header. Importing into a profile of another kind
  is rejected, as a foreign identity already is (`Foreign`).
- **Data layout:** `files/profiles/<id>/`, holding:
  - `profile.json` (`identity`, `kind`, `hidden`);
  - the database;
  - blobs;
  - the vault.

  Every profile has its own vault key. The question of what a seized, locked phone shows is
  open (below).

## Networks one by one

| Network | How | Notes |
|---|---|---|
| Yggdrasil | the current adapter, refactored to the contract | rotation and carriers already exist or are planned |
| Tor | arti (Rust, MIT/Apache) with v3 onion services | no UDP and 0.3–1 s latency, so calls get a latency warning from `caps()`; a constantly running onion service costs battery; bridges are needed in Russia |
| I2P | i2pd (BSD-3) embedded, spoken to over SAM v3; watch the Rust implementations | repliable datagrams; frames above ~10 KiB are split by the adapter; reseed is blocked in Russia, so reseed from a file or over a carrier; the Android build is the heaviest part |
| Peerage-native | **not a separate network for now** | NAT traversal is the `direct_path` capability inside a profile without `hides_ip`, with the rendezvous over the Peerage peer network (ADR-016 §4). A separate overlay is revisited after 1.0 |

## What we pay

- **Duplicated contacts.** The same person on two networks is two contacts and two chats,
  and groups cannot mix networks. That is the price of profiles that cannot be linked; the
  interface states it plainly.
- **No automatic fallback** between networks. Carriers cover censorship instead.
- **Multi-profile work in the app:** profile picker, vault keys, notification channels,
  services.
- **One forced address rotation** for existing installs.

## Plan

**Before step 8 (format; after the freeze these become migrations and protocol versions):**

- **F1.** Address keys are derived from the device-scoped label with the network kind.
  `TK_0 == IK` is removed, and installs at epoch 0 rotate on upgrade.
- **F2.** The signed address record v2 (`kind`, `len`, `signer`, version) on the common
  signed-record format.
- **F3.** `Addr` is variable-length:
  - in the database (`contacts.transport_key` holds the address with its kind);
  - in the contact code v2 (TLV);
  - in the backup header.
- **F4.** The `profiles/<slot>/` layout with opaque slot names and no global profile list (see the open question below), plus `kind` and `hidden` in `profile.json`, which lives inside the container.
- **F5.** The rule for deriving profile n from the phrase (spec only).

**Refactoring before the first new adapter (not format; it is cheaper now):**

- **R1.** The transport contract from §4. Yggdrasil-specific code moves out of the engine
  into its adapter; the network settings screen reads `status()` and `entry_points`.
- **R2.** A per-profile scheduler with no shared ticks.

**After the beta:**

- **P1.** Several profiles, all on Yggdrasil, one active at a time. This tests the whole
  profile machinery without a new network, and is useful on its own (work and personal).
- **P2.** Carriers: wss and proxy for Yggdrasil (`censorship-transport.md`).
- **P3.** Tor (arti).
- **P4.** I2P (required).
- **P5.** All profiles receive at the same time.
- **P6.** The direct path for calls with NAT traversal (needs the Peerage peer network from
  ADR-016).

## Decisions taken (2026-11-19)

- **One phrase per profile** (§3): profile n is not derived from another profile's words by
  default. The derivation rule stays recorded for a possible opt-in later.
- **Tor first, then I2P** (P3 → P4): arti is Rust and embeds directly; I2P is still required.

## Open question: what a locked, seized phone shows

The owner is choosing between the list of profiles and nothing. The proposed design for
"nothing":

- **No list on disk.** Each profile is a sealed container `profiles/<slot>/`, where
  `slot = H(k)` and `k = Argon2id(password, device salt)`. Everything that names the profile
  is inside the container under its vault key: `profile.json`, database, blobs, vault meta.
- **One neutral password field at start.** A password opens exactly one container, with one
  KDF run per attempt. A wrong password and a nonexistent profile look the same.
- **The main profile may keep Keystore and biometric unlock** (the vault levels that exist
  today, `VaultKeyManager`). Other profiles use the `PASSWORD` level only: biometrics and the
  device PIN belong to the device, not to a profile.
- **What else must not name a profile:**
  - notification channels (generic ones, shared by all profiles);
  - notification and foreground-service texts (no profile name);
  - Keystore aliases (opaque);
  - the saved log (current profile only);
  - launcher shortcuts (main profile only);
  - the recents screen (`FLAG_SECURE`, already in place).
- **What still leaks:**
  - the number and size of container directories on a filesystem image;
  - the app's storage size in system settings.

  Decoy slots (a fixed number of containers filled with random data in size buckets) close
  this and can be added later **without a format change**, because slot names are opaque
  from the start.
- **Duress comes for free.** Under pressure, one gives the password of a harmless profile.
- **Limits:**
  - while a profile receives in the background (stage 2), its key is in memory;
  - a forgotten password cannot be hinted, by design; only that profile's 24 words restore it.

The format consequence is part of F4: opaque slot names and no global list.

## How to check the decision is followed

- `grep` over the engine and messenger finds no network kind outside the adapter factory.
- No feature branches on the network name; it reads `caps()`.
- A test: a contact code or backup of another kind is rejected; an address record signed for
  one kind does not verify for another.
- A test: no address equals the identity key at any epoch.
