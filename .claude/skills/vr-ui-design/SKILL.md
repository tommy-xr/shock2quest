---
name: vr-ui-design
description: >-
  Hard-won principles for designing and implementing VR UI and interactions in
  shock2quest - panels, pointers, pause/system UI, input edges, and the
  verification discipline that catches VR-only bugs. Load whenever designing or
  building a VR interaction (menus, panels, HUD, inventory, diegetic UI,
  controller input), reviewing such a change, or planning a VR UX feature.
  Complements AGENTS.md section 3 (flat/VR render parity) with the
  interaction-level rules learned in the frontend-menu campaign (PRs
  #994-#1009).
---

# VR UI & interaction principles

Each rule below was paid for with a real bug or device round-trip. A `#N`
citation is the PR that implemented the rule or the issue that tracks a
remaining gap - PR/issue and open/closed state are marked where it matters.

## Rendering & lifecycle

1. **Never stop rendering.** Loading, "pause", and session-state changes must
   keep submitting frames - a frozen compositor is disorienting in-headset,
   and a render thread that dies or stalls takes Android's lifecycle/input
   pump down with it, so the watchdog fires an app-wide ANR (#1009). Known
   gap: a cold load's main-thread GPU build (~2 s) still blocks frame
   submission at the end of the loading screen - see AGENTS.md's
   `loading_screen` notes and issue #1011 (open). For any *pause* layer,
   sim-freeze means dt=0 while frames keep flowing (design intent - the pause
   skeleton is in flight, not merged).
2. **Degrade, don't panic, in the frame loop.** Transient OpenXR errors
   (untracked poses, `ERROR_POSE_INVALID`) are normal for a frame or two after
   the headset wakes. Honor the view-state flags; submit no layers and retry
   rather than `.unwrap()`. (#1009; remaining unhardened sites tracked in
   #1010, open.)

## Panels (system UI: menus, pause, dialogs)

3. **Place on entry, gravity-aligned, world-locked, lazy recenter.** Never
   gaze-glue a panel (UI painted on the face is an anti-pattern) and never let
   it roll or pitch with the head. Place once from the *tracked* head pose -
   position from the head, direction yaw-only - then leave it world-locked;
   re-place with a short eased turn (0.3 s smoothstep) only after a sustained
   large deviation (>60 deg / >1 m held ~1 s). Use `FrontendPanelAnchor`
   (`shock2vr/src/ui/panel_anchor.rs`, introduced by PR #998); do not
   hand-roll placement - the hardcoded-origin variant was the pre-#998 bug,
   and the hardcoded-height copy in `debug_map` survives as issue #999 (open).
4. **The panel basis must be honest.** Local +Z faces the viewer, +X is the
   viewer's right, content y-flip applied exactly once as a rotation. Any
   compensating rotation "that makes it render right" is hiding a basis error
   that will invert on the other runtime. (#994 - the 180-degree saga.)

## Pointing & input

5. **One pointer arbitration, everything reads it.** Hover highlight and
   click derive from the single shared `vr_frontend_pointer`
   (`shock2vr/src/ui/frontend_pointer.rs`); PR #1008 (open) renames it to
   `vr_frontend_pointer_pass` and makes the ray beams and hit dot read the
   same pass, so the picture can never promise a hover the menu won't give
   (its negative test forces the two-dots-one-highlight case). Never compute
   "what am I pointing at" a second time for a visual - see AGENTS.md section
   3 for why independent paths drift.
6. **Rising-edge everything, and enter screens "already pressed".** A screen
   entered while a button is held (dying with trigger down, scene swap under a
   press) must not treat the held state as a click - initialize
   `last_pressed = true`. A hand not currently driven must have its button
   state cleared so a stale press can't latch. (#997 game-over insta-Quit;
   desktop `--vr` routing in #994.)
7. **Guard the zero quaternion.** Untracked controllers/head arrive as the
   ZERO quaternion, and cgmath's `rotate_vector` silently returns the input
   unrotated - a fixed ray at panel center, a panel pinned to world -Z. Any
   code consuming a pose must treat near-zero magnitude as "untracked: no
   pointer / identity / skip", never as a valid pose. (#994, #997.)
8. **Discrete, non-contextual inputs go through `InputAction`** (AGENTS.md
   "Adding a New Action") - that's what makes them drivable over HTTP and the
   on-device debug server with no extra work. **Contextual hand interactions
   (trigger pull, grab, drop, pointing) are NOT actions** - they stay in
   `VirtualHand`/`InputContext`, which read game state. Routing a contextual
   gesture through the action system is a wrong-subsystem bug.

## Design defaults

9. **Diegetic-first for in-game UI, panel-stack for system UI.** In-world
   interactions (inventory, MFDs, keypads, devices) belong on world surfaces
   the hands operate; the frontend panel stack is for system UI (main menu,
   pause, load, game over). Keep the two layers' input ownership crisp
   (design intent for the pause layer: system UI freezes sim and makes hands
   pointers-only; diegetic UI runs live).
10. **Edge-policy table up front.** For any new modal surface, decide in one
    line each: opening while dead / during a transition / on a frontend scene /
    with buttons held / with metagame (Tab) mode active. Implement each as a
    guard with a test.

## Domain behaviors agents consistently get wrong

- **Audio logs are COLLECTED, not inventoried - and collection does NOT play
  them.** Pickup files the log as collected (readable later); playback is the
  player's explicit `ReadLastUnreadLog` action. The current VR auto-play on
  pickup is a documented *stopgap* because VR lacks an action mapper (issue
  #921) - do not preserve it as "faithful", and do not model logs as
  carryable objects.
- **Keycards are COLLECTED, not inventoried.** Frobbing a `PropKeySrc` item
  emits `AcquireKeyCard`, records the credential in `QuestInfo`'s key-card
  list (a separate mechanism from quest bits - contrast `FrobQB`), destroys
  the pickup, and doors consult it implicitly via `can_unlock`. Never put a
  keycard in the inventory grid or require wielding one at a door. Known gap:
  the VR *hold/grab* path currently just grabs the card physically instead of
  collecting it - issue #583 (open) tracks reconciling the pickup paths; the
  intended spec is that both frob and hold collect.
- When implementing any pickup, check which model the original game uses
  (collected flag / credential vs. inventory object) before defaulting to
  "add to inventory" - `cargo dq` on the template's properties/links usually
  answers it.

## Verification discipline

11. **`debug_runtime --vr` is a harness, not a headset.** It proves geometry,
    parity, and interaction logic deterministically - use it for every
    iteration. But tracked poses, session lifecycle, compositor behavior, and
    perf only exist on device: anything touching those needs a Quest pass
    (vr-device-loop skill), with a **fresh install verified by timestamp** -
    stale APKs have produced false negatives twice.
12. **Drive the device like the harness - for input.** The on-device debug
    server (`debug-port.txt` + `adb forward`, PR #996) injects the same input
    channels as the debug runtime, so on-device interaction (hover, click,
    head pose) is agent-verifiable without a human in the headset; overridden
    channels win until released - clear before disconnecting. It has **no
    `/v1/step` and no `/v1/screenshot`**: the device free-runs (not
    deterministic), and capture still goes through adb / the vr-device-loop
    scripts.
13. **Evidence standards beyond AGENTS.md sections 3-4** (which stay
    canonical for both-presentation render verification and PR media): read
    the captured images - byte size is not a content signal (solid frames
    compress identically); verify hover/hit agreement numerically (label-rect
    luminance) when correctness of *which* element reacted matters; and for
    timing-dependent device bugs, prefer deterministic fault injection over
    waiting for a natural repro (#1009's technique).

## Held items and body inventory

14. **One item, one owner, including within a frame.** A support hand steers
    the primary hand's item; it never owns a second copy. Reserve a released
    entity until its effects finish, because a release may become a backpack
    deposit. The other hand must not acquire its still-present collider during
    that frame. Test simultaneous grabs/releases, not only alternating input.
15. **Attached gloves follow the final physical item.** Drive the weapon toward
    the controller, then derive attached glove transforms from the synchronized,
    collision-resolved weapon. Sampling the controller for one and the physics
    body for the other separates them during locomotion and impacts. Verify the
    relative grip transform every frame while walking and touching a wall.
16. **Body storage needs an explicit grip edge and a clear refusal.** Reaching
    through a slot while holding must not silently store an item. Deposit on a
    deliberate release; retrieval requires a fresh squeeze. Define occupied/full
    behavior before implementation. A refused shoulder deposit retains the item,
    plays a refusal cue, and explains how to re-grip; leaving the zone must not
    unexpectedly drop it behind the player. Enter disabled/recovered states
    disarmed so stale input cannot create a gesture.
17. **Tracking validity is separate from plausible pose values.** A finite pose
    and nonzero quaternion can still be a stale runtime fallback. Body gestures
    must honor live head/hand validity (`InputContext::pose_tracking`) as well as
    numeric validation. Head-relative body targets use horizontal heading, not
    head pitch/roll. Treat their offsets as estimates until tested seated and
    standing in a headset; debug-runtime coordinates do not prove reach comfort.
18. **Reuse ownership and inventory transitions.** Preserve the exact entity,
    ammo, condition, and script state. Keep the normal `Drop`/`Hold` signals when
    redirecting a release, and preserve deferred effects returned by shared
    handlers. Check real capacity and reserve simultaneous destinations before
    claiming releases. Carry additional body storage through save/load and level
    transitions explicitly; hiding a world model is not storage.
19. **Resolve readouts per hand and per weapon.** Dual wielding makes a global
    “current weapon” ambiguous. Ammo, ammo type, condition, settings, and their
    actions must all come from the same explicit entity. Exercise gun/gun and
    gun/psi-amp combinations. Preserve the complete authored UI canvas when
    mounting it on a glove; avoid arbitrary cropping to make it fit.

## Physical glove fit

- Use `debug_gloves` for the passthrough fit experiment; its controls and units
  are in `DEVELOPMENT.md` under “Glove fit check”. Forward is global (default
  −15 cm); side/up/size and pose-reference selection remain scene-only previews.
- Trace the pose actually consumed. The Quest runtime binds both grip and aim,
  but gameplay currently locates the **aim** spaces for hand transforms. A
  binding declaration is not evidence that grip drives the glove. Compare both
  in the fit scene; they can differ in orientation as well as translation.
- Compare the physical wrist, palm and fingertips while holding controllers,
  at several orientations, changing offset and size independently. Passthrough
  visibility does not imply optical hand tracking. A translated silhouette in
  a debug screenshot does not establish real-hand registration.
- Fit controls must also reach the pause menu’s separate pointer gloves while
  the fit scene is active. Share the visual calibration transform; keep the
  tracked beam, hit dot and click arbitration coherent. Test leaving the scene
  so preview settings cannot leak into normal menus.
- Diagnose orientation-dependent error with controller-local side/up offsets
  (mirror side for left/right), then inspect wrist versus fingertip alignment
  before assuming translation alone is sufficient. The wearer reported aim
  reference / forward −15 cm as a useful candidate, with residual palms-down
  error. The user chose −15 cm as the global forward default; remaining axes
  still require physical verification.
- Keep `dark::SCALE_FACTOR` a load-time unit convention. It also feeds tracked
  meter conversion; changing only some consumers mixes units. A glove-size
  preview should scale about the hand origin without rescaling head/eye poses.
  Keep rendered gloves, wrist mounts, held-item transforms and cached/baked
  grip samples in the same corrected frame. Global forward is applied once to a
  copy of tracked hand input before menu/gameplay routing (`glove_fit::calibrated_input`),
  preserving the raw input and head/eye poses. Do not also shift the mesh or bake
  a live dev parameter into grip/wrist caches.

## Glove readout implementation notes

- `hand_glove::GloveRenderer::wrist_frame` provides the calibrated mount:
  local +Y runs from wrist toward fingers and +Z points out of the glove's
  back. `hud/virtual_arms.rs::wrist_panel_transform` places the bio readout at
  the wrist facing dorsally. The held weapon's ammo/psi readout caps the cuff
  opening where the arm enters, facing back along the forearm (local -Y),
  rotated 90 degrees counterclockwise as viewed face-on and sized inside the
  cuff rim. Apply this roll in the panel plane after the cuff-facing rotation.
  Treat offsets and size as glove-specific fit values, not a general UI spacing rule.
- The compact gun layout lives in `hud/ammo_panel.rs::emit`, shared by the
  flat HUD and glove: bold centered count, ammo icon left, condition badge
  upper right, and ammo type/current fire mode below. The expanded interface
  reserves room for its clickable controls. Bound type and mode separately so
  long authored tags cannot displace the mode; resolve both from that hand's gun.
- `interaction.rs` passes the final visible hand poses into
  `create_wrist_hud_panels`; preserve this attachment so weapon physics and
  support grips move glove and readout together. Mirrored hands already have
  readable bases from `wrist_frame`; do not mirror the text a second time.
- For a flush mount, inspect the cuff opening end-on for legibility and an
  oblique view for clipping or a floating gap, with a weapon actually held. Check both
  hands. Keep the shared canvas layout intact while adjusting its mount;
  verify comfort and glance readability in-headset before calling the fit final.

## Evidence for physical interactions

20. **Pair pictures with state assertions.** Before/after screenshots can show a
    release, but cannot prove the stored item is the original instance. Assert
    entity identity, ownership, containment, physics removal/restoration, and
    retained weapon state. Include refusal, tracking recovery, and simultaneous
    hand cases. Show the interaction's approach and release, not just an empty
    hand afterward. Label debug zones as instrumentation; they do not establish
    production discoverability or headset tracking reliability.
