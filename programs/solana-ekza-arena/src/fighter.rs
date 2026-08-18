//! Deterministic rolled-fighter generator.
//!
//! This is the Rust half of the web client's `rollFighter(seed, faction)`
//! contract.  The draw order is deliberately small and pinned:
//!
//! 1. rarity from the shared `[600, 280, 90, 29, 1]` ladder;
//! 2. HP variance in `0..=tier`;
//! 3. faction-primary variance in `0..=tier`;
//! 4. for Epic+, one power variance draw in `0..=1`.
//!
//! `reveal_avatar_mint` supplies commit-reveal SlotHashes entropy as `seed`.
//! The result is persisted in the mint-keyed `ArenaAssetData` PDA, so clients
//! read these values instead of trusting a symbol or re-rolling locally.

use crate::affix::{next, RARITY_WEIGHTS, TIER_BY_RARITY};

pub const FACTION_MOSS: u8 = 0;
pub const FACTION_SPARK: u8 = 1;
pub const FACTION_VOID: u8 = 2;
pub const FACTION_STONE: u8 = 3;

pub const FIGHTER_SYMBOL_MOSS: &str = "EKZAF0";
pub const FIGHTER_SYMBOL_SPARK: &str = "EKZAF1";
pub const FIGHTER_SYMBOL_VOID: &str = "EKZAF2";
pub const FIGHTER_SYMBOL_STONE: &str = "EKZAF3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FighterStats {
    pub hp: i16,
    pub attack: i16,
    pub armor: i16,
    pub speed: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RolledFighter {
    pub stats: FighterStats,
    /// Shared affix-rarity id: Common=0, Rare=1, Epic=2, Legendary=3,
    /// Mythic=4.
    pub rarity: u8,
    pub tier: u8,
    pub skill_ids: Vec<&'static str>,
}

/// Parse only the four canonical, protocol-issued fighter symbols.  Prefix
/// matching is intentionally forbidden: `EKZAFTR`, `EKZAF10`, and strings with
/// padding are not authenticated fighter intents.
pub fn faction_from_symbol(symbol: &str) -> Option<u8> {
    match symbol {
        FIGHTER_SYMBOL_MOSS => Some(FACTION_MOSS),
        FIGHTER_SYMBOL_SPARK => Some(FACTION_SPARK),
        FIGHTER_SYMBOL_VOID => Some(FACTION_VOID),
        FIGHTER_SYMBOL_STONE => Some(FACTION_STONE),
        _ => None,
    }
}

pub fn symbol_for_faction(faction: u8) -> Option<&'static str> {
    match faction {
        FACTION_MOSS => Some(FIGHTER_SYMBOL_MOSS),
        FACTION_SPARK => Some(FIGHTER_SYMBOL_SPARK),
        FACTION_VOID => Some(FIGHTER_SYMBOL_VOID),
        FACTION_STONE => Some(FIGHTER_SYMBOL_STONE),
        _ => None,
    }
}

#[inline]
fn weighted_rarity(state: &mut u64) -> u8 {
    let total: u32 = RARITY_WEIGHTS.iter().copied().sum();
    let mut roll = (next(state) % u64::from(total)) as u32;
    for (index, weight) in RARITY_WEIGHTS.iter().copied().enumerate() {
        if roll < weight {
            return index as u8;
        }
        roll -= weight;
    }
    (RARITY_WEIGHTS.len() - 1) as u8
}

#[inline]
fn roll_range(state: &mut u64, lo: i16, hi: i16) -> i16 {
    debug_assert!(hi >= lo);
    if hi <= lo {
        return lo;
    }
    lo + (next(state) % (i64::from(hi) - i64::from(lo) + 1) as u64) as i16
}

/// Roll a fighter from commit-reveal entropy and a canonical faction id.
///
/// Callers must pass one of `FACTION_*`; an invalid id returns `None` rather
/// than silently falling back to a balance profile.
pub fn roll_fighter(seed: u64, faction: u8) -> Option<RolledFighter> {
    let (mut stats, primary, default_skill, bonus_skill) = match faction {
        FACTION_MOSS => (
            FighterStats {
                hp: 11,
                attack: 1,
                armor: 1,
                speed: 0,
            },
            FACTION_MOSS,
            "moss_skin",
            "heavy_guard",
        ),
        FACTION_SPARK => (
            FighterStats {
                hp: 9,
                attack: 3,
                armor: 0,
                speed: 1,
            },
            FACTION_SPARK,
            "fire_opener",
            "glass_cannon",
        ),
        FACTION_VOID => (
            FighterStats {
                hp: 9,
                attack: 2,
                armor: 0,
                speed: 2,
            },
            FACTION_VOID,
            "quickstep",
            "jewelry_focus",
        ),
        FACTION_STONE => (
            FighterStats {
                hp: 11,
                attack: 2,
                armor: 1,
                speed: 0,
            },
            FACTION_STONE,
            "stone_oath",
            "heavy_guard",
        ),
        _ => return None,
    };

    let mut state = seed;
    let rarity = weighted_rarity(&mut state);
    let tier = TIER_BY_RARITY[rarity as usize];
    let hp_bump = roll_range(&mut state, 0, i16::from(tier));
    let primary_bump = roll_range(&mut state, 0, i16::from(tier));
    let power_bump = if tier >= 3 {
        roll_range(&mut state, 0, 1)
    } else {
        0
    };

    stats.hp = stats.hp.saturating_add(hp_bump);
    stats.attack = stats.attack.saturating_add(power_bump);
    match primary {
        FACTION_MOSS => stats.hp = stats.hp.saturating_add(primary_bump),
        FACTION_SPARK => stats.attack = stats.attack.saturating_add(primary_bump),
        FACTION_VOID => stats.speed = stats.speed.saturating_add(primary_bump),
        FACTION_STONE => stats.armor = stats.armor.saturating_add(primary_bump),
        _ => unreachable!(),
    }

    let mut skill_ids = vec![default_skill];
    if rarity >= crate::affix::RARITY_EPIC {
        skill_ids.push(bonus_skill);
    }

    Some(RolledFighter {
        stats,
        rarity,
        tier,
        skill_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fighter_symbols_are_exact_and_round_trip() {
        for faction in FACTION_MOSS..=FACTION_STONE {
            let symbol = symbol_for_faction(faction).unwrap();
            assert_eq!(faction_from_symbol(symbol), Some(faction));
        }

        for spoof in ["EKZAF", "EKZAFTR", "EKZAF4", "EKZAF10", "EKZAF0 ", "ekzaf0"] {
            assert_eq!(faction_from_symbol(spoof), None, "accepted {spoof}");
        }
    }

    #[test]
    fn rolls_are_deterministic_and_faction_specific() {
        let seed = 0x0123_4567_89ab_cdef;
        let moss = roll_fighter(seed, FACTION_MOSS).unwrap();
        assert_eq!(moss, roll_fighter(seed, FACTION_MOSS).unwrap());
        assert_ne!(moss.stats, roll_fighter(seed, FACTION_SPARK).unwrap().stats);
        assert!(roll_fighter(seed, 4).is_none());
    }

    #[test]
    fn fixed_vectors_pin_the_web_parity_contract() {
        let cases = [
            (0, FACTION_MOSS),
            (1, FACTION_SPARK),
            (0x0123_4567_89ab_cdef, FACTION_VOID),
            (u64::MAX, FACTION_STONE),
        ];

        let actual: Vec<_> = cases
            .into_iter()
            .map(|(seed, faction)| roll_fighter(seed, faction).unwrap())
            .collect();

        // Exact expected values are deliberately explicit: changing a table or
        // RNG draw order is an ABI change for web/on-chain parity.
        assert_eq!(
            actual,
            vec![
                RolledFighter {
                    stats: FighterStats {
                        hp: 12,
                        attack: 1,
                        armor: 1,
                        speed: 0,
                    },
                    rarity: 0,
                    tier: 1,
                    skill_ids: vec!["moss_skin"],
                },
                RolledFighter {
                    stats: FighterStats {
                        hp: 10,
                        attack: 3,
                        armor: 0,
                        speed: 1,
                    },
                    rarity: 0,
                    tier: 1,
                    skill_ids: vec!["fire_opener"],
                },
                RolledFighter {
                    stats: FighterStats {
                        hp: 12,
                        attack: 2,
                        armor: 0,
                        speed: 4,
                    },
                    rarity: 2,
                    tier: 3,
                    skill_ids: vec!["quickstep", "jewelry_focus"],
                },
                RolledFighter {
                    stats: FighterStats {
                        hp: 12,
                        attack: 2,
                        armor: 2,
                        speed: 0,
                    },
                    rarity: 2,
                    tier: 3,
                    skill_ids: vec!["stone_oath", "heavy_guard"],
                },
            ]
        );
    }

    #[test]
    fn all_factions_stay_in_the_balanced_stat_band() {
        for faction in FACTION_MOSS..=FACTION_STONE {
            for seed in 0..10_000u64 {
                let roll = roll_fighter(seed, faction).unwrap();
                assert!((1..=5).contains(&roll.tier));
                assert!(roll.stats.hp >= 9 && roll.stats.hp <= 21);
                assert!(roll.stats.attack >= 1 && roll.stats.attack <= 9);
                assert!(roll.stats.armor >= 0 && roll.stats.armor <= 6);
                assert!(roll.stats.speed >= 0 && roll.stats.speed <= 7);
                assert!(!roll.skill_ids.is_empty() && roll.skill_ids.len() <= 2);
            }
        }
    }
}
