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
