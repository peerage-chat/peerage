# Peerage

A serverless, end-to-end encrypted messenger. Phones talk to each other directly over the
[Yggdrasil](https://yggdrasil-network.github.io/) mesh — there is no server of ours in the
data path, no account, no phone number. Groups use MLS (RFC 9420). The domain logic is a
Rust engine; the Android client is Jetpack Compose.

**Status:** a working Android prototype on the way to v1.0. 1:1 and group chat, offline
delivery, images/files/voice messages, voice and video calls, backups and search work and
are covered by tests; some parts were checked only on an emulator. What exactly is
verified, and what is broken: **[docs/project-state.md](docs/project-state.md)**.

## What is different

- **No servers at all.** Delivery, offline queueing and file transfer happen between the
  devices themselves. If both sides were never online at the same time, the message waits
  on the sender's device and is resent when the peer appears.
- **MLS groups without a server:** members are added and removed with real revocation and
  key rotation; the group admin orders commits.
- **Content-addressed attachments:** files are fetched in BLAKE3-verified chunks directly
  from the other person's phone; EXIF/GPS is stripped before sending; large downloads need
  consent.
- **Receiver-enforced budgets:** every peer gets a rate and storage budget by trust tier,
  and floods are dropped before decryption.
- **Size padding:** text, images, files and voice look the same on the wire; a call invite
  looks like an ack.
- **Honest limits:** what we do not protect against (metadata, timing, quantum adversaries,
  multiple devices) is written down in the [threat model](docs/architecture/threat-model.md).

## Documentation

| Document | What |
|---|---|
| [docs/project-state.md](docs/project-state.md) | What works, how it was verified, what is broken |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | The system end to end: crates, transport, crypto, delivery, media, Android |
| [docs/protocol-spec.md](docs/protocol-spec.md) | Wire protocol and the engine ↔ client contract |
| [docs/SECURITY.md](docs/SECURITY.md) · [threat model](docs/architecture/threat-model.md) | Security goals, audits and findings, guarantees and non-goals |
| [docs/adr/](docs/adr/) | Architecture decision records |
| [docs/README.md](docs/README.md) | Index of all documents |

Some internal documents (design, plans, bug notes) are in Russian.

## Building

The Rust core and the Go wrapper around yggdrasil-go build on Linux (WSL on Windows).

```sh
scripts/test-all.sh                          # the whole Rust test suite
bash scripts/build-android-libs.sh x86_64    # libygg + Rust core for the emulator (arm64-v8a for phones)
cd android && ./gradlew :app:assembleDebug   # the APK
```

Toolchain: Rust 1.95+, Go 1.24+, Android NDK r28c + cargo-ndk, Gradle 8.9 + JDK 21.
Details: [ARCHITECTURE.md §10](docs/ARCHITECTURE.md#10-build-and-development).

## How it is built

The code is written with AI assistance (Anthropic's Claude, via Claude Code) under the
direction of the maintainer, who makes the architecture and product decisions and tests on
devices. Commits carry a `Co-Authored-By` trailer naming the model.

## Security

Please report vulnerabilities privately — see [docs/SECURITY.md](docs/SECURITY.md).

## License

[GPL-3.0-only](LICENSE).
