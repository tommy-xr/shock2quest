# Astra VR wrist readouts

Health and psi occupy the dorsal watch face on both gloves. Each underside shows the weapon actually held in that hand: clip count, ammo icon and ammo type, with a dedicated amp mount deferred because it retains its authored forearm. Empty hands, melee weapons and a supporting hand have no ammo plate. Dual wielding a pistol and shotgun therefore gives independent counters; a gun beside the amp keeps its own ammo display. Monitors share the glove visibility rule: the psi amp and unstripped legacy weapon hands have no glove plate.

The plates use the calibrated glove's wrist/palm basis and the final visible hand pose, including physical weapon displacement and two-hand support. Left-hand text uses a proper rotation rather than a mirrored text surface. Cropped shipped BIO/AMMO art and overlays share the existing flat/MFD layout. The watch has no clickable controls; entering the cyber interface suppresses these plates as before.

## Next stack layer: dual weapons in the cyber interface

The current interface still resolves its single weapon panel using the existing right-hand-first convention. Expand it to two labeled panels (Left weapon / Right weapon) when needed. Resolve a weapon entity once for each panel and bind reload, ammo cycling and settings to that same entity; revalidate ownership when activating a control. Do not merely duplicate the current global controls. Keep a single panel for flat presentation, where only one weapon is wielded.

## Optional placement mode

An editable weapon-pinned ammo plate can reuse the per-weapon readout. Store its position/XYZ rotation/uniform scale with the weapon grip metadata and expose it in Explorer. Attach it to the final weapon transform, so physical contact and two-hand rotation keep it aligned. This should be an alternative to underside ammo, not a second copy. Larger weapons and the psi amp's authored forearm are especially useful fit tests.

## Verification

Use debug_weapons in VR with pistol (template -17), shotgun (-19) and psi amp (-247), discovering concrete entity IDs each run. Check dorsal and palm views, independent 12/6 counts, gun plus amp, empty hand, and opening/closing the cyber interface. Unit tests cover per-weapon selection, hand swaps, mixed gun/amp readouts, crop parity and non-mirrored opposite faces. Headset review remains necessary for comfort and readability at arm's length.
