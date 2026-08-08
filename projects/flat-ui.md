# Flat-Mode Metagame UI — Design Note

> Status: 📋 Investigation complete; implementation not started. Written 2026-07-10.
> Goal: a faithful emulation of the original System Shock 2 **flat-screen metagame
> interface** — Tab-toggled cursor mode, inventory grid, expanded BIOFULL/AMMOFULL
> readouts, and MFD side panels (keypad, container loot, logs, map, …) — so flat
> mode can play the game the way the 1999 original did.
>
> **Driving issue:** [#435] — flat mode cannot interact with world-space GUIs; a
> playtest is hard-blocked at the medsci1 cryo-exit keypad (mission id **1681**,
> code **45100**, SwitchLink → door **1739**). The repo owner's decision: the
> faithful fix is to emulate the original's flat UI (frob keypad → keypad **MFD
> overlay** + mouse cursor), *not* to bolt crosshair-hover onto VR's world-space
> panels (the direction sketched in the issue is superseded by this doc).
> The keypad MFD is therefore the earliest user-visible increment (§6, PR 2).

[#435]: https://github.com/tommy-xr/shock2quest/issues/435

---

## 1. How the Original Flat UI Behaves (behavior spec)

Sources: the official EA/LGS manual ([PDF](https://retrogamer.biz/wp-content/uploads/2016/06/System-Shock-2-Manual.pdf), pp.6-15), the shodan.fandom wiki ([Cyber Interface](https://shodan.fandom.com/wiki/Cyber_Interface), [Commands](https://shodan.fandom.com/wiki/Commands_(System_Shock_2)), [Keypad](https://shodan.fandom.com/wiki/Keypad), [Replicator](https://shodan.fandom.com/wiki/Replicator), [Version History](https://shodan.fandom.com/wiki/Version_History_(System_Shock_2))), systemshock.org ([HUD-size topic 4754](https://www.systemshock.org/index.php?topic=4754.0)), [PCGamingWiki](https://www.pcgamingwiki.com/wiki/System_Shock_2), and pixel inspection of the wiki's 1024×600 shoot/use-mode screenshots. Facts that could not be sourced are marked **[unverified]**.

### 1.1 Two modes: Shoot vs Use

The manual (p.6) defines exactly two interaction modes:

- **Shoot mode** — minimal HUD; mouse = look; **LMB fires/swings**; **RMB uses
  (frobs) the highlighted object** under the reticle.
- **Use mode** ("metagame") — more windows; mouse = **cursor**; **RMB uses the
  object under the cursor; LMB picks it up**.

**Entering:** `I` or `Tab` ("Inventory MFD toggle") brings up the MFD windows;
while they're up the view does not rotate (manual p.7). **Exiting** (p.7,
verbatim): "left-click in the world view, press TAB/I or click on the middle
button at the bottom of the screen" — i.e. an LMB on empty world in use mode
*closes the panels* rather than firing. A separate bindable `toggle_mouse`
command flips only mouselook⇄cursor without changing panels (the "hybrid"
state); `frob_toggle` is RMB's frob-or-enter-metagame (matches the engine
source, §2.1). (Sources differ on which command Tab binds by default —
`toggle_inv` per the manual's key list vs `toggle_mouse` per the shipped
`default.bnd` reading in §2.1; either way Tab toggles cursor mode, which is all
our `ToggleUseMode` action needs to replicate.)

**While in use mode the game does not pause.** Keyboard movement/lean/jump and
weapon keys (R reload, B ammo cycle, O setting, number keys, H/P hypos) stay
live; only mouse-turn is surrendered.

### 1.2 HUD baseline vs expanded

**Always-on (shoot mode):** bottom-left compact bio meter (HP bar + cross icon +
number, psi bar + crescent + number); bottom-right compact weapon/ammo box (ammo
type picture, rounds, condition circle; psi power when the amp is wielded; blank
for melee); top message line; optional minimap (top-right), compass, crosshair,
target brackets + enemy health bar. *(shock2quest's `flat_hud.rs` already ships
this baseline, §4.4.)*

**Use mode adds:**

- **Top strip — inventory + equip** (§1.5).
- **Bottom-left expands** into the full bio panel (`BIOFULL`): bars + numbers
  plus **research (test tube), "?" query, MAP buttons**, **nanite counter**, and
  **cyber-module counter** (manual p.8).
- **Bottom-right expands** into the full weapon/PDA panel (`AMMOFULL`): **PDA /
  keycards / MFD(stats) buttons**, large **SETTING** button (= O), **RELOAD**
  (= R), triangular **ammo-cycle** (= B) (manual p.9).
- **Bottom-center**: the small close chevron (the "middle button").
- Up to **two docked MFD side panels** below the inventory strip: device/PDA
  panels **upper-left**, character-stats panel **upper-right** (§1.3).

### 1.3 MFD panel taxonomy and docking

One panel per side at a time; both sides can be open at once (the reference
screenshot shows inventory + PDA-left + stats-right + both expanded bottom
panels simultaneously). Every panel has a corner close button and a sideways tab
label on its inner edge. Confirmed panels:

| Panel | Contents | Dock |
| --- | --- | --- |
| PDA | tabs **EMAIL / LOGS / NOTES / HELP**, sorted by deck; unread highlighted; notes auto-track | left |
| Log/e-mail viewer | portrait, deck icon, sender/subject, transcript, scroll arrows | left |
| Keypad | digit grid + readout (§1.4) | left |
| Container/corpse loot | "searching a container or a body opens a separate window and left clicking on the contents picks them up" (manual p.7) | left |
| Replicator | 4 items w/ name+price, balance at bottom; purchase drops the item into the physical hopper (not inventory); hacked ≈25% cheaper | left |
| Upgrade units (Stats/Tech/Weapons/Psi), O/S upgrade | dedicated MFDs (`open_mfd 35-38`, `20`), module costs | left |
| HACK / REPAIR / MODIFY | a **side tab on the device/weapon's own MFD** — not a separate screen (manual pp.13-14) | left |
| Weapon settings | RMB a weapon in inventory → its settings MFD (+ MODIFY/REPAIR tabs) | left |
| Character stats | sub-tabs **STATS / TECH / CMBT / PSI** | right |
| Keycards, Research, Map | card list; research progress + REPORTS; automap w/ annotations, nav markers, minimap toggle | left/right |

This left/right split matches the engine's `exclude_list_left/right` exactly
(§2.2).

### 1.4 Keypad behavior

- Codes are **5 digits**; layout is a 3×3 grid of 1-9 + bottom row `0` and wide
  `C` (clear), with a numeric readout above showing typed digits.
- **Frobbing a keypad in shoot mode pops its MFD and enters cursor mode**
  ("Individual MFD windows may also be called up by using selected items, such
  as information terminals and keypads", manual p.7) — you can enter a code you
  never "learned" (players type 45100 straight from guides).
- Correct code → door unlocks/opens ("Access granted, Keypad inactive");
  keypads are also hackable via the HACK side tab (critical failure breaks the
  keypad).
- **[unverified]** exact wrong-code feedback (buzz/readout flash) and whether
  the panel auto-closes on success — confirm against gameplay video during PR 2.
  (The engine source shows the check fires at exactly 5 digits, §2.3; the
  feedback lives in the gamesys script, which the leak lacks.)

### 1.5 Inventory, cursor, drag-drop

- Grid strip across the **top** of the screen with **equip slots (weapon /
  armor / implants) at its right end**; multi-slot items occupy footprints;
  stackables show a count in one square.
- **The cursor carries the item**: left-click lifts an item onto the cursor
  (the cursor *becomes* the object icon, §2.4), left-click places/swaps;
  auto-placement on world pickup.
- **RMB an inventory item = use it** (hypo consumes, weapon opens settings).
  Drag-onto flows: ammo→gun (reload w/ that type), maintenance tool→weapon,
  chemical→research.
- **ALT = split stack** (SPLIT cursor), **CTRL / "?" = query** an item; hover
  shows equipment condition.
- **LMB in the 3D view throws the cursor item into the world; RMB applies it
  to the crosshair target** when that target explicitly accepts the item as a
  tool. A refused/ignored item stays on the cursor. An item can stay on the
  cursor across UI states (a NewDark patch note covers level-transition with
  an object on the cursor).
- Loot windows: left-click contents to take (manual p.7).
- Audio logs: pickup downloads to the PDA; `U` plays the last unread; incoming
  **e-mail auto-plays**; BACKSPACE stops. **[unverified]** whether log *pickup*
  auto-plays.

### 1.6 Resolution and anchoring

Designed for **640×480**; the original renders HUD art 1:1 anchored to screen
edges (big resolution = tiny UI). Anchors: inventory strip top; message line
top; device MFD upper-left; stats MFD upper-right; bio bottom-left; weapon
panel bottom-right; chevron bottom-center; minimap top-right. NewDark adds
`d3d_disp_scaled_2d_overlay` (integer factor or a virtual WxH resolution,
stretched). **shock2quest's `UiCanvas` 640×480 virtual canvas +
`PreserveAspect` letterboxing (§4.4) is precisely the NewDark
"virtual resolution" approach**, so no new scaling design is needed.

## 2. How the Dark Engine Implements It (leaked source, verified)

Browsable mirror used for all citations: **https://github.com/dima424658/darkengine**
(`main` branch — the 2010 Dreamcast-devkit leak of the Thief 2 codebase; the SS2
game/UI layer is complete under **`src/shock/`**, ~446 `shk*` files). Raw files
fetch from `raw.githubusercontent.com/dima424658/darkengine/main/src/shock/<file>`.
(Secondary mirror: `DeathEngine2/LookingGlass-DarkEngine`; the unreleased *Deep
Cover* fork under `src/deepc/` has near-copies `dpckeypd.cpp`/`dpcovrly.cpp` as
cross-reference.) One known gap: the leak has the engine + shock UI but **not
SS2's gamesys script module**; the single place that matters is flagged in §2.3.

### 2.1 The shooter ⇄ metagame mode switch (`shkgame.cpp`)

Three globals: `bool shock_mouse` (cursor mode on), `int shock_cursor_mode`
(`SCM_NORMAL / DRAGOBJ / USEOBJ / LOOK / PSI / SPLIT`, `shkcurm.h`), and
`ObjID drag_obj` ("what is ON the cursor"). **`MouseMode(bool, bool)`
(`shkgame.cpp:381`)** is the single toggle:

- refuses to enter while the `HideInterface` quest var is set, and refuses to
  **leave** while `drag_obj != OBJ_NULL` (you can't exit the metagame holding an
  item on the cursor);
- swaps the keyboard context (`HK_GAME_MODE` ↔ `HK_GAME2_MODE`) and masks
  `UI_EVENT_MOUSE_MOVE` out of the binder so mouselook stops;
- entering loads the `cursor` PCX and restores the last cursor position; leaving
  recenters the mouse and clears HUD-select.

Bindable commands (`ShkCommands[]`, `shkgame.cpp:2144-2146`): **`toggle_mouse`**
(`ShockToggleMode`, line 705 — what SS2's shipped `default.bnd` binds to **Tab**;
the key itself is data, not code) and **`frob_toggle`**
(`ShockFrobAndMaybeToggleMode`, line 739 — default RMB: frob the object under the
crosshair, and only toggle into the metagame when the target has *no* world-frob
action — so RMB on a door opens it, RMB on nothing opens the interface).

Panel choreography on switch — `ShockOverlayMouseMode()` (`shkovrly.cpp:1261`):
entering cursor mode turns ON `kOverlayFrame`, `kOverlayTicker`, `kOverlayInv`,
`kOverlayMouseMode` and turns OFF `kOverlayCrosshair` (reverse on exit), playing
schemas `mainpanel_op` / `mainpanel_cl`; leaving force-closes every overlay whose
`needmouse` flag is set. The little bottom-center "return to mouselook" button is
itself an overlay (`shkmlook.cpp`, art `MICELOOK`, buttons `ML0/ML1` — all present
in our `res/iface/`).

### 2.2 Panel/MFD management — the overlay system (`shkovrly.cpp/.h`, `shkovcst.h`)

A flat enum of **50 overlays** (`shkovcst.h`: `kOverlayInv`=0, `kOverlayFrame`,
`kOverlayKeypad`=13, `kOverlayPDA`=24, … `kOverlayMouseMode`=49) with modes
`Off/On/Toggle` and flags (`AlwaysDraw`, `Modal`, `Translucent`, …). Each panel is
an **`sOverlayFunc` vtable** (`shkovrly.h:62`): draw, init/term, mouse, dclick,
dragdrop, key, **up/down sound-schema names**, state-change, per-pixel
transparency test (click-through of irregular art), **`distance`** (auto-close
when the player walks away from the bound object), **`needmouse`** (opening the
panel force-enters cursor mode), alpha, update.

Mechanics worth copying:

- **`SetOverlay(which, mode)`** (`shkovrly.cpp:742`): flips state, auto-enters
  mouse mode when `needmouse` (`if (!shock_mouse) MouseMode(TRUE,TRUE)`), plays
  the open/close schema, lets the panel build/destroy its button gadgets.
- **Two dock slots with mutual exclusion**: `exclude_list_left[]` /
  `exclude_list_right[]` (`shkovrly.cpp:313-323`). **Left MFD slot** = the
  world-object panels (Keypad, Container, HRM/replicator plug, Book, Email, PDA,
  Map, Elevator, Turret…); **right MFD slot** = character panels
  (Stats/Skills/Psi/TechSkill/Map). Opening a panel closes the others in its slot.
- **Geometry** (`shkmfddm.h`, canonical 640×480): left MFD `(2, 124, 188×300)`,
  right MFD `(450, 124, 188×300)`; `shkiftul.cpp`'s `SetLeftMFDRect()` /
  `SetRightMFDRect()` re-anchor at higher resolutions (right MFD hugs the right
  edge, both drop toward the bottom). `shkiftul.cpp` also owns the 4 MFD nav tabs
  (`estats/etech/ecmbt/epsi` art at y=270 inside the right MFD).
- **Object binding**: one overlay at a time binds to a world object
  (`ShockOverlaySetObj`, `gOverlayObj`) — how the keypad knows *which* keypad it
  operates, and what the walk-away distance check measures against.
- Z-order is a fixed `gOverlayOrder[]` array; `ShockOverlayDoFrame()` draws in
  order and routes clicks with per-panel transparency tests.
- The always-on bottom bar buttons (logs→PDA, keys, MFD toggle, query, research,
  maps) are `kOverlayFrame` (`shkiface.cpp`, art `ifbtn<n><state>`, schemas
  `subpanel_op/cl`); the shooter-mode item-name strip is `kOverlayMiniFrame`
  (art `frame`).

### 2.3 The keypad MFD (`shkkeypd.cpp`, 552 lines)

- **Docks in the left MFD**; background art **`keypad2`**; close button
  `CloseOff/CloseOn` at (163, 8, 20×21); typed-value readout drawn with font
  `fonts\keyfonta` at rect + (14, 14).
- **Digit hit-testing**: 11 invisible buttons over the baked-in art, `44×60` each,
  columns x = 15/62/109, rows y = 43/104/165/226 (bottom row `0` and `CE`);
  `key_value[] = {1..9, 0, -1}` (−1 = clear). A raw-key handler grabs keyboard
  focus while open and maps `'0'-'9'`/numpad to the same `KeypadButton()`.
  *(shock2quest's VR `KeyPadGui` already mirrors this layout almost exactly —
  left margin 15, 45×60 buttons — see §4.2.)*
- **Entry logic** (`KeypadButton`, line 275): each press plays schema `bkeypad`
  and appends: `keypad_num = keypad_num*10 + digit`. At **exactly 5 digits** it
  sends (deferred) an `sKeypadMsg` — a script message named **`"KeypadDone"`**
  carrying `int code` (`shkscrm.h:53`) — to the bound keypad object.
- **The code check lives in the gamesys script** (not in the leak): the script
  receives `KeypadDone`, compares against the **`KeypadCode`** property (an int
  property registered by the engine at `shkfrob.cpp:170-182`), and on success
  sends **`TurnOn` along the keypad's `SwitchLink`** to the door. Two engine-side
  corroborations: `shkaipth.cpp:95-121` (`DoorOpenable()` walks `SwitchLink`
  backwards from doors and treats keypad-coded sources as blockers), and
  `cShockGameSrv::Keypad(obj)` (`shkscrpt.cpp:582`) → `ShockKeypadOpen(o)` →
  `ShockOverlayChangeObj(kOverlayKeypad, kOverlayModeOn, o)` — i.e. **the panel is
  opened by the frob script**, exactly the shape our `Effect::OpenPanel` proposes.
- **Hacking tie-in** (`ShockKeypadStateChange`, line 486): if the bound object is
  broken or has a `HackDiff`, opening the keypad also raises the `kOverlayHRMPlug`
  hack overlay so the player can hack instead of typing.

### 2.4 Inventory grid (`shkinv.cpp`, `shkincst.h`, `shkinvpr.h`)

- Grid **15×3** (`MAX_INV_COLUMNS/ROWS`), cell **35×34** px; flat
  `ObjID inv_array[45]` where multi-cell items occupy several slots; footprints
  from the **`InvDims`** property. Panel art `invback`, icon area at offset
  (4, 17), 523×98. *(shock2quest's `ContainerGui::inv_container` — 15×3 at
  (4, 18), 35×32 slots — is already a faithful copy; §4.2.)*
- **The cursor IS the item**: `ShockInvLoadCursor(o)` sets the hardware cursor to
  the object's `objicon\` PCX and `shock_cursor_mode = SCM_DRAGOBJ`; there is no
  separate drag ghost.
- **Drag & drop** (`ShockInvDragDrop`, `shkinv.cpp:1109`): paperdoll equip rects
  first; drop-on-occupied does an inv-inv tool frob or a
  `pContainSys->CombineTry(...)` stack combine; grid placement is
  `SetInvObj(player, slot, obj)` ("automagically handles swapping and combining"),
  schema `place_item`. **Dropping outside the panels throws the item into the
  world** (`ShockInterfaceClick` → `ThrowObj`). Containers (`shkcont.cpp`,
  `OverlayContainer`, art `contain`/`bcontain`) **reuse `ShockInvDrawObjArray`**
  for their grids — container loot and backpack are one rendering path, which our
  shared `ContainerGui` already mirrors.

### 2.5 Art + sound references in code (`LoadPCX` sites)

| Panel / element | Source | Art |
| --- | --- | --- |
| Inventory | `shkinv.cpp:137` | `invback`, `block`, `CloseOff/On` |
| Keypad | `shkkeypd.cpp:149` | `keypad2`, `CloseOff/On`, font `keyfonta` |
| Bio/health | `shkmeter.cpp:57` | `biofull`, `hpbar` |
| Ammo/weapon | `shkammov.cpp:126-127` | `ammoback`, `ammofull`, `ammo0/1`, `ammoarw0/1` |
| Main bar | `shkiface.cpp:105` | `ifbtn<0-5><0-1>`, `frame` (miniframe) |
| MFD nav tabs | `shkiftul.cpp:45` | `estats/etech/ecmbt/epsi` + states |
| Mouselook button | `shkmlook.cpp` | `MICELOOK`, `ML0/ML1` |
| HUD brackets | `shkhud.cpp` | `brack0-3`, `hpbar0-2` |
| Cursors | `shkgame.cpp`, `shklooko.cpp` | `cursor`, `lookcur`, `cybercur`, `objicon\*` |
| Map | `shkmap.cpp` | `mapback`, `nomap`, `minimap`, `minion/minioff` |
| Hacking | `shkhrm.cpp` | `hacking`, `hackmetr`, `hacklt00/01`, `hackpip1/2` |

Sound schemas: `mainpanel_op/cl` (mode switch), `subpanel_op/cl` (panel
open/close), `bkeypad` (key press), `btabs`/`bclick2` (tabs), `place_item`,
`rollover`.

---

## 3. Asset Inventory (verified against `/Users/bryphe/ss2-data-unpacked`)

Everything the flat UI needs ships in the already-mounted archives. The repo mounts
`res/iface.crf`, `res/intrface.crf`, `res/objicon.crf`, `res/strings.crf` and the
font archives today (`shock2vr/src/lib.rs:602-613`), so **every asset below resolves
by name through the existing `AssetCache` with no new mounting work**. All UI art is
8-bit paletted PCX (decoded by `PcxFormat::load_indexed`,
`engine/src/texture_format.rs:75`, with optional palette-index-0 transparency).

### 3.1 `res/iface/` — the in-game HUD + MFD art (733 files)

Dimensions read directly from the PCX headers (python3/PIL). The recurring sizes
tell the layout story: **188×296 = one MFD side panel**, **636×121 = the inventory
strip**, **260×64 = an expanded (FULL) HUD readout**, **640×480 = a full-screen
backdrop**.

**MFD panel backdrops (188×296 unless noted):**

| File | Panel |
| --- | --- |
| `KEYPAD.PCX` / `KEYPAD2.PCX` | numeric keypad — verified by decoding both: `KEYPAD.PCX` is the empty chrome, `KEYPAD2.PCX` has the digit buttons **baked into the art** (diff bbox (15,43)-(153,285), matching the source's invisible 44×60 hit rects, §2.3). The original and the VR gui both use `keypad2` |
| `CONTAIN.PCX` / `Container.pcx` | container / corpse loot |
| `REPLIC.PCX` | replicator purchase |
| `LOG.PCX`, `MEDIA.PCX`, `EMAIL.PCX` | audio log / media playback / email |
| `PDA.PCX` | PDA / info |
| `RESEARCH.PCX` | research |
| `HACK.PCX`, `MODIFY.PCX`, `REPAIR.PCX` | hack / modify / repair minigame panels |
| `ELEVATOR.PCX` | elevator floor select (+ `ELEV10..51.PCX` floor buttons, `ELBUTT0/1` 138×28) |
| `TURRMFD.PCX` | turret control |
| `SKILLS.PCX`, `STATS.PCX`, `TRAITS.PCX`, `TECHNIC.PCX`, `PSITRAIN.PCX`, `TRAIN.PCX` | upgrade-station / character panels |
| `ACCESS.PCX`, `RESREP.PCX`, `QUERY.PCX`, `GAMEBACK.PCX` | access-card list, res/rep, query, minigame back |
| `SETTINGS.PCX` (188×300) | weapon settings |

**HUD readouts:**

| File | Size | Role |
| --- | --- | --- |
| `BIO.PCX` | 128×64 | compact bio-monitor (already used by the flat HUD) |
| `BIOFULL.PCX` | 260×64 | **expanded** bio-monitor (use-mode) — left 128px matches `BIO.PCX` crop, right half adds implant/status readouts |
| `AMMOBACK.PCX` | 94×64 | compact ammo gauge (already used) |
| `AMMOFULL.PCX` | 260×64 | **expanded** weapon/ammo panel (ammo-type select `AMMOSET0/1` 12×43, cycle arrows `ammocyc0/1`, setting buttons) |
| `HPBAR.PCX`, `PSIBAR.PCX` | 80×14 | bar fills (already used) |
| `INVBACK.PCX` | 636×121 | bottom **inventory strip** (use-mode): 15×3 grid on the left, **EQUIP paperdoll on the right** (weapon column + armor torso + 2 implant slots — the original's `equip_rects[]`, §2.4) |
| `MAPBACK.PCX` | 636×296 | full map panel |
| `MINIMAP.PCX` | 128×128 | minimap frame (+ `MINIMAP0/1`, `map_*.pcx` 16×16 blips) |
| `WSTATE1..11.PCX` | 14×14 | weapon condition pips |
| `LETTRBOX.PCX` | 640×69 | cutscene letterbox |
| `Meta.pcx` / `Meta2.pcx` | 640×480 | full-screen metagame backdrops (stats/PDA screens) |

**Keypad widgets:** per-digit button pairs `KEY<d>0.pcx` / `KEY<d>1.pcx` (normal /
lit, 32×32) for digits 0–9, `KEYN0/1.pcx` (clear/“nul”), `KEYS0/1.pcx`; large
`KEY0/KEY1.PCX` (44×60). The VR `KeyPadGui` already draws exactly these
(`shock2vr/src/scripts/gui/keypad.rs:24-26`, `:78` — `key<d>0/1.pcx`).

**Cursors:** `CURSOR.PCX` (12×16, the arrow), `USECUR.PCX` / `LOOKCUR.PCX` /
`splitcur.pcx` (32×32 contextual cursors), `PSICUR.PCX`, `CYBERCUR.PCX`,
`CROSSHAI.PCX` (16×16, shooter-mode reticle — already used by the flat HUD).

**MFD tab buttons (the “PDA” row):** `PLOGS0/1`, `PMEDIA0/1`, `PNOTES0/1`,
`PVIDEO0/1`, `PEMAIL0/1` (40×18 up/down pairs), `EEQUIP/EPSI/ESKILLS/ESTATS/ETECH`
pairs, `PICN00..39{,_0,_1,_2}.pcx` (66×30 button states), `IFBTN*0/1` (38×36),
`RETURN0/1`, `PGUP/PGDN`, `UP/DOWN/LEFT/RIGHT` arrows — i.e. every button has a
`…0` (normal) / `…1` (pressed or lit) texture pair, the same convention
`ButtonHoverBehavior::Texture` already exploits.

**Minigame art families:** `HRM*` (hack/repair/modify meters), `SWINE*`, `hog*`,
`race*`, `pong*`, `KBPAD*`, `DH*` (the GamePig minigames), `OWMON*/OWTERR*/OWLOOT*/
OWCMBT*` (psi overworld icons), `tekskil*/tekstat*` (tech skill icons), `PSI1..5`
(+`BLOK`) psi tier art, `TRAIT00..16`, `D001..D006` (204×156 portraits).

`res/iface/fonts/`: `MAINFONT.FON`, `MAINAA.FON`, `BOLDAA.FON`, `bignum.fon`,
`keyfont.fon`, `KEYFONTA.FON`, `numfont.fon` (duplicated in `res/fonts/` with
`BLUEAA.FON`, `DIMMED.FON`). The existing `FONT_IMPORTER`
(`dark/src/importers/font_importer.rs:6`) parses these; the flat HUD renders
`mainfont.fon` today.

### 3.2 `res/intrface/` — full-screen shell screens (menus), *not* the in-game UI

Pairs of `SCREEN.PCX` + `SCREENR.BIN` (LTRB int16 widget rects, parsed by
`dark::importers::UI_LAYOUT_IMPORTER`, `dark/src/importers/ui_layout_importer.rs:61`)
for MAIN/NEWGAME/OPTIONS/GAMELOD/GAMESAV/LOADING/DEBRIEF…, plus per-mission map
subfolders (`MEDSCI1/`, …) and `METAFONT.FON`.

**Key negative finding:** there is **no `*R.BIN` layout file for any in-game
panel** — no keypad, inventory, BIOFULL, or MFD rects. The R.BIN mechanism covers
only the full-screen shell screens; **in-game panel layouts are hardcoded in the
engine source** (§2). So panel widget coordinates for our implementation come from
the Dark source / measurement, not from data files — same as the VR `KeyPadGui`
already does with its hand-tuned digit grid.

### 3.3 `metaui_r.res` — a red herring (investigated)

`file` calls it "Arhangel archive data"; it is actually an **"LG Res File v2"**
(Looking Glass resource archive — magic `LG Res File v2\r\n\x1a`; directory offset
at header +0x7C; 10-byte directory entries of `id:u16, packed:u24, type:u8,
unpacked:u24, flags:u8`). It contains 6 resources (ids 600–605). Decoding resource
600 (640×480, 8-bit, with resource 601 as its 768-byte palette) reveals… the
**Thief-era "metagame" shell screen**: a VIEW BRIEFING / PLAY MISSION / QUIT menu.
"Metaui" in Dark-engine jargon means the *between-missions shell*, not SS2's
in-game metagame HUD. **Conclusion: irrelevant to this feature; nothing to build.**
(The repo's CRF reader can't open it — CRFs are zip archives, this is not — and it
doesn't need to.)

### 3.4 `res/objicon/` — inventory icons (234 files)

`PropObjIcon` (e.g. `"icn_psi"`) names a `<icon>.pcx` here; `ContainerGui` already
renders them this way (`shock2vr/src/scripts/gui/container.rs:127`). Icon art is
sized in inventory-slot multiples (`PropInventoryDimensions` gives the slot
footprint, ibid. `:86-92`).

### 3.5 `res/strings/` — panel captions

All parsed by the existing strings importer
(`dark/src/importers/strings_importer.rs`):

| File | Contents |
| --- | --- |
| `HUDUSE.STR` | frob captions — `keypad:"Enter code"`, `human_corpses:"Search corpse"`, `containers:"Search container"`, … |
| `LOCKMSG.STR` | keypad/lock failure lines (`"Please insert hardware override."` …) |
| `MISC.STR` | HUD messages — `AccessRequired`, elevator floor names (`ElevLevel1..`), reload/equip lines |
| `invcursor.str` | cursor-verb captions for held items |
| `MINIGAME.STR`, `HACKTEXT.STR`, `RESEARCH.STR`, `NOTES.STR`, `MAPTEXT.STR` | per-panel text |

---

## 4. What shock2quest Already Has (survey)

The punchline: **all the panel *logic* already exists** — built for VR as
world-space quads — and is presentation-agnostic enough to reuse. What's missing is
a **flat presentation + a mouse-cursor input source**, and an **open/close model**
(VR panels float permanently; the original flat UI opens an MFD on frob).

### 4.1 The declarative GUI system (`shock2vr/src/gui/`)

- **`Gui<TState, TMsg>` trait** (`gui/mod.rs:39-61`): pure
  `get_components(cursor, entity, world, state) -> Vec<GuiComponent<TMsg>>` +
  `handle_msg(...) -> (TState, Effect)`. Completely presentation-agnostic — layouts
  are in **panel-local pixels** (`GuiConfig::screen_size_in_pixels`, e.g. 188×296
  for the keypad, `scripts/gui/keypad.rs:171-176`).
- **`GuiComponent`** (`gui/gui_component.rs:17-46`): `Image` / `Button` (with
  `on_click`, `on_grab`, `ButtonHoverBehavior::Texture` hover swap) / `Text` /
  `Inventory`. Hit-testing is already pure: `GuiComponent::get_event(last, current)`
  (`gui_component.rs:532-576`) resolves rising-edge clicks/grabs against component
  rects in panel pixels.
- **`GuiScript`** (`gui/gui_script.rs:42-160`): the host `Script` attached to the
  entity. Each `update` it emits `Effect::SetUI { components, world_size, … }`
  (`:78-84`); on `MessagePayload::GUIHover { screen_coordinates, is_triggered,
  is_grabbing, hand }` (`:99-156`) it hit-tests and routes events into
  `Gui::handle_msg`. **`GUIHover` is the entire input contract** — anything that can
  synthesize normalized panel coordinates + press state can drive every existing
  panel unchanged.
- **`ProxyGuiScript`** (`gui/proxy_gui_script.rs:55-89`) + **`GuiManager`**
  (`gui/gui_manager.rs:48-142`): the *VR presentation*. `SetUI` builds a kinematic
  world-space quad (`CollisionGroup::ui()`, `gui_manager.rs:75-83`); the VR hand
  raycast sends `Hover` (`virtual_hand.rs:322` — **the only `Hover` sender in the
  codebase**), which the proxy converts from a world-space hit point into normalized
  panel coordinates. A flat presentation replaces exactly this pair — nothing above
  it.
- **Gating:** `Effect::SetUI` is only processed under `--experimental gui`
  (`mission/mission_core.rs:2250-2270`, gate at `:2257`). In a default flat session
  the panels neither render nor interact — part of why #435 sessions saw *nothing*
  on frob.

### 4.2 Panels already implemented against that trait (`shock2vr/src/scripts/gui/`)

Registered in `scripts/mod.rs` by Dark script name — i.e. **the mission data already
attaches them** to the right entities:

| Script name(s) | Gui | Notes |
| --- | --- | --- |
| `keypad`, `keypadunhackable` (`scripts/mod.rs:574-575`) | `KeyPadGui` (`scripts/gui/keypad.rs`) | full digit grid (`key<d>0/1.pcx`), value display, **code check against `PropKeypadCode` + `TurnOn` to all SwitchLinks** (`:216-238`), `bkeypad`/`hacksucc` sounds. This is the logic #435 needs — it only lacks a flat way in. |
| `containerscript` (`:437`) | `ContainerGui::loot_container()` (`scripts/gui/container.rs:27`) | `contain.pcx` 188×296, 4×4 grid; enumerates the entity's `Contains` links (`:76-93`), renders `PropObjIcon` icons sized by `PropInventoryDimensions`, grab-to-hand + frob msgs (`:170-205`) |
| `internal_inventory` (`:491`) | `ContainerGui::inv_container()` (`container.rs:39`) | `invback.pcx` 635×120, **15×3 grid — the player backpack UI already exists**; attached to the synthetic player-inventory entity (`inventory/player_inventory_entity.rs:20`) |
| `elevatorbutton` (`:681`) | `ElevatorGui` | floor select |
| `replicatorscript` (`:718`) | `ReplicatorGui` | purchase panel |
| `minigameboy` (`:638`) | `GamePigGui` | minigames |

### 4.3 Flat-mode interaction today (`FlatPlayerController`) — the #435 gap

`shock2vr/src/flat_player_controller.rs:139-155`: crosshair raycast each frame
(already includes `InternalCollisionGroups::UI`), then on the use-key rising edge
sends `MessagePayload::Frob` to the target (`:240-252`). But `GuiScript` ignores
`Frob` (`gui_script.rs:94-158` — only `GUIHover`/`ProvideForConsumption` are
handled), so **keypads/containers/elevators do nothing in flat mode**. The
interaction seam is clean: `PlayerInteraction` trait (`interaction.rs:44-105`) with
`VrInteraction` / `FlatInteraction` chosen at `mission_core.rs:446-448`.

### 4.4 Screen-space 2D stack (ready to be the flat presentation)

- **`UiCanvas`** (`shock2vr/src/ui/mod.rs:146-303`): image/bar/text/opacity at a
  virtual 640×480 resolution, rendered via `render_screen_space` with
  `ScaleMode::PreserveAspect` letterboxing.
- **`pointer_to_canvas`** (`ui/mod.rs:102-119`): maps the normalized mouse pointer
  into canvas pixels, returning `None` in the letterbox bars — **the cursor math is
  already written and unit-tested**.
- **Flat HUD** (`hud/flat_hud.rs`): the shooter-mode baseline already ships —
  crosshair + `BIO.PCX` bio-monitor + `HPBAR/PSIBAR` + `AMMOBACK` ammo gauge + psi
  overload meter, pure-function canvas builder with unit tests, e2e screenshot test
  (`tools/shock2-sdk/test/flat-hud.e2e.test.ts`). Its constants are already derived
  from the original layout; `BIO.PCX` is the left crop of `BIOFULL.PCX`
  (`flat_hud.rs:35-37`), so the use-mode expansion is literally "swap the backdrop
  and extend".
- **Full-screen scene precedents:** `MainMenuScene` (`scenes/main_menu.rs:76-99` —
  pure `resolve_click` pointer hit-testing pattern) and `LoadingScene`
  (`scenes/loading.rs` — PCX + R.BIN composition).

### 4.5 Mouse pointer plumbing

- **`InputContext::pointer: Option<Pointer2D>`** (`input_context.rs:17-42`) —
  normalized [0,1] position + pressed, already flows through every runtime.
- **Desktop** (`runtimes/desktop_runtime/src/main.rs:320-327`, `:648-658`): when
  `game.wants_pointer()` (a `GameScene` method, default `false`,
  `game_scene.rs:90`; today only `MainMenuScene` returns true) the runtime shows
  the OS cursor and populates `pointer`; otherwise the mouse is captured for look.
  **The mode switch the original's Tab needs already exists** — it just needs to be
  driven by mission UI state instead of by scene identity alone.
- **Debug runtime** (`runtimes/debug_runtime/src/main.rs:482`, `:1566-1655`): holds
  a persistent `InputContext` patched via `POST /v1/control/input` channels
  (`head.look`, `*_hand.trigger`, …). **There is no pointer channel yet** — adding
  `pointer.position [x,y]` / `pointer.pressed 0|1` to `apply_input_patch` is a
  ~20-line change and gives full headless cursor control.
- **Input actions** (`shock2vr/src/input/`): the discrete-action pipeline
  (desktop keys + `POST /v1/input/action` + SDK `game.input.trigger`) is the right
  home for the Tab toggle. Desktop mapper currently binds P/Space/I/B/R/T/Y/S/L
  (`runtimes/desktop_runtime/src/input_mapper.rs:32-72`) — **Tab is free**.

### 4.6 `Contains` links at entity creation — the loot bug behind “items in odd positions”

Verified by search: **no code path consumes `Contains` at instantiation**
(`entity_creator.rs` and `mission/mod.rs` have zero references; the only `Contains`
consumers are the save system `save_load/mod.rs:56`, the pickup-transfer helper
`mission_core.rs:1472`, the debug inventory view `mission_core.rs:4475-4540`, and
`ContainerGui`). Consequences, confirmed in data:

- medsci1 entity **219** (Male Corpse 1) has `Contains → 1407` (Psi Amp); 1407
  carries its own authored `PropPosition` (−13.39, −1.08, −45.86) ≈ 2.7 units from
  the corpse (−15.93, −1.47, −44.73). The engine instantiates it there as a normal
  physical prop. In the original game an item with an incoming `Contains` link is
  **not world-placed** — it exists only inside the container's loot panel.
- So today: contained loot lies on the floor at editor-authored spots (or clipping
  into furniture), which playtests have misread as item-placement bugs, and the
  container panels (even in VR with `--experimental gui`) show icons for items that
  *also* exist in the world.
- Counter-example for calibration: the Cryo Card (medsci1 mission id 1050) has
  **no** incoming `Contains` link — genuinely world-placed pickups exist and must
  keep working.

**Debug-runtime representation caveat (for playtest honesty):** `/v1/entities`
reports contained items at those world positions, so an automated playtest can
“see” (and with `player/give`-style shortcuts, acquire) loot that the real game
gates behind the loot panel. Until the container PR lands (§6, PR 3), playtest
reviews should treat “item at odd/low position with an incoming `Contains` link”
as *pending loot UI*, not a placement bug — and afterwards, honest looting must
drive frob → panel → take (§6 PR 3's e2e shows how).

### 4.7 Debug/e2e infrastructure relevant here

- `DebugEntityMessage::Frob` via `POST /v1/entities/:id/message`
  (`game_scene.rs:587-599`, debug main `:1981`) — can frob the keypad headlessly.
- Deterministic `/v1/step` + fully-rendered `/v1/screenshot`; SDK
  (`tools/shock2-sdk`) with `entities.list({filter})` (discover by name/template —
  runtime ids are unstable), `sendMessage`, `input.trigger`, `input.set`,
  `waitFor`, screenshot; e2e suite patterns in `test/*.e2e.test.ts`.

---

## 5. Proposed Architecture

### 5.1 Principles

1. **Reuse the `Gui` layer wholesale; replace only the presentation + input
   source.** `KeyPadGui`/`ContainerGui`/`ElevatorGui`/`ReplicatorGui` and
   `GuiScript`'s state/hit-test/effects stay untouched and stay shared with VR.
   Flat mode adds a *second* presentation of the same components — exactly the
   split the codebase already makes for the HUD (`virtual_arms` vs `flat_hud`,
   `hud/flat_hud.rs:3-7`).
2. **The input contract is `MessagePayload::GUIHover`.** A flat panel host maps
   mouse → panel-local normalized coordinates and sends `GUIHover` to the panel
   entity — the same message `ProxyGuiScript` synthesizes from the VR hand ray
   (`proxy_gui_script.rs:69-85`). One message type in, all panels work.
3. **Panels are *opened*, not floating.** Original flat behavior: frob → MFD
   overlay + cursor; close → cursor released. VR keeps its always-on world quads.
4. **Every increment headlessly verifiable** (repo core principle): pointer
   channels + a UI-introspection endpoint land *with* the first panel, so e2e
   tests click real digits instead of teleporting state.

### 5.2 Sketch

```
                    ┌──────────── shared, unchanged ───────────┐
   mission data →  │ scripts/gui/* (KeyPadGui, ContainerGui…) │
   (PropScripts)    │ GuiScript: state, hit-test, Effects      │
                    └───────▲──────────────────────┬───────────┘
                   GUIHover │                      │ Effect::SetUI(components)
        ┌───────────────────┴───┐         ┌────────┴─────────────────────┐
  VR    │ VirtualHand ray →     │         │ GuiManager → world quads     │  (existing)
        │ ProxyGuiScript        │         │ (CollisionGroup::ui)         │
        └───────────────────────┘         └──────────────────────────────┘
        ┌───────────────────────┐         ┌──────────────────────────────┐
  Flat  │ FlatUiHost:           │         │ FlatUiHost render:           │  (new)
        │ mouse → panel coords, │         │ components → UiCanvas rect   │
        │ LMB → is_triggered    │         │ (MFD anchor, 640×480 canvas) │
        └───────────────────────┘         └──────────────────────────────┘
```

**`FlatUiHost`** (new, `shock2vr/src/mission/` or `shock2vr/src/hud/`):

- Holds flat UI mode: `Shooter` | `Use` (cursor visible), plus
  `active_panel: Option<EntityId>` (the object-bound MFD — mirroring the
  original's `gOverlayObj` single-object binding, §2.2).
- **Open:** `GuiScript::handle_message` gains a `Frob` arm returning a new
  `Effect::OpenPanel { entity }` (VR mission_core ignores it; flat sets
  `active_panel`, enters cursor mode — the original's `needmouse` semantics).
  This keeps “what is a panel” knowledge in the script, not in a script-name
  table, and matches the original where the frob script opens the overlay
  (`cShockGameSrv::Keypad`, §2.3).
- **Render:** intercept `Effect::SetUI` in flat mode (`mission_core.rs:2250`) —
  instead of `GuiManager`, stash the latest `components` for the active panel and
  draw them into the 640×480 `UiCanvas` at the **left MFD anchor `(2, 124)`**
  (the original's world-object panel slot, `shkmfddm.h` §2.2), after the flat
  HUD. Component rects are already panel-local pixels; the mapping is one affine
  transform. (The right MFD slot `(450, 124)` is reserved for the later
  character/stats panels.)
- **Input:** each frame in cursor mode, map `InputContext::pointer` through
  `pointer_to_canvas` (`ui/mod.rs:102`) → subtract panel anchor → divide by
  `screen_size_in_pixels` → send `GUIHover { screen_coordinates, is_triggered:
  lmb, is_grabbing: rmb-held?, hand: Right }` to `active_panel`. Draw
  `CURSOR.PCX` at the pointer.
- **Close:** the panel's close button, Tab (toggle out), or **LMB on the bare 3D
  view** (the original's exit gesture, §1.1) → clear `active_panel`, leave
  cursor mode (unless Tab metagame mode is active). Also **auto-close when the
  player walks away** from the bound object (the original's per-overlay
  `distance` check, §2.2) — this matters for keypads next to doors that open
  under the player.
- `Mission::wants_pointer()` returns `active_panel.is_some() || use_mode` so the
  desktop runtime flips the OS cursor exactly as it does for the menu.

**Tab / metagame mode:** a new `InputAction::ToggleUseMode` (Tab on desktop; the
action pipeline makes it HTTP/SDK-triggerable for free, `input/actions.rs`).
Use-mode = cursor + inventory strip (`internal_inventory`'s `ContainerGui`
rendered as a **top-docked** panel — the original anchors the 636×121 strip at
the top of the canvas, §1.6 — the same way MFDs render) + `BIOFULL`/`AMMOFULL`
expanding the compact readouts in place (bottom-left / bottom-right). Movement
keys keep working (original behavior; mouse-look is what's surrendered).

**Containment:** at instantiation, entities with an incoming `Contains` link are
created **without world presence** (skip model/physics or park them, matching
save/restore semantics) until taken (transfer link → player backpack, reusing
`mission_core.rs:1472`) or dropped. §6 PR 3 details the increment.

### 5.3 Headless verification surface (lands with PR 1)

- `POST /v1/control/input` channels: `pointer.position [x,y]` (normalized),
  `pointer.pressed 0|1` (`apply_input_patch`, debug `main.rs:1566`).
- `GET /v1/ui`: current mode, active panel entity + template, and the active
  panel's **component list with canvas-space rects + semantic labels** (e.g.
  `button digit=4 rect=[…]`), so tests click “digit 4” without hardcoding pixels.
  This is the UI analogue of `/v1/physics/bodies`.
- Screenshot diff (panel visibly appears) stays the redundant visual check.

### 5.4 Shared vs flat-only (explicit)

| Shared with VR (untouched) | Flat-only (new) |
| --- | --- |
| `Gui` trait, all `scripts/gui/*` panels, `GuiScript` state machine, `GUIHover` contract, `Effect` vocabulary (`TurnOn` to SwitchLinks, sounds, grabs) | `FlatUiHost` (mode, active panel, anchor layout), `UiCanvas` panel rendering, cursor draw + `pointer_to_canvas` mapping, Tab action, `wants_pointer` wiring, pointer debug channels, `/v1/ui` |
| Containment-at-creation fix (VR loot panels also stop double-placing items) | BIOFULL/AMMOFULL expanded readouts, inventory strip docking |

---

## 6. Incremental Plan (small, independently shippable PRs)

### PR 1 — Pointer + UI introspection plumbing (no visible change)

`pointer.position`/`pointer.pressed` debug channels; `InputAction::ToggleUseMode`
(bound Tab, dispatch to a new effect, no-op for now); `GET /v1/ui` skeleton
(mode only). Negative e2e: `/v1/ui` reports `shooter`, pointer channels accepted.

### PR 2 — Keypad MFD on frob (**resolves #435**)

`GuiScript` `Frob → Effect::OpenPanel`; `FlatUiHost` with a single MFD slot at the
original left-MFD anchor `(2, 124)`; `SetUI` interception in flat mode (also ungate
it from `--experimental gui` for the flat path); `GUIHover` synthesis from the
pointer; cursor draw; close button + walk-away auto-close.
**e2e (`keypad.e2e.test.ts`):** launch `medsci1.mis` flat → discover keypad by
`template_id` −258 + `KeypadCode` 45100 (mission id 1681; never hardcode runtime
ids) → `sendMessage Frob` → `/v1/ui` shows panel + screenshot diff → click 4-5-1-0-0
via pointer channels on `/v1/ui` rects → assert door 1739 (template −206, by name
"Sci Med Door") opens (physics body position change / `TranslatingDoor` state) and
`hacksucc` fires. Negative test first: pre-PR the frob produces no `/v1/ui` panel.

### PR 3 — Container loot: containment at creation + loot MFD

(a) Skip world instantiation for entities with incoming `Contains` (keep save/load
round-trip; VR benefit too); (b) frob corpse/container → `ContainerGui`
loot panel in the MFD slot; click/drag item → transfer `Contains` to player
backpack (`mission_core.rs:1472` helper). **e2e:** corpse 219 → panel → take Psi
Amp → appears in `player_inventory()`; world no longer contains a stray Psi Amp at
(−13.4, −1.1, −45.9); Cryo Card 1050 still world-placed. Updates the playtest-skill
guidance (§4.6 caveat).

### PR 4 — Tab metagame mode: cursor + inventory strip

`ToggleUseMode` flips cursor mode; **top-docked** `internal_inventory` grid
(15×3 `ContainerGui` already registered; original anchor §1.6); original click
semantics as capacity allows — LMB lift-to-cursor/place, RMB use (§1.5) — with
drag-into-world throw as the drop path. The `INVBACK` equip-paperdoll slots
(weapon/armor/implants, §3.1) are stubbed art-only here — wiring equip is its
own follow-up, as are ALT-split and CTRL-query. e2e: Tab → `/v1/ui` mode `use` +
inventory rects; click a carried item; Tab again → shooter restored.

### PR 5 — BIOFULL/AMMOFULL expanded readouts

In use mode swap `BIO.PCX`→`BIOFULL.PCX`, `AMMOBACK`→`AMMOFULL` with ammo-type
select buttons wired to the existing `CycleAmmo` machinery. Unit tests on the
canvas builder (pattern: `flat_hud.rs` tests) + screenshot e2e.

### PR 6+ — remaining MFD families (each its own PR)

Audio log/email playback panel (text + `LOG.PCX`/`EMAIL.PCX`), map panel
(`MAPBACK.PCX`, reuse `debug_map` machinery), replicator (`ReplicatorGui` exists),
elevator (`ElevatorGui` exists), then research/hack/modify/upgrade stations.
Sequence by playtest demand — logs and elevator unblock the most progression.

Each PR: `RUSTFLAGS="-D warnings" cargo check -p shock2vr -p desktop_runtime -p
debug_runtime`, unit tests on pure layout/hit-test fns, an `SHOCK2_E2E=1` scenario
test, `missions.e2e.test.ts` for load safety, and pr-visuals GIF/PNG for anything
visible.

---

## 7. Open Questions / Risks

- **Panel anchors are now known** (left MFD `(2,124)`, right `(450,124)`,
  inventory strip top, bio bottom-left, ammo bottom-right — §1.6/§2.2) but the
  exact BIOFULL/AMMOFULL/INVBACK x-offsets aren't cited from source yet
  (`flat_hud.rs`'s (2,414) bio anchor is consistent with bottom-left + 64px art
  height at 480). Pin them from `shkmeter.cpp`/`shkammov.cpp`/`shkinv.cpp` rects
  when implementing PR 4/5 — they're constants in code, not data files (§3.2).
- **`SetUI` interception vs a parallel flat host:** intercepting keeps one code
  path but couples flat rendering to the effect stream; a `FlatUiHost` that *calls*
  `Gui::get_components` directly (skipping `GuiScript`) would duplicate state.
  Current lean: intercept, keep `GuiScript` the single state owner.
- **`--experimental gui` gate:** flat panels should work by default (they resolve
  #435); does the VR world-quad path stay gated? Lean: yes, ungate only flat.
- **Keypad check timing fidelity:** the original defers the code check until
  **exactly 5 digits** are typed (`KeypadButton`, §2.3); shock2quest's
  `KeyPadGui` checks after *every* press (`keypad.rs:216-238`) — functionally
  equivalent for entering a correct code, but the 5-digit deferral (and a wrong-
  code failure response) is the faithful behavior to converge on in PR 2 or a
  follow-up.
- **Two MFD slots:** the original runs left (world-object panels) + right
  (character panels) slots with per-slot mutual exclusion (§2.2). PRs 2–4 need
  only the left slot + the inventory strip; the exclusion-list mechanic arrives
  with the first right-slot panel.
- **Grab semantics on flat** (`is_grabbing`, `Handedness`) are VR-shaped; the flat
  host maps LMB-drag → right-hand grab. Watch for panels that distinguish hands.
- **Contained-item edge cases:** items both `Contains`-linked *and* referenced by
  traps/scripts; containers destroyed before looting (original spills loot as a
  corpse "search"?); nested containers (depth-2 already modeled in save/load).
- **Cursor art at high resolution:** 12×16 `CURSOR.PCX` scales with the canvas;
  PreserveAspect letterboxing already handles non-4:3.
