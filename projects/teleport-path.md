# TeleportPath parity

`TeleportPath` drives the eight SHODAN membranes through the eight authored
`TelePath` markers. The relation has no payload. It is distinct from `Teleport`
and moving-terrain `TPath`.

The [script reference](https://thiefmissions.com/telliamed/allscripts.html)
identifies `BeginScript`, `Timer(TeleportMe)` and `TelePath`, but omits timing
and pose semantics. These were checked against the user's retail binaries:

- 25AE `allobjs-windows-x86_64.dll`, SHA-256
  `30f0a0ef3a7cadb985812ef6f0945027275d7f885fb5238f27a42493d821efd8`:
  `BeginScript` at `0x180025fa0` schedules a one-shot 1500 ms timer
  (`0x5dc` at `0x180026030`). `OnTimer` at `0x180026080` calls the
  pose-copy method and then link advancement, and rearms the timer.
  The pose method at `0x180026180` reads the linked marker's position and
  facing and calls Object.Teleport, with reference frame zero. There is no
  interpolation. The next-link method starts at `0x180026290`.
- Classic `allobjs.osm`, SHA-256
  `096c3a1e25f9751e71cf3e05c834866ad61ebd9fbced8cf3917a4a6e3466a645`,
  corroborates the ordering: vtable slots `0xb8`/`0xbc` point to pose-copy
  `0x1003a390` and link-advance `0x1003a550`. Both timer registrations use
  float `0x3fc00000` (1.5 seconds), converted to milliseconds by the wrapper.
  Object service slots `0x4c`, `0x50`, `0x54` are Position, Facing, Teleport
  respectively, matching the original `src/script/objscrpt.h` interface.
  Failure to find an onward link leaves the current destination intact.

Consequently the first timer copies the starting marker (no positional change
in `shodan.mis`); the first visible 45-degree hop occurs at 3 seconds. A long
frame produces one hop and a newly scheduled delay, not several catch-up hops.

The port keeps an equivalent cursor and remaining delay in `SavedScriptState`
instead of mutating the authored link. Hydration remaps the cursor's entity ID.
Movement uses the existing `SetPositionRotation` effect, so physics and rendered
poses follow the same path as other scripted teleports. Missing/deleted markers
are inert, and an endpoint without an onward link remains the destination.

Runtime verification uses `/Users/bryphe/ss2-25th`, with `sshock2.kpf` and
`mods/sshock2ee.kpf` verified; the classic binary above is research evidence,
not a substitute runtime asset root. `shodan-shield-orbit.e2e.test.ts` covers
both presentations, one complete ring, marker facing, destruction of a membrane,
a rotating gap, and save/load halfway through a timer.
