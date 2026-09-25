# Belt MFD device spike

An opt-in alternative to the personal access card from #1533. The instrument
returns to the authored belt anchor when released. Player balances, credentials,
logs and research stay in their existing owners; the device is a transient
presentation, not another inventory item or a second copy of player state.

Enable **Pause → Developer → Body inventory → MFD device prototype** on Quest
or any VR runtime. The session-only `vr_mfd_device` toggle defaults off and
is also available through the debug API. For a repeatable desktop launch, run:

```sh
cargo dbgr --mission earth.mis --vr --experimental mfd_device
```

For layout inspection with a mouse, the same compact canvas has a flat preview:

```sh
cargo dbgr --mission earth.mis --experimental mfd_device_preview
```

The preview does not simulate drawing or scanning; frob a nearby object through
the debug API to open its MFD. Both presentations use the same compact canvas.
Normal flatscreen and VR behavior stay behind the existing paths when neither
flag is supplied.

## Interaction

1. Squeeze an empty hand at the belt buckle to draw the instrument.
2. Point that hand at an object within two metres. A cyan beam/hit marker and
   the idle screen's target name identify the object that will receive the scan.
3. Pull the holding hand's trigger once. A held trigger cannot repeat scans.
   The free hand points at the screen: trigger clicks, squeeze pulls loot out.
4. Release the holding squeeze anywhere to return the instrument and close
   its UI. It cannot be dropped, duplicated, inventoried or sold.

Credential readers retain the card's close-range automatic scan. This is useful
for presenting a credential to a locked switch; it still checks the actual
keyring and lock rules. Other scans require the trigger. A purchase, keypad
entry or HRM attempt still requires a subsequent UI action.

The footer contains nanites, cyber modules, LOG, ACCESS, MFD, RES, MAP and ?. Press ?
to make the next scan read-only, including a machine or weapon that would
normally open an interactive panel. Ordinary props open item information.
LOG uses the existing collected-log reader, ACCESS the access-card list, MFD the
character sheet, and RES the research overview.

## Composition and reuse

The body is a simple solid slab with a stepped-corner bezel. Its main screen is
188 native pixels wide (16 cm in this first fit), preserving the original
MFD proportions. Its 268×376 canvas reserves space to the right for the
original 73×194 hack/repair/modify plug. The bottom uses AMMOFULL art with
balance wells and the original utility button art. Scan hints and the target name
appear on the idle screen; there is no separate status footer.
This is deliberately placeholder geometry; the inherited card hand pose needs
proper device-specific authoring before shipping.

`mfd_device` translates the complete original left MFD into its screen slot;
the character sheet translates from its original right MFD slot instead.
Text measurement and alignment still happen once in the shared canvas.
The same tile translation is inverted for input and used by debug introspection.
The free-hand ray uses existing pointer arbitration and existing trigger/grab
swallowing. Device geometry renders in world space, with ordinary depth, without
the cyber interface's dimming or system-overlay depth clear.

Scans resolve one physics hit for target feedback and dispatch. Known UI scripts
receive their ordinary frob; unrelated world props are only inspected. Weapon
settings receive an explicit scanned target, validated as a nearby gun; ordinary
weapon-settings selection continues to require a held weapon. Existing HRM
code retains requirements, costs and outcomes.

## Current scope and follow-up

- Replicators, containers/corpses, keypads, security machines and credential
  readers use their existing panels/rules. Grabbing a loot slot uses the normal
  entity/hand ownership transfer.
- Guns open the existing settings and applicable repair/modify plug. This is
  a prototype of presentation and targeting, not a rewrite of HRM mechanics.
- A carried research specimen can be scanned to begin research and open its
  report. A world specimen opens the overview and asks the player to hold it;
  existing research progress still requires carrying the specimen.
- Scanning a chemical can supply the active project's requested chemical,
  including a nearby world chemical. Existing requirement checks determine
  consumption, and a stack loses only one unit. Unneeded chemicals are retained.
- Flashlight, device-specific glove grip poses, direct fingertip touch,
  haptic tuning and production discoverability remain follow-up work.
- The first fit is larger than a modern phone. Evaluate legibility and comfort
  seated/standing before reducing it or deciding whether the HRM plug should
  become a physical fold-out. Headless screenshots cannot establish reach,
  stereo comfort or finger registration. No Quest was attached for this spike.

## Edge policy

Death, disabled player controls, or the cyber interface suppress the device.
Mission transitions create fresh transient state. Draw and tracking recovery
start trigger-disarmed; a release must be observed before a new scan. The
holding ray is excluded from UI arbitration, and a free hand beginning a grab
on the panel cannot also grab the world. Returning the device clears its panel
and utilities. The experiment runs with the world live.

## Verification

The durable scenario is `tools/shock2-sdk/test/vr-mfd-device.e2e.test.ts`:

```sh
cd tools/shock2-sdk
npm run build
SHOCK2_E2E=1 node --test dist/test/vr-mfd-device.e2e.test.js
```

It covers both hands, explicit scan/no auto-purchase, held-trigger debounce,
free-hand utility selection, return and redraw. The existing personal-card
scenarios cover the flag-off baseline. Rust tests cover tile/input geometry,
trigger recovery, shared panel behavior and card ownership. PR media records
additional exercised interactions; claims about unexercised paths remain
implementation scope, not measured device evidence.

## Retail layout and hologram refinement

AMMOFULL is drawn at its authored 260×64 size. Its black wells
contain nanites and cyber modules with their original icons and counts. The
housing alone is mirrored: the raised end sits on the right below HRM, its
wells are on the right, and the native button strip is on the left. The
RES, ?/MAP, LOG and MFD strip follows #1705; ACCESS occupies the remaining
space between the strip and the wells. Icons and text are never mirrored. Scan mode uses a green screen and the shared
MFD font for aiming/trigger instructions. Device loot uses the complete
`contain.pcx` canvas and its original grid; free-standing VR loot keeps its
holographic grid. Both renderers and hit tests consume the same resolved layout.

A committed scan retains a translucent copy of the target model above the
screen until another scan or holstering. The model's bounding sphere is fitted
to an 8.5 cm diameter and rotates at 35 degrees per simulation second. This is
render-only geometry, with per-object opacity and lighting overrides: it has no
collider or inventory identity and does not alter the target's material. A faint
unlit green copy preserves its silhouette against dark backgrounds. Targets
without a loaded model simply omit the miniature. The wider lower housing and
hologram clearance still need headset comfort testing.

## Weapon HRM from the device

Working supported guns offer MODIFY; broken guns offer REPAIR. The scanned
world gun may enter and complete the existing paid HRM board without being
wielded, but only while it is the active device's explicit target and remains
within three metres. Skill, condition, modification-level and nanite checks
still run on each attempt. Holstering, leaving reach, or selecting another
weapon invalidates that authorization. Normal weapon interfaces still require
wielding. Runtime captures completed a 20-nanite pistol modification and a
3-nanite repair on the same world entity.

## Hologram placement comparison

The default remains above the device's top edge. Enable **Developer → Body
inventory → MFD hologram over screen** (`vr_mfd_hologram_screen`) to compare a
projection hovering over the screen's upper image area. Its bounding sphere
clears the glass by 8 mm, with the same model, size, rotation and transparency.
The original 2D art stays visible underneath so this experiment can reveal
occlusion/readability tradeoffs before choosing a default. This placement has
only been inspected in headless VR captures, not stereo on a headset.
