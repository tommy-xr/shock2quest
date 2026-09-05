use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{read_i32, read_string_with_size, read_u32};

const TECH_SKILL_COUNT: usize = 5;
const MAX_RESEARCH_CHEMICALS: usize = 7;

/// Dark's `sTechSkills`: hack, repair, modify, maintenance, research.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TechSkillValues(pub [i32; TECH_SKILL_COUNT]);

impl TechSkillValues {
    fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 20, "a tech-skill record must contain five i32 values");
        Self(std::array::from_fn(|_| read_i32(reader)))
    }

    pub fn repair(self) -> i32 {
        self.0[1]
    }

    pub fn modify(self) -> i32 {
        self.0[2]
    }

    pub fn maintenance(self) -> i32 {
        self.0[3]
    }

    pub fn research(self) -> i32 {
        self.0[4]
    }
}

/// An object's base tech values. Research uses the research field as its
/// required skill, defaulting to one when this property is absent.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropBaseTechDesc(pub TechSkillValues);

impl PropBaseTechDesc {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(TechSkillValues::read(reader, len))
    }
}

/// The separate required-tech record used when operating an object.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropRequiredTechDesc(pub TechSkillValues);

impl PropRequiredTechDesc {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(TechSkillValues::read(reader, len))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropResearchTime(pub i32);

impl PropResearchTime {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 4, "P$RsrchTime must contain one i32 second count");
        Self(read_i32(reader))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropResearchText(pub String);

impl PropResearchText {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(read_localized_string(reader, len))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropObjLookString(pub String);

impl PropObjLookString {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        Self(read_localized_string(reader, len))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropResearchReport(pub u32);

impl PropResearchReport {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 4, "P$RsrchRep must contain one 32-bit report mask");
        Self(read_u32(reader))
    }

    pub fn first_report_number(self) -> Option<u32> {
        (self.0 != 0).then(|| self.0.trailing_zeros() + 1)
    }
}

/// The ordered chemical gates for one researchable archetype. Thresholds are
/// expressed in authored research seconds, before the player's skill speedup.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropChemicalNeeded {
    pub chemicals: [String; MAX_RESEARCH_CHEMICALS],
    pub thresholds_secs: [i32; MAX_RESEARCH_CHEMICALS],
}

impl PropChemicalNeeded {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(
            len, 476,
            "P$ChemNeede must be the retail 7*64-byte labels + 7*i32 record"
        );
        let chemicals = std::array::from_fn(|_| read_string_with_size(reader, 64));
        let thresholds_secs = std::array::from_fn(|_| read_i32(reader));
        Self {
            chemicals,
            thresholds_secs,
        }
    }
}

fn read_localized_string<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> String {
    assert!(
        len >= 4,
        "localized strings include a four-byte length prefix"
    );
    let _stored_len = read_u32(reader);
    read_string_with_size(reader, len as usize - 4)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn parses_tech_skills_and_research_fields() {
        let bytes = [0_i32, 2, 3, 4, 5]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        let parsed = PropBaseTechDesc::read(&mut Cursor::new(bytes), 20);
        assert_eq!(parsed.0, TechSkillValues([0, 2, 3, 4, 5]));
        assert_eq!(parsed.0.research(), 5);

        assert_eq!(
            PropResearchTime::read(&mut Cursor::new(600_i32.to_le_bytes()), 4),
            PropResearchTime(600)
        );
        assert_eq!(
            PropResearchReport::read(&mut Cursor::new(0x10_u32.to_le_bytes()), 4)
                .first_report_number(),
            Some(5)
        );
    }

    #[test]
    fn parses_localized_string_record() {
        let value = b"AATText: \"Toxin research text\"\0";
        let mut bytes = (value.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(value);
        assert_eq!(
            PropResearchText::read(&mut Cursor::new(bytes.clone()), bytes.len() as u32).0,
            "AATText: \"Toxin research text\""
        );
    }

    #[test]
    fn parses_ordered_research_chemical_layout() {
        let mut bytes = Vec::new();
        for name in ["Chem #4", "Chem #2", "Chem #4", "", "", "", ""] {
            let mut field = name.as_bytes().to_vec();
            field.resize(64, 0);
            bytes.extend(field);
        }
        for threshold in [30_i32, 60, 240, 0, 0, 0, 0] {
            bytes.extend(threshold.to_le_bytes());
        }

        let parsed = PropChemicalNeeded::read(&mut Cursor::new(bytes), 476);
        assert_eq!(&parsed.chemicals[..3], ["Chem #4", "Chem #2", "Chem #4"]);
        assert_eq!(&parsed.thresholds_secs[..3], [30, 60, 240]);
    }
}
