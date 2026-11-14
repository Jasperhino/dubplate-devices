# Dubplate Devices

A small **macOS menu-bar app** with **Off / DDJ-GRV6 / XDJ-XZ**. The MIDI responders and lifecycle run in Rust. A small Objective-C adapter supplies the native menu, USB inventory checks, and control of the separately installed Jig app.

This is an experimental local tool, not a supported AlphaTheta controller. A completed protocol exchange does not prove that every rekordbox feature is available. This tool provides no audio interface, mixer, deck controls or Launchpad mapping. The main Dubplate app remains separate.

## Run

1. Install [Jig](https://neimad.fr/apps/jig/) separately. This integration targets its 0.1.0 UI. Keep only one Jig instance open, with no unrelated device attached.
2. Open `dist/Dubplate Devices.app` after building it.
3. Choose **Enable control…** and allow **Dubplate Devices** in System Settings → Privacy & Security → Accessibility. This lets the selector operate Jig's named controls; it does not grant our executable Apple's USB host-controller entitlement.
4. Use the record icon in the menu bar to select a device. The app imports its own profiles into Jig if needed, confirms USB enumeration, then opens the MIDI pair.
5. Choose **Off** to close MIDI and detach the app's USB device. Quit performs the same cleanup.

The first launch shows a small setup window until Accessibility is available. Closing that window leaves the menu-bar app running. **Open Jig** reveals the background USB host. Clicking the status row shows the full status/error text. **Device profiles…** opens the included profiles for manual import if automatic import fails.

Start the emulator before opening rekordbox for the most predictable discovery. If rekordbox was already running and does not connect, select Off, then the device again. Status distinguishes waiting for MIDI, received MIDI, and a verified XZ handshake. It deliberately does not equate received MIDI with a feature unlock.

## Build

Requires macOS 14+, Rust, and Apple's Command Line Tools. No Node, browser runtime, local server or external C helper executable is needed.

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
sh scripts/build-app.sh
```

The script builds for the current Mac's architecture and ad-hoc signs the app for local use. A rebuilt ad-hoc app may need its Accessibility permission refreshed. Distribution signing and notarization are not configured.

Read-only diagnostics:

```sh
'dist/Dubplate Devices.app/Contents/MacOS/dubplate-devices' --diagnose
```

The Accessibility result belongs to the process invoking diagnostics. A terminal-launched copy can report `0` even when the GUI app has permission; the GUI's actual switching result is authoritative. After rebuilding locally, remove and re-add the app in Accessibility if macOS retains the old build's permission entry.

Logs: `~/Library/Logs/Dubplate Devices.log`. The log records lifecycle and handshake states, not full MIDI traffic. Only one instance runs at a time. The app starts Off; it does not register a login item or remember an active emulator across launches.

## Ownership and failure handling

- The two bundled profiles have private `DUBPLATE-…` serials. Cleanup targets only those profiles, never a physical controller or a user's other Jig device.
- The app refuses to start alongside another XZ, GRV6 or FLX10 USB identity, or an existing XZ/GRV6 MIDI endpoint.
- USB is confirmed before MIDI is created. During switching, MIDI closes before USB detaches. A failed start attempts to undo its USB attachment.
- Losing the USB device closes MIDI and displays an error; it does not reconnect in a loop.
- If USB cleanup fails, the status explicitly reports that cleanup remains necessary. Quit remains open for a retry, rather than silently claiming success.
- A force-kill or crash cannot guarantee USB cleanup because Jig owns that device. Relaunching this app attempts to detach its stale profiles; alternatively detach them in Jig. Normal Quit and termination signals attempt cleanup.
- This is not yet a performance reliability certification. No real XDJ-XZ coexistence is supported or tested by this helper.

## Why Jig is still required

The MIDI portion is standalone. The virtual USB identity is hosted by Jig because its signed executable carries Apple's managed `com.apple.developer.usb.host-controller-interface` entitlement. Our app does not acquire that entitlement by controlling Jig, and ordinary ad-hoc signing cannot grant it. [Apple's DTS explanation and request process](https://developer.apple.com/forums/thread/802495).

Jig's official site advertises a headless `jig` CLI. The inspected 0.1.0 download contains the GUI executable and a privileged helper, but no separate CLI executable. This implementation therefore uses the installed GUI app in the background, **not a headless CLI**. It leaves the signed Jig bundle unchanged.

No Jig binaries, provisioning profiles, signing material, or extracted proprietary code are included. The downloaded build calls itself proprietary and a private test build; redistribution permission has not been established. Shipping Jig inside this app would require suitable redistribution terms and a signed component from its vendor. Free download availability is not treated as permission to bundle it.

## Files

- `src/protocol.rs`: bounded SysEx parser and XZ exchange, ported from the earlier local C responder.
- `src/midi.rs`: virtual CoreMIDI pair and protocol processing. GRV6 receives messages without sending transport commands.
- `src/main.rs`: serial switching, cleanup, logging, single-instance guard.
- `native/macos.m`: AppKit menu, Jig integration, IOKit checks.
- `resources/*.jigbp`: private USB-only profiles; no additional class-compliant USB MIDI port.

The old research binaries and the main Dubplate repository are not runtime dependencies.

See [VALIDATION.md](VALIDATION.md) for the checks performed on the local build.
