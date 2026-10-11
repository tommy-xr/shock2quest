# Nightdive monster idle deformation

The remaster's `mods/sshock2ee.kpf:sq_scripts/visuals_monsteridles.nut`
unconditionally registers `ND-MonsterIdles` on Once-Grunts, Rumbler, Arachnid
and Overlords. Those declarations are outside comments and have no SCP
mission fingerprint or cheat-setting condition. `big_head_mode` controls only
the separate bighead channel, which this change does not implement. Source
SHA256: `16035dddacc7258980692c0c1d9e237c3909d2865dbb63293450c011f2b0ddb1`.
This establishes the authored registration/driver, not a comparison against a
running Nightdive executable.

`MonsterIdle::for_entity` resolves those four names through the loaded gamesys
metadata and template ancestry. A model without the named `worm` target is
unaffected. The authored expression is
`sin((SimTimeMilliseconds + originalObjectId * 360) / 400) + 0.5`;
its range is intentionally -0.5 to 1.5. Authored mission objects retain their
original object-id phase. Spawned objects receive their own instance phase,
saved with their last weight so an entity-id remap does not change it.

BaseMonster updates this private saved state only when not frozen, killed or
in AI mode 5. Suppression holds the last weight, including through save/load.
Thawing rejoins the simulation-time expression. A read-only script query hands
the weight to the common scene-object path, which writes only the named worm
channel. Other facial channels, shared geometry and skeleton palettes remain
intact. The PMNM renderer applies the resulting deformation before skinning.

Verification uses the 25AE root `/Users/bryan/ss2-25th` with `sshock2.kpf` and
`mods/{400,patch_ext,scp,shtup,sshock2ee}.kpf`. Negative-first flat/VR SDK tests
check two pipe hybrids and Command1's authored Rumbler (original object 2313),
including death retention. Unit tests cover the real BaseMonster Freeze path,
AI mode 5, saved phase/weight, thawing, and independent morph channels on shared
geometry. The existing timed-stasis projectile tests additionally check that
rendered worm weights stay frozen and survive save/load.
