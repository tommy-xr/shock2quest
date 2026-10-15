# Quest player guide

See the [installation guide](../INSTALL.md) to set up your headset.
For keyboard and mouse bindings, see the [desktop runtime guide](../runtimes/desktop_runtime/README.md).
Flat-mode equivalents are included below where relevant.

## Controls


| Control | Action |
| --- | --- |
| Left stick | Move |
| Right stick left/right | Turn (30° snap by default; center the stick between turns) |
| Crouch physically, or click the left stick | Crouch; click again to stand |
| Lower face button: left **X** / right **A** | Jump, even with both hands full; close the cyber interface when it is open |
| Left **Menu** | Tap to toggle the cyber interface; hold 0.5 seconds to pause |
| Grip | Pick up and hold an item; release to drop or throw it |
| Trigger | Use the held item, fire a gun, or cast with the psi amp; point and trigger to interact |

**Options → Comfort / Controls** configures the movement vignette and snap or
smooth turning in both VR and flatscreen. Settings persist across sessions. Options is available from the
main menu and while paused.

The **upper face button** (left **Y** / right **B**) acts on what that hand holds:

| Held item | Tap | Hold 0.5 seconds |
| --- | --- | --- |
| Gun | Switch fire mode | Eject the loaded clip |
| Psi amp | Swap current and alternate powers | Open the power selector |
| Ammo clip | Swap to the next compatible ammo type in your backpack | — |
| Empty hand or other item | Open/close the newest unread audio log; replay the latest once all are read | — |

## Weapons and powers

Reload by taking a clip from the belt's ammo pouch or
the cyber interface's inventory strip and bringing it to the gun. With a
partially loaded gun in the other hand, matching ammo draws at most the missing
rounds; the rest stay in your backpack. If the selected reserve runs out, the
pouch automatically offers another stocked compatible type, showing its icon
and count. A different type draws a replacement magazine. A full gun still offers a spare clip;
tap the clip hand's upper button (**B** on the right, **Y** on the left) to
cycle through compatible ammo carried in your backpack. Inserting another ammo
type returns the loaded rounds to your backpack. In the amp's power selector, use that hand's
stick up/down for tiers and left/right for purchased powers; confirm with its
upper button or trigger. Each amp remembers its current and alternate powers.

The pouch also offers carried prisms and worm beakers for compatible guns.
With a laser pistol or EMP rifle held, it offers one portable battery at a time
and shows the number carried. Release unused ammo or a battery over the pouch
to return it to your backpack. Drawing an item does not use it.

## Parrying

Meet a pipe hybrid's incoming pipe with your held wrench or other
melee weapon to block it. Guns can also block, costing 5 condition points per
parry; a gun at zero condition cannot block. Placement matters: a stationary guard works, but the
pipe must touch your weapon before it touches you. A block clangs, pulses the
blocking controller, and sends the hybrid's swing back to its ready pose.

## Maintenance tools

In flat inventory, drag a tool onto a ranged weapon.
In VR, bring the held tool close to a weapon and pull the trigger or release
it onto the weapon; the weapon can be in your other hand. The preview shows
condition gained and the one-tool cost. Maintenance skill determines the gain;
broken weapons need repair first. A refused use consumes nothing.
Auto-repair devices use the same drag or close-contact gesture on a broken
item, consume one device, and bypass the Repair skill and minigame.
Recyclers use the same gesture to convert an item's entire stack into nanites;
the preview shows its value. The recycler itself is reusable.
To collect worm ammunition, touch an empty beaker to a worm pile in VR and
pull the trigger or release it. In flat mode, use the pile while carrying an
empty beaker. Small beakers hold four rounds; large beakers hold eight.

Broken replicators show static in the MFD and tricorder. Select their Repair
control to attempt the skill-gated repair minigame; a win restores the catalog,
and a failed attempt leaves the machine broken and available for another try.
Use the catalog's page arrows to reach the fifth and sixth items.

Empty hands ease toward a partial point at usable controls or a partial grip
near reachable items, occupied holsters, stocked ammo pouches, and climbing holds.
These previews only pose the fingers; squeeze or pull the trigger to interact.

## Inventory and equipment

Crates and corpses open in the held tricorder's MFD,
or in the cyber interface when the tricorder is stowed. Grip an inventory item, point at its destination,
and release. The preview is green when it fits or merges, amber for automatic
placement elsewhere, and red when releasing will drop it into the world.
Pass an item between hands by squeezing the receiving hand nearby, then releasing
the carrying hand. Stow a compact gun, melee weapon, or amp at either thigh by
releasing it there; squeeze again to retrieve it. Holsters also work while the
cyber interface is open, including items drawn directly from its inventory.
An empty hand near a body slot keeps its world trigger; only a squeeze claims it.
Empty holsters, chest mounts and shoulders, and the ammo pouch while the other
hand has no gun, ignore empty hands entirely, so they neither highlight nor shadow
nearby targets.

## Access and pickups

Grip nanites, cyber modules, software upgrades, or found
access cards, then release anywhere to download them. Draw your personal access
card from the belt buckle to scan readers, shops, and trainers; release to return
it to the belt. Scanning opens an interface; select purchases separately.

## Chest quick slots

Release a held hypo or battery over either chest mount to
store it; squeeze with either empty hand to retrieve it. Each slot shows a circle
and the stored item’s icon. Its colour previews the target, and a sound and short
vibration confirm storage. The count totals matching items across backpack cells.
Drawing takes one item and refills from remaining stock; returning a matching item merges into its
reserve. The slots add no inventory capacity. A slot with a different item,
an incompatible item, or a full backpack with no matching stack keeps the item
in hand; squeeze again to re-grip before releasing elsewhere. Drawing never uses
the item; release the trigger before deliberately using it.
Empty slots let empty-hand grabs of nearby world objects pass through.

## Climbing

Squeeze an empty hand near a ladder or ledge to grab it, then move
that hand to pull yourself along. A cyan marker confirms the hold. To climb onto
a deck, keep one hand attached while placing the other on the edge, then pull
down or inward. To descend, crouch, grip near the ladder's top, pull yourself past
the edge, and raise the held hand to lower your body. Release grip to let go.

## Armor

Open the cyber interface and point at armor in your inventory, then
pull the trigger to equip it (double-click in flat mode). The paperdoll's ARMOR
slot shows the worn suit; point and trigger there to remove it. Only one suit
can be worn, and it keeps its backpack cells. Strength requirements and research
apply. Powered armor shows remaining charge and recharges at a recharge station;
empty armor gives no protection. Worm Skin grants +2 PSI but spends one PSI point
every 30 seconds, or one health point when PSI is empty.

## Implants

Bring a held implant to the narrow socket on the pinky side of the opposite wristband and
release grip to install it. Squeeze with the empty opposite hand to remove it.
Either wrist can hold your first implant; the other locks while it is occupied.
Cybernetically Enhanced allows both wrists at once. The implant seats inside
the raised rim, with a separate bar below the housing showing its remaining charge. Installed implants keep their backpack slot and need power to
supply their bonuses. Recharge them at a recharge station.

Development builds carry additional debug keys and a Developer options screen -
see [development guide](../DEVELOPMENT.md#debug--developer-keys).

[Back to Shock2Quest](../README.md)
