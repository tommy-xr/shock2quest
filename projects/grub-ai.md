# Grub object-model AI

`GrubAI` is a sibling of `AnimatedMonsterAI` under `BaseMonster`. It reads
awareness and navigation independently of animation, sets physical velocity,
and advances the object's authored joint tweq. `SwarmerAI` remains a follow-up;
its flight controller can reuse the awareness layer without adopting the grub's
ground movement or skeletal motion clips.

## Authored behavior and reused components

- `MobileAwareness` uses the existing alertness state machine, caps, delays,
  visibility, last-known target publication and noise messages.
- Ground movement uses `PathFollowSteeringStrategy` for chase and wander,
  including asynchronous route requests, waypoint progress and stall recovery.
  A failed/pending real route never falls back to chasing through a wall.
  Debug scenes without navigation can chase a visible target directly.
- `P$AI_MoveSp` is the Dark move-speed input; the motor applies the original
  `SetObjImpulse` factor of 7.5. `P$AI_TurnRa` is degrees per second.
- `P$AI_Grub_C` supplies leap range, speed limits and cooldown. The retail
  template has zero bite distance/intensity and an empty bite stimulus; its
  authored Anti-Human contact stim supplies damage. Contact is spent once per
  leap to prevent repeated collision messages from multiplying a strike.
- Death uses the existing Slay/Flinderize path. BaseMonster forwards child AI
  snapshots, including awareness, animation phase, leap cooldown and contact
  latch. Stasis pauses both the controller and the wiggle and blocks contact
  attacks. Routes are recomputed after loading.

## Joint animation

`CfgTweqJo` is a 132-byte configuration: an eight-byte header, six 20-byte
parameter configurations, and a one-based primary parameter plus padding.
`StTweqJoi` contains seven four-byte states. `JointPos` supplies six initial
values. The Grub enables three linear, bouncing parameters, each from -30 to
+30 degrees at 10 degrees per 100 milliseconds.

LGMD's field previously labeled `parent_idx` is actually the parameter index.
The child/next links encode hierarchy. `SetObjectJointParameters` maps each
parameter onto the appropriate sub-object, including parts sharing a parameter.
Rotation occurs about local Dark X (runtime -X), after the authored pivot;
sliding uses the same axis. This preserves the existing per-joint transform
path for cameras and turrets. The helper is currently driven by GrubAI, not a
new global tweq pass: other jointed props are not implicitly activated. Nonlinear
jitter/multiply curves are not implemented by this helper.

## Physics boundaries

Hatched grubs previously became kinematic fixtures through inherited FrobInfo;
emitted grubs became unconstrained dynamic props through the projectile path.
Both now use the same dynamic sphere, with physical rotation locked. The motor
owns yaw. Grub geometry faces -X while navigation uses +Z, so heading is converted
at that boundary in both directions.

The remaster's `grub3.bin` has a lower rest extent of approximately -.052, while
its inherited sphere offset is .36 and radius .2. Applying the inherited offset
unchanged buries the visible model .16 below support. Creation derives the
sphere offset from the loaded model's lower bound plus radius (approximately
.148), including model scale. This metadata is separate from existing model
interaction bounds. Support and perception use the live sphere; resting solver
contacts handle curved pod surfaces that a vertical probe can miss.

## Damage volume

Damage follows four padded capsules fitted to the joint-local geometry of the
head, chest, abdomen and tail. Each capsule encloses its section along its longest
axis with an extra .025 model units of radial padding and rounded ends. The
existing hitbox manager places these non-solid proxies using the same animated
joint transforms as the rendered mesh. All four count as normal body damage.
The support sphere remains for movement and ordinary physical contacts; shot
ray refinement, flat melee and held VR melee use the segment proxies when present.
Slow physical projectiles retain the existing engine contact behavior.

The grub retains five authored hit points. Tests verify hits beyond the old
sphere, misses through its empty upper volume, proxy damage forwarding and
cleanup on death. Weapon balance/skill-dependent shots-to-kill have not been
playtested in this pass. Proxies are rebuilt after save/load, like skeletal
creature hitboxes.

## Deliberate limits of this first controller

The hop is a motor approximation, not full original physics parity. Dark keeps
applying velocity control after the leap. Our motor instead treats authored
speeds as limits and bounds the apex to target height. Applying the raw retail
50 ft/s upward speed as free ballistic motion sent the grub several storeys
above the target in an open scene. Emitter launches remain ballistic until
landing; they are not replaced by a chase velocity in midair.

Ground following refuses unsupported steps. It does not add wall/ceiling
crawling or humanoid door-frob behavior. Separate nonzero bite stimuli and the
original random collision-escape charge are not implemented in this first pass;
the existing path follower provides stall recovery.

## Evidence and checks

- Real `rec1.mis` pod 131 near the elevator: hatch, upright landing, natural
  pursuit/hop and player damage.
- `grub-ai.e2e.test.ts`: joint animation without a skeletal clip, natural pursuit,
  bounded upright hops, segment ray hits, proxy death/cleanup and isolated floor placement.
- Existing annelid egg/goo volley and ops4 emitter/save tests.
- Unit coverage for binary layouts, parameter mapping, animation phase restore,
  support geometry, body creation, alert caps and committed leap state.

Reference implementations examined:
[Grub combat](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/SHOCK/SHKAIGRA.CPP),
[joint tweqs](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/ENGFEAT/TWEQCTRL.CPP),
[object articulation](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/DarkEngine/LIBSRC/MD/RENDER.C),
and [AI velocity control](https://github.com/DeathEngine2/LookingGlass-DarkEngine/blob/main/thief_2_service_release/rdrive/prj/thief2/src/AI/AIUTILS.CPP).
