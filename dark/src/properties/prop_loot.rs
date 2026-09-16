use std::io::{self, SeekFrom};

use shipyard::Component;

use crate::ss2_common::{read_single, read_string_with_size, read_u32};

use serde::{Deserialize, Serialize};

/// Slots in a loot table, and the ceiling on how many draws it makes.
pub const MAX_LOOT_SLOTS: usize = 6;

/// `u32` pick count, six 64-byte names, six `u32` rarities, six `f32` values.
const RECORD_SIZE: usize = 4 + MAX_LOOT_SLOTS * (64 + 4 + 4);

/// One outcome in a creature's loot table.
#[derive(Debug, Component, Clone, PartialEq, Serialize, Deserialize)]
pub struct LootSlot {
    /// Archetype name of the item, resolved by symbolic name at generation
    /// time. Blank is a real outcome - the draw yields nothing.
    pub item: String,
    /// Relative weight of this slot against the other five. The shipped tables
    /// happen to sum to 100, but the draw normalizes by the actual total.
    pub rarity: u32,
    /// How desirable this outcome is. Only two shipped tables author it.
    pub value: f32,
}

/// `P$LootInfo` - the randomized table a creature's corpse is filled from.
///
/// On disk, 436 bytes in the order the chunk header's own record size implies:
/// `u32` draw count, six 64-byte item names, six `u32` rarities, six `f32`
/// values.
#[derive(Debug, Component, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PropLootInfo {
    /// Independent draws against the table, each of which may yield nothing.
    pub picks: u32,
    pub slots: Vec<LootSlot>,
}

impl PropLootInfo {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropLootInfo {
        let start = reader.stream_position().unwrap();
        // Every shipped record is the full fixed size. Reading a short one
        // field-by-field would run off the end of the chunk, so refuse it
        // whole rather than half-decoding it.
        if (len as usize) < RECORD_SIZE {
            reader.seek(SeekFrom::Start(start + len as u64)).unwrap();
            return PropLootInfo::default();
        }
        let picks = read_u32(reader);
        let items: Vec<String> = (0..MAX_LOOT_SLOTS)
            .map(|_| read_string_with_size(reader, 64))
            .collect();
        let rarities: Vec<u32> = (0..MAX_LOOT_SLOTS).map(|_| read_u32(reader)).collect();
        let values: Vec<f32> = (0..MAX_LOOT_SLOTS).map(|_| read_single(reader)).collect();
        reader.seek(SeekFrom::Start(start + len as u64)).unwrap();
        PropLootInfo {
            picks,
            slots: (0..MAX_LOOT_SLOTS)
                .map(|index| LootSlot {
                    item: items[index].clone(),
                    rarity: rarities[index],
                    value: values[index],
                })
                .collect(),
        }
    }

    /// Total weight across every slot - the denominator of a single draw.
    pub fn total_rarity(&self) -> u32 {
        self.slots.iter().map(|slot| slot.rarity).sum()
    }

    /// The slot a draw of `roll` (in `0..total_rarity()`) lands on.
    pub fn slot_for_roll(&self, roll: u32) -> Option<&LootSlot> {
        let mut running = 0;
        self.slots.iter().find(|slot| {
            running += slot.rarity;
            roll < running
        })
    }
}

/// `P$GuarLoot` - an item added on top of the table, but only for a player who
/// took the Cyber-Assimilation O/S upgrade.
#[derive(Debug, Component, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropGuaranteedLoot(pub String);

/// `P$RGuarLoot` - an item added on top of the table unconditionally.
#[derive(Debug, Component, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropReallyGuaranteedLoot(pub String);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn record(picks: u32, items: &[&str], rarities: [u32; 6], values: Option<[f32; 6]>) -> Vec<u8> {
        let mut bytes = picks.to_le_bytes().to_vec();
        for index in 0..MAX_LOOT_SLOTS {
            let mut name = [0u8; 64];
            if let Some(item) = items.get(index) {
                name[..item.len()].copy_from_slice(item.as_bytes());
            }
            bytes.extend(name);
        }
        bytes.extend(rarities.iter().flat_map(|r| r.to_le_bytes()));
        if let Some(values) = values {
            bytes.extend(values.iter().flat_map(|v| v.to_le_bytes()));
        }
        bytes
    }

    /// The shipped OG-Pipe table, in its on-disk order.
    #[test]
    fn parses_a_full_record_and_consumes_it() {
        let bytes = record(
            2,
            &["5 Nanites", "Med Patch", "Soda Can", "", "OG Organ", ""],
            [20, 5, 25, 45, 5, 0],
            Some([2.0, 2.0, 2.0, 0.0, 0.0, 0.0]),
        );
        assert_eq!(bytes.len(), 436);
        let mut reader = Cursor::new(bytes);
        let prop = PropLootInfo::read(&mut reader, 436);
        assert_eq!(reader.position(), 436);
        assert_eq!(prop.picks, 2);
        assert_eq!(prop.slots[0].item, "5 Nanites");
        assert_eq!(prop.slots[0].rarity, 20);
        assert_eq!(prop.slots[0].value, 2.0);
        // A blank name is a real slot: the 45-weight "nothing" outcome.
        assert_eq!(prop.slots[3].item, "");
        assert_eq!(prop.slots[3].rarity, 45);
        assert_eq!(prop.total_rarity(), 100);
    }

    /// A record shorter than the fixed size is refused whole - reading it
    /// field-by-field would run past the end of the chunk.
    #[test]
    fn refuses_a_short_record_without_reading_past_it() {
        let bytes = record(1, &["Arach. Organ"], [20, 80, 0, 0, 0, 0], None);
        assert_eq!(bytes.len(), 412);
        let mut reader = Cursor::new(bytes);
        let prop = PropLootInfo::read(&mut reader, 412);
        assert_eq!(reader.position(), 412);
        assert_eq!(prop, PropLootInfo::default());
        assert_eq!(prop.total_rarity(), 0);
    }

    /// Weights are cumulative across the slots in order, so each roll in
    /// `0..total` maps to exactly one outcome.
    #[test]
    fn a_roll_selects_the_slot_its_weight_covers() {
        let prop = PropLootInfo {
            picks: 1,
            slots: vec![
                LootSlot {
                    item: "a".into(),
                    rarity: 20,
                    value: 0.0,
                },
                LootSlot {
                    item: "".into(),
                    rarity: 30,
                    value: 0.0,
                },
                LootSlot {
                    item: "c".into(),
                    rarity: 50,
                    value: 0.0,
                },
            ],
        };
        assert_eq!(prop.slot_for_roll(0).unwrap().item, "a");
        assert_eq!(prop.slot_for_roll(19).unwrap().item, "a");
        assert_eq!(prop.slot_for_roll(20).unwrap().item, "");
        assert_eq!(prop.slot_for_roll(49).unwrap().item, "");
        assert_eq!(prop.slot_for_roll(50).unwrap().item, "c");
        assert_eq!(prop.slot_for_roll(99).unwrap().item, "c");
        assert!(prop.slot_for_roll(100).is_none());
    }
}
