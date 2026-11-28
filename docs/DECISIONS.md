# Decisions made while the author was away (night of 23 Nov 2026)

Each line: what was decided and why. Anything here can be reversed in the morning.

| # | Decision | Why |
|---|---|---|
| 1 | Licence GPL-3.0, not MIT as the first draft said | Matches NetSpeedGauge (its LICENSE was copied), and lets us learn from GPL references such as the Linux hid-kysona driver |
| 2 | HID access in `shared` through the `hidapi` crate | Same code on Mac and Windows; only drawing and Bluetooth stay per platform |
| 3 | Protocols tested by replaying recorded exchanges (`ReplayTransport`, fixture files) | Windows has no dongles tonight (KVM on the Mac); replays let it test without them |
| 4 | Demo mode `JUICEBAR_DEMO=1` with sample devices | Lets the Windows UI be built and screenshotted without the dongles |
| 5 | Tone colours: Apple system green, orange and red | Readable on light and dark menu bars; same values on Windows |
| 6 | Catalog never names a brand-shared chip on its own (one mouse dongle is shared by several brands) | Showing one brand's model for another brand's mouse would be wrong |
| 7 | One remembered Bluetooth device of the same kind is used as the name; several are not | One is a safe match; with several, guessing could show the wrong name |
| 8 | Kontrol's wasm drivers were NOT executed; only their text and simulator were read | A safety check blocked running downloaded third-party code, which is fair; reading plus our own Rust code against the real dongle got the keyboard working |
| 9 | Keyboard charging flag follows the simulator (bit 4) until checked | The real reply at 100% had bit 0 set; could be "full" or "charging". Morning check: cable in and out |
| 10 | Mouse left as "Not connected" on Mac tonight | Its interface needs Input Monitoring, a grant only the user can give; the app requests it on first run |
| 11 | A known device that does not answer shows "Not connected", not "Unknown" | It is recognised, just asleep or off |
| 12 | Icon is a juice glass with a straw, not a battery; level = lowest %, juice = tone | His ask at 03:22: unique to Juice Bar and recolourable. Geometry lives in shared/src/glyph.rs so Mac and Windows draw the same mark; SVG export uses currentColor |
| 13 | App icon: orange to pink tile, white glass, yellow juice, white bolt | Warm "juice" colours, the bolt keeps the battery meaning; generated from the same geometry (mac/examples/make_icon.rs) |
| 14 | Exact catalog name beats a remembered Bluetooth name | A keyboard's Bluetooth name can be a generic one; the catalog holds the exact model |
| 15 | Unknown device: a submenu with Report device and Try to detect; known device: hold ⌥ to see Report | A Mac menu row has one click target; ⌥ alternates are the native way to offer a second action |
| 16 | One long-lived HID thread owns hidapi | hidapi on macOS crashed when a second thread created it; also cheaper than a thread per scan |
| 17 | Bluetooth names and battery from `system_profiler -json`, off the main thread, on scan only | No private APIs; zero cost between scans (measured 0.0% CPU idle) |
| 18 | Input Monitoring is requested only when a dongle that needs it is plugged in | Most users never see the prompt; his mouse dongle needs it |
| 19 | Windows flyout is our own window with Acrylic (DWM transient backdrop), not a native popup menu | A native menu cannot hold right-aligned coloured %, a bolt, or Report and Try to detect buttons in one row; Windows 11's own tray flyouts use Acrylic too. Without transparency effects, in high contrast or before Windows 11 22H2 it paints solid |
| 20 | Windows quit shortcut shown as Ctrl+Q, settings as Ctrl+, | Windows has no system quit key; Alt+F4 would only close the flyout. Both work while the flyout has focus |
| 21 | Windows accent comes from AccentPalette (light shade in dark mode, dark shade in light mode) | That is the shade Windows Settings draws toggles with; the raw AccentColor looked wrong next to it |
| 22 | Windows "% in icon" draws the bold number in the tone instead of the glass | Two or three digits do not fit inside a 16 px glass and stay readable |
| 23 | Windows reads hidden devices too (scan with hidden cleared), and filters them only in the menu and icon | `scan_dongles` drops hidden ids, but Settings must list them to turn them back on |
| 24 | Reset in Settings resets the saved choices but leaves Launch at login alone | Launch at login is the startup list, not a saved choice; the confirm card says so |
| 25 | Windows .ico built from shared/assets/juicebar.png by windows/tools/make_icon.ps1 | No Pillow on the Windows machine; .NET drawing gives the same resize, so one master PNG still feeds both icons |
