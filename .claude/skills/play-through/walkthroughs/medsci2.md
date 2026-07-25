# Walkthrough — medsci2 (Medical / Crew round trip to MedSci R&D)

Context for the `playtest` agent and the `play-through` reviewer. This is the
**genuine MedSci2 excursion required by the MedSci1 campaign**, cross-checked
against retail walkthroughs and the entities, containment links, key regions,
quest bits, and transitions in `medsci2.mis` and `medsci1.mis`.

This is not a movement script. Observe and react, but do not count entering
`medsci2`, visiting Watts' empty office, or returning through a bulkhead as
completion. The excursion's purpose is to acquire the Crew card, use it to enter
Crew Quarters, loot the R&D card from Watts' desk, carry it back into MedSci1,
and use it to reach the actual Dr. Watts and learn code **12451** in fiction.

## Important map correction

Retail guides often describe Medical, Crew, and R&D as one continuous Deck 2
route. The port splits that route between two mission files:

- **`medsci2.mis`** contains Medical, Biopsy, Crew Quarters, Watts' office, and
  the R&D access card.
- **`medsci1.mis`** contains the R&D sector, the physical Dr. Watts encounter,
  Watts' audio logs, and the maintenance conduit keypad that accepts 12451.

Therefore, the physical Watts encounter is **not** a MedSci2 entity. A tester
searching `medsci2` for Watts, or claiming to have received 12451 merely by
reaching his office, is on the wrong map / has not completed the objective.

Mission object ids below are stable only within the named `.mis` file. Runtime
entity ids are not stable; discover entities by name and `template_id` each run.
Positions are mission `PropPosition` coordinates and are route anchors, not
authorization to teleport.

## Valid campaign start

Start from the advancing `medsci1` campaign after the second power-cell puzzle:
the dead cell from Medical reception has been charged, inserted into Aux Power,
and the **north Medical bulkhead** has been opened normally. Enter through its
button at about `(43.9, 0.0, -76.4)`, transitioning to **`medsci2` loc 100**.

A fresh direct launch of `medsci2.mis` can inspect entities, but does not prove:

- that the power-cell / bulkhead gate worked;
- that inventory and quest state survived the first transition;
- that the acquired cards survive the return transition; or
- that the full MedSci1 → MedSci2 → MedSci1 objective loop is playable.

Do not use `/v1/player/give`, raw teleport, direct transition, injected quest
bits, or entity messages in place of the in-world interactions below.

## Mission topology and transitions

The expected campaign route makes a loop rather than simply touching MedSci2:

| leg | real interaction | mission object / position | result |
| --- | --- | --- | --- |
| MedSci1 → Medical | powered north bulkhead button | `medsci1` id 1009 @ about (43.9, 0.0, −76.4) | `medsci2`, loc 100 |
| Medical → Crew | Crew card reader | `medsci2` id 681 @ (41.6, 0.0, −70.9), or opposite-side reader id 1277 | opens Security Door id 1635 |
| Crew → MedSci1 | southern return bulkhead button | `medsci2` id 470 @ (28.1, 0.0, −20.4) | `medsci1`, loc 300, near the R&D approach |
| alternate backtrack | northern return bulkhead button | `medsci2` id 463 @ (26.5, 0.0, −76.4) | `medsci1`, loc 100 |
| MedSci1 → R&D | R&D card reader | `medsci1` id 1464 @ (−2.4, 0.2, 9.7), or opposite-side reader id 1576 | opens Security Door id 425 and awards the linked objective XP |

The southern return is the retail-style continuation after Crew and places the
player on the useful side of the MedSci1 loop. The northern button is a valid
way back if the player deliberately backtracks, but returning without the R&D
card is not progress.

## Required items and objective evidence

### 1. Crew card — Grassi / Biopsy corpse

- `medsci2` mission object **1904**, `Crew Card`, parent template **−532**
- contained by MS Male Corpse **id 105** at
  **(−16.1, −6.6, −112.7)**
- key region **8**
- quest bit **`grassicard`** is set to raw `1` (`incomplete`) when frobbed

From the upper Medical rooms, reach the lower Biopsy area by the real ladder
stack around **(−13.8, −3.6, −116.0)**. Frob the corpse, inspect its container
MFD, and click the Crew Card. It is contained loot and never legitimately lies
on the floor at the card's authored coordinates.

The card must then be used at a region-8 reader. Reader id 681 SwitchLinks both
the Crew Security Door and EmailTrap id 536, so a genuine use opens the blast
door and advances the accompanying objective communication.

### 2. R&D card — Watts' Crew-quarters desk

- `medsci2` mission object **772**, `R and D Card`, parent template **−533**
- contained by Desk #2 **id 503** at **(39.9, −5.7, 34.5)**
- key region **4**
- quest bit **`RDGrab`** is set to raw `1` (`incomplete`) when frobbed
- the same desk also contains the optional MFD Game Player

Proceed through Crew Quarters to the office marked by Dr. Watts sign id 422 at
about **(42.5, −4.3, 41.9)**. Watts himself is not here. Frob the desk and click
the R&D Card in its container panel.

There are no region-4 R&D readers in `medsci2`; mission data contains only the
region-8 Crew readers. This is deliberate evidence that the R&D card must be
carried back to `medsci1`.

### 3. Watts and the 12451 log — after returning to MedSci1

- Dr. Watts is `medsci1` mission object **734** at
  **(−18.5, −4.6, 30.2)**
- the posed / invulnerable Watts entity contains audio logs ids 251 and 252
- log **id 251** has `PropLog { deck: 2, log: 14 }` and carries quest-bit
  metadata named **`wattsy`**
- the shipped `LEVEL02.STR` transcript explicitly conveys conduit code
  **12451**

Use the carried R&D card at one of the MedSci1 region-4 readers, pass through
the now-open R&D door, and traverse the sector to Watts. Reach and frob the
physical character; his loot MFD must open. Click the deck-2 log-14 disc and
read its transcript through the game UI. A valid run records deck 2 / log 14 in
`player.collected_logs`, shows **12451** to the player, and advances the
mission's campaign-objective chain: `wattsy=incomplete` (raw marker bit 1),
`Note_2_6=complete`, and `Note_2_7=incomplete` (the newly active note containing
12451).

Knowing 12451 from this guide, a source file, or a previous run is not objective
coverage. Nor is a visible transcript alone sufficient if the campaign remains
stuck on “get the code from Dr. Watts.” The campaign requires both a working
in-fiction conveyance path and the authored objective advancement.

## Ordered objective snapshots

Capture `/v1/quests` at each boundary; do not infer progression from doors or
text alone:

| genuine boundary | expected objective evidence |
| --- | --- |
| Crew Card clicked in Grassi's corpse | `grassicard=incomplete` (raw 1), then `Note_2_9=complete` |
| R&D Card clicked in Watts' Crew-office desk | `RDGrab=incomplete` (raw 1), then `Note_2_5=complete` |
| return to MedSci1, use the R&D card, physically reach Watts | `Note_2_3=complete`; `Note_2_6` remains the active “get the code” objective |
| click Watts' deck-2/log-14 disc | deck 2 / log 14 collected, `wattsy=incomplete` (raw 1), `Note_2_6=complete`, `Note_2_7=incomplete` |
| later accept 12451 at MedSci1 keypad 809 | `Note_2_7=complete` |

The first two marker-to-note transitions are explicit `FrobQB` /
Simple-QB-trigger chains in `medsci2.mis`. The Watts chain is authored in
`medsci1.mis`: Simple QB Trigger id 1284 watches `wattsy` and SwitchLinks QB
setters for `Note_2_6=complete` and `Note_2_7=incomplete`.

## Critical path

### Phase A — enter Medical and find Grassi

1. From the real MedSci1 frontier, charge and install the second power cell,
   frob the powered north bulkhead button, and verify the mission becomes
   `medsci2` at loc 100 with carried state intact.
2. Explore the Medical / Biopsy route using bounded movement. Handle hybrids,
   cameras, and other threats through normal combat or sensible avoidance.
3. Descend the Biopsy ladder near `(−13.8, −3.6, −116.0)` to the lower floor.
   Do not raw-teleport to the corpse.
4. Frob MS Male Corpse id 105 and click the contained Crew Card. Confirm the
   player acquired key region 8 and `grassicard` appeared.
5. Climb / traverse back out of the lower Biopsy area by normal movement.

### Phase B — use Crew access and search Watts' office

6. Reach the Crew entrance around `(39.2, 0.5, −69.7)`. Use the actual Crew
   card reader and allow Security Door id 1635 to open before crossing it.
7. Traverse Crew Quarters rather than jumping to its far end. Treat its
   enemies, doors, lower-level changes, and container interactions as gameplay
   coverage.
8. Find Watts' marked office and frob Desk #2 id 503. Click the contained
   **R and D Card** and confirm key region 4 plus `RDGrab`.
9. Walk back to the southern Medical / Science bulkhead and frob return button
   id 470. Verify transition to `medsci1` loc 300 without losing the R&D card,
   Crew card, inventory, health, or quest bits.

### Phase C — finish the cross-map objective in MedSci1 R&D

10. Walk to R&D reader id 1464 or 1576 and use the carried region-4 card. Wait
    for Security Door id 425 to open, then cross it normally.
11. Traverse R&D to Dr. Watts id 734 at `(−18.5, −4.6, 30.2)`. Frob Watts and
    verify his loot panel contains both audio logs.
12. Click audio log id 251 (deck 2 / log 14) and open/read its transcript in the
    log UI. Confirm the visible text conveys **12451** and the collected-log
    state persists. Inspect `/v1/quests`: `wattsy` must appear, `Note_2_6` must
    become complete, and `Note_2_7` must become the active incomplete note. A
    readable transcript without this transition is a campaign blocker, not a
    completed objective.
13. Only then return to the maintenance conduit keypad in MedSci1 and enter
    12451 as part of the enclosing `medsci1` walkthrough.

## Acceptance checklist — what counts as genuine play

A MedSci2 round-trip pass is valid only if the session evidence shows all of:

- Entry through the powered MedSci1 north bulkhead from campaign state, not a
  fresh direct MedSci2 launch used as end-to-end proof.
- Bounded traversal to the lower Biopsy area and back, including the ladder;
  no raw teleport bridging the vertical gate.
- Grassi's corpse container opened and Crew Card id 1904 clicked through the
  loot MFD; region 8 and `grassicard` recorded.
- The real Crew reader used, Security Door id 1635 opened, and the player
  physically crossed into Crew Quarters.
- Watts' office reached through Crew; Desk id 503 opened and R&D Card id 772
  clicked through the loot MFD; region 4 and `RDGrab` recorded.
- A real return-bulkhead interaction and MedSci2 → MedSci1 transition, with
  both cards and quest state preserved across the mission boundary.
- The R&D card used at a region-4 reader in MedSci1 and R&D Door id 425 crossed.
- The physical Watts entity reached and frobbed; deck-2 log 14 taken from his
  loot panel, recorded in `player.collected_logs`, and its in-game transcript
  visibly read as containing 12451.
- The same real log acquisition setting `wattsy` and driving
  `Note_2_6=complete` → `Note_2_7=incomplete`; visible text or collected-log
  state alone cannot substitute for campaign progression.
- Critical-route threats handled with player movement/combat/avoidance, not
  debug `Damage`, `TurnOn`, or direct-transition messages.

The following are specifically **insufficient**: touching both mission files,
opening either return bulkhead without the R&D card, giving either card through
the debug API, injecting `grassicard` / `RDGrab` / `wattsy`, using the R&D card
without first opening Crew legitimately, typing 12451 from walkthrough
knowledge, or reading `LEVEL02.STR` outside the game instead of acquiring
Watts' disc. A visible 12451 transcript while `wattsy` is absent or
`Note_2_6` / `Note_2_7` remain unchanged is also insufficient.

## Engine watch-points for triage

1. **Contained-card acquisition combines two scripts.** Crew and R&D cards use
   `KeyCardScript` to acquire their key region and `FrobQB` to set their quest
   bit / destroy the world entity. Verify both effects happen after a container
   click. A disappearing card without the matching region or quest bit is a
   real `[gameplay]` bug.
2. **The cross-mission boundary is the core test.** Region-4 access and
   `RDGrab` must survive MedSci2 → MedSci1. Do not work around a lost card by
   giving another one or manually opening Door 425.
3. **Watts' loot/log reader has a likely integration edge.** The current
   `creature-loot.e2e.test.ts` proves that Watts opens a loot panel and that
   clicking log 14 records it. It then teleports to the contained disc's own
   authored position before reopening the reader because the disc retains that
   position and the reader's walk-away check can close when the player stands
   at Watts. A genuine playtest must not use that workaround. If clicking the
   disc at Watts cannot leave/open a readable transcript containing 12451,
   classify it as a `[gameplay]` bug in the objective's conveyance path.
4. **Watts is intentionally posed and invulnerable.** Do not attack or
   debug-damage him. The required interaction is frob → container MFD → log.
5. **The current log script does not apply `wattsy`: campaign-blocking feature
   gap.** Log id 251 carries `PropQuestBitName("wattsy")`, and mission trigger
   1284 depends on that marker to complete `Note_2_6` and activate `Note_2_7`.
   The port's `MediaGui` / `CollectLog` path currently records the log, resolves
   its strings, plays audio, and fires SwitchLinks, but does not consume the
   quest-bit metadata. If real acquisition therefore leaves `wattsy` absent and
   `Note_2_6` / `Note_2_7` unchanged, classify it as a campaign-blocking
   `[gameplay]` **feature gap**. Do not bless a visible-transcript-only pass,
   inject the marker, or type 12451 to play past it.
6. **Traversal is coverage.** The Biopsy ladder, Crew reader/door, desk loot
   UI, southern bulkhead, R&D reader/door, and Watts container/log UI are all
   required gates. Stop and report the first failed real interaction rather
   than warping past it.

## Sources

- [SShock2.com full walkthrough, Deck 2](https://www.sshock2.com/ss2walk/) —
  Biopsy corpse, Crew card, Watts' office, R&D card, Watts, and 12451 route.
- [GameFAQs guide by DC](https://gamefaqs.gamespot.com/pc/185706-system-shock-2/faqs/7173) —
  independent Medical → Crew → R&D ordering and Watts location.
- [GameFAQs guide by garkimasera](https://gamefaqs.gamespot.com/pc/185706-system-shock-2/faqs/27508) —
  R&D-card desk, Watts logs, 12451, and return to the maintenance conduit.
- [GameBanshee Medical](https://www.gamebanshee.com/systemshock2/walkthrough/medscimedical.php)
  and [Science](https://www.gamebanshee.com/systemshock2/walkthrough/medsciscience.php) —
  split-area confirmation of the Crew-office card and Watts encounter.
- Local `cargo dq entities medsci2.mis ...` and
  `cargo dq entities medsci1.mis ...` inspection (2026-07-23) — containment,
  positions, key regions, card-reader SwitchLinks, transitions, Watts, and logs.
- `Data/res/strings/LEVEL02.STR` (`LogText14`),
  `tools/shock2-sdk/test/creature-loot.e2e.test.ts`,
  `tools/shock2-sdk/test/log-reader.e2e.test.ts`,
  `shock2vr/src/scripts/internal_keycard_script.rs`, and
  `shock2vr/src/scripts/frob_qb.rs` — current code conveyance, persistence, and
  interaction watch-points.
