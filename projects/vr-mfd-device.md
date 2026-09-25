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
3. Hold the target steady for 0.4 seconds to scan automatically, including items
   held in the other hand. Each continuous focus scans once. Disable
   `vr_mfd_focus_scan` to compare explicit holding-hand trigger scans.
   An empty other hand points at the screen: trigger clicks, squeeze pulls loot out.
   An occupied hand retains its weapon trigger and contextual face buttons.
4. Release the holding squeeze anywhere to return the instrument and close
   its UI. It cannot be dropped, duplicated, inventoried or sold.

Credential readers retain the card's close-range automatic scan. This is useful
for presenting a credential to a locked switch; it still checks the actual
keyring and lock rules. Other targets use the focus dwell (or the optional trigger mode). A purchase, keypad
entry or HRM attempt still requires a subsequent UI action.

The footer contains nanites, cyber modules, LOG, ACCESS, MFD, RES, MAP and ?. Press ?
to make the next scan read-only, including a machine or weapon that would
normally open an interactive panel. Ordinary props open item information.
LOG uses the existing collected-log reader, ACCESS the access-card list, MFD the
character sheet, and RES the research overview.

## Composition and reuse

The default body is the original stepped frame with an 8 mm solid backing.
`vr_mfd_body=3` selects it; 0, 1, and 2 retain the dark `scipass.bin`,
`upgrade.bin`, and `magci.bin` comparisons. The full 268×376 face is 14 cm
wide by default (`vr_mfd_width`). A 3.5 cm grip bezel (`vr_mfd_grip_margin`)
keeps the authored card pinch clear of the screen and footer.

**Developer → Hands & gloves → Tricorder grips** provides independent left/right
edge selection and six-axis placement. Bottom, left, top and right grips pivot
the device around the authored hand contact. Translation is controller-local;
pitch/yaw/roll also pivot around the contact. On the stepped frame the grip
bezel moves to the selected edge. These are live developer presets for testing
portrait/landscape holds; automatic edge acquisition is not implemented.
Finger curls still use the card pinch. See DEVELOPMENT.md for the HTTP keys.

The native 188-pixel main screen reserves room for the original HRM plug.
Only the AMMOFULL housing is mirrored, placing the balance wells and raised
end on the right with HRM above it. Native footer controls and their original
hit-testing are projected onto the device, rather than maintaining a second
button-action table.

Shared canvas viewports fit, crop, and optionally rotate complete panels.
Layout, text measurement and alignment happen once; both renderers and pointer
input use the same resolved rectangles and inverse transform. This supports
left panels, the right character sheet, query pages and the wide map without
panel-specific element positioning. Device geometry uses ordinary world depth.

Scans resolve one target for feedback and dispatch. World physics occludes the
ray; held items use their visible model bounds because their colliders do not
participate in ordinary interaction rays. Known UI scripts
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
- The adjustable first fit is larger than a modern phone. Evaluate legibility and comfort
  seated/standing before reducing it or deciding whether the HRM plug should
  become a physical fold-out. Headless screenshots cannot establish reach,
  stereo comfort or finger registration. No Quest was attached for this spike.

## Edge policy

Death, disabled player controls, or the cyber interface suppress the device.
Mission transitions create fresh transient state. Draw and tracking recovery restart the focus dwell. In trigger mode they
start disarmed; a release must be observed before a new scan. The
holding ray and occupied hands are excluded from UI arbitration. An empty hand beginning a grab
on the panel cannot also grab the world. Returning the device clears its panel
and utilities. Entering pointer control also requires releasing trigger/grab;
a press held while drawing, changing map presentation, or recovering tracking
cannot click. Drawing or opening a device panel closes any old world loot quad.
Live creatures cannot open loot. The experiment runs with the world live.

## Verification

The durable scenario is `tools/shock2-sdk/test/vr-mfd-device.e2e.test.ts`:

```sh
cd tools/shock2-sdk
npm run build
SHOCK2_E2E=1 node --test dist/test/vr-mfd-device.e2e.test.js
```

It covers both hands, explicit scan/no auto-purchase, focus dwell and one-shot
scanning, world/held chemical consumption, held-weapon targeting, occupied-hand
firing, empty-hand utility selection, world-quad replacement, return/redraw,
both landscape map modes, and working screen input for all eight hand/edge
combinations after live position/angle adjustments. The existing personal-card
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
proportionally to the display (about 5.2 cm at the default width) and rotates at 35 degrees per simulation second. This is
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

The default now hovers over the screen's upper image area. Disable
**Developer → Body inventory → MFD hologram over screen**
(`vr_mfd_hologram_screen`) to compare the original top-edge placement. The
hovering model's bounding sphere clears the glass by 8 mm, with the same model, size, rotation and transparency.
The original 2D art stays visible underneath so this experiment can reveal
occlusion/readability tradeoffs while tuning the display. This placement has
only been inspected in headless VR captures, not stereo on a headset.

## Button gallery and map

The gallery uses collected logs, a research project, player stats and an access
card rather than empty placeholders. MAP has two presentations selected by
`vr_mfd_map_wide`: false rotates the complete map within the main screen so
the user turns the device sideways; true opens a wider panel above the body.
Both preserve map proportions, footer controls, and close-button input. The
physical body stays fixed in either mode. The gap beside the wide panel does
not claim pointer hits. Selecting MFD replaces the map, and MAP toggles it closed.
Zoom and pan remain follow-up work.

The belt-draw recording drives the real squeeze/release path with simulated
controller poses. It shows the authored card grip and the selected device body,
not a headset recording or a finished grip animation.

The idle screen uses retail `iface/query.pcx`, with target name in its title
and scan instructions in its description. There is no extra floating hint.
