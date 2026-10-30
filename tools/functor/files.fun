// Game data the player owns, never in the repo: `ss2/` is System Shock 2's
// data/res and `ss2data/` its data folder (the missions), read from this
// machine's `ss2` and `ss2data` mounts (see README). SS2's file names are
// case-insensitive (a mission's texture list says `Medsci_1/wall`, the
// archive `MedSci_1/WALL.PCX`), so both match ignoring case. A standalone
// headset would read the copy players already keep for shock2quest.
let config =
  Config.create()
  |> Config.external("ss2/",
       Source.mount("ss2") |> Source.ignoreCase |> Source.device("/sdcard/shock2quest/sshock2.kpf!data/res/"))
  |> Config.external("ss2data/",
       Source.mount("ss2data") |> Source.ignoreCase |> Source.device("/sdcard/shock2quest/sshock2.kpf!data/"))
