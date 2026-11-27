# Battery protocols

How each family is read. Bytes verified against real hardware unless marked otherwise.

## bytech (Kreo Swarm 75 keyboard, dongle 3554:fa09)

- Interface: usage page 0xFF02, report id **19** (0x13), 19 byte packets in and out. Kontrol's simulator says report id 4; the real dongle rejects that.
- Packet: `[cmd, count=1, index=0, (profile<<4)|len, payload.., checksum]`, checksum = `(19 + sum of bytes 0..17) & 0xFF`.
- `GET_ONLINE` 0x07 -> payload `[1]` when the keyboard is awake.
- `GET_POWER` 0x4A -> payload `[percent, flags]`. Verified 23 Nov: `13 4a 01 00 02 64 01 ... c5` = 100%, flags 0x01.
- Flags: simulator puts charging in bit 4. Real reply had 0x01 at 100%. Open: does bit 0 mean charging or full? Check with the cable in and out.
- The keyboard also pushes `0x0A` events unasked: `13 0a 01 00 04 05 64 01 00` (looks like `[05=battery, percent, flags, 0]`). Skipped for now; can drive live updates later.
- `GET_ATTRIBUTES` 0x05 -> uuid + firmware version; possible device-reported name source later.

## yuandaxin (dongle 04f3:026f; Kreo Hawk Pro and the Amkette XS Flow Pro Plus share it)

- Interface: usage page 0x000A; input report 4, feature reports 5, 6, 188 (sizes from the real descriptor).
- Driver fields: `levelPercent`, `charging`, `voltageMv`.
- Battery: the mouse pushes input report 4 about every 5 s on its own: `01 04 01 <percent> 00 00 00 01 03 02 ...`
  (0x32 = 50%). Recorded 28 Nov 2026 by logging Kontrol's WebHID traffic in the browser; Juice Bar only
  listens for up to 6 s, it never sends anything. Charging bit not identified yet.
- On macOS the interface needs Input Monitoring (privilege violation without it), which only the user can grant.

## Sources

- Kreo Kontrol, kontrol.kreo-tech.com: device list, simulator packet layouts (read as text, never executed).
- Real hardware on the author's Mac, read commands only.
