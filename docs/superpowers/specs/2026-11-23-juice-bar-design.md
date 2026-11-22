# Juice Bar design

Battery % of wireless mice and keyboards in the Mac menu bar and the Windows tray. Free, open source, GPL-3.0 (same as NetSpeedGauge).

## Problem

- Over Bluetooth, macOS and Windows show a mouse or keyboard battery. Over a 2.4 GHz dongle they show nothing.
- Vendor tools (for example Kreo Kontrol, a Chrome only WebHID page) do read it, but only for their own brand, only in a browser tab, and only while that tab is open.
- Existing open source tray apps (BatteryBar, BatteryTray, Wireless Device Battery Monitor, mouse_tray) are Windows only.

## Goals

- One glance tells whether anything needs charging; one click shows each device's %.
- Works for any brand that can be read, not just the author's devices; names come from the device, never hard coded.
- Native on each OS (AppKit look on Mac, Fluent look on Windows), near zero CPU and power.

## Non goals

- No changing device settings (DPI, lighting, macros). Read only, apart from the opt in probe.
- No system Batteries widget or Control Center integration (needs a Swift widget extension and App Store style signing; decided against, 23 Nov).
- No server, no account, no telemetry.

## Anti goals

- Showing a wrong name (for example the author's Amkette mouse as "Kreo Hawk Pro" because both share one chip) is worse than "Unknown mouse".
- Any automatic write to an unknown device.

## Icon

- A battery glyph drawn unlike the system one, coloured by the LOWEST connected device:

| # | Colour | Rule |
|---|---|---|
| 1 | Green | Every device above 85% |
| 2 | White (template, follows menu bar) | Lowest is 30 to 85% |
| 3 | Amber | Lowest is 20 to 30% |
| 4 | Red | Lowest is below 20% |

- Devices on USB (charging) and devices not connected do not count.
- Boundaries: exactly 85 is white, exactly 30 is amber, exactly 20 is amber.

## Menu

```
Juice Bar
Kreo Swarm 75      2.4 GHz      82%
XS Flow+1          2.4 GHz    ⚡15%
Unknown mouse      2.4 GHz   Report
─────────────────────────────────
Settings...                   ⌘,
Quit Juice Bar                ⌘Q
```

- Name left aligned; connection type (`2.4 GHz`, `Bluetooth`, `USB`) and % right aligned; % coloured with the icon rule; bolt when charging. No battery bars.
- "Juice Bar" header on both OSes. Shortcuts shown next to Settings and Quit.
- Unknown device: still listed, as "Unknown mouse" or "Unknown keyboard", with Report (and Try to detect) at the end of the SAME row. Known devices carry Report in their right click menu, for a wrong name or %.
- Not connected: row in grey, "Not connected" instead of %.

## Look and feel

- Follows every system appearance change live, no restart: light / dark, accent colour, Reduce transparency, Increase contrast (Mac); light / dark app mode, accent colour, transparency effects, high contrast (Windows). Asked by the author, 23 Nov.
- Polished, not a plain data list: native translucent materials (Mac vibrancy, Windows Mica / Acrylic), SF Pro / Segoe UI Variable, rounded rows, careful spacing, a short native fade when the menu opens, colour dots that match the icon.
- Cheap by construction: materials are composited by the OS GPU path, so they cost near zero. The UI draws only when it is open or a value changes; no timers, no animation loops while closed.
- Budget, measured before each release: average CPU under 0.1% idle, memory under 30 MB, zero wakeups between polls. A change that breaks the budget does not ship.

## Battery sources

Tried in order; first that answers wins.

| # | Source | Kind |
|---|---|---|
| 1 | OS Bluetooth battery (Mac IOKit `BatteryPercent`, Windows Bluetooth battery service) | Standard |
| 2 | HID Battery Strength usage (Generic Device Controls 0x06:0x20, Battery System page 0x85) | Standard |
| 3 | Logitech HID++ 2.0, Razer, SteelSeries, Corsair | Vendor, publicly documented |
| 4 | Compx (17 byte reports on 0xFF02:0x0002) and SinoWealth (feature reports 0x80 and 0x90) | Chip family, publicly documented |
| 5 | Kreo Kontrol families: bytech, yuandaxin, rongyuan, instant | Chip family, from Kontrol |
| 6 | Probe: each layer's read only battery request, in turn | Runs ONLY when the user clicks Try to detect on that device |

- Author's dongles, checked 23 Nov: neither exposes layer 2. Keyboard `0x3554:0xFA09` (Kreo Swarm 75, bytech, vendor pages 0xFF02/0xFF04); mouse `0x04F3:0x026F` (yuandaxin, vendor channel on page 0x0A).
- Layer 5 bytes live in Kontrol's WebAssembly drivers; build step 1 recovers them by decoding the wasm or by passively logging dongle traffic while Kontrol runs. Logging never writes.
- Licences: protocol facts are re implemented in our own code, projects credited in the README; licence of each checked before reading its source.

## Device names

First match wins; a name the user sets in Settings beats all.

| # | Source |
|---|---|
| 1 | Name or model the device reports over its own protocol |
| 2 | A Bluetooth name the OS remembers for a device of the same kind, offered once for the user to confirm (author's mouse: "XS Flow+1", keyboard: "Kreo Swarm 5.0") |
| 3 | Juice Bar device list (vendor id, product id, protocol to name), grown from Report submissions |
| 4 | USB product string, unless generic ("2.4G Wireless Receiver") |
| 5 | "Unknown mouse" / "Unknown keyboard" |

- Same chip sold under 2 brands shares one id, so source 3 alone never names such a device; it only says the family.

## Report device

- Opens a prefilled GitHub issue on this repo: vendor id, product id, HID usage pages, matched protocol, OS version, app version. No serials, no names unless the user adds them. User sees the text before sending.

## Polling and power

- Read on launch, on wake from sleep, on device connect, then every 5 minutes (setting). Nothing runs between reads.
- Devices that push battery events (bytech parses them) update without polling.

## Settings

- Rename a device, hide a device, poll interval, icon shows colour only or also the lowest %, launch at login.
- Reset gets an in window confirm card from day one (lesson from NetSpeedGauge task 3).

## Build

- Rust workspace copied in shape from NetSpeedGauge: `shared` (protocols, name chain, colour rule, settings), `mac` (AppKit status item and menu), `windows` (tray icon and Fluent flyout).
- HID access through `hidapi`. Mac needs Input Monitoring permission for vendor pages; first run explains why.

## Open, left to the builder

- Exact byte layouts per protocol (build step 1 for Kreo families, open source references for the rest).
- Whether the Mac Bluetooth name match (name source 2) can use a stronger link than "same kind" (for example a shared serial over both modes).
