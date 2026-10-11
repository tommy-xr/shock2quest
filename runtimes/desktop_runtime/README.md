# Desktop runtime

Run Shock2Quest on desktop with keyboard and mouse, or simulate VR hands for testing.
See the [development guide](../../DEVELOPMENT.md#3a-desktop-windows-osx) for prerequisites
and [game data setup](../../DEVELOPMENT.md#2-provide-data-files).

## Running

From the repository root:

```sh
cargo dr       # Flatscreen
cargo dr --vr  # Simulated VR hands
```

## Controls

- `Mouse` - look around (flat); in `--vr`, aim the head unless `Q` or `E` is held
- `W` `A` `S` `D` - move around (hold `Shift` to move faster)
- `Q` / `E` - hold to lean left/right in flat mode; release to return. Both keys cancel. Leaning works while crouched and stops against solid geometry. In `--vr`, these keys still control the simulated left/right hand: mouse aims, left click triggers, and right click grabs.
- `B` (or `T`) - cycle ammunition type in flatscreen, then automatically reload from matching reserve
- `R` - reload the current ammunition type
- `Ctrl` - crouch
- `Space` - jump
- `Tab` - toggle the cyber interface ("use" mode): a cursor-driven UI over the 3D view
- `Alt+S` / `Alt+L` - quick save / quick load

- Equip a carried weapon with the number row: `1` Wrench, `2` Pistol,
  `3` Shotgun, `4` Assault Rifle, `5` Laser Pistol, `6` EMP Rifle,
  `7` Electro Shock, `8` Grenade Launcher, `9` Stasis Field Generator,
  `0` Fusion Cannon, `-` Crystal Shard, `=` Viral Proliferator,
  `\` Worm Launcher, and `` ` `` / `~` Psi Amp

For gameplay interactions such as using maintenance tools and equipping armor,
see the [player guide](../../docs/playing.md), which includes flat-mode equivalents.
Additional debug keys are listed in the
[development guide](../../DEVELOPMENT.md#debug--developer-keys).

[Back to Shock2Quest](../../README.md)
