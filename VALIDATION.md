# Local validation — 2026-11-14

Validated the locally built, ad-hoc signed macOS app with installed Jig 0.1.0 and rekordbox. A physical Launchpad Pro was connected throughout. No physical XDJ-XZ was connected and no tracks were played.

## Passed

- Release build and strict code-signature verification.
- `cargo test --locked`: four protocol tests, including a byte-for-byte successful XZ exchange recorded in the prior local session, malformed fields, fragmented/interleaved SysEx, replay rejection and fresh-handshake state reset.
- `cargo clippy --all-targets --locked -- -D warnings`.
- First launch: setup window and native menu show Off, DDJ-GRV6 and XDJ-XZ.
- Missing Accessibility permission: device selection displays a clear error and creates no USB/MIDI device.
- GRV6: private USB identity enumerated, named MIDI pair opened, incoming messages received from rekordbox.
- XZ first use: app opened and confirmed its bundled profile's import into Jig without manual import.
- XZ: exactly one `XDJ-XZ (1)` MIDI source and destination; private XZ USB present and GRV6 USB absent.
- XZ: live handshake completed with rekordbox, both from the first connection and after switching from GRV6.
- XZ → GRV6: XZ removed before GRV6 started; MIDI received. Opened Groove Circuit in rekordbox and visually verified the drum slots rather than a plan-restriction message.
- GRV6 → XZ: old USB removed, new device connected, live XZ handshake completed again.
- XZ → Off: menu displayed `Off — no emulated device`; IOKit reported zero owned GRV6/XZ devices; independent CoreMIDI enumeration contained only the physical Launchpad's `Live Port`, `Standalone Port`, and `MIDI Port` in both directions.
- Setup window closed; helper process left running in the menu bar, Off.

## Scope

This proves local device discovery, the XZ exchange, the observed Groove Circuit panel availability, and selector cleanup. It does not certify audio performance, every rekordbox feature, real-XZ coexistence, extended operation, or future Jig/rekordbox versions. Quit and failure paths share the same cleanup implementation, but forced-crash recovery and every possible external state change have not been exhaustively exercised.

Jig's SwiftUI Detach button did not reliably act on accessibility presses. The final build uses the native **Session → Detach** menu command, checks the selected profile's private serial, and verifies removal in IOKit. Jig is briefly unhidden behind the foreground app while operating its UI, then hidden after confirmation. This is background GUI control, not a headless engine.

Runtime evidence remains in `~/Library/Logs/Dubplate Devices.log`. The repo does not contain private library data or full MIDI session recordings.
