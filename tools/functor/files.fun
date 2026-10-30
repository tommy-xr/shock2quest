// Game data the player owns, never in the repo: `ss2/` is System Shock 2's
// data/res, read from this machine's `ss2` mount (see README). A standalone
// headset would read the copy players already keep for shock2quest.
let config =
  Config.create()
  |> Config.external("ss2/",
       Source.mount("ss2") |> Source.device("/sdcard/shock2quest/sshock2.kpf!data/res/"))
