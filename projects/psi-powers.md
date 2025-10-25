# PSI Powers Deep Dive

## High-Level Context

System Shock 2 implements psionics by combining C++ runtime glue with data-driven archetypes and scripts:

- **Player Runtime (`cPlayerPsi`)** manages point totals, selected power state, schema playback, overload logic, cursor swaps, and networking hooks (`references/darkengine/src/shock/shkpsi.cpp:201`-`594`). Activation branches by `PsiPower` type (shot, sustained, shield, cursor) and, except for projectile shots, adds the corresponding meta-property to the player to bootstrap scripts (`shkpsi.cpp:327`-`338`).
- **Property Layer** exposes designer-tunable data: `PsiPower`, `PsiShield`, and `PsiState` properties (`shkpsibs.h:37`, `shkpsipr.cpp:74`-`166`). Listeners cache these in `PsiPowersInit` so the runtime can query cost, type, and per-power float parameters (`shkpsipw.cpp:91`-`166`).
- **Entity-Script System** delivers the bulk of behavior. Each psi archetype carries scripts (in `shock.osm`) that consume the meta-property, respond to `TurnOn`/`DeactivatePsi`/`PsiTarget`, and use the `ShockPsi` script service to query shield durations or mark themselves overloaded (`shkpsisc.cpp:25`-`45`).
- **Hard-Coded Hooks** across the engine tailor gameplay when specific powers are active:
  - Weapon recoil and jam logic respect Still Hand and Stability (`shkplgun.cpp:537`-`710`).
  - Melee damage scales with Berserk (`shkmelee.cpp:523`-`527`).
  - Overlays only render Radar/Seeker blips if their powers are active (`shkrdrov.cpp:90`-`104`).
  - The HRM minigame grants CyberHack overload bonuses (`shkhrm.cpp:484`-`520`).
  - Invisibility is forcibly dropped on attack start (`shkplgun.cpp:838`-`839`, `shkmelee.cpp:578`-`582`).
- **Purely Data-Driven Powers** (Quickness, Vitality, Teleport, etc.) rely entirely on the meta-prop scripts: all the runtime supplies is the activation event and accessors such as `PsiPowerGetData` and `PsiPowerGetTime`.

In the VR port (`shock2vr`), we currently expose only a placeholder composite script for the psi amp, with no supporting properties, UI, or systems; practical functionality is therefore unimplemented (`shock2vr/src/scripts/mod.rs:529`-`538`).

## Implementation Plan for shock2vr

1. **Recreate the Player PSI Runtime**
   - Model psi points, selected power, and overload state in Rust (mirroring `PsiState`, `PsiPower`, `PsiShield` data structures).
   - Load `PsiPower`/`PsiShield`/`PsiState` properties from mission data through the existing dark-format readers so designers’ metadata is available without manual duplication.
   - Implement activation flow: validation, cost deduction (including metapsi modifiers), branching by type, and meta-property application/removal. Provide equivalent schema playback hooks or events we can route to our audio layer.
   - Surface `ShockPsi`-like services so scripts or runtime systems can query durations, report deactivations, and check overload flags.

2. **UI and Selection Workflow**
   - Port the psi amp overlay concept to VR: selection radial or palm-mounted UI, display power tiers, costs, and point totals.
   - Mirror the training/buy interfaces to unlock powers, reusing the cost tables from `shkparam.cpp` if available in the data set.
   - Ensure input bindings let the player quickbind powers, begin overloads, and activate cursor-targeted abilities.

3. **Subsystem Hooks**
   - Weapons: add recoil, accuracy, breakage, and condition logic that queries the new runtime for Still Hand/Stability. Ensure invisibility deactivates on firing.
   - Melee: apply Berserk multipliers and enforce invisibility drop.
   - Movement/physics: integrate Quickness, Levitation/Feather Fall, and similar powers by adjusting speed, jump, fall damage, or gravity based on active statuses.
   - HUD overlays: gate radar/seeker visuals behind corresponding power checks, and expose `PsiTarget` messaging for object interactions.
   - Hacking/services: account for CyberHack overload bonuses when computing success odds, similar to HRM logic.

4. **Power-Specific Behaviors**
   - For each `PsiPower` meta-prop, recreate the original script behavior in Rust systems or Lua/script equivalents:
     - Offensive/projectile powers (Cryokinesis, Pyrokinesis, Electro, Mines): spawn archetyped projectiles using current PSI stat multipliers.
     - Sustained buffs (Shield, Vitality, Quickness): apply timed modifiers to player stats and cleanup on deactivation.
     - Cursor powers (Teleport, Force Wall, Enrage): implement targeting workflows, cooldowns, and object messaging using our VR interaction model.
   - Where behaviors rely on Stim/Response chains (e.g., Terror, Pull), either port those systems or approximate them with our existing gameplay components.

5. **Testing and Tooling**
   - Build debug commands akin to `psi_all`/`set_psi_points` to accelerate testing.
   - Add automated regression checks for key hooks (weapon recoil, melee damage, radar overlay) to ensure powers remain wired as other gameplay evolves.

Executing this plan will align the VR port with the original engine’s division of responsibilities—central runtime glue, data-driven power definitions, and cross-system hooks—while giving us room to modernize UI and scripting where appropriate. Use the reference paths noted above to cross-check behavior when porting individual powers.
