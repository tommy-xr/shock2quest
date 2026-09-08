# Base Agility movement

The native Stats Trainer can sell Agility because shared player locomotion now consumes the saved base stat. It charges the existing Normal-difficulty `STATCOST` row; no new currency, save field, or runtime-specific movement path is involved. This addresses #1422, not the broader #1310 modifier work.

## Authored data and integration

The original [`ShockPlayer` recalculation](https://github.com/dima424658/darkengine/blob/main/src/shock/shkplayr.cpp#L919) selects `GAMEPARAM.speed[agility - 1]`, substitutes 1 for a zero entry, and installs a translation-only speed scale. The [`sGameParams` layout](https://github.com/dima424658/darkengine/blob/main/src/shock/shkparam.h#L82) is 19 floats: throw power, eight bash coefficients, eight speed scales, overlay distance, and frob distance. The speed table begins at byte 36 of the 76-byte chunk.

The shipped 25th Anniversary `sshock2.kpf/data/shock2.gam` table is `[1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.85, 2.0]`. Valid custom gamesys tables take precedence. Missing/truncated tables, or tables with negative/nonfinite entries, use this documented retail fallback. Authored zero values retain the original zero-to-one substitution. A normalized ratio that overflows or underflows uses the corresponding retail ratio. Base stat lookup clamps to the table's 1–8 range; native training still caps at 6.

The port has an established Agility-1 input rate of 25 Dark units/s (10 world units/s after `SCALE_FACTOR = 2.5`). It predates stat consumption: commit `f1e2a899` merely extracted the existing literal into `PLAYER_MOVE_SPEED`. That is not asserted to be the original game's exact absolute speed. One named boundary, `agility_movement_scale`, divides the selected authored entry by the level-1 entry. Consequently Agility 1 remains 10 world units/s, Agility 2 becomes 10 × 1.3/1.2 ≈ 10.833, and Agility 6 becomes 10 × 1.7/1.2 ≈ 14.167. This preserves established starting movement while matching authored relative progression.

The multiplier applies once to ordinary right-stick translation in the shared mission update. Direction, analog magnitude, collision and stance resolution retain their existing behavior. Flat push-to-climb redirects that scaled input as before; physical VR hand pulls and mantle trajectories are not amplified. Turning, jump launch, gravity, developer vertical input, and detached camera flight keep their own existing rates. The table and stat are read each update, so a native purchase or current-build load takes effect without a cached movement field.

## Deferred work

This change does not implement Agility fall vulnerability, weapon kickback/handling, temporary implant/psi/hypo modifiers, Speedy, or a general movement modifier stack. It does not introduce retail run/walk direction factors or a crouched movement penalty. Their absence remains separate from the supported base-stat movement consumer.

## Validation

`agility-movement.e2e.test.ts` measures ordinary flat and VR input at controlled stats, including unchanged level-1 pace, authored ratios, partial reverse input and crouched strafe. It checks turn and detached-camera independence, real hand-pull distance and flat ladder redirection. A fresh MedSci trainer fixture purchases Agility with the native button, measures movement, and saves/loads within the same build. Parser/unit tests cover packed field order, every truncated record length, custom tables, zero semantics, invalid data and bounded indexing. No campaign save is used or published.
