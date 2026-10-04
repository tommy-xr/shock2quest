//! Persistent choices for the new modification flow, separate from retail's
//! mutable descriptors. This foundation does not yet enroll live weapons.
//!
//! The installation caller supplies family eligibility and verifies ownership,
//! condition, skill and payment. Only the mission effect applier may commit the
//! returned state and consume resources; scripts must not mutate the world.
use dark::properties::GunSettingDesc;
use serde::{Deserialize, Serialize};
use shipyard::Component;

pub const MAX_UPGRADES: usize = 4;
pub const MAX_PAID_UPGRADES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponUpgrade {
    Flashlight,
    Laser,
    Silencer,
    LowMaintenanceI,
    LowMaintenanceII,
    AlternateFire,
    ExtendedCapacity,
}

impl WeaponUpgrade {
    pub const ALL: [Self; 7] = [
        Self::Flashlight,
        Self::Laser,
        Self::Silencer,
        Self::LowMaintenanceI,
        Self::LowMaintenanceII,
        Self::AlternateFire,
        Self::ExtendedCapacity,
    ];
}

/// Device use may buy any next tier; it never banks an extra paid slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeSource {
    Modify,
    Device,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeError {
    StaleTier,
    MaximumTier,
    RequiresDevice,
    Unsupported,
    AlreadyInstalled,
    RequiresLowMaintenanceI,
    NotInstalled,
}

impl std::fmt::Display for UpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::StaleTier => "The weapon's modification tier has changed.",
            Self::MaximumTier => "The weapon already has four modifications.",
            Self::RequiresDevice => "Further modifications require a French-Epstein device.",
            Self::Unsupported => "This upgrade is not supported by the weapon.",
            Self::AlreadyInstalled => "This upgrade is already installed.",
            Self::RequiresLowMaintenanceI => "Low Maintenance I is required first.",
            Self::NotInstalled => "This accessory is not installed.",
        })
    }
}

impl std::error::Error for UpgradeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponAccessory {
    Flashlight,
    Laser,
}

/// Owned by the individual weapon. Tier is derived, never separately stored.
/// Toggle preferences persist while holstered; the renderer decides whether
/// the weapon is currently equipped and may emit its effect.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedWeaponUpgrades")]
pub struct WeaponUpgrades {
    choices: Vec<WeaponUpgrade>,
    flashlight_enabled: bool,
    laser_enabled: bool,
}

impl WeaponUpgrades {
    pub fn choices(&self) -> &[WeaponUpgrade] {
        &self.choices
    }

    pub fn tier(&self) -> usize {
        self.choices.len()
    }

    pub fn has(&self, choice: WeaponUpgrade) -> bool {
        self.choices.contains(&choice)
    }

    /// Structural eligibility only: `supported` is the caller's weapon-family
    /// policy, not a way to bypass rank, duplication or source restrictions.
    pub fn can_install(
        &self,
        choice: WeaponUpgrade,
        supported: &[WeaponUpgrade],
        source: UpgradeSource,
        expected_tier: usize,
    ) -> Result<(), UpgradeError> {
        if self.tier() != expected_tier {
            return Err(UpgradeError::StaleTier);
        }
        if self.tier() >= MAX_UPGRADES {
            return Err(UpgradeError::MaximumTier);
        }
        if source == UpgradeSource::Modify && self.tier() >= MAX_PAID_UPGRADES {
            return Err(UpgradeError::RequiresDevice);
        }
        if !supported.contains(&choice) {
            return Err(UpgradeError::Unsupported);
        }
        if self.has(choice) {
            return Err(UpgradeError::AlreadyInstalled);
        }
        if choice == WeaponUpgrade::LowMaintenanceII && !self.has(WeaponUpgrade::LowMaintenanceI) {
            return Err(UpgradeError::RequiresLowMaintenanceI);
        }
        Ok(())
    }

    /// Returns a prospective state without modifying the source. Commit this
    /// only after all gameplay checks succeed, together with resource spending.
    pub fn with_upgrade(
        &self,
        choice: WeaponUpgrade,
        supported: &[WeaponUpgrade],
        source: UpgradeSource,
        expected_tier: usize,
    ) -> Result<Self, UpgradeError> {
        self.can_install(choice, supported, source, expected_tier)?;
        let mut next = self.clone();
        next.choices.push(choice);
        Ok(next)
    }

    pub fn damage_multiplier(&self) -> f32 {
        1.0 + 0.08 * self.tier() as f32
    }

    pub fn wear_multiplier(&self) -> f32 {
        let maintenance = if self.has(WeaponUpgrade::LowMaintenanceII) {
            0.50
        } else if self.has(WeaponUpgrade::LowMaintenanceI) {
            0.75
        } else {
            1.0
        };
        (1.0 - 0.05 * self.tier() as f32) * maintenance
    }

    /// Evaluate one authored setting. Always pass the original description,
    /// never a previously evaluated result or a retail-modified descriptor.
    /// No live ammunition, condition, reload timing or ammo usage is changed.
    /// Non-damaging weapons retain their original stimulus semantics until
    /// their family policy explicitly supports damage scaling.
    pub fn effective_setting(&self, base: &GunSettingDesc, scales_damage: bool) -> GunSettingDesc {
        let mut setting = base.clone();
        if scales_damage {
            setting.stim_modifier *= self.damage_multiplier();
        }
        if self.has(WeaponUpgrade::ExtendedCapacity) && setting.clip > 0 {
            setting.clip = setting.clip.saturating_mul(2);
        }
        setting
    }

    pub fn accessory_enabled(&self, accessory: WeaponAccessory) -> bool {
        match accessory {
            WeaponAccessory::Flashlight => self.flashlight_enabled,
            WeaponAccessory::Laser => self.laser_enabled,
        }
    }

    pub fn set_accessory_enabled(
        &mut self,
        accessory: WeaponAccessory,
        enabled: bool,
    ) -> Result<(), UpgradeError> {
        let (choice, value) = match accessory {
            WeaponAccessory::Flashlight => {
                (WeaponUpgrade::Flashlight, &mut self.flashlight_enabled)
            }
            WeaponAccessory::Laser => (WeaponUpgrade::Laser, &mut self.laser_enabled),
        };
        if !self.choices.contains(&choice) {
            return Err(UpgradeError::NotInstalled);
        }
        *value = enabled;
        Ok(())
    }
}

/// Keep the same invariants after loading as after installing. Family policy
/// is deliberately not serialized: changes to that policy must not erase an
/// already installed choice. This is same-build validation, not save migration.
#[derive(Deserialize)]
struct SavedWeaponUpgrades {
    choices: Vec<WeaponUpgrade>,
    flashlight_enabled: bool,
    laser_enabled: bool,
}

impl TryFrom<SavedWeaponUpgrades> for WeaponUpgrades {
    type Error = UpgradeError;

    fn try_from(saved: SavedWeaponUpgrades) -> Result<Self, Self::Error> {
        let mut state = Self::default();
        for choice in saved.choices {
            state = state.with_upgrade(
                choice,
                &WeaponUpgrade::ALL,
                UpgradeSource::Device,
                state.tier(),
            )?;
        }
        if saved.flashlight_enabled {
            state.set_accessory_enabled(WeaponAccessory::Flashlight, true)?;
        }
        if saved.laser_enabled {
            state.set_accessory_enabled(WeaponAccessory::Laser, true)?;
        }
        Ok(state)
    }
}

#[cfg(test)]
mod tests;
