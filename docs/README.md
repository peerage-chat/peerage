# Peerage — documentation

A serverless end-to-end encrypted messenger over the **Yggdrasil** mesh. One Rust core; the
Android client is a thin layer on top. GPL-3.0-only.

This page is the index. Start with **[ARCHITECTURE.md](ARCHITECTURE.md)** for the whole
picture, and **[project-state.md](project-state.md)** for what actually works today.

## Where to look

| Need | Document |
|---|---|
| **The system end to end** | [ARCHITECTURE.md](ARCHITECTURE.md) — crates, data flow, transport, crypto, media, Android, build |
| **What works, what is verified, what is broken** | [project-state.md](project-state.md) |
| **Cross-cutting rules** (do not break) | [architecture/invariants.md](architecture/invariants.md) — FFI / channels / SQLite / MLS / UI |
| **Subsystems → features** | [SUBSYSTEMS.md](SUBSYSTEMS.md) |
| **Wire protocol and engine contract** | [protocol-spec.md](protocol-spec.md), [adr/](adr/) (architecture decisions) |
| **Security and threat model** | [SECURITY.md](SECURITY.md), [architecture/threat-model.md](architecture/threat-model.md) (assets, adversaries, guarantees), [architecture/security-audit-2026-07-02.md](architecture/security-audit-2026-07-02.md) (crypto audit, Russian) |
| **Scope and order of work** | [v1.0-scope.md](v1.0-scope.md), [completion-plan.md](completion-plan.md), [ROADMAP.md](ROADMAP.md) (Russian) |
| **UI** | [design/app-ui.md](design/app-ui.md) (language, source of truth), [design/call-ui.md](design/call-ui.md) (calls), [design/tokens.md](design/tokens.md) (colours, spacing) — Russian |
| **Plans** | [plans/media-webrtc-parity.md](plans/media-webrtc-parity.md), [plans/security-hardening.md](plans/security-hardening.md) (Russian) |
| **Known bugs** | [BUGS.md](BUGS.md) (Russian) |
| **History** | [archive/](archive/) |

## Quick start

```sh
# Rust (Linux / WSL):
scripts/test-all.sh                           # the whole suite
bash scripts/live-two-node.sh                 # two headless nodes on the public mesh

# Android:
bash scripts/rebuild-peerage-android.sh x86_64    # Rust-only rebuild of the .so (emulator)
bash scripts/build-android-libs.sh x86_64     # full: libygg + Rust (when the ygg.go FFI changed)
./gradlew :app:assembleDebug                  # APK (from android/)
./gradlew ktlintFormat                        # before committing Kotlin
```

Build details: [ARCHITECTURE.md §10](ARCHITECTURE.md#10-build-and-development). The public
peers in `peers/public-peers.txt` go stale; current ones:
<https://publicpeers.neilalexander.dev/>.

## Toolchain

Rust 1.95+ · Go 1.24+ (libygg, Linux/WSL) · Android NDK r28c + cargo-ndk · Gradle 9.9 + JDK 21.

## License

[GPL-3.0-only](../LICENSE).
