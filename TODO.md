TODO:
- [ ] Get CI green for changes

Recent work
- flat runtime -> fix alpha transparency / z-order break
- flat runtime -> fix aiming ofset
- modernize combat: add recoil / accuracy modifier (like counter-strike)

- Fable: finish ragdolls
- Fabel: finish AI pathing & testing


SNACKS:

- [ ] Implement frobbing keycards (collecting when held)
- [ ] implement tweqallbyname -> eng

- Player 'damage'
- Player 'death'
- Ladder handling - for both desktop and vr (climbable flag in phys attr)
- Melee attacking: player
  - Have a 'script' for simulating a slow swipe (low damage) or fast swipe (high damage)
  - Add raycast test to see what is being hit

- Main menu
- Fix ranged attack when monsters only support melee
- Nanite counting / gui upgrade for replicator
- Cybernetic module counting / ui for upgrade stations
- OS upgrade stations
- Why is load/save broken?

TODO:

- Shift + left key or mouse right
- Get GLTF / hand models working
- Start cutting APK releases - automate releases
- Create a website with instructions on how to install
- Test with new assets
- Psi powers

- Figure out how to get macOS build running by re-pointing binary - DYLD path or LD_LIBRARY_PATH. How to co-locate binaries?
- Same for Windows
- Same for Android
- HUDSelectable prop?

Can do these in parallel:

- Skip cutscene by pressing trigger
- What are the entry points for a cutscene? How do we know to play a cutscene?

- [ ] Combat: Handle Player Health
  - [ ] Should just be a number on player state?
  - [ ] Render in HUD somehow...
- [ ] Combat: Handle Player Death
  - [ ] Camera fall down animation, static
- Create a placeholder UI for 'assets not found'
- Create a floating menu 'mission'
- AIWatchObj link: Apparitions
  - Parse ApparStart link
  - Handle ScriptMessage 'ApparBegin', 'ApparEnd' -> these should just be scripts?
  - Implement 'Signal'?
  - Example 'grassi app' in medsci
  - Quick iteration: spawn near that one
- Fix slay
  - Make robots blow up, spawn items
  - Create corpse and make container available
  - Maybe there's another way to figure out what to slay?
- command2.mis:
  - Why is door not transparent?
  - Why is door not moving?
- rec:
  - why is the cutscene flaky?
- Android pipeline improvements (CI build is green; release mode + artifact still todo)
  - Build w/ release
  - Add APK as artifact
- [ ] Thinking about other screens:
  - [ ] Add intro screen
    - [ ] Add configuration for base path
    - [ ] Pick default base path
    - [ ] Refactor mission to support intro screen
      - [ ] How to refactor `lib.rs` / `mission/mod.rs` -> distribute responsibilities between them
    - [ ] Screens: No Access
    - [ ] Screens: No Data
  - [ ] What subset of mission functionality needs to be reusable?
    - Physics
    - GUI
    - Sound
- [ ] Thinking about menus:
  - [ ] Add shock2 menu, once we pass the other ones..
- [ ] VR: Is handedness already considered?
  - [ ] Where would the scale be factored in?
- [ ] VR: Weapon scale
- [ ] Hack for blood spangs
  - [ ] Replace template with a different one
  - [ ] Add bitmap, tweq destroy
- [ ] Camera AI - If player still visible after 3 seconds, switch to alarm - SwitchLink ecology ? How does triggering happen?
- [ ] Security system AI
- [ ] Add ecology
  - [ ] Hook up to security system
    - [ ] Frob disable
  - [ ] Show alert in HUD
- [ ] Screen when assets aren't available
  - [ ] Just placeholder for now; from mod
- [ ] Events for footstep sounds?
- [ ] Player death
  - [ ] If bio-reconstruction, spawn there
- [ ] Hitbox
  - [ ] Adjust damage based on hitbox
  - [ ] Add flinch percentage based on where hit, and animation timing
- [ ] Raycast melee weapons
- [ ] AI - Render health marker
- [ ] AI - How do melee attacks work?
  - Looks like they may trigger UNK5, UNK6
- [ ] Screen for menu.

  - [ ] Refactor state management for desktop runtime
    - [ ] Create decoupled camera - make sure the pruning is actually working as expected!
  - [ ] Handle start button (Escape on desktop)
  - [ ] Show menu screen, menu background loop
  - [ ] Pause updating the current mission, show menu screen instead

- [ ] Refine how we make the UI visible
  - [ ] Create enum:
    - [ ] AlwaysVisible (Distance cut-off)
    - [ ] Distance(always_show, fade_range)
    - [ ] Gaze
  - [ ] Fix body bounding box
  - [ ] Option to always face player?
- [ ] Hold items
  - [ ] Create placeholders for items on the body
  - [ ] Snap inventory and PDA to it
  - [ ] Add left/right holsters
- [ ] Inventory v2: Positioning
  - [ ] How do we decide the snap logic?
    - We'll need to decide the actual points by rounding.
    - We'll need a property to use to decide the position of the object. Is that something saved already?
  - [ ] How do we show whether the drop is valid?
    - Render drop position and red/green
    - What asset to use?
  - [ ] Styling
    - add new inventory item
    - Use invback.png
  - Refactor inventory to have a 'Drop(entity_id, x, y)' option
- [ ] GUI: Replicator v2
  - [ ] Store nanites in QuestInfo
  - [ ] Enable collection of nanites
  - [ ] Render current nanites
  - [ ] Deduct nanites
  - [ ] Check cost of nanites
- [ ] GUI: Elevator v2 (Questbits)
  - [ ] If no power: `intrface/power.pcx`
    - What is questbit?
  - [ ] If hydro not unblocked: show blocked
    - What is questbit?
    - What does this look like in game?
- [ ] GUI PDA
  - [ ] Create object for it
    - [ ] Create mock in gimp
  - [ ] Press 'tab' -> teleport in front of player
  - [ ] `ammofull.pcx` or `biofullpcx`?
  - [ ] `pda.pcx`
  - [ ] Have button to create in front of plyer
    - [ ] Should reposition when pressed again
    - [ ] What are those interface items to use?
  - [ ] States
- [ ] VR: Handle embedded assets
  - [ ] AssetCache strategy for embedded items
  - [ ] Bring in custom inventory UI
- [ ] Hands:
  - [ ] Load fbx
    - [ ] Fbx libraries don't support animations / skinning...
    - [ ] ...will need to find another approach.
    - [ ] Try fbx2gltf, and import gltf?
    - [ ] Modify texture for TetraCorp
  - [ ] Maybe there are poses here? (https://github.com/ValveSoftware/steamvr_unity_plugin/tree/master/Assets/SteamVR/Models)
    - [ ] Or these? (https://github.com/ValveSoftware/steamvr_unity_plugin/tree/master/Assets/SteamVR/InteractionSystem/Poses)
  - [ ] Record hand position
    - [ ] Oculus hand tracking?
    - [ ] Way to save snapshot
  - [ ] Figure out how to save animations
  - [ ] Lerp between animations in virtual hand
- [ ] GUI: Gamepig
  - [ ] Will need to refactor grabbables -> have a trait for it
  - [ ] Handle the 'drop into' for the game
    - Read property for minigames (Player: Mini Games -> seems like an int prop)
    - Have way to register mini games (place holder game X)
    - Send 'ProvideForConsumption' to gamepig
      - If gamepig sees MiniGame prop, will 'contain' it
      - Show first contained entity in inventory
    - Would need to adjust container to allow dropping items into others
  - [ ] Create a button to 'drop' the cartridge
    - If 'ProvideForConsumption' - already has a link, pop that one out
- [ ] PSI Powers
  - [ ] Render UI for current psi power
- [ ] GUI: PDA v2?
  - [ ] Activate via wrist-watch?
- [ ] Text Rendering v2
  - [ ] Font sizing for world text?
  - [ ] Implement font cache for meshes
  - [ ] Fix short name rendering
    - [ ] Stack size included
    - [ ] Why is med hypo not showing up in med1?
    - [ ] Cache these results
- [ ] GUI: Gamepig
  - [ ] Show title screen.
    - [ ] Add 'GamePig' title text
  - [ ] Blink 'Insert a cartridge'
    - [ ] Add 'Insert a cartridge' blinking text
- [ ] PDA v2
  - [ ] Render logs
- [ ] Built in items:
  - [ ] PDA container
- [ ] Save v2:
  - [ ] Serialize / deserialize scripts
    - [ ] How to handle spawning scripts?
      - [ ] Maybe just add 'values'so that the script could optionally deserialize
      - factor out of initialize entity
      - or have populator handle it
    - [ ] ScriptedActionBehavior will be tricky - need to serialize effects too
- [ ] GUI: Other
  - [ ] Separate gui scripts
    - [ ] Security
    - [ ] OS Upgrade
    - [ ] PSI upgrade
    - [ ] Stat upgrade
    - [ ] Weapon upgrade
    - [ ] Tech upgrade
- [ ] Tweq, v2
  - [ ] Implement tweq scale
- [ ] Reloading - collide with ammo type?
  - [ ] Spike: will need additional way to manipulate arm
    - 1. Right hand
    - 2. Left hand
    - 3. Right holster
    - 4. Left holster
    - 5. Inventory
    - 6. PDA
    - 7. Right Chest item
    - 8. Left chest item
    - 9. Right overhead item
    - 0. Left overhead item
- [ ] Weapon handling - v2: Physics based holding, recoil
  - Handle recoil like in other game (w/o physics model)
- [ ] Architecture:
  - [ ] Can we store some of the mappings (id_to_model, etc) in special "runtime props"? These wouldn't get serialized, but would just be used for runtime. May simplify the code for rendering...
  - [ ] Could also use the world.run_with_data function
- [ ] Collision sound between items
  - [ ] Need to create physics entities for world with material tags
  - [ ] On contact force, play audio based on class tags -> use env_sound schema. Attentuate volume
  - [ ] Footstep tool
- [ ] Grabbing items - pick up ammo, objects
  - [ ] How does it feel in VR?
  - [ ] Wrench:
    - [ ] Implement collision with surrounding.
      - Register collision listener. If one of the entities is held entity, try putting in damage
    - [ ] Send damage
      - Create event for damage
      - Custom event, that can have a default handler?
      - TriggerCollide event
- [ ] Particles
  - [ ] Emit via radius (ie, engineering)
- [ ] Fix chemical rendering issues
- [ ] Item consumption
  - [ ] Consume keycard for eng
    - [ ] How to survive level transitions? Requires serialization?
    - [ ] Test by spawning keycard (figure out ent?) in eng2
  - [ ] Consume simulation cards
    - [ ] Test by spawning cards (figure out ent?)
- [ ] Velocity based movement?
  - [ ] Should be physically based, but will need a stable way to represent springs. Ideally could be constraints, but not yet supported in Rapier.
  - [ ] Try using velocity instead of spring physics for now, for hand gestures
    - How does it work for position?
    - How does it work for rotation?
- [ ] Tweq'ing
  - [ ] Parse Tweq properties
    - [ ] Remaining rotation
    - [ ] Emit
    - [ ] Joint
  - [ ] Create system
    - [ ] EmitTweq
  - [ ] Fix rotation tweq when not full rotation
- [ ] Explosions (ie, re301 in barrel)
  - [ ] Implement physics to push items away
  - [ ] Implement damage
- [ ] Skinned model - incorporate weights into animation
- [ ] Factor playerscript out to use existing link infra
- [ ] Implement collision sounds
  - [ ] Create physics representation per-texture
  - [ ] Resolve material, material2 in env sounds
- [ ] Corpse: Why is LD$Corpse not fully populated? Is there a default it uses if not fully specified?
- [ ] Perf: Can we minimize draw calls for materials based on cell?
- [ ] Create a localized string mapper
  - [ ] Use dictionary in lookup method.
- [ ] Weapons: Source/Stim Act/React
- [ ] Spangs: Particle system not working correctly for blood spatter - why?
  - [ ] Observations: Len is 324 vs 380 - so there is some difference in format not accounting for. Both the alpha and scale velocity look way off. Get smoke effect fixed.
- [ ] AI Behavior Improvements
  - Wander: Scan rays up/down to try and capture more of environment?
  - Add 'cliff' detector
  - Add 'stuck' detector
TODO(Oculus)

- Check text rendering
- Can we enable mip-mapping?
- Upgrade openxr, remove vendoring, test on device

TODO(future)

- [ ] wgpu: https://github.com/philpax/wgpu-openxr-example/blob/main/src/main.rs - Looks promising, like VR + multiview are available: [Title](https://github.com/gfx-rs/wgpu/issues/2186)
      TODO(high-level):
- [ ] More traps: https://www.systemshock.org/index.php?topic=5701.msg60896#msg60896
- [ ] Enable (basic) climbing

REFACTORS:

- [ ] Clean up duplication between logdiscscript and trap_email

REFERENCES: - [ ] Create type vs actor type: https://github.com/infernuslord/DarkEngine/blob/c8542d03825bc650bfd6944dc03da5b793c92c19/cam/src/shock/shkcret.h
TRAP REFERENCES: - [ ] https://www.ttlg.com/forums/showthread.php?t=73828 - [ ] Station.mis / ChooseService: http://thevault.collective-illusion.com/sec/ofctut.html - [ ] Good overview: https://www.systemshock.org/index.php?topic=2305.0 - [ ] CS9_MasterControl: https://www.thiefmissions.com/telliamed/allscripts.html
ADVENTURES:

- [ ] Use better enum types for items, like: https://enodev.fr/posts/rusticity-convert-an-integer-to-an-enum.html
  - [ ] P$Collision (collision_types)
- [ ] Code-signing - get a release keystore / be able to build APK in release mode
- [ ] Render hand positions + pointer (\*)
  - [ ] How will the API need to change? We'll obviously have to pass this to the renderer
- [ ] Shodan custcene - ops CS9_MasterControl

TODO(font)

TODO(lighting)

- [ ] Read bsp nodes
- Compute object lights: https://github.com/infernuslord/DarkEngine/blob/c8542d03825bc650bfd6944dc03da5b793c92c19/cam/src/render/objlight.c

- Backlog:
- [ ] Create prelude for engine
  - [ ] Move material to module
  - [ ] Move meshes to module
- [ ] Create variant for scene object
  - [ ] Transform
  - [ ] Light
  - [ ] Mesh
- [ ] Fix up warnings to have a clean slate
- [ ] Move SystemShock2Level -> Scene Object to a separate module (asset importer)

Optimization:

- [ ] Use portal info to determine which objects to render: https://www.youtube.com/watch?app=desktop&v=1AUxDCHaw84

BACKLOG:

- [ ] [Material] Create wireframe material (color + setting points mode) (deferred, not straightforward in OpenGLES)

BACKLOG:

- [ ] Enable spacewarp
- [ ] TODO: Get building on WASM
- [ ] ESRGAN: Automatic texture upscaling
  - [ ] https://github.com/xinntao/Real-ESRGAN

```UI

Custom(String) => {

}

let handle = Handle::new();
let ui = Ui {
    parent: entity_id,
    screen_size: vec2(2.0, 2.0),
    pixel_size: vec2(256, 256),
    anchor: Entity(entity_id),
    offset: vec3(0.0, 0.0, 1.0),
    elements: vec![
        img(texture),
        button().onclick("click-1").position(vec2(0.0)).size(vec2(1.0)).children(vec![
            img("image-name").width(32).height(32).x(50).y(50),
            text("string").font("font-file").width(32).height(32).x(50).y(50),
        ]),
    ]
}

Effect::UpdateUI {
   handle,
   ui,
}

Release Checklist
- Run `clippy --fix`
- Run test cases (community assets?)
- Remove `println` & derps
- Publish apk
```

### Year Summary

2023: Big things achieved

- Portal-based rendering
- Physics integration
- Initial UI
- Open sourced
- GitHub actions for Windows, Mac, Linux, Android
- Music, song reading
- Animated Textures
- Motion / Animations
- Initial AI
- Level Transitions
- Serialize / Deserialize
- Scripted Sequences

2025:

- Upgraded Rapier
- Started teleportation feature
- Render AVI vidoes
- Basic multi-pass lighting
- Ability to parse and understand map formats
- Add the 'dark viewer' (c dv) tool
- Add the 'dark query' (c dq) tool
- Experimented with 'shodan' for automation
- Finished speech db parsing / got basic sounds
- AI: Awareness model for camera
- AI: Initial AIPATH parsing
- Progress on station mis
- Fixed some animation quirks
- Progress on a 'debug runtime' that LLMs can use to 'play the game
- Added debug scenes
- Refactored 'mission_core' out
- Added arm-mounted displays
- Got oculus runtime building again

2026 (so far):

- AI: Generalized alertness model across monsters, turrets, cameras
- AI: A\* pathfinding integration + interactive path visualization
- Unified input action system (replaced Command trait; shared across desktop/VR/debug runtimes)
- Finished debug runtime: HTTP action injection, pathfinding test status, crash surfacing (503s + panic callstacks)
- TypeScript SDK ('playwright for shock2quest') - launch/verify/shutdown lifecycle, typed API, e2e scenario tests
- Mission smoke tests: all 22 loadable missions verified loading (found shodan.mis AIPATH crash, #267)
- Fixed shodan.mis AIPATH crash: parser now handles chunk version 3.4 (widened IDs) and fails gracefully on malformed data; smoke test 23/23 (#267)
- First green CI (Build & Unit Test) since December
- Wound animation, attack/death sounds

2026: Things I hope we can do

- Intro, menu screens
- Render AVI videos
- Lighting
- Hand models
- Gameplay
  - Melee weapons
  - Ecology
  - Player death
  - Nanites, cyber modules
  - PSI powers
- Polish
- Release
