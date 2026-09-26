//! Session-owned developer navigation, shared by the frontend and pause hosts.
use std::sync::{Arc, Mutex};

use crate::dev_params::{self, DevCategory, DevParamId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DevParamsLocation {
    pub category: Option<DevCategory>,
    pub locked: bool,
}

impl DevParamsLocation {
    pub fn category(self) -> DevCategory {
        self.category.unwrap_or(DevCategory::Root)
    }

    fn index(self) -> usize {
        self.category() as usize + usize::from(self.locked) * DevCategory::ALL.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevParamsRow {
    Category(DevParamsLocation),
    Parameter(DevParamId),
    Bulk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevParamsNavigation {
    pub location: DevParamsLocation,
    scrolls: [usize; DevCategory::ALL.len() * 2],
}

impl Default for DevParamsNavigation {
    fn default() -> Self {
        Self {
            location: DevParamsLocation::default(),
            scrolls: [0; DevCategory::ALL.len() * 2],
        }
    }
}

/// Owned by Game's persistent pause menu; frontend scenes clone this handle.
/// No static state: a fresh Game starts at the category root.
pub type DevParamsSession = Arc<Mutex<DevParamsNavigation>>;

impl DevParamsNavigation {
    pub fn scroll(&self) -> usize {
        self.scrolls[self.location.index()]
    }
    pub fn scroll_mut(&mut self) -> &mut usize {
        &mut self.scrolls[self.location.index()]
    }

    pub fn enter(&mut self, location: DevParamsLocation) {
        self.location = location;
    }

    /// False at the ordinary root means Back should leave the developer host.
    pub fn back(&mut self) -> bool {
        if let Some(parent) = self.location.category().parent() {
            self.location.category = Some(parent);
        } else if self.location.locked {
            self.location = DevParamsLocation::default();
        } else {
            return false;
        }
        true
    }

    pub fn params_under(&self, category: DevCategory) -> impl Iterator<Item = DevParamId> + '_ {
        dev_params::all()
            .filter(move |(_, p)| p.locked == self.location.locked && category.contains(p.category))
            .map(|(id, _)| id)
    }

    pub fn bulk_members(&self) -> Vec<DevParamId> {
        if DevCategory::Visualizations.contains(self.location.category()) {
            self.params_under(self.location.category()).collect()
        } else {
            Vec::new()
        }
    }

    pub fn rows(&self) -> Vec<DevParamsRow> {
        let category = self.location.category();
        let mut rows = Vec::new();
        if !self.bulk_members().is_empty() {
            rows.push(DevParamsRow::Bulk);
        }
        for child in DevCategory::ALL {
            if child.parent() == Some(category) && self.params_under(child).next().is_some() {
                rows.push(DevParamsRow::Category(DevParamsLocation {
                    category: Some(child),
                    locked: self.location.locked,
                }));
            }
        }
        rows.extend(
            dev_params::all()
                .filter(|(_, p)| p.category == category && p.locked == self.location.locked)
                .map(|(id, _)| DevParamsRow::Parameter(id)),
        );
        if category == DevCategory::Root
            && !self.location.locked
            && dev_params::all().any(|(_, p)| p.locked)
        {
            rows.push(DevParamsRow::Category(DevParamsLocation {
                category: None,
                locked: true,
            }));
        }
        rows
    }

    pub fn breadcrumb(&self) -> String {
        let mut labels = Vec::new();
        let mut category = self.location.category();
        while category != DevCategory::Root {
            labels.push(category.label());
            category = category.parent().unwrap();
        }
        if self.location.locked {
            labels.push("Locked");
        }
        labels.push("Developer");
        labels.reverse();
        labels.join(" > ")
    }
}
