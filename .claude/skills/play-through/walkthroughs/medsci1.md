# Walkthrough context — medsci1 (MedSci Deck 2)

Use this as **intent and decision context**, not as a coordinate script. The
playtester must still look at screenshots, move through collision-valid space,
discover runtime entity IDs on each launch, and interact with the real world.
Reaching the Engineering trigger by teleporting to it is only a transition smoke
test; it is not a MedSci playthrough.

## Mission objective and success condition

The critical-path objective is to obtain the maintenance-shaft code from Dr.
Watts, return to the shaft beside the main elevator, open it with `12451`, and
descend into `eng1`. Reviewed campaign evidence may accumulate these
prerequisites across bounded sessions, but each prerequisite needs a ledger
checkpoint record pointing to its reviewed `data.json`; a frontier save alone
is not evidence. Before declaring the mission complete, a manager-labeled final
validation replay must exercise the full world flow below from a fresh start and
end with the active mission changing to `eng1.mis`.

External route cross-checks:

- [SShock2 MedSci walkthrough](https://www.sshock2.com/ss2walk/)
- [GameBanshee MedSci Science walkthrough](https://www.gamebanshee.com/systemshock2/walkthrough/medsciscience.php)

Mission-data facts were cross-checked with `cargo dq entities medsci1.mis`.

## Critical path and decisions

### 1. Escape the cryo starting area

1. Search the nearby corpse and take the wrench. Equip it.
2. Break the fallen air duct obstructing the ladder, then climb the ladder. In
   flat mode, push toward the ladder to ascend; look down and push to descend.
3. Use the door button, take the Cryogenics access card from the next room, and
   use the nearby card slot.
4. Obtain the audio log from the corpse at the keypad door, enter `45100`, and
   continue.
5. Crouch through the narrow vent and drop into the powered-door room.
6. Take the dead power cell, recharge it at the station in the same room, and
   insert the charged cell into the door receptor.
7. Ride the lift to the upper cryo level, find the Science Sector access card in
   an adjoining room, and use it to leave cryo.

Important decision rule: if the agent cannot loot the corpse, swing the wrench,
climb the first ladder, enter the keypad code, crouch through the vent, operate
the recharger/receptor, or ride the lift, stop at the **first** failed mechanism.
Distinguish a gameplay feature gap from a missing automation control. Do not
silently replace the whole sequence with `give`, remote `Frob`, quest mutation,
flight, or teleport and then claim it passed.

### 2. Open the Medical bulkhead

1. Use the Science card at the sector door. The main elevator is nearby but is
   unpowered; the maintenance shaft opposite it is the eventual exit.
2. Find the dead power cell near the Medical bulkhead.
3. Follow the science-sector route to the pump station. This requires descending
   another ladder and dealing with the guarded recharging room.
4. Recharge the cell, climb back out, return to the receptor, and activate the
   bulkhead into Medical.

### 3. Obtain Crew and R&D access

1. In Medical, follow the route past the radiation rooms toward the multilevel
   ladder room.
2. Descend to the corpse holding the Deck 2 Crew access card, climb back up, and
   return to the locked Crew door near the Medical bulkhead.
3. Work through Crew Quarters to Dr. Watts' office and take the Research &
   Development access card.
4. Return through the bulkhead to the Science sector.

### 4. Find Watts and enter Engineering

1. Use the R&D card at the door near the replicator and proceed through R&D.
2. Descend the lift to Dr. Watts. Let the authored encounter complete and obtain
   the audio log containing maintenance code `12451`.
3. Return to the maintenance shaft opposite the main elevator.
4. Enter `12451`, open the shaft, and descend its ladder. Crossing the transition
   volume should load `eng1.mis`.

## Authored data landmarks

These stable mission-file facts help review a run; resolve concrete runtime IDs
by name/template every launch.

- Wrench template: `-928`. The starting wrench is contained by a corpse; it is
  not merely a loose item to grant from anywhere.
- Cryo keypad: mission entity `1681`, code `45100`, around
  `(-26.53, 0.24, -12.66)`.
- Science card: template `-159`, around `(-16.56, 0.82, -68.94)`.
- Dead power cells: template `-1862`; MedSci contains two because both powered
  door sequences are part of the intended route.
- Ladder objects inherit `PropPhysAttr.climbable != 0`. The final shaft ladder is
  around `(12.58, -5.62, -42.69)`.
- Final keypad: mission entity `809`, code `12451`, around
  `(6.34, 0.43, -47.44)`.
- MedSci to Engineering transition: `TrapTripLevel`, `PropDestLevel("eng1")`,
  `PropDestLoc(21)`, around `(12.68, -5.64, -43.59)`.

Coordinates are diagnostic landmarks, never permission to teleport during a
playtest. They are useful for checking that the agent is pursuing the correct
object and for reproducing a validated blocker in isolation.

## Review checklist

A reviewer should reject a fresh session—or a resumed session without a reviewed
ledger checkpoint and source `data.json` for the skipped prerequisite—as shallow
or invalid when it:

- surveys the spawn room without attempting the wrench/debris interaction;
- uses vertical debug flight and calls that ladder traversal;
- grants key items from across the map rather than reaching their authored
  container/location;
- directly triggers the Engineering transition without completing the route;
- reports a decorative object as broken without matching it to a walkthrough
  step or an authored property/link.

When an automation control is absent, record that gap, preserve a screenshot and
frontier, and use the narrowest diagnostic bypass only to discover the **next**
blocker. A bypass does not make the bypassed gameplay checkpoint pass.
