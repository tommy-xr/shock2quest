# VR personal access card

Accepted design, 2026-09-12. Supersedes PR #1387's proposed body anchors/card
interaction. Reuse the current shoulder backpack and shared body-inventory frame.

## Interaction contract

- Grip nanites, cyber modules, supported software upgrades or found access cards
  to hold them. Credit nothing until release. Release anywhere collects exactly
  once through the existing scripts, independent of backpack capacity.
- Confirm collection immediately with sound and a pulse in the releasing hand.
  A short download trail may travel from the pickup to the personal card;
  animation and visibility never control whether collection succeeds.
- The personal card represents balances, software and collected credentials.
  It exists from the start on the belt, draws into either free hand, and returns
  on release. It is not an inventory item and cannot be lost or consumed.
- In VR, credential-locked doors/readers and machines require a physical scan.
  Unlocked ordinary doors retain normal operation. Machine scanning opens or
  authorizes its interface; spending still requires explicit purchase selection.
- One scan per presentation, rearmed by withdrawal. Distinct success/denial cues.
  Tracking loss, pause, death and transitions cannot invent grabs or scans.
- Flat interactions retain their existing behavior. Audio/data logs retain their
  separate immediate-collection behavior.

## Implementation increments

1. Hold/release collection and sound/haptics.
2. Persistent belt card and download feedback.
3. Mandatory readers with device interaction verification.

Implemented in `mission/personal_card.rs` and the shared hand/collection paths.
Current machine readers: replicators, stat/weapon/tech/psi trainers, OS upgrade
machines, computers/security computers, energy stations and resurrection stations.
Authored KeyDst objects are credential readers. Unimplemented machine scripts
remain unimplemented; this change does not invent their service behavior.

Verification covers deterministic VR collection and scanning plus flat/log
regressions. The replacement PR records device verification and its limits.
Headless poses establish placement and state changes; they do not establish
seated/standing reach comfort or how a haptic pulse feels.
