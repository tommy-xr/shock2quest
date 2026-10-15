//! Supported gameplay data selections; remaster art layers are independent.

use dark::mission::identity::MissionIdentity;
use std::{io, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameMod {
    Scp,
}

impl FromStr for GameMod {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "scp" => Ok(Self::Scp),
            _ => Err(format!(
                "unsupported gameplay mod '{value}'; supported: scp"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Campaign {
    #[default]
    Base,
    Scp,
}

impl Campaign {
    pub fn from_mods(mods: &[GameMod]) -> Self {
        if mods.contains(&GameMod::Scp) {
            Self::Scp
        } else {
            Self::Base
        }
    }
    pub fn gamesys(self) -> &'static str {
        match self {
            Self::Base => "shock2.gam",
            Self::Scp => "shockscp.gam",
        }
    }
    /// Validate source fingerprints before reading geometry or applying saved entities.
    pub fn read_identity<T: io::Read + io::Seek>(
        self,
        mission: &str,
        reader: &mut T,
    ) -> io::Result<MissionIdentity> {
        reader.rewind()?;
        let toc = dark::ss2_chunk_file_reader::read_table_of_contents(reader);
        let identity = MissionIdentity::read(mission, &toc, reader)?;
        self.validate_mission(&identity)?;
        reader.rewind()?;
        Ok(identity)
    }
    pub fn validate_mission(self, identity: &MissionIdentity) -> io::Result<()> {
        if identity.gamesys.as_deref() != Some(self.gamesys())
            || (self == Self::Scp && !identity.matches_scp_patch(&identity.mission, self.gamesys()))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "campaign {self:?} requires {}{}; {} declares {:?}, scplevel={:?}",
                    self.gamesys(),
                    if self == Self::Scp {
                        " and scplevel=1"
                    } else {
                        ""
                    },
                    identity.mission,
                    identity.gamesys,
                    identity.quest_variables.get("scplevel")
                ),
            ));
        }
        Ok(())
    }
    pub fn validate_save(self, saved: Self) -> io::Result<()> {
        if self != saved {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("save uses {saved:?} campaign; current campaign is {self:?}"),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, shipyard::Unique, serde::Serialize)]
pub struct CampaignContext {
    pub campaign: Campaign,
    pub loaded_gamesys: String,
    pub mission: MissionIdentity,
}

impl CampaignContext {
    pub fn matches_scp_patch(&self, mission: &str) -> bool {
        self.campaign == Campaign::Scp
            && self
                .mission
                .matches_scp_patch(mission, &self.loaded_gamesys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn campaign_rejects_mixed_missions_and_saves() {
        let mut identity = MissionIdentity {
            mission: "medsci1.mis".into(),
            gamesys: Some("shock2.gam".into()),
            ..Default::default()
        };
        assert!(Campaign::Base.validate_mission(&identity).is_ok());
        assert!(Campaign::Scp.validate_mission(&identity).is_err());
        identity.gamesys = Some("shockscp.gam".into());
        assert!(Campaign::Scp.validate_mission(&identity).is_err());
        identity.quest_variables.insert("scplevel".into(), 1);
        assert!(Campaign::Scp.validate_mission(&identity).is_ok());
        assert!(Campaign::Base.validate_mission(&identity).is_err());
        assert!(Campaign::Base.validate_save(Campaign::Scp).is_err());
        assert!(Campaign::Scp.validate_save(Campaign::Base).is_err());
        assert!(Campaign::Scp.validate_save(Campaign::Scp).is_ok());
        assert!("unknown".parse::<GameMod>().is_err());
        assert_eq!(Campaign::from_mods(&[]), Campaign::Base);
        assert_eq!(Campaign::from_mods(&[GameMod::Scp]), Campaign::Scp);
    }
}
