# Decisions made while the author was away (night of 23 Nov 2026)

Each line: what was decided and why. Anything here can be reversed in the morning.

| # | Decision | Why |
|---|---|---|
| 1 | Licence GPL-3.0, not MIT as the first draft said | Matches NetSpeedGauge (its LICENSE was copied), and lets us learn from GPL references such as the Linux hid-kysona driver |
| 2 | HID access in `shared` through the `hidapi` crate | Same code on Mac and Windows; only drawing and Bluetooth stay per platform |
| 3 | Protocols tested by replaying recorded exchanges (`ReplayTransport`, fixture files) | Windows has no dongles tonight (KVM on the Mac); replays let it test without them |
| 4 | Demo mode `JUICEBAR_DEMO=1` with sample devices | Lets the Windows UI be built and screenshotted without the dongles |
| 5 | Tone colours: Apple system green, orange and red | Readable on light and dark menu bars; same values on Windows |
| 6 | Catalog never names a brand-shared chip on its own (the mouse dongle is shared by Kreo Hawk Pro and Amkette) | The author asked never to show "Hawk Pro" for his mouse |
| 7 | One remembered Bluetooth device of the same kind is used as the name; several are not | One is a safe match ("XS Flow+1"); with several, guessing could show the wrong name |
| 8 | Kontrol's wasm drivers were NOT executed; only their text and simulator were read | A safety check blocked running downloaded third-party code, which is fair; reading plus our own Rust code against the real dongle got the keyboard working |
| 9 | Keyboard charging flag follows the simulator (bit 4) until checked | The real reply at 100% had bit 0 set; could be "full" or "charging". Morning check: cable in and out |
| 10 | Mouse left as "Not connected" on Mac tonight | Its interface needs Input Monitoring, a grant only the user can give; the app requests it on first run |
| 11 | A known device that does not answer shows "Not connected", not "Unknown" | It is recognised, just asleep or off |
| 12 | Icon is a juice glass with a straw, not a battery; level = lowest %, juice = tone | His ask at 03:22: unique to Juice Bar and recolourable. Geometry lives in shared/src/glyph.rs so Mac and Windows draw the same mark; SVG export uses currentColor |
| 13 | App icon: orange to pink tile, white glass, yellow juice, white bolt | Warm "juice" colours, the bolt keeps the battery meaning; generated from the same geometry (mac/examples/make_icon.rs) |
| 14 | Exact catalog name beats a remembered Bluetooth name | His keyboard showed "Kreo Swarm 5.0" (its Bluetooth name); "Kreo Swarm 75" is the exact model |
| 15 | Unknown device: a submenu with Report device and Try to detect; known device: hold ⌥ to see Report | A Mac menu row has one click target; ⌥ alternates are the native way to offer a second action |
| 16 | One long-lived HID thread owns hidapi | hidapi on macOS crashed when a second thread created it; also cheaper than a thread per scan |
| 17 | Bluetooth names and battery from `system_profiler -json`, off the main thread, on scan only | No private APIs; zero cost between scans (measured 0.0% CPU idle) |
| 18 | Input Monitoring is requested only when a dongle that needs it is plugged in | Most users never see the prompt; his mouse dongle needs it |
