use std::io;

use shipyard::Component;

use crate::ss2_common::{read_i32, read_single, read_u32};

use serde::{Deserialize, Serialize};

/// `P$PsiPower` - per-power definition carried by the 35 psi power
/// meta-prop templates (`MetaProperty → Psi Powers → Level 1..5`, e.g.
/// `Cryokinesis` -1143). Layout observed in `shock2.gam` (28 bytes):
/// three ints followed by four floats.
///
/// The float meanings vary per power and match the published gameplay
/// formulas - e.g. Adrenaline Overproduction (`Berserk`) carries
/// `[0.13, 1.0]` (melee damage ×(0.13×PSI²+1)), Metacreative Barrier
/// (`ForceWall`) carries `[150, 50, 5]` (barrier HP 150 + 50×PSI above
/// PSI 5).
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPsiPower {
    /// Unique power id (1..39, not contiguous).
    pub power_id: i32,
    /// How the power activates. Observed values 0..4: 0 = projectile shot
    /// (Cryokinesis, Pyrokinesis, Terror, PsiCharm, PsiMines, Electro
    /// Dampen), 1 = sustained/timed self effect (duration from
    /// [`PropPsiShield`]), 3 = world-targeted (Teleport), 4 = inventory
    /// item-targeted (Fabricate, ElectroPsi, Alchemy). 2 covers the
    /// remaining instant/special powers (heals, CyberHack, SomaDrain,
    /// ForceWall); the per-value semantics firm up as powers get
    /// implemented, so it stays a raw int for now.
    pub activation_type: i32,
    /// Psi points consumed on activation - equals the power's tier (1..5).
    pub psi_cost: i32,
    /// Per-power tuning data (stat bonus, percentages, ...).
    pub data: [f32; 4],
}

impl PropPsiPower {
    pub fn read<T: io::Read>(reader: &mut T, _len: u32) -> PropPsiPower {
        let power_id = read_i32(reader);
        let activation_type = read_i32(reader);
        let psi_cost = read_i32(reader);
        let data = [
            read_single(reader),
            read_single(reader),
            read_single(reader),
            read_single(reader),
        ];
        PropPsiPower {
            power_id,
            activation_type,
            psi_cost,
            data,
        }
    }
}

/// `P$PsiPowerD` - the first learned-psi-powers dword (4 bytes): a bitmask
/// of psi disciplines by power id, **bit index = power id** (ids 0..=31).
/// `The Player` template (-384) authors it `0x00000000` in the retail
/// `shock2.gam` (the original grants powers at runtime - character creation
/// and trainers); the one non-zero authored value in the shipped data is
/// `earth.mis` entity 243 (`Starting_Location`, alongside `P$BaseStats`
/// starting-loadout props): `0x49` = bits {0, 3, 6}.
///
/// The bit-index-equals-power-id layout is corroborated by `psihelp.str`,
/// which keys every discipline's description as `Psi<power_id>` and shows
/// the gaps at ids 0/8/16/24/32 are the five "Tier N Neural Capacity"
/// pseudo-disciplines - so `0x49` reads as {First Tier Neural Capacity,
/// Kinetic Redirection, Projected Cryokinesis}, a coherent psi starting
/// loadout. See `shock2vr::psi` for where the bits are interpreted.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPsiPowerLearned {
    pub bits: u32,
}

impl PropPsiPowerLearned {
    pub fn read<T: io::Read>(reader: &mut T, _len: u32) -> PropPsiPowerLearned {
        PropPsiPowerLearned {
            bits: read_u32(reader),
        }
    }
}

/// `P$PsiPower2` - the second learned-psi-powers dword (4 bytes), the
/// companion to [`PropPsiPowerLearned`] for power ids 32..=63 as bit
/// `power_id - 32` (ids run up to 39, so a single dword can't hold them
/// all). Authored `0x00000000` everywhere in the shipped data (only `The
/// Player` -384 carries it, in the gamesys).
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPsiPowerLearned2 {
    pub bits: u32,
}

impl PropPsiPowerLearned2 {
    pub fn read<T: io::Read>(reader: &mut T, _len: u32) -> PropPsiPowerLearned2 {
        PropPsiPowerLearned2 {
            bits: read_u32(reader),
        }
    }
}

/// `P$PsiShield` - duration formula for sustained psi powers, carried by
/// the same power meta-prop templates as [`PropPsiPower`] (12 bytes:
/// three ints). Despite the chunk name it is not shield-specific: the
/// active duration is `base + per_psi × PSI` seconds, matching the
/// published tables - e.g. Psychogenic Agility `[120, 60]` = "120 +
/// 60×PSI sec", Photonic Redirection (`Inviso`) `[5, 5]` = "5 + 5×PSI
/// sec".
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPsiShield {
    /// Base duration in seconds.
    pub duration_base: i32,
    /// Additional seconds per point of the player's PSI stat.
    pub duration_per_psi: i32,
    /// Observed 0 or 1; meaning not yet established.
    pub flags: i32,
}

impl PropPsiShield {
    pub fn read<T: io::Read>(reader: &mut T, _len: u32) -> PropPsiShield {
        PropPsiShield {
            duration_base: read_i32(reader),
            duration_per_psi: read_i32(reader),
            flags: read_i32(reader),
        }
    }
}

/// `P$PsiState` - the player's psi point pool, carried by `The Player`
/// template (-384) in the gamesys (12 bytes: three ints, observed
/// `[40, 50, 50]`). The first two are the current and maximum psi
/// points; the third's meaning is not yet established.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPsiState {
    /// Current psi points.
    pub psi_points: i32,
    /// Maximum psi points.
    pub max_psi_points: i32,
    /// Observed 50 on `The Player`; meaning not yet established.
    pub unknown: i32,
}

impl PropPsiState {
    pub fn read<T: io::Read>(reader: &mut T, _len: u32) -> PropPsiState {
        PropPsiState {
            psi_points: read_i32(reader),
            max_psi_points: read_i32(reader),
            unknown: read_i32(reader),
        }
    }
}
