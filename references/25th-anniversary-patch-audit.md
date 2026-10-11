# Nightdive patch inventory

Audit of `origin/main` **e3def9742d2828443e75cff28e5d330d862cd377**, 2026-10-11,
for [#2187](https://github.com/tommy-xr/shock2quest/issues/2187). This supersedes
the old compatibility spike's outstanding list for mission/gamesys patches.
It changes documentation only. It is an asset/source comparison, not a visual
comparison against a running Nightdive executable.

## Evidence and applicability

The inspected installation is `/Users/bryan/ss2-25th`, verified by `sshock2.kpf`
and `mods/{400,patch_ext,scp,shtup,sshock2ee}.kpf`. The audited archive is
`mods/sshock2ee.kpf`, SHA256
`22663dae9b7118781e7ea6dc4ff329c6ecc2b7436918abff1d85961e7ce72011`.
[The complete inventory](25th-anniversary-patch-inventory.json) contains **74
DML and 61 NUT entries**, each with its exact archive path, byte length, SHA256,
declared fingerprint, classification, Rust reference and assessment. Paths in
this document refer to entries in that archive unless a repository path is given.
The proprietary scripts themselves are not redistributed.

Important distinction: **source-active** means a statement is outside comments,
with an explicit patch or archetype attachment where described. It does not
prove native Squirrel module discovery, load order, or that a registered debug
cutscene is reachable in retail play. Those uncertainties are called out below;
there was no native Nightdive runtime verification in this audit.

The retail `base.kpf:defaults/cam_mod.ini` declares the Nightdive/400/SHTUP/SCP/
patch_ext stack. Direct binary chunk inspection finds `GAM_FILE=shockscp.gam`
in `mods/scp.kpf:medsci1.mis`, versus `shock2.gam` in
`sshock2.kpf:data/medsci1.mis`. Our
[`data_file_mounts`](../shock2vr/src/data_files.rs) mounts base raw data, while
[`Game::init`](../shock2vr/src/lib.rs) explicitly requests `shock2.gam`.
Mounting upgraded mesh/texture families does **not** select SCP missions or
satisfy `GAM shockscp.gam` / `QVAR "scplevel" == 1` fingerprints.

[#2195](https://github.com/tommy-xr/shock2quest/issues/2195) tracks that campaign
identity prerequisite. Until it is resolved, a confirmed missing SCP patch
mapping is a **conditional parity gap**, not proof that a base-game object with
the same numeric ID is broken. Never apply these mission IDs unconditionally.
`dbmods/shockscp.gam.dml` has filename applicability, even though it lacks a
FINGERPRINT block. All 23 `ND-envmap.dml` patches are SCP-QVAR gated.

## How to read the classifications

- **Implemented:** the named behavior has a Rust path; this is not a claim of
  byte-for-byte equivalence of every statement in a helper library.
- **Intentionally different:** the port selects base campaign data or uses its
  own ECS, UI, physical VR interaction or single-player architecture. This
  records the current implementation boundary, not an irrevocable product
  decision to reject a remaster feature.
- **Irrelevant/debug/commented-out:** empty/commented experiments or development
  tools without established normal campaign activation.
- **Confirmed gap:** a concrete authored behavior/data mapping differs from or
  lacks a consumer in Rust. Its fingerprint and activation limitations still
  apply. File counts are not defect counts: repeated face/env patches share a
  single issue, and mixed files have secondary assessments below.

## Implemented behaviors and deliberate replacements

| Authored source | Current equivalent and boundary |
| --- | --- |
| `animations_weapons.nut`; `libs/{pointrig,viewmodel,weapon_handler}.nut` | [`dark/src/weapon_animation.rs`](../dark/src/weapon_animation.rs) parses the literal animation tables; [`flat_weapon_animation.rs`](../shock2vr/src/flat_weapon_animation.rs) supplies the runtime driver. Squirrel service classes are not required. VR uses physical hands/weapon handling. Do not report all weapon animation as absent. |
| `impactor.nut`, `effects_hitspangs.nut`, active stimulus definitions in `ND-materials.dml` | [`script_util.rs::ballistic_impact_appearance`](../shock2vr/src/scripts/script_util.rs) selects real projectile families/materials; [`particle_effects.rs`](../shock2vr/src/particle_effects.rs) supplies material spangs and decals. ND-materials' example create_obj receptrons are commented out. This is a native replacement, not literal receptron execution. |
| `effects_electrical_projectiles.nut` | `particle_effects.rs` has EMP riders using NDsbll/NDarc/NDsprk; Fusion also has a Rust recipe. Exact Droid Spark/Fusion authored component parity is **not established** by this audit; neither absence nor exact parity follows from the extension. |
| `object_plants.nut` | [`plant_animation.rs`](../shock2vr/src/plant_animation.rs) implements the three authored channels and world-position phase, gated by template ancestry. Landed in [#2189](https://github.com/tommy-xr/shock2quest/pull/2189), commit `889808af`; #2182 is closed. |
| `fix_objconsumebutton.nut` | [`obj_consume_button.rs`](../shock2vr/src/scripts/obj_consume_button.rs) handles required inventory consumption, one-shot delivery and final model/tweq progression through effects. No DML engine needed. |
| `libs/{ND,archetype_system,animation_handler,helpers,player}.nut`, `defines/{defines_props,defines_links,defines_motions}.nut` | Rust properties, links, scripts, motion system and effect application replace the service layer. Their individual consumers remain separate: an animation scheduler does not establish facial parity. |
| `libs/{ui,ui_handler}.nut`, `medsci1.mis/no-minimap.dml` | Native shared canvas/VR UI replaces KEX overlays. The latter suppresses SCP's minimap-enabling script on object 842, not a missing combat mechanic. |
| `achievements.nut`, 16 mission `achievements.dml`, `ND-global_achievements.dml` | KEX platform awards/counters have no equivalent platform backend. They are separate from gameplay. Mixed helper changes must not disappear into this classification: shodan:743 attaches ND_scpDieShodanDie, and achievements.nut also overrides CyberPsi/SomaDrain; compare those through the existing psi/ending systems if SCP is selected. |
| `ND-mp-scpfixes.{dml,nut}`, `earth.mis/mp-respawn.dml`, `MTKI-fixTripwireTx.dml`, `libs/multiplayer_handler.nut` | Peer ownership, network handoff, duplicate emission, avatar TripwireTx/Rx relaying and respawn placement target multiplayer. Single-player trigger scripts already consume physics events. The NUT also contains local energy-station/trait/camera helpers, so this is not a claim that every line is network-only; their SCP attachment must be checked before porting them. |

## Remaining behavior inventory

| Behavior and authored wiring | Source finding / tracking |
| --- | --- |
| Face properties in `ND-lipsync.dml`; `monster_script.nut` adds Face State; `ND-Vocalize.nut` supplies speaker/position traps | No Face/Vocalize lifecycle consumer in `dark/src` or `shock2vr/src`. [#2180](https://github.com/tommy-xr/shock2quest/issues/2180); PMNM infrastructure [#2179](https://github.com/tommy-xr/shock2quest/issues/2179) landed in [#2192](https://github.com/tommy-xr/shock2quest/pull/2192), commit `d6f18d1f`. It supplies per-instance morph geometry/blending, not a gameplay facial driver. |
| `medsci1.mis/cs0601.dml` | Under scplevel=1, remove sound from 1185, remove SwitchLink 1189→1185, give 734 cs0601/TrapSound, add 1189→734. This is speaker/trigger relocation, not just geometry. #2180 plus #2195. |
| `rec1.mis/{cs2601,lipsynctest}.dml` | Both actively move Cortez speech 674→99 and SwitchLink 672→99, with MeshTex/Face. **lipsynctest is not wholly debug/commented:** its alternative model is commented, its rewiring is active. Treat duplicate application idempotently. #2180. |
| `<mission>/<mission>.dml` for command2, eng1, eng2, hydro2, medsci1, medsci2, ops3, rec1, rec2 | Active Face/Vocalize wiring associates real actors with schemas/traps. Hydro2:1811→1290 and 1812→1272; Ops3:990→136 replace trap scripts with ND_AppVocalizeAtObjectPos. Command2:1950/1948/2315 link to cs2001..cs2005 traps using ND_TrapVocalizeAtObject. Existing TrapSound is not an equivalent speaker association. #2180. |
| `ND-shodan.dml`, `ops1.mis/ND-shodancutscene.dml`, `object_shodan.nut` | Create NDShodanScreen archetype/four named screens, link 241 to them, append hijack on 135, change light 163, and animate eyes/show/hide/schema messages. [`cs9.rs`](../shock2vr/src/scripts/cs9.rs) uses legacy screen fades and estimated durations. [#2184](https://github.com/tommy-xr/shock2quest/issues/2184). ND_ShodanScreen extends ND_AnimatedObject, not ND_SpeechShapeKeyAnimated: do not infer mouth timing from the disconnected helper. |
| `ND-protocoldroid.dml`, `object_protocoldroid.nut` | GAM shockscp.gam gate creates damage effect, parts, corpse, Flinderize/Corpse/particle links and script attachment. Active half-health visual/model effect and corpse frames differ from current base monster. [#2185](https://github.com/tommy-xr/shock2quest/issues/2185). The elaborate alternative AI state machine is commented out. |
| `object_egg.nut` | Active ND_Egg rig/opening morph is separate from existing BaseEgg hatching. [#2183](https://github.com/tommy-xr/shock2quest/issues/2183). |
| `object_botmvein.nut` | Vein receives ND_BotmVein: 30 animation frames, 20 model frames, 20Hz, `vein` prefix. [#2186](https://github.com/tommy-xr/shock2quest/issues/2186). |
| `visuals_monsteridles.nut` | Once-Grunts/Rumbler/Arachnid/Overlords receive worm idle; frozen/dead gating matters. [#2181](https://github.com/tommy-xr/shock2quest/issues/2181). big_head_mode is a separately enabled cheat. |
| `object_grub.nut` | Grub and Grub Rotatable receive ND_Grub, 20 frames at 20Hz (`NDgrub`). GrubAI navigation/leaping does not animate this model sequence. [#2198](https://github.com/tommy-xr/shock2quest/issues/2198). |
| `medsci1.mis/ND-decorations.dml`, `object_cryodoor.nut` | 496 gets PODD_OPEN, X=80 and joint bounce 0.20/0/45; ND_CryoDoor plays door1slam on completion. OnCreate activation/audio are commented. Four other objects receive WindowResizer. [#2196](https://github.com/tommy-xr/shock2quest/issues/2196). |
| `many.mis/ND-decorations.dml` | 1577→CRYST02; 87..92→CRYST06/04/03/02/01/03 under SCP fingerprint; no equivalent override table. #2196. |
| `windowresizer.nut`, `fix_windowsize_override.nut`, `defines/defines_windows.nut` | Active Windows attachment and geometry/frame/physics conversion; overrides medsci1:207, rec2:1416/1695/93/1696/1691/271/1693, shodan:823. Generic collision offsets do not perform that conversion. [#2197](https://github.com/tommy-xr/shock2quest/issues/2197). Native registration order remains to verify. |
| `ND-apparitions.dml` → `ND-Vocalize.nut::ND_Apparition` | PlaySchemaAtLocationNet with kSoundNoNetworkSpatial; [`apparition.rs`](../shock2vr/src/scripts/apparition.rs) explicitly emits spatial=false. [#2199](https://github.com/tommy-xr/shock2quest/issues/2199), separate from facial timing. |
| `fix_shodanteleportblock.nut` | Teleport archetype attachment deletes target/link in ops1/shodan; `mission_core.rs`'s PsiTeleport arm sets markers without that guard. [#2200](https://github.com/tommy-xr/shock2quest/issues/2200). Confirm native registration and intended power activation before implementation. |
| `ND-alpha-sign-fix.dml`, `ND-heartpillowfix.dml`, `ND-wormskin_fix.dml`, hydro2 alpha patch | RenderOrder/depth changes for signs and removal of Lilypad/-1398/Worm Skin alpha; hydro2 overrides 188/146/273. No RenderOrder consumer/equivalent table. [#2201](https://github.com/tommy-xr/shock2quest/issues/2201); visual severity still requires verification. |
| All 23 `ND-envmap.dml` | ENVMAPVAR Global=`env/<mission>` has no consumer although env files mount. [#2202](https://github.com/tommy-xr/shock2quest/issues/2202); determine actual material consumers before enabling. |
| `fix_cheeseborger.nut` | scpCheeseborger checks Borg trait and full HP; current Comestible(15)/apply_comestible_use consumes at full HP and lacks that guard. [#2203](https://github.com/tommy-xr/shock2quest/issues/2203), conditional on SCP attachment. Ordinary base food consumption is intentionally separate. |

Existing [#2188](https://github.com/tommy-xr/shock2quest/issues/2188) covers organic
shared fill/rim passes: `dark/src/util/material_overlays.rs` explicitly filters
those out while retaining dedicated shine. This is an MTL limitation, not a
missing DML/NUT engine, and remains separate from this inventory.

## Mixed files and unresolved activation

These are recorded rather than promoted into speculative gameplay bugs:

- `eng2.mis/eng2.dml` also separates Seat1..Seat4 to prevent multiplayer overlap;
  `command2.mis/command2.dml` removes bogus Frozen on droid 2401. Their facial
  portions are classified above; these secondary changes need effective SCP
  mission comparison after #2195.
- `hydro1.mis/turnbull.dml` disables AI_Efficiency on Turnbull and moves 136 to
  (-65.5,123.5,-0.25). `earth.mis/tutorial_pistol_fix.dml` changes 246 reliability
  (min .50, max 5, degrade 1, silencing 0, break threshold 0).
  `shodan.mis/no_music.dml` clears SONGPARAMS. All are SCP gated; base campaign
  selection is the current difference, not conclusive evidence of a current
  equivalent gameplay failure.
- `shockscp.gam.dml` changes Droid Fusion Explosion stimulus period to 1,
  adds Weapon VibFile and ToTheMaxMeta achievements. Do not confuse the latter
  with explosion timing. Selecting SCP requires separately validating these
  source deltas; no claim of exact effect equivalence is made here.
- `gamesys_misc.nut` has an active SHODAN Fake Camera→citcam change among debug
  XYZ/Raypoint/Origin definitions; its alpha experiments are commented.
  `gamesys_weapons.nut` has active offsets, prism-casing GunFlash/velocity and
  flash/light changes. Rust handles weapon offsets, shell ejection and flashes,
  but exact attachment/property equivalence for these definitions is unresolved.
- `object_pipewrench.nut` creates optional Pipe Wrench/Avatar Pipe Wrench;
  `ND-poster.dml` creates ND-trposter. Creation of an archetype alone does not
  prove a campaign spawn/use. `vr_config.rs` explicitly documents pipewrench_h
  as lacking a selected gamesys template. These are not confirmed missing
  player encounters.
- `defines/defines_cutscenes.nut` registers credits_1..17 (including duplicate
  credits_9), test and liptest scenes. `libs/cutscene_handler.nut` is a registry/
  executor. No normal campaign trigger was established for the experimental
  scenes; registration alone cannot establish a missing retail cutscene.
- `sq_scripts/debug/*`, model preview/checklist, ray tester and dev_notes are
  tooling. `object_xerxes.nut`, defines_cheats and defines_schemas are empty or
  commented. `ND-grub.dml`'s alternative corpse/debris patch is commented out;
  this does not invalidate the separate active `object_grub.nut` animation.

## Locale and UI layout are separate

`lib.rs::mod_layer_may_override` deliberately excludes Nightdive string stubs
rather than displaying `$` tokens; real base/SCP string tables are mounted.
The current supported locale lookup and native canvas are not KEX localization
or `.itl` layout execution. Report a concrete untranslated label or placement
mismatch separately, not as a missing mission patch. Upgraded meshes/textures,
archive mounting, material support and authored weapon clips already have native
paths and must not be counted missing solely because DML/NUT is not executed.

## Reproduce the inventory check

From the repository root, with the verified installation available:

```bash
DARK_ASSET_PATH=/Users/bryan/ss2-25th python3 - <<'PY'
import collections, hashlib, json, os, pathlib, zipfile
root = pathlib.Path(os.environ['DARK_ASSET_PATH'])
assert (root / 'sshock2.kpf').is_file()
for name in ['400', 'patch_ext', 'scp', 'shtup', 'sshock2ee']:
    assert (root / 'mods' / (name + '.kpf')).is_file()
audit = json.loads(pathlib.Path('references/25th-anniversary-patch-inventory.json').read_text())
archive = root / audit['archive']
assert hashlib.sha256(archive.read_bytes()).hexdigest() == audit['archive_sha256']
with zipfile.ZipFile(archive) as z:
    expected = {p for p in z.namelist() if p.endswith(('.dml', '.nut'))}
    entries = audit['entries']
    assert len(entries) == len({e['path'] for e in entries}) == 135
    assert {e['path'] for e in entries} == expected
    assert collections.Counter(pathlib.Path(p).suffix for p in expected) == {'.dml': 74, '.nut': 61}
    for e in entries:
        data = z.read(e['path'])
        assert len(data) == e['bytes']
        assert hashlib.sha256(data).hexdigest() == e['sha256']
        assert pathlib.Path(e['rust_reference']).is_file()
        assert e['classification'] in {'implemented', 'intentionally different', 'irrelevant/debug/commented-out', 'confirmed gap'}
print('PASS: all 135 entries, hashes, source references and classifications')
PY
```

This check establishes inventory completeness and evidence identity, not runtime
behavior. No runtime/E2E or native Nightdive visual equivalence is claimed.
