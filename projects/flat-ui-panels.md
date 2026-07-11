# Flat-Mode MFD Panels, Wave 2 — Design Note

> Status: 📋 Investigation complete; implementation not started. Written 2026-07-11.
> Sequel to `projects/flat-ui.md` (PRs 1–5 of that plan are on main: `FlatUiHost`
> left-MFD panel on frob, pointer→`GUIHover`, `/v1/ui` introspection, Tab metagame
> mode + inventory strip/drag, BIOFULL/AMMOFULL). This note covers the **next five
> panel families**: (1) audio-log/email playback, (2) elevator, (3) trainer
> stations, (4) O/S upgrade machines, (5) the map/automap — each investigated on
> four surfaces (original behavior, assets/strings, engine state, concrete test
> entity) and sliced into a conflict-aware build plan (§7).
>
> **Headline findings** (verified hands-on, 2026-07-11):
> - **The elevator already works end-to-end** on main: in a live medsci1 debug
>   runtime, frobbing the Master Elevator Button opened `ElevatorGui` in the left
>   MFD via the generic `FlatUiHost` path, and a pointer click on "1: Engineering"
>   transitioned the game to `eng1.mis`. What remains is fidelity + labels + e2e
>   (§2.3, PR A).
> - **The canonical trainer cost tables are IN `shock2.gam`** as parseable chunks
>   (`STATCOST`/`WTECHCOST`/`WSKILLCOST`/`PSICOST`), decoded in §3.2 and matching
>   the community-documented values exactly — no build agent needs to guess them.
> - **Cyber modules do not exist in the engine yet** (`Effect::AwardXP` is a
>   `warn!` TODO); the currency is the one true prerequisite for panels 3–4.

Dark Engine citations below are from the leaked-source mirror
`github.com/dima424658/darkengine`, `src/shock/` (same mirror `flat-ui.md` §2
uses). Gameplay facts are from the SS2 manual, shodan.fandom.com, GameBanshee,
systemshock.org and GameFAQs (solipsa); anything unverifiable is marked
**[unverified]**.

---

## 0. Shared original-engine context (new findings beyond flat-ui.md §2)

- **Overlay ids** (`shkovcst.h:15-64`): `kOverlayBuyTraits 20` (O/S), `kOverlayPDA
  24`, `kOverlayEmail 25` (the log/email *reader* — one overlay, four modes),
  `kOverlayMap 26`, `kOverlayBuyStats/BuyTech/BuyWeapon/BuyPsi 35-38`,
  `kOverlayMiniMap 45`, `kOverlayElevator 46`.
- **All five families dock in the LEFT slot** (`exclude_list_left`,
  `shkovrly.cpp:313-323`) — except the map, which appears in *both* lists because
  it **spans the full width** ({2,2}–{638,302}, both MFD slots). No right-slot
  (character-sheet) panel is needed this wave, so `FlatUiHost`'s single-slot
  model survives; only the map needs a "wide panel" mode (§5.5).
- **Panels are opened by the frobbed object's script** via the game service
  `OverlayChangeObj(which, mode, obj)` (`shkscrpt.cpp:596-624`) — the exact shape
  of our `Effect::OpenPanel`. The map is the exception: it opens from the BIOFULL
  **MAP button / `M` key**, with no bound world object.
- **Script messages**: `shkscrm.h` defines only `KeypadDone` and `YorNDone`.
  Elevator and O/S use plain string messages (`"EndLevel"`, `"Used"`); trainers
  round-trip through **no script at all** — the buy callback writes player
  properties directly (§3.1).

---

## 1. Panel: Audio Log / Email Playback

### 1.1 Original behavior

Manual (pp.9–10) + `shkemail.cpp` (501 lines) / `shkpda.cpp` (825 lines):

- **Log pickup does NOT auto-play**: "Picking up a log downloads it into your
  PDA"; a `LogPickup` message shows, the object is destroyed, and **`U`** plays
  the last unread log (`play_unread_log` bind, `shkpda.cpp:711-761`). BACKSPACE
  stops playback. **Emails auto-play on receipt** and trigger the PDA.
- **Storage model**: nine per-deck player properties `Logs1..Logs9`, each four
  32-bit bitmasks (`Emails, Logs, Notes, Videos` — `shkplayr.cpp:1426-1432`),
  plus a campaign-saved `LOGTIMES` file-var of pickup timestamps for sort order.
  Frobbing a log calls the `UseLog` service (`shkscrpt.cpp:437-497`): find the
  deck bit on the object's own `Logs<N>` property, OR it into the player's, then
  destroy the object.
- **The reader MFD** (`kOverlayEmail`, four modes `Email/Log/ResRep/Help`,
  backgrounds of the same names): portrait at (15,13), deck icon at (83,13),
  header + word-wrapped transcript in text rect (15,105,136×175), scroll
  buttons column at x=159 (lineup y154, pgup y174, pgdn y203, linedown y232),
  Return at y252 (closes reader, opens PDA). `TriggerLog(usetype, level, which,
  show_mfd)` can play **audio-only** (`show_mfd=FALSE`) or open the panel.
- **Data lookup** (`shkemail.cpp:277-301`): text/name/portrait/icon all come
  from the per-deck string table `level%02d.str` — keys `LogName<n>`,
  `LogText<n>`, `LogPortrait<n>`, `LogIcon<n>` (and `Email*` equivalents).
  Portrait/icon values name PCX files in the **`book\`** art directory. Audio is
  a schema named `sprintf("%s%02d%02d", "Em"|"Log", deck, n)` → e.g. `Log0220`,
  played via `SchemaPlay` with a completion callback.
- **PDA browser** (`kOverlayPDA`, `pda.pcx`): deck-switch arrows, 9-row list,
  tabs EMAIL/LOGS/NOTES/VIDEO at y=268; unread entries drawn in the dimmed
  font; notes tab driven by quest vars `Note_<deck>_<n>` (1 = open, 2 = done).
- **The 45100 log** (verified): from **Taz Amanpour**, "re: New code" — text
  "…I'll set the new code to 45100…" — found **on a corpse near the first
  keypad**; the code opens the **Cryo Recovery A exit** keypad (the #435 door,
  medsci1 keypad 1681 → door 1739). Sources: systemshock.org/shocklogs,
  GameBanshee MedSci walkthrough, LEVEL02.STR itself (§1.2).

### 1.2 Assets & strings (verified in `/Users/bryphe/ss2-data-unpacked`)

| Asset | Details |
| --- | --- |
| `res/iface/LOG.PCX`, `EMAIL.PCX`, `MEDIA.PCX`, `PDA.PCX` | 188×296 panel backdrops |
| `res/iface/PLOGS0/1`, `PEMAIL0/1`, `PNOTES0/1`, `PVIDEO0/1`, `PMEDIA0/1` | 40×18 tab pairs |
| `res/iface/PGUP0/1`, `PGDN0/1` (18×26), `RETURN0/1` (18×34) | scroll/return buttons |
| `res/book/AMANPOUR.PCX`, `POLITO.PCX`, `WATTS.PCX`, `GRASSI.PCX`, … | 58×84 portraits; deck icons (`MEDICON` etc.) live here too |
| `res/strings/LEVEL01..09.STR` | `LogName20:"AMANPOUR …re: New code"`, `LogText20:"…45100…"`, `LogPortrait20:"Amanpour"`, `LogIcon20:"MedIcon"` — parseable by the existing strings importer |
| `res/snd2/vLogs/english/LOG0220.WAV`, `res/snd2/vEmails/english/EM02*.WAV` | log/email audio; `snd2.crf` is already mounted |

**Gap: `res/book.crf` is NOT mounted** (`shock2vr/src/lib.rs:612-623` mounts 12
archives; book is absent but present in the data root). One-line fix, needed for
portraits/deck icons.

### 1.3 Engine state

- **`LogDiscScript`** (`shock2vr/src/scripts/logdiscscript.rs`, registered as
  `logdiscscript`): on Frob reads `PropLog {deck, email, log, ...}`, plays
  `Effect::PlaySound` named `LOG<deck:02><log:02>` (resolves via
  `resolve_schema`, passes through to `LOG0220.wav`), fires SwitchLinks,
  **destroys the entity**. No text shown, nothing stored, no replay possible.
- **`TrapEmail`** (`trap_email.rs`, `trapemail`/`emailroom`): on TurnOn emits
  `Effect::PlayEmail {deck, email}` → `mission_core.rs:2660` dedupes via
  `QuestInfo.played_emails` and plays `EM<deck:02><n:02>.wav` on the "email"
  audio channel. Auto-play parity with the original — audio only, no reader UI.
- **`PropLog` quirk** (verify before relying on it): log discs carry
  `{deck:2, email:33, log:N}` and email traps `{deck:2, email:N, log:33}` —
  `33` is evidently a "not set" sentinel, yet both scripts *test* `email > 0` as
  the validity check. A build agent must key off the field its panel actually
  uses (log vs email) and treat 33-in-the-other-field as unset, not real data.
- **No PDA storage**: destroyed logs are gone; `QuestInfo` has `played_emails`
  only. No `U`-key action, no notes, no reader.
- **Observability gap**: `/v1/audio/recent` (`shock2vr/src/audio_log.rs`) only
  records **environmental-schema** plays (`play_environmental_sound`,
  `mission_core.rs:4235-4261`) — `PlaySound`/`PlayEmail` never appear there.
  Extend recording to those paths so the e2e can assert the audio actually
  resolved (there is no other headless audio check).
- Text rendering, word-wrap, fonts (`mainfont.fon`), portrait image components:
  all already supported by `GuiComponent::Text/Image`.

### 1.4 Data reality (medsci1)

| Mission id | What | Key data |
| --- | --- | --- |
| **1608** | **Amanpour "New code" audio log** (template −76) | `PropLog {deck:2, log:20}`, pos (−29.38, −1.11, −10.95); text = LEVEL02.STR `LogText20` (contains "45100"); audio `LOG0220.WAV` |
| 1338 | Audio log (deck 2, log 1) | `PropHasRefs(true)` — world-placed |
| 1131 / 513 / 638 | EmailTraps | `PropLog {deck:2, email:1 / 3 / 15}` → `EM0201/03/15.WAV` |
| 12 more logs | deck 2, logs 2,4,5,7,11,14,18,21,23,32… | enumerate via `--filter "S$logdiscscript"` |

### 1.5 Verdict & gaps

🟡 **Yellow** — playback machinery + strings + audio all work today; the whole
panel is *presentation + storage*: a reader `MediaGui`, log collection in
`QuestInfo`, book.crf mount, audio observability. No engine unknowns. The full
PDA browser (deck tabs, unread sort) is **out of scope this wave** — reader-only.

---

## 2. Panel: Elevator

### 2.1 Original behavior

`shkelev.cpp` (403 lines), `kOverlayElevator 46`:

- **Layout**: background `elev`; five 142×54 floor buttons at x=13,
  y = 11/67/123/179/235 (**top button = deck 5**, index-inverted); button art
  `elev<floor><state>` off/on pairs; floor labels `ElevLevel<n>` from
  `MISC.STR` drawn at TEXT_X 60 inside the button; the **current floor's "on"
  bitmap is force-drawn** and inert; close button (163,8,20×21).
- **Floor list is data, not code**: gamesys file-var `Elev` (`sElevParams`,
  `char m_levels[5][64]`, `shkparam.cpp:466-490`) holds the five mission
  basenames. **Decoded from our `shock2.gam`: `Eng1, medsci1, Hydro2, ops2,
  Rec1`** — exactly the list `ElevatorGui` hardcodes today. Current floor =
  current mission basename compared to that list.
- **Gating**: quest var **`ElevState`** — 0: no power, panel draws `power.pcx`
  only; 1: partially blocked ("worm goo": only buttons ≥ deck 2 work; others
  show `ElevBlocked` from MISC.STR); 2: fully accessible. Game-flow: the main
  elevator is locked until Engineering restores power; deck 6 is a *separate*
  Rec↔Command elevator (never in this 5-floor panel).
- **On click** (`ElevButton`, lines 90-141): send `"EndLevel"` to player
  objects, then `TransModeSwitchLevel(m_levels[4-button], ELEVATOR_MARKER=22,
  …)` — **destination marker 22 is hard-coded in the engine**, which validates
  `ElevatorGui`'s `loc: Some(22)`.
- All loose objects in the car ride along between decks **[not modeled here;
  out of scope]**.

### 2.2 Assets & strings

`ELEVATOR.PCX` + `ELEV.PCX` (188×296 — `elev.pcx` is what the code loads),
`ELEV10/11 … ELEV50/51` (142×54 off/on floor buttons), `ELBUTT0/1` (138×28),
`CloseOff/On`. Strings: `MISC.STR` `ElevLevel1..5` ("Engineering (1)" …
"Recreational (5)") and `ElevBlocked` ("Error!  Shaft inaccessible!").
**`power.pcx` for the no-power state — present? [unverified; check
`res/iface/POWER.PCX` when implementing].**

### 2.3 Engine state — **verified working end-to-end**

Live test (debug runtime, medsci1, port 8160, 2026-07-11):

1. `POST /v1/player/teleport {x:2.5,y:0.5,z:-40.4}` (within the 4.0-unit
   walk-away auto-close radius — a frob from farther away opens the panel and
   the distance check closes it the same frame).
2. `POST /v1/entities/:id/message {"type":"Frob"}` on the Master Elevator
   Button → `/v1/ui` shows the panel: `elev.pcx` at (2,124,188×296), five
   `elev<f>0.pcx` buttons + text labels.
3. Pointer channels click the `elev10.pcx` button → **mission transitions to
   `eng1.mis`** (`/v1/info` mission confirms). `eng1.mis` has `PropStartLoc(22)`
   markers (objs 464, 1864), so the destination convention is honored.

`ElevatorGui` (`shock2vr/src/scripts/gui/elevator.rs`, 96 lines, script
`elevatorbutton`) gaps vs the original:

- `/v1/ui` buttons have **`label: null`** (the host's `semantic_label` only
  knows keypad art) — automation must click by texture name today.
- Stops hardcoded (happens to match the `Elev` file-var); label list says
  "Hydropondics" (typo); order top→bottom is 5→1 like the original ✓.
- No current-floor highlight/inert state; no `ElevState` gating; hover art
  (`elev<f>1.pcx`) wired but commented out; labels drawn as engine text rather
  than `ElevLevel<n>` strings.
- `BaseElevator` (within-level moving platforms) is separate and already
  works; `station.mis` `oldstylebaseelevator` is unimplemented (not this wave).

### 2.4 Data reality (medsci1)

| Mission id | What |
| --- | --- |
| **1041** | **Master Elevator Button** (template −1723, scripts `ElevatorButton` + `TweqDepressable`, model `ele`) — pos (0.39, 0.0, −40.4); **no links** (floor data is global, §2.1) |
| 1054 / 1055 / 1240 (+1053) | "Double Elevator Door" entities around the shaft |
| eng1: 464, 1864 | `PropStartLoc(22)` arrival markers |

### 2.5 Verdict & gaps

🟢 **Green** — functionally done on main; PR A is fidelity + labels + a durable
e2e. Only open question is `ElevState` sourcing (which quest bit medsci1/eng1
actually set — inspect `/v1/quests` during a playthrough; ship ungated, as
today, if unclear, since gating *adds* a lock we currently don't need).

---

## 3. Panel: Trainer / Upgrade Stations

### 3.1 Original behavior

`shktrain.cpp` (631 lines; stats/tech/weapons share one implementation) +
`shktrpsi.cpp` (psi), overlays 35–38. **Each machine opens one category overlay
directly** — there is no in-panel category chooser; the "category" is which
machine you frobbed.

- **Layout** (`shktrain.cpp:75-99`): background `train` (shared by
  stats/tech/weapons; psi uses `psitrain`); five 90×32 buy rows at x=13,
  y = 21/55/89/123/157; per-row columns value/next/cost at x-offsets 51/76/94;
  UNDO button (158,146,18×44, art `undo0/1`); description text (13,214)–
  (172,288); module-pool counter at (68,195); headers
  `TrainHeading`/`TrainCost`/`TrainPoints` and row names
  `StatName<i>`/`TechSkill<i>`/`WpnSkill<i>` from MISC.STR; hover help from
  `stathelp.str`/`skilhelp.str`. Psi adds a tier-selector strip (12,10)–(154,28).
- **Costs are gamesys file-vars**, not code: `sStatCost[5][5]`,
  `sTechSkillCost[5][6]`, `sWeaponSkillCost[4][6]`, `sPsiCost[40]`
  (`shkparam.h:24-63`), scaled by `sDiffParams.m_traincost[difficulty]`. See
  §3.2 — we decoded them from `shock2.gam` directly.
- **Currency**: cyber modules are the **stack count of a hidden inventory
  object** in equip slot `kEquipFakeCookies` (`shkplayr.cpp:1132-1161`) — which
  is why SS2's module-pickup script is literally named `expcookie`. Not a quest
  var.
- **Buying** (`buy_cb`, `shktrain.cpp:445-516`): no script message — the
  callback directly increments the player property
  (`BaseStatsDesc`/`BaseTechDesc`/`BaseWeaponDesc`), `AddPool(-cost)`,
  `RecalcData`. Errors: `ErrorMaxed`, `ErrorExpensive` (overlay text). **UNDO
  restores a snapshot of property+pool taken when the panel opened.** Cap is 6,
  hard-coded. The 4th weapon row (Exotic) is hidden until quest var
  **`AlienWeapons`** is set. Psi purchase requires the prior tier
  (`ErrorNoTier`), then `AddPool(-cost)` + `AddPsiPower`.
- The panel **auto-closes when you walk away** (standard overlay `distance`) —
  our `FlatUiHost` already does this.

### 3.2 The cost tables — decoded from `shock2.gam` (primary source)

Chunks in our shipped gamesys (offset/length from the chunk TOC; payload starts
24 bytes after each chunk header). Decoded 2026-07-11; **identical to the
community tables** (shodan.fandom.com "Cybernetic Upgrade Units", GameFAQs
solipsa guide), so both sources corroborate:

| Chunk | Shape | Values (cost to buy the *next* level, Normal difficulty) |
| --- | --- | --- |
| `STATCOST` | `[5 stats][5]` | every stat: 3, 8, 15, 30, 50 (levels 2→6; all start at 1) |
| `WTECHCOST` | `[5 skills][6]` | every tech skill: **10**, 5, 8, 12, 25, 50 (level 1 costs more than 2 — real) |
| `WSKILLCOST` | `[4 skills][6]` | every weapon skill: **12**, 6, 8, 15, 36, 50 |
| `PSICOST` | `[5 tiers][8]` | per tier: first int = tier unlock (10/20/30/50/75), next 7 = per-power cost in that tier (3/5/8/12/20) |
| `DIFFPARAM` | mixed f32/i32 | contains the per-difficulty train-cost multipliers — floats 0.85 / 1.0 / 1.39 / 1.79 are present, matching Easy/Normal/Hard/Impossible ×0.85/×1.0/×1.39/×1.79 (round down). Exact field layout **[unverified]** — recommend Normal-only first. |
| `Elev` | `char[5][64]` | `Eng1, medsci1, Hydro2, ops2, Rec1` (§2.1) |

Recommended: add a small gamesys param reader in `dark/src/gamesys/` for these
five chunks (the chunk TOC reader already exists —
`ss2_chunk_file_reader`); hardcoding the table with this citation is an
acceptable fallback but parsing is ~50 lines and also feeds the elevator.

### 3.3 Engine state

- **`SkillTrainerScript`** (`scripts/skill_trainer.rs`): intentional no-op stub
  (#424) for `statstrainer`/`techtrainer`/`psitrainer`/`weapontrainer` — logs
  the frob, nothing else. Replace registrations with `gui_script(TrainerGui)`.
- **`PlayerStats`** (#459, `shock2vr/src/player_stats.rs`): persistent
  stats/skills/psi-disciplines in `QuestInfo`, serialized through save/load and
  transitions, surfaced at `/v1/info` (verified live: full stats block).
  Trainers mutate exactly this struct.
- **No currency**: `Effect::AwardXP` → `warn!("!! TODO !!: Award XP")`
  (`mission_core.rs:2336`); `TrapEXPOnce` already fires it from `PropExp` (this
  is the "Polito awards 4 cyber modules" trap the playtest saw — the award is
  currently dropped); the `expcookie` pickup script is `UnimplementedScript`.
  **PR C1 (§7) closes this loop before any trainer UI.**
- Psi: `Effect::GrantPsiPower {template_id}` already exists for trainers/debug;
  `PlayerPsiKnownPowers` is still rebuilt per level load (#424 note) — psi
  trainer purchases need the known-powers set persisted or re-derivable from
  `PlayerStats` (flag for PR C2; storage in `PlayerStats.psi_disciplines`
  exists).
- Difficulty: no difficulty setting exists in shock2quest yet → Normal costs.

### 3.4 Data reality (medsci1)

All four trainer types exist in medsci1 (the column outside Cryo Recovery A —
same room where the EXP trap awards the first modules):

| Mission id | Machine | Script |
| --- | --- | --- |
| **1352** | Stats Trainer (template −581, model `statbild`) | `StatsTrainer` |
| 1317 | Tech Trainer (−1436) | `TechTrainer` |
| 1354 | Weapon Trainer (−1437) | `WeaponTrainer` |
| 1355 | Psi Trainer (−1583) | `PsiTrainer` |

### 3.5 Verdict & gaps

🟡 **Yellow** — data model (#459) and cost data are ready; blocked only on the
cyber-module economy (small, well-defined). UNDO snapshot and psi-tier rules are
the only intricate bits. UI is a straightforward `Gui` impl on known art.

---

## 4. Panel: O/S Upgrade Machines

### 4.1 Original behavior

`shktrait.cpp` (413 lines), `kOverlayBuyTraits 20` ("traits" = O/S upgrades):

- **One pick from all 16 traits** (not a 4-option subset): selection matrix
  rect (15,76)–(152,209), 4 icons per row (`which = col + row*4 + 1`), icons
  `trait%02d` loaded by enum value; already-owned row (up to 4 slots — the
  whole game has exactly 4 machines) at (15,35), 35×34 cells, empty-slot art
  `eTrait1`; description (15,214)–(174,264) = `Trait<n>` from `TRAITS.STR`;
  header `TraitHeader` from MISC.STR; background `Traits`. **No confirmation
  step** (GameBanshee warns "click only when you're absolutely sure").
- **Purchases are FREE** (no module cost). Side effects in code:
  `kTraitAble` → +8 cyber modules, `kTraitSpeedy` → speed scale ×1.15,
  `kTraitCybernetic` → inventory refresh; everything else → `RecalcData`
  (consumed by combat/inventory formulas elsewhere).
- **Storage**: player property **`TraitsDesc`** — `m_traits[4]` slots
  (`shkplayr.cpp:1462-1463`), *not* quest vars. Trait ids 1–16 in `shktrcst.h`
  enum order = `TRAITS.STR` `Trait1..16` order.
- **One-time-use**: on buy the machine object gets a plain `"Used"` script
  message (its script then refuses to reopen), plus a `gTraitBought` latch
  while the panel is up.
- 4 machines in the game: MedSci (crew-lounge area), Hydroponics, Recreation
  mall, Rickenbacker bridge.

The 16 traits (`TRAITS.STR`, verified; effects cross-checked fandom/GameBanshee):
Strong Metabolism, Pharmo-Friendly, Pack-Rat, Speedy, Sharpshooter (tooltip
says 15%, actual +35% — the string is the *bugged label*), Naturally Able (+8
modules), Cybernetically Enhanced (2 implants), Tank (+5 max HP), Lethal Weapon
(+35% melee), Security Expert, Smasher, Cyber-Assimilation, Replicator Expert,
Power Psi, Tinker, Spatially Aware (automap always filled).

### 4.2 Assets & strings

`TRAITS.PCX` (188×296 backdrop), `TRAIT00..16.PCX` (34×32; `TRAIT00` 36×36),
`TRAIT.PCX`; `res/strings/TRAITS.STR` `Trait0..16` (Trait0 is the "find a
machine" hint). All in already-mounted `iface.crf`/`strings.crf`.

### 4.3 Engine state

- `traitmachine` script registered as **`NoopScript`** — no behavior at all.
- No trait storage anywhere; `PlayerStats` is the natural home
  (`os_traits: Vec<u8>` / BTreeSet, ≤4, serialized for free).
- One-time-use: the used state must survive save/load → a serialized runtime
  prop on the machine entity (pattern: existing runtime props +
  `save_load`), or a quest bit keyed by mission object id. The original's
  "machine script remembers `Used`" maps to the former.
- **Implementable-now effects** (each has an existing consumer):
  Tank (max-HP path exists), Naturally Able (needs PR C1 currency), Speedy
  (flat/VR locomotion speed scale), Lethal Weapon / Sharpshooter (melee /
  ranged damage paths), Spatially Aware (map reveal — after PR B2).
  **Storage-only for now**: Pack-Rat (inventory grid is fixed 15×3),
  Cybernetically Enhanced (implant slots not modeled), Security Expert,
  Smasher, Cyber-Assimilation, Replicator Expert (nanites don't exist), Power
  Psi, Pharmo-Friendly, Strong Metabolism, Tinker. The PR must list which are
  live vs stored.

### 4.4 Data reality

**No O/S machine in medsci1.** Nearest test entity: **medsci2.mis mission id
133** ("Trait Machine", template −2307, model `traitma`, script `TraitMachine`
inherited). medsci2 loads fine in the e2e suite, so testing there is cheap.

### 4.5 Verdict & gaps

🟡 **Yellow** — simple panel (a 4×4 icon grid + description + one effect
dispatch), clean storage story, good strings/art. Depends on PR C1 only for the
Naturally Able perk (can land before C1 with that perk stored-not-granted, but
sequencing after C1 is cleaner). Main design decision: where "used" lives.

---

## 5. Panel: Map / Automap

### 5.1 Original behavior

`shkmap.cpp` (1049 lines), `kOverlayMap 26` + `kOverlayMiniMap 45`:

- **Opened from the BIOFULL MAP button or `M`** — no bound world object, no
  walk-away close. Full map **spans both MFD slots**: {2,2}–{638,302},
  re-centered for wider screens; frame art `mapback` (636×296), fallback
  `nomap`; close at (606,6).
- **Per-level pages** from `intrface/<level>/`: `page001` (normal) and
  `page001a` (the "automapped"/Spatially-Aware variant).
- **Visited tracking**: mission file-var `EXPLORED[64]` — **one byte per map
  location**, not per room. The room the player stands in is mapped to a
  location via the room object's **`MapLoc`** property; entering marks it
  explored (`ShockMapSetExplored`). Reveal granularity = map location (a
  hand-authored region covering several rooms).
- **Region decals**: LTRB rects from `<level>/p001xa` (BIN, same importer
  format we already parse); per-location art `p001x%03d` (explored, dim) and
  `p001r%03d` (current location, bright) blitted at each rect.
- **Markers**: any object with `MapObjIcon` (PCX name) is drawn; `MapObjRotate`
  rotates it with heading — the **player marker is just the player's
  MapObjIcon** (circle-with-arrow). World→map transform comes from two
  `MapRef` scale markers (`m_frame == -1`) + per-location reference markers
  (`m_frame == maploc`); `m_rotatehack` handles 90°-rotated level maps.
  Markers in unexplored locations are hidden.
- **Mouseover text**: `MapText_<level>_<loc>` from `MAPTEXT.STR`, drawn at
  (166,268). **Nav markers/annotations**: click a marker → brackets → type to
  edit its `MapText` property; `N` drops a Nav Marker; DEL removes.
- **Minimap**: same renderer into a 128×128 player-centered canvas at top
  right, translucent, shoot-mode only; toggle button on the map panel
  (`minimap0/1`, lamp `minion/minioff`).
- **Spatially Aware** trait → `page001a` + everything revealed.

### 5.2 Assets & strings

`MAPBACK.PCX` 636×296, `NOMAP.PCX` 593×281, `MINIMAP.PCX` 128×128,
`MINIMAP0/1` 142×22, `MINION/MINIOFF` 22×22, blips `map_nav/med/ene/rec/rep/trn`
16×16; per-mission `res/intrface/MEDSCI1/english/`: `PAGE001.PCX` 614×260,
`PAGE001A.PCX`, `P001R000..009.PCX` + `P001X000..009.PCX` (67×60-ish region
decals), `P001RA.BIN` (+`P001XA.BIN` rects). `MAPTEXT.STR`
`MapText_<Mission>_<loc>` labels. All already mounted.

### 5.3 Engine state

Surprisingly far along on data, zero on gameplay:

- **`MapRenderer`** (`shock2vr/src/map_renderer.rs`, 137 lines): renders
  `PAGE001.PCX` + revealed `P001R###` decals at `P001RA.BIN` rects as
  world-space quads (VR-shaped); `revealed_slots` is caller-provided. Used only
  by the **`debug_map`** scene, which reveals slots on a timer.
- **`MapChunkData`** (`dark/src/map/mod.rs`): loads both `P001RA.BIN` and
  `P001XA.BIN` rect lists.
- **All map properties parse already**: `PropMapLoc` (32 entities in medsci1
  carry it), `PropAutomap {page, location}`, `PropMapRef {x,y,frame}`,
  `PropMapText`, `PropMapObjIcon`, `PropMapObjRotate` (the Trainers template
  ships `PropMapObjIcon("in")`).
- **Room entities exist at runtime**: `create_room_entities`
  (`mission_core.rs:4046+`) instantiates the mission `RoomDatabase` as sensor
  entities with SwitchLinks — the begin/end-intersect machinery that room
  triggers use is the same signal needed for "player entered room → mark its
  `MapLoc` explored".
- **Missing**: visited-set storage (nothing in `QuestInfo`; original is
  mission-scoped `EXPLORED` — ours must be a per-mission map inside `QuestInfo`
  to survive save/load *and* deck re-entry), player marker + `MapRef`
  world→map transform, any flat presentation, an open-map input action, panel
  hosting without a bound entity (§5.5), minimap (defer).

### 5.4 Data reality (medsci1)

Rooms/markers with `PropMapLoc`: mission ids 378, 571, 618–624, … (32 total,
names like "3 FreeSwim", "7 To Med" — the digit prefix is the map location).
Map pages: `MEDSCI1/english/PAGE001.PCX` + 10 R/X decal pairs + `P001RA.BIN`.
`MAPTEXT.STR` has `MapText_Medsci1_*` entries. **[unverified]: whether medsci1's
map needs `m_rotatehack`** — compare marker positions against the page art
during PR B2.

### 5.5 Hosting design note (the one FlatUiHost change this wave)

Two deltas to `FlatUiHost`, both small:

1. **Wide panel**: the map's `GuiConfig.screen_size_in_pixels` = 636×296; the
   host currently anchors any panel at (2,124). A 636-wide panel fits the
   canvas at that anchor (2+636=638, 124+296=420) — rendering under the
   top-docked inventory strip like the original's lower-anchored variant. So
   this may be zero-change; verify letterboxed hit-testing at that width.
2. **Unbound panel**: recommended approach is the **`internal_inventory`
   precedent** — a synthetic player-owned entity
   (`inventory/player_inventory_entity.rs`) carrying a `MapGui` `GuiScript`, so
   `SetUI`/`GUIHover`/`/v1/ui` all work unchanged; a new
   `InputAction::ToggleMap` (M key + HTTP) opens it via `Effect::OpenPanel` on
   that entity, and the host skips the distance auto-close when the bound
   entity is the synthetic one (or the synthetic entity tracks the player).

### 5.6 Verdict & gaps

🔴 **Red** (most net-new work): needs visited tracking + persistence, room→
location wiring, world→map transform, unbound-panel hosting, and an input
action — but every ingredient (rects, decals, props, room sensors, strings) is
already parsed and shippable art exists. Minimap and annotations/nav markers
are explicitly deferred.

---

## 6. Cross-cutting: the cyber-module economy (prerequisite for §3/§4)

The original stores modules as a hidden inventory stack ("fake cookies") — for
us the pragmatic equivalent is a counter beside `PlayerStats`:

- `QuestInfo`/`PlayerStats` gains `cyber_modules: i32` (serialized for free,
  visible in `/v1/info` like the rest of #459).
- `Effect::AwardXP { amount }` (already fired by `TrapEXPOnce` from `PropExp` —
  medsci1's Polito award works day one) increments it instead of `warn!`.
- `expcookie` script: frob/pickup → `AwardXP(PropExp)` + destroy (mirrors
  `TrapEXPOnce`; module pickups litter every deck).
- Spend path: a new `Effect::SpendCyberModules { amount } -> bool`-style flow
  used by the trainer buy (atomic check-and-decrement inside the effect
  handler, since `Gui::handle_msg` is pure).
- BIOFULL already renders in use mode; adding the live module counter to it is
  optional polish in PR C1 (the manual places nanite + module counters there).

---

## 7. Build plan

Sizing reference: wave-1 PRs landed at roughly 300–800 LOC each including
tests. Every PR below follows the house rules: negative-first e2e
(`SHOCK2_E2E=1`), `/v1/ui` semantic labels for anything clickable,
`RUSTFLAGS="-D warnings" cargo check -p shock2vr -p desktop_runtime -p
debug_runtime`, `missions.e2e.test.ts` for load safety, pr-visuals GIF/PNG.

### 7.1 The PRs

**PR A — Elevator fidelity + labels + e2e** (~200–350 LOC)
Files: `scripts/gui/elevator.rs`; `gui/gui_component.rs` + `gui/mod.rs` +
`mission/flat_ui_host.rs` (add an explicit `label: Option<String>` to
`GuiComponent::Button`/render-info — the generic labeling mechanism every later
panel uses; keypad's art-derived fallback stays); e2e `elevator.e2e.test.ts`.
Content: floor labels from `MISC.STR` `ElevLevel<n>`; current-floor lit+inert;
hover art re-enabled; typo fix; optional `Elev` file-var read if PR C2's
gamesys param reader is extracted first (else keep the verified hardcode with a
citation comment).
**e2e**: launch medsci1 → teleport within 4u of the button (discover by name
"Master Elevator Button") → Frob → `/v1/ui` panel with `label:"Engineering (1)"`
etc. → pointer-click that rect → `/v1/info` mission == `eng1.mis` and player
spawns near a `StartLoc(22)` marker. Negative first: on main, labels are null.

**PR B1 — Log/email reader MFD + collection** (~500–700 LOC)
Files: `lib.rs` (mount `res/book.crf`); new `scripts/gui/media.rs` (`MediaGui`:
`LOG.PCX`/`EMAIL.PCX` backdrop, portrait `book/<name>.pcx`, deck icon, name +
word-wrapped `LogText<n>`, PGUP/PGDN scroll, close); `logdiscscript.rs`
(frob → collect into `QuestInfo` + `OpenPanel` reader + play audio; keep
SwitchLinks; stop destroying-before-storing); `trap_email.rs` (auto-open reader
in flat mode alongside `PlayEmail`); `quest_info.rs` (collected logs per deck);
`mission_core.rs`/`audio_log.rs` (record `PlaySound`/`PlayEmail` into
`/v1/audio/recent`); strings lookup via existing importer.
**e2e (`media.e2e.test.ts`)**: negative: today frobbing log 1608 produces no
`/v1/ui` panel and no transcript anywhere. Then: discover the Amanpour log (by
template/name + `PropLog log:20`), Frob → `/v1/ui` panel whose text element
**contains "45100"** → `/v1/audio/recent` contains `LOG0220` → log recorded in
QuestInfo; EmailTrap 1131 TurnOn → `EM0201` plays once, replay-deduped.
(If audio observability proves awkward, the PR may defer audio *assertion* —
not audio playback — and must say so.)

**PR B2 — Map panel** (~600–900 LOC, stacks on B1's QuestInfo edits)
Files: new `scripts/gui/map.rs` (`MapGui` composing `MapChunkData` +
`PAGE001`/decals/markers into `GuiComponent`s — port `map_renderer.rs` logic to
panel space); `mission/flat_ui_host.rs` (wide/unbound panel, §5.5);
`inventory/player_inventory_entity.rs` pattern for the synthetic map entity;
`input/actions.rs` + dispatcher + desktop mapper (`ToggleMap`, key M);
`quest_info.rs` (per-mission explored-locations set); `mission_core.rs` (player
room-intersect → `MapLoc` → mark explored); e2e.
**e2e (`map.e2e.test.ts`)**: negative: `ToggleMap` action unknown /
no panel. Then: launch medsci1 → trigger `ToggleMap` → `/v1/ui` shows the map
panel with `PAGE001` image + player marker element; walk the player through a
doorway (control input) → explored count in `/v1/ui` (or a `/v1/info` field)
increases and a `P001R###` decal element appears; explored set survives
save/load (`/v1/save` + `/v1/load`). Screenshot diff for the page art.
Deferred, stated in the PR: minimap, nav markers/annotations, `MapText`
mouseover, rotatehack levels (assert medsci1 unrotated first).

**PR C1 — Cyber-module economy** (~250–400 LOC)
Files: `player_stats.rs` or `quest_info.rs` (counter), `mission_core.rs`
(`AwardXP` handler), `scripts/mod.rs` + tiny `expcookie` script,
`game_scene.rs`/debug snapshot (`/v1/info` field), optional BIOFULL counter.
**e2e**: negative: `/v1/info` has no module count / stays 0 after the medsci1
EXP trap fires. Then: TurnOn the "Experience 1" trap (mission id 395 carries
`MapLoc`… discover the EXP trap by script `TrapEXPOnce`) → count increases by
`PropExp`; give-and-frob an expcookie item → count increases; persists across
`TransitionLevel` and save/load.

**PR C2 — Trainer MFDs** (~550–750 LOC, stacks on C1)
Files: new `dark/src/gamesys/params.rs` (read `STATCOST`/`WTECHCOST`/
`WSKILLCOST`/`PSICOST`/`Elev` chunks — §3.2 layouts); new
`scripts/gui/trainer.rs` (one `TrainerGui` with a mode enum, art per §3.1);
`scripts/mod.rs` (replace the four `SkillTrainerScript` registrations);
`scripts/effect.rs` + `mission_core.rs` (`TrainSkill`-style effect: check pool,
decrement, bump `PlayerStats`, cap 6); UNDO = snapshot on open, restore effect.
Psi mode: tier gate from `PlayerStats`, power grant via existing
`GrantPsiPower`; **flag**: known-psi-powers persistence across loads must be
re-derived from `PlayerStats` (#424 residue) — in-scope for the psi rows.
Exotic row hidden without `AlienWeapons` quest bit. Normal-difficulty costs
only.
**e2e**: negative: frobbing Stats Trainer 1352 logs the stub, no panel. Then:
set modules (via C1's trap or a debug give) → Frob 1352 → `/v1/ui` rows labeled
"Strength cost 3" → click → `/v1/info` strength 2, modules −3, **persists
across a level transition and save/load**; buy with 0 modules refused
(`ErrorExpensive` text element); UNDO refunds while open; cap-6 refusal.

**PR C3 — O/S upgrade MFD** (~350–500 LOC, stacks on C2)
Files: new `scripts/gui/traits.rs` (`TraitGui`: 4×4 matrix of `TRAIT##.PCX`,
description from `TRAITS.STR`, owned-slots row); `scripts/mod.rs`
(`traitmachine` → gui_script); `player_stats.rs` (`os_traits`, ≤4); a
serialized per-machine used flag (runtime prop through `save_load`); effect
application for the live subset (Tank, Naturally Able, Speedy — each cited in
the PR; everything else storage-only, listed).
**e2e**: negative: frobbing medsci2 Trait Machine 133 does nothing. Then:
Frob → `/v1/ui` 16 labeled trait buttons → pick "Tank" → `/v1/info` shows the
trait + max HP +5 → panel refuses a second pick; re-frob the machine →
used/refuses; both survive save/load.

### 7.2 Conflict-aware sequencing & agent split

Shared-file touch map:

| File | A | B1 | B2 | C1 | C2 | C3 |
| --- | --- | --- | --- | --- | --- | --- |
| `gui/gui_component.rs` (label field) | ✍ | – | – | – | – | – |
| `mission/flat_ui_host.rs` | ✍ (labels) | – | ✍ (wide/unbound) | – | – | – |
| `quest_info.rs` / `player_stats.rs` | – | ✍ | ✍ | ✍ | ✍ | ✍ |
| `scripts/mod.rs` (registrations) | – | ✍ | ✍ | ✍ | ✍ | ✍ |
| `scripts/effect.rs` + `mission_core.rs` effect arms | – | ✍ | ✍ | ✍ | ✍ | – |
| `lib.rs` (mounts) | – | ✍ | – | – | – | – |
| `dark/` (gamesys params) | – | – | – | – | ✍ | – |

Cross-track overlaps are **additive** (new enum variants, new match arms, new
struct fields, one-line registrations) — rebases are mechanical. The only
structural shared change is PR A's button-label field, so **A lands first**.

**Recommended: 3 parallel build agents**

- **Agent 1 (small, fast): PR A** — elevator. Lands first; unblocks the label
  mechanism for everyone. Afterwards this agent is free for review/rebase duty.
- **Agent 2: PR B1 → PR B2 stacked** — logs/email then map. Internally ordered
  (both touch QuestInfo + panel-content patterns; B2 additionally touches
  FlatUiHost, so it rebases on A + B1).
- **Agent 3: PR C1 → PR C2 → PR C3 stacked** — economy, trainers, O/S. Depends
  on the merged stats model only; C-track never touches FlatUiHost.

Merge order: **A → C1 → (B1 ∥ C2) → (B2 ∥ C3)**. B and C tracks are fully
parallel after A; the only cross-track rebase points are `quest_info.rs` /
`scripts/mod.rs` / `mission_core.rs` one-liners.

### 7.3 Research gaps a build agent must NOT guess at

1. **`DIFFPARAM` field layout** (difficulty cost multipliers): multipliers
   0.85/1.39/1.79 are confirmed present, but the struct layout is unverified —
   ship Normal-only costs (table §3.2 is exact for Normal).
2. **`ElevState` progression**: which script/trap sets 0→1→2 in real data —
   inspect quest bits in a playthrough before adding the gate; shipping ungated
   (today's behavior) is acceptable and strictly more playable.
3. **`PropLog` sentinel 33** (§1.3): verify against several entities before
   generalizing; never treat the *other* field as a real id.
4. **`POWER.PCX`** existence for the elevator no-power art (§2.2).
5. **Map `rotatehack` / `MapRef` transform**: verify the medsci1 marker
   transform against `PAGE001.PCX` visually (pr-visuals screenshot) before
   trusting it on other decks; `command1`/rotated decks are out of scope.
6. **Original log-pickup auto-open**: the original does *not* auto-open the
   reader on pickup (it shows `LogPickup` + `U` plays audio). PR B1's
   frob→open-reader choice is a deliberate, documented deviation (honest
   playtests must be able to *read* codes in-fiction); if the owner prefers
   fidelity, implement `U` (`PlayUnreadLog` action) + message-line instead —
   decide in the PR, don't silently pick.
7. **Sharpshooter 15% vs 35%**: the string says 15, the engine does 35 — if
   implemented live, use 35% and keep the original (bugged) label; cite
   shodan.fandom.com/wiki/O/S_Upgrades.
8. **Whether codes auto-enter PDA Notes** in the original: unverified; Notes
   are out of scope this wave.

---

## 8. Out of scope this wave (explicitly)

Full PDA browser (deck tabs, unread sort, notes/video tabs); minimap + compass;
nav markers/map annotations; research/hack/modify/repair panels; replicator
polish (nanites don't exist yet); right-slot character-sheet MFDs
(STATS/TECH/CMBT/PSI tabs — natural wave 3, needs the two-slot exclusion
mechanic from flat-ui.md §2.2); elevator car object-carry-over; difficulty
scaling; `oldstylebaseelevator` (station).
