# Mac and Windows contract

Both apps are thin. Everything that is the same on both lives in `shared`; `mac` and `windows` only
draw and listen to the OS. Same file names and same function names on both sides, so a reader who
knows one app knows the other. Owner of `shared` and `mac`: the Mac session. Owner of `windows`: the
Windows session. A change to `shared` needed by Windows: say so in a message, Mac makes it (or
Windows makes it on its own branch and says so), never both at once.

## Shared (`juicebar_shared`), already in the repo

| # | Module | What both apps call |
|---|---|---|
| 1 | `device` | `Device`, `Battery`, `Connection` (`Dongle`, `Bluetooth`, `Usb`), `DeviceKind`, `Device::counts_for_icon`, `Device::is_unrecognised` |
| 2 | `tone` | `tone_for_percent(u8) -> Tone`, `icon_tone(&[Device]) -> Tone`, `lowest_percent(&[Device])` |
| 3 | `scan` | `list_usb_devices(&HidApi)`, `read_usb_device(&HidApi, &UsbDevice)`, `build_device(...)` |
| 4 | `names` | `resolve_name(kind, &NameCandidates)`; apps fill `bluetooth_memory` from their `bluetooth.rs` |
| 5 | `settings` | `Settings::load`, `save`, `data_folder` (Application Support / %APPDATA%\JuiceBar) |
| 6 | `report` | `report_url(device, interfaces, os, version)`; apps open it in the browser |
| 7 | `demo` | `is_demo_mode()` (env `JUICEBAR_DEMO=1`, debug builds only), `sample_devices(keyboard, mouse)` for UI work without dongles |
| 8 | `protocols` | `protocol_by_id`, `all_protocols` (probe), `Reading`; fixtures in `shared/tests/fixtures` replay without hardware |

Tone colours, same values on both (sRGB): Green `#34C759`, Amber `#FF9F0A`, Red `#FF453A`
(Apple system colours, dark variants). Plain = the OS text colour (template image on Mac, theme
text colour on Windows).

## Platform files, same name on both sides

| # | File | Job | Functions (same names) |
|---|---|---|---|
| 1 | `main.rs` | start, single instance | `main()` |
| 2 | `app.rs` | state, poll timer, wake and device change, calls `scan` | `App::start()`, `App::refresh()`, `App::on_wake()`, `App::on_device_change()` |
| 3 | `status_icon.rs` | menu bar item (Mac) / tray icon (Windows); battery glyph in the tone, optional % | `create_status_icon()`, `update_status_icon(tone, lowest_percent, shows_percent)` |
| 4 | `menu_ui.rs` | the dropdown (Mac NSMenu) / flyout (Windows, Fluent look, Mica) | `show_menu(&[Device])`, `build_device_row(&Device)` |
| 5 | `settings_ui.rs` | settings window | `open_settings()`; pages General (poll interval, % in icon, launch at login), Devices (rename, hide, confirm Bluetooth name), About |
| 6 | `theme.rs` | light/dark, accent, contrast, transparency, live changes | `current_theme()`, `on_theme_change(callback)` |
| 7 | `bluetooth.rs` | OS-reported Bluetooth batteries and remembered names | `bluetooth_devices() -> Vec<Device>`, `remembered_bluetooth_names(kind) -> Vec<String>` |
| 8 | `startup.rs` | launch at login | `is_enabled()`, `set_enabled(bool)` |
| 9 | `confirm_sheet.rs` | in-window confirm card (Reset) | `show_confirm(title, detail, on_confirm)` |

Menu row layout (both): name left; connection label (`2.4 GHz`, `Bluetooth`, `USB`) and % right,
% coloured by `tone_for_percent`, bolt (⚡) before % when charging, grey "Not connected" when no
reading. Unrecognised device: "Unknown mouse" with `Report` and `Try to detect` at the right end of
the SAME row. Known device: Report in its context menu. Header "Juice Bar". Footer: Settings (⌘, /
Ctrl+,) and Quit (⌘Q / Alt+F4 equivalent shown). No battery bars.

Budget (both, checked before release): idle CPU under 0.1%, memory under 30 MB, no timers while
the menu is closed except the poll.

## Builds

Same as NetSpeedGauge: `.github/workflows/release.yml` builds `JuiceBar-mac-arm64.zip` (Apple
Silicon), `JuiceBar-x64.exe` and `JuiceBar-arm64.exe`, plus checksums, on a `v*` tag.

## Testing without the dongles

The KVM gives the dongles to one machine at a time (Mac tonight). Windows works with
`JUICEBAR_DEMO=1` and the fixture replays; live check when the KVM switches.
