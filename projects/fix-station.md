# Fix Station.mis Level Transitions

## Problem Summary

Station.mis level transitions are currently broken, forcing a hardcoded redirect to medsci1.mis in the ChooseService script. Players cannot properly experience the multi-iteration station sequence as designed in the original game.

## Investigation Results

Using the dark_query CLI tool, I analyzed the station.mis entity structure and discovered the complete transition system design and what's missing.

### How Station.mis is SUPPOSED to Work

**The Multi-Iteration System:**
1. **START entities** (917, 918, 919 etc.) trigger based on initial conditions
2. **YEAR entities** (953=YEAR-1, 956=YEAR-2, 957=YEAR-3) control iterations
3. **Router entities** cascade the signals through complex logic chains
4. **Exit tripwires** (741, 124, 745) at position (-22.6, -5.6, varying Z) should eventually activate
5. **Exit markers** (125, 126, 127) with `PropDestLevel("MedSci1")` and `PropDestLoc(2502)` lead out of station.mis

**Entity Flow Example:**
```
START_01 (917) → YEAR-1 (953) → Router (818) → [complex routing] → Exit Tripwire (741) → Exit Marker (125) → MedSci1
```

### The Core Problem: Missing ChooseMission Script

All three exit markers (125, 126, 127) use the **ChooseMission** script, but this script doesn't exist in the codebase:

```rust
// From entity 125, 126, 127:
PropScripts { scripts: ["ChooseMission"], inherits: true }
PropDestLevel("MedSci1")
PropDestLoc(2502)
```

**Current Status:**
- ✅ TrapNewTripwire script exists and should work
- ✅ TrapRouter script exists and forwards messages correctly
- ✅ PropDestLevel and PropDestLoc properties are implemented
- ❌ **ChooseMission script is missing** - this is the blocker
- ❓ P$CharGenRo property is unparsed (may be needed for iteration control)

### Additional Context

**Exit Trigger Locations:**
- Tripwire 741 at (-22.6, -5.6, 8.0) → Exit Marker 125
- Tripwire 124 at (-22.6, -5.6, 3.2) → Exit Marker 126
- Tripwire 745 at (-22.6, -5.6, -4.8) → Exit Marker 127

All tripwires use `PropTripFlags { trip_flags: ENTER | PLAYER }` and should activate when the player walks through.

## Implementation Plan

### Phase 1: Implement ChooseMission Script (Critical - Unblocks station.mis)

**Priority: HIGH** - This is the minimum needed to make station.mis functional.

#### 1.1 Create ChooseMission Script
**File:** `/Users/bryphe/shock2quest/shock2vr/src/scripts/choose_mission.rs`

```rust
use dark::properties::{PropDestLevel, PropDestLoc};
use shipyard::{EntityId, Get, View, World};
use crate::physics::PhysicsWorld;
use super::{Effect, MessagePayload, Script};

pub struct ChooseMission {}
impl ChooseMission {
    pub fn new() -> ChooseMission {
        ChooseMission {}
    }
}

impl Script for ChooseMission {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_dest_level = world.borrow::<View<PropDestLevel>>().unwrap();
                let level_file = v_dest_level.get(entity_id).unwrap();

                let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
                let maybe_dest_loc = v_dest_loc.get(entity_id).ok().map(|dest_loc| dest_loc.0);

                Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                    level_file: format!("{}.mis", level_file.0),
                    loc: maybe_dest_loc,
                })
            }
            _ => Effect::NoEffect,
        }
    }
}
```

#### 1.2 Register ChooseMission Script
**File:** `/Users/bryphe/shock2quest/shock2vr/src/scripts/mod.rs`

Add to the script registration:
```rust
mod choose_mission;
// ... in script creation function:
"ChooseMission" => Box::new(choose_mission::ChooseMission::new()),
```

#### 1.3 Remove Hardcoded Redirect
**File:** `/Users/bryphe/shock2quest/shock2vr/src/scripts/choose_service.rs`

Update to use proper destination properties like LevelChangeButton does:
```rust
match msg {
    MessagePayload::TurnOn { from: _ } => {
        let v_dest_level = world.borrow::<View<PropDestLevel>>().unwrap();
        let level_file = v_dest_level.get(entity_id).unwrap();

        let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
        let maybe_dest_loc = v_dest_loc.get(entity_id).ok().map(|dest_loc| dest_loc.0);

        Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
            level_file: format!("{}.mis", level_file.0),
            loc: maybe_dest_loc,
        })
    }
    _ => Effect::NoEffect,
}
```

#### 1.4 Test Basic Functionality
- Load station.mis and verify player can trigger exit tripwires
- Verify transition to medsci1.mis works
- Test that destination location (2502) is respected

### Phase 2: Investigate Iteration System (Medium Priority)

**Goal:** Understand why station.mis iterations aren't triggering properly.

#### 2.1 Implement P$CharGenRo Property (if needed)
**Research needed:** Determine what P$CharGenRo controls
- Add property definition to `dark/src/properties/mod.rs`
- May control which iteration/year the player starts in

#### 2.2 Debug Iteration Triggers
**Investigation tasks:**
- Why aren't START entities (917, 918, 919) activating?
- What should trigger the initial START entity?
- Is there a level initialization script that's missing?
- Do we need save game state to track iterations?

#### 2.3 Test Iteration Flow
**Verify the routing works:**
- START_01 → YEAR-1 → Router chain → eventual exit trigger
- Multiple iterations cycle properly
- Each iteration shows different content/progression

### Phase 3: Polish and Validation (Low Priority)

#### 3.1 Enhanced Testing
- Test all three exit points (125, 126, 127)
- Verify different starting conditions lead to different flows
- Test save/load during station.mis progression

#### 3.2 Documentation
- Document the station.mis iteration system
- Add comments explaining the routing logic
- Update CLAUDE.md with station.mis analysis

## Testing Strategy

### Manual Testing Checklist
- [ ] earth.mis → station.mis transition works (ChooseService fix)
- [ ] Player can walk around station.mis without getting stuck
- [ ] Exit tripwires at (-22.6, -5.6, varying Z) are triggerable
- [ ] station.mis → medsci1.mis transition works (ChooseMission implementation)
- [ ] Destination location 2502 is respected in medsci1.mis
- [ ] No regression in other level transitions

### Automated Testing
- Add integration test for level transition flow
- Test script registration and message handling
- Verify property reading (PropDestLevel, PropDestLoc)

## Risk Assessment

**Low Risk Changes:**
- ChooseMission script implementation (follows existing patterns)
- Script registration (standard process)

**Medium Risk Changes:**
- ChooseService script modification (affects earth.mis → station.mis)
- May need to handle cases where destination properties are missing

**Dependencies:**
- PropDestLevel and PropDestLoc properties (already implemented)
- TrapNewTripwire script (already working)
- Level transition infrastructure (already working)

## Success Criteria

1. **Phase 1 Complete:** Player can complete station.mis and progress to medsci1.mis
2. **Phase 2 Complete:** Station.mis iterations work as designed in original game
3. **No Regressions:** All other level transitions continue working
4. **Code Quality:** Implementation follows existing script patterns and is well-tested

## Files Modified

**New Files:**
- `shock2vr/src/scripts/choose_mission.rs`

**Modified Files:**
- `shock2vr/src/scripts/mod.rs` (script registration)
- `shock2vr/src/scripts/choose_service.rs` (remove hardcoded redirect)
- `dark/src/properties/mod.rs` (P$CharGenRo property, if needed)

## Future Considerations

- Other missions may have similar missing scripts - this analysis approach could be applied elsewhere
- The dark_query CLI tool proved invaluable for this investigation and should be used for similar issues
- Consider creating automated tests that verify level transition completeness

---

*Analysis performed using dark_query CLI tool on 2025-10-24*