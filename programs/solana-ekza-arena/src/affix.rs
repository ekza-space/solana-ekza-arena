//! Deterministic, seed-driven affix generator — v2 "Inventory Hero" (spec §10).
//!
//! This module is intentionally free of any Anchor / Solana account
//! dependencies so it can be exercised with a plain `cargo test`. Both the
//! on-chain program and the TS frontend mirror (`affix.ts`) MUST roll
//! identically; this is the canonical reference implementation. Any change to
//! the pinned tables or the RNG-draw order is a breaking change to the shared
//! contract — bump the golden vector version.
//!
//! ## v2 dotune summary (spec §10)
//! Category decides WHICH stat, rarity/tier decide HOW MUCH:
//!   * 3 core stats — HP · ATK · DEF (DEF == the legacy `armor`). SPD is a minor
//!     secondary affix only, never a primary axis.
//!   * Every drop gets a GUARANTEED primary affix chosen by `base_type`
//!     (Weapon→FlatAtk, Armor→FlatArmor/DEF, Head→FlatHp, Charm→wildcard pick
//!     from the SECONDARY pool).
//!   * Rarity gates how many SECONDARY ("spicy") affixes pile on top
//!     (Common 0 / Rare 1 / Epic 2 / Legendary 3) drawn from the SECONDARY
//!     pool (elements / lifesteal / crit / minor speed).
//!
//! ## RNG-draw order (CONTRACT — the TS mirror must consume `next()` identically)
//! 1. `weighted_pick(RARITY_WEIGHTS)`                      -> 1 `next()` draw
//! 2. `tier = TIER_BY_RARITY[rarity]`                      -> no draw
//! 3. PRIMARY affix:
//!      * Weapon/Armor/Head: kind is fixed by `base_type`  -> no draw
//!      * Charm:             `weighted_pick(SECONDARY)`    -> 1 `next()` draw
//!      * value: `roll_range(lo*tier, hi*tier)`            -> 1 `next()` draw
//! 4. `secondary_count = SECONDARY_COUNT_BY_RARITY[rarity]`-> no draw
//! 5. for each secondary slot (0..secondary_count):
//!      * dedup loop: up to 4 attempts, each a
//!        `weighted_pick(SECONDARY)`                       -> 1 `next()` per attempt
//!        (stops at the first kind not already rolled; 4 dups in a row => skip)
//!      * on success: `roll_range(lo*tier, hi*tier)`       -> 1 `next()` draw
//! 6. `element` = first affix carrying an element, else None  (no draw)

// ---------------------------------------------------------------------------
// PRNG — splitmix64 (spec §4, UNCHANGED from v1)
// ---------------------------------------------------------------------------

/// splitmix64 increment (golden-ratio gamma).
pub const SPLITMIX64_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// The avalanche / finalizing mix of splitmix64 (no gamma add).
#[inline]
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Advance the generator and return the next 64-bit output (spec §4).
#[inline]
pub fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(SPLITMIX64_GAMMA);
    mix(*state)
}

/// Single-shot splitmix64 mix used for seed derivation (spec §3).
/// `splitmix64_mix(x)` == the standard splitmix64 applied to `x`.
#[inline]
pub fn splitmix64_mix(x: u64) -> u64 {
    mix(x.wrapping_add(SPLITMIX64_GAMMA))
}

/// `lo + next(state) % (hi - lo + 1)` (spec §4). `hi` is inclusive.
#[inline]
pub fn roll_range(state: &mut u64, lo: i64, hi: i64) -> i64 {
    debug_assert!(hi >= lo);
    let span = (hi - lo + 1) as u64;
    lo + (next(state) % span) as i64
}

/// Weighted index pick: `roll = next % sum(weights)`, then walk buckets.
#[inline]
fn weighted_pick(state: &mut u64, weights: &[u32]) -> usize {
    let total: u32 = weights.iter().copied().sum();
    let mut roll = (next(state) % total as u64) as u32;
    for (i, &w) in weights.iter().enumerate() {
        if roll < w {
            return i;
        }
        roll -= w;
    }
    weights.len() - 1
}

// ---------------------------------------------------------------------------
// Base type ids (skin ≠ stats: base_type drives mechanics, spec §2/§10.2)
// ---------------------------------------------------------------------------

pub const BASE_WEAPON: u8 = 0;
pub const BASE_HEAD: u8 = 1;
pub const BASE_ARMOR: u8 = 2;
pub const BASE_CHARM: u8 = 3;

// ---------------------------------------------------------------------------
// Element ids (mirror ArenaElement order: None, Fire, Ice, Poison, Holy)
// ---------------------------------------------------------------------------

pub const ELEM_NONE: u8 = 0;
pub const ELEM_FIRE: u8 = 1;
pub const ELEM_ICE: u8 = 2;
pub const ELEM_POISON: u8 = 3;

// ---------------------------------------------------------------------------
// Affix kind ids (order is part of the contract — Rust + TS must match)
// ---------------------------------------------------------------------------

pub const KIND_FLAT_HP: u8 = 0;
pub const KIND_FLAT_ATK: u8 = 1;
pub const KIND_FLAT_ARMOR: u8 = 2; // DEF
pub const KIND_FLAT_SPEED: u8 = 3;
pub const KIND_ELEMENT_FIRE: u8 = 4;
pub const KIND_ELEMENT_ICE: u8 = 5;
pub const KIND_ELEMENT_POISON: u8 = 6;
pub const KIND_LIFESTEAL: u8 = 7;
pub const KIND_CRIT: u8 = 8;

// ---------------------------------------------------------------------------
// Rarity (Common, Rare, Epic, Legendary) — spec §6/§10
// ---------------------------------------------------------------------------

pub const RARITY_COMMON: u8 = 0;
pub const RARITY_RARE: u8 = 1;
pub const RARITY_EPIC: u8 = 2;
pub const RARITY_LEGENDARY: u8 = 3;

// ---------------------------------------------------------------------------
// PINNED TABLES (THE CANON — the frontend copies these byte-identically)
// ---------------------------------------------------------------------------

/// RARITY_TABLE: Common 60 | Rare 28 | Epic 9 | Legendary 3 (weights, spec §6).
pub const RARITY_WEIGHTS: [u32; 4] = [60, 28, 9, 3];

/// TIER_BY_RARITY: Common 1 | Rare 2 | Epic 3 | Legendary 4 (spec §6).
/// Tier is deterministic — no jitter.
pub const TIER_BY_RARITY: [u8; 4] = [1, 2, 3, 4];

/// SECONDARY_COUNT_BY_RARITY (spec §10.3 step 4):
/// Common 0 | Rare 1 | Epic 2 | Legendary 3. Every item also carries exactly
/// one guaranteed PRIMARY affix on top of this count.
pub const SECONDARY_COUNT_BY_RARITY: [u8; 4] = [0, 1, 2, 3];

/// Max affixes an item may carry = 1 primary + 3 secondary (Legendary).
/// Matches the on-chain account cap (spec §7).
pub const MAX_AFFIXES: usize = 4;

// ---------------------------------------------------------------------------
// PRIMARY_RANGE — guaranteed primary affix value ranges per kind (spec §10.2/§10.3).
// HP rolls bigger than ATK/DEF (HP is a pool stat; ATK/DEF are per-hit).
// value = roll_range(lo * tier, hi * tier).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub lo: i16,
    pub hi: i16,
}

/// Head primary — HP (largest magnitude).
pub const PRIMARY_RANGE_HP: Range = Range { lo: 20, hi: 40 };
/// Weapon primary — ATK.
pub const PRIMARY_RANGE_ATK: Range = Range { lo: 8, hi: 16 };
/// Armor primary — DEF (== `armor` field).
pub const PRIMARY_RANGE_ARMOR: Range = Range { lo: 6, hi: 12 };

/// PRIMARY_RANGE lookup for the three deterministic primary kinds.
#[inline]
pub fn primary_range(kind: u8) -> Range {
    match kind {
        KIND_FLAT_HP => PRIMARY_RANGE_HP,
        KIND_FLAT_ATK => PRIMARY_RANGE_ATK,
        KIND_FLAT_ARMOR => PRIMARY_RANGE_ARMOR,
        // Charm's wildcard primary is drawn from the SECONDARY pool and uses
        // the SECONDARY def's range, not this table; this arm is unreachable
        // for the deterministic primaries.
        _ => Range { lo: 1, hi: 1 },
    }
}

/// Guaranteed primary affix KIND by base_type (spec §10.2).
/// `None` => Charm wildcard (weighted_pick from the SECONDARY pool).
#[inline]
pub fn primary_kind_for(base_type: u8) -> Option<u8> {
    match base_type {
        BASE_WEAPON => Some(KIND_FLAT_ATK),
        BASE_ARMOR => Some(KIND_FLAT_ARMOR),
        BASE_HEAD => Some(KIND_FLAT_HP),
        BASE_CHARM => None, // wildcard
        _ => Some(KIND_FLAT_HP),
    }
}

// ---------------------------------------------------------------------------
// SECONDARY pool (the "spicy" affixes) + SECONDARY_RANGE (spec §10.3).
// { kind, label, weight, lo, hi, element }. lo/hi ARE the SECONDARY_RANGE for
// that kind; value = roll_range(lo*tier, hi*tier). Elements use lo == hi so the
// value is fixed per-tier ("fixed for elements") while still consuming exactly
// one `next()` draw — keeping the RNG-draw order uniform with the flats.
// Keep byte-identical with the TS mirror (same order, weight, lo/hi, element).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct SecondaryDef {
    pub kind: u8,
    pub label: &'static str,
    pub weight: u32,
    pub lo: i16,
    pub hi: i16,
    pub element: u8,
}

pub const SECONDARY: [SecondaryDef; 6] = [
    SecondaryDef {
        kind: KIND_FLAT_SPEED,
        label: "Flat Speed",
        weight: 24,
        lo: 1,
        hi: 3,
        element: ELEM_NONE,
    },
    SecondaryDef {
        kind: KIND_ELEMENT_FIRE,
        label: "Fire Damage",
        weight: 14,
        lo: 5,
        hi: 5,
        element: ELEM_FIRE,
    },
    SecondaryDef {
        kind: KIND_ELEMENT_ICE,
        label: "Ice Damage",
        weight: 14,
        lo: 5,
        hi: 5,
        element: ELEM_ICE,
    },
    SecondaryDef {
        kind: KIND_ELEMENT_POISON,
        label: "Poison Damage",
        weight: 14,
        lo: 5,
        hi: 5,
        element: ELEM_POISON,
    },
    SecondaryDef {
        kind: KIND_LIFESTEAL,
        label: "Lifesteal",
        weight: 12,
        lo: 1,
        hi: 3,
        element: ELEM_NONE,
    },
    SecondaryDef {
        kind: KIND_CRIT,
        label: "Crit",
        weight: 10,
        lo: 2,
        hi: 4,
        element: ELEM_NONE,
    },
];

/// Pinned weights of the SECONDARY pool, in pool order (mirror of `SECONDARY`).
pub const SECONDARY_WEIGHTS: [u32; 6] = [24, 14, 14, 14, 12, 10];

// ---------------------------------------------------------------------------
// Rolled result (primitive, Anchor-free)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RolledAffix {
    pub kind: u8,
    pub value: i16,
    pub element: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RolledItem {
    pub base_type: u8,
    pub rarity: u8,
    pub tier: u8,
    pub affixes: Vec<RolledAffix>,
}

impl RolledItem {
    /// First affix carrying an element, else `ELEM_NONE` (spec §10.3 step 6).
    pub fn element(&self) -> u8 {
        self.affixes
            .iter()
            .map(|a| a.element)
            .find(|&e| e != ELEM_NONE)
            .unwrap_or(ELEM_NONE)
    }
}

/// Roll a full item from `seed` for the given `base_type` (spec §10.3).
///
/// The RNG-draw order documented at the top of this module is part of the
/// shared contract; do not reorder draws without bumping the golden vector.
pub fn roll_item(seed: u64, base_type: u8) -> RolledItem {
    let mut state = seed;

    // 1. rarity
    let rarity = weighted_pick(&mut state, &RARITY_WEIGHTS) as u8;
    // 2. tier (deterministic, no jitter)
    let tier = TIER_BY_RARITY[rarity as usize];
    let t = tier as i64;

    let mut affixes: Vec<RolledAffix> = Vec::with_capacity(MAX_AFFIXES);

    // 3. GUARANTEED primary affix (spec §10.2).
    match primary_kind_for(base_type) {
        Some(kind) => {
            // Weapon/Armor/Head: kind fixed, value from PRIMARY_RANGE.
            let r = primary_range(kind);
            let value = roll_range(&mut state, r.lo as i64 * t, r.hi as i64 * t) as i16;
            affixes.push(RolledAffix {
                kind,
                value,
                element: ELEM_NONE,
            });
        }
        None => {
            // Charm wildcard: weighted_pick from the SECONDARY pool, value from
            // that SECONDARY def's range.
            let idx = weighted_pick(&mut state, &SECONDARY_WEIGHTS);
            let def = &SECONDARY[idx];
            let value = roll_range(&mut state, def.lo as i64 * t, def.hi as i64 * t) as i16;
            affixes.push(RolledAffix {
                kind: def.kind,
                value,
                element: def.element,
            });
        }
    }

    // 4. secondary count by rarity
    let secondary_count = SECONDARY_COUNT_BY_RARITY[rarity as usize] as usize;

    // 5. SECONDARY affixes, dedup against ALL already-rolled kinds (incl primary).
    //    1 initial attempt + up to 3 re-rolls = 4 attempts total, else skip.
    for _ in 0..secondary_count {
        let mut chosen: Option<&SecondaryDef> = None;
        for _attempt in 0..4 {
            let idx = weighted_pick(&mut state, &SECONDARY_WEIGHTS);
            let def = &SECONDARY[idx];
            if !affixes.iter().any(|a| a.kind == def.kind) {
                chosen = Some(def);
                break;
            }
        }
        if let Some(def) = chosen {
            let value = roll_range(&mut state, def.lo as i64 * t, def.hi as i64 * t) as i16;
            affixes.push(RolledAffix {
                kind: def.kind,
                value,
                element: def.element,
            });
        }
        // else: dedup exhausted -> skip this slot
    }

    RolledItem {
        base_type,
        rarity,
        tier,
        affixes,
    }
}

// ---------------------------------------------------------------------------
// Human-readable names (for golden fixture / debugging; pure helpers)
// ---------------------------------------------------------------------------

pub fn base_type_name(base_type: u8) -> &'static str {
    match base_type {
        BASE_WEAPON => "Weapon",
        BASE_HEAD => "Head",
        BASE_ARMOR => "Armor",
        BASE_CHARM => "Charm",
        _ => "Unknown",
    }
}

pub fn rarity_name(rarity: u8) -> &'static str {
    match rarity {
        RARITY_COMMON => "Common",
        RARITY_RARE => "Rare",
        RARITY_EPIC => "Epic",
        RARITY_LEGENDARY => "Legendary",
        _ => "Unknown",
    }
}

pub fn element_name(element: u8) -> &'static str {
    match element {
        ELEM_NONE => "None",
        ELEM_FIRE => "Fire",
        ELEM_ICE => "Ice",
        ELEM_POISON => "Poison",
        _ => "Unknown",
    }
}

pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        KIND_FLAT_HP => "FlatHp",
        KIND_FLAT_ATK => "FlatAtk",
        KIND_FLAT_ARMOR => "FlatArmor",
        KIND_FLAT_SPEED => "FlatSpeed",
        KIND_ELEMENT_FIRE => "ElementFire",
        KIND_ELEMENT_ICE => "ElementIce",
        KIND_ELEMENT_POISON => "ElementPoison",
        KIND_LIFESTEAL => "Lifesteal",
        KIND_CRIT => "Crit",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_SEED: u64 = 0x1234_5678_9ABC_DEF0;
    const GOLDEN_PATH: &str =
        "/Users/wotori/git/notes/projects/Ekza/dev-artifacts/affix-golden-vectors.json";

    /// Recompute a secondary def by kind (test helper).
    fn secondary_def(kind: u8) -> Option<&'static SecondaryDef> {
        SECONDARY.iter().find(|d| d.kind == kind)
    }

    /// Expected value range [lo*tier, hi*tier] for an affix, given how it was
    /// rolled (primary-by-base_type vs. drawn-from-secondary).
    fn value_bounds(item: &RolledItem, idx: usize) -> (i64, i64) {
        let a = &item.affixes[idx];
        let t = item.tier as i64;
        // Primary slot (idx 0): for Weapon/Armor/Head the primary kind is fixed
        // and uses PRIMARY_RANGE; for Charm and all secondary slots the kind is
        // from the SECONDARY pool.
        if idx == 0 && primary_kind_for(item.base_type).is_some() {
            let r = primary_range(a.kind);
            (r.lo as i64 * t, r.hi as i64 * t)
        } else {
            let d = secondary_def(a.kind).expect("affix kind must be in SECONDARY pool");
            (d.lo as i64 * t, d.hi as i64 * t)
        }
    }

    #[test]
    fn determinism_same_seed_same_item() {
        for &seed in &[0u64, 1, 42, GOLDEN_SEED, u64::MAX, 0xDEAD_BEEF_CAFE_F00D] {
            for base in [BASE_WEAPON, BASE_HEAD, BASE_ARMOR, BASE_CHARM] {
                let a = roll_item(seed, base);
                let b = roll_item(seed, base);
                assert_eq!(a, b, "seed {seed:#x} base {base} not deterministic");
            }
        }
    }

    #[test]
    fn invariants_hold_across_seeds() {
        for seed in 0u64..4000 {
            for base in [BASE_WEAPON, BASE_HEAD, BASE_ARMOR, BASE_CHARM] {
                let item = roll_item(seed, base);

                // tier matches the table
                assert_eq!(item.tier, TIER_BY_RARITY[item.rarity as usize]);

                // exactly one guaranteed primary; secondary count gated by rarity.
                let want_secondary = SECONDARY_COUNT_BY_RARITY[item.rarity as usize] as usize;
                assert!(!item.affixes.is_empty(), "always a primary affix");
                assert!(
                    item.affixes.len() <= 1 + want_secondary,
                    "seed {seed} base {base}: too many affixes"
                );
                assert!(item.affixes.len() <= MAX_AFFIXES);

                // guaranteed primary kind by base_type (§10.2).
                match primary_kind_for(base) {
                    Some(kind) => assert_eq!(
                        item.affixes[0].kind, kind,
                        "seed {seed} base {base}: wrong primary kind"
                    ),
                    None => {
                        // Charm primary must come from the SECONDARY pool.
                        assert!(
                            secondary_def(item.affixes[0].kind).is_some(),
                            "seed {seed}: charm primary not from secondary pool"
                        );
                    }
                }

                // no duplicate affix kinds; values within scaled range.
                for (i, a) in item.affixes.iter().enumerate() {
                    for b2 in &item.affixes[i + 1..] {
                        assert_ne!(a.kind, b2.kind, "duplicate affix kind seed {seed}");
                    }
                    let (lo, hi) = value_bounds(&item, i);
                    assert!(
                        (a.value as i64) >= lo && (a.value as i64) <= hi,
                        "value {} out of [{lo},{hi}] for kind {} seed {seed}",
                        a.value,
                        a.kind
                    );
                }
            }
        }
    }

    #[test]
    fn weapon_always_has_flat_atk_primary() {
        for seed in 0u64..4000 {
            let item = roll_item(seed, BASE_WEAPON);
            assert_eq!(
                item.affixes[0].kind, KIND_FLAT_ATK,
                "weapon primary must be FlatAtk (seed {seed})"
            );
            // Common weapons are primary-only; rarer ones add spicy secondaries.
            if item.rarity == RARITY_COMMON {
                assert_eq!(item.affixes.len(), 1, "common = primary only (seed {seed})");
            }
        }
    }

    fn item_to_json(seed: u64, item: &RolledItem) -> String {
        let mut affixes = String::new();
        for (i, a) in item.affixes.iter().enumerate() {
            if i > 0 {
                affixes.push_str(",\n");
            }
            affixes.push_str(&format!(
                "    {{ \"kind\": \"{}\", \"id\": {}, \"value\": {}, \"element\": \"{}\" }}",
                kind_name(a.kind),
                a.kind,
                a.value,
                element_name(a.element)
            ));
        }
        format!(
            "{{\n  \"version\": 2,\n  \"seed\": \"{:#018x}\",\n  \"base_type\": \"{}\",\n  \"rarity\": \"{}\",\n  \"rarity_id\": {},\n  \"tier\": {},\n  \"element\": \"{}\",\n  \"affixes\": [\n{}\n  ]\n}}\n",
            seed,
            base_type_name(item.base_type),
            rarity_name(item.rarity),
            item.rarity,
            item.tier,
            element_name(item.element()),
            affixes
        )
    }

    #[test]
    fn golden_vector_v2_weapon() {
        let item = roll_item(GOLDEN_SEED, BASE_WEAPON);
        let json = item_to_json(GOLDEN_SEED, &item);

        // Write the shared v2 fixture the frontend track asserts against.
        std::fs::write(GOLDEN_PATH, &json).expect("write golden vector fixture");
        println!("golden vector v2 written to {GOLDEN_PATH}\n{json}");

        // Stable invariants on the golden vector.
        assert_eq!(item.base_type, BASE_WEAPON);
        assert_eq!(item.affixes[0].kind, KIND_FLAT_ATK, "weapon primary = FlatAtk");
        assert_eq!(item.tier, TIER_BY_RARITY[item.rarity as usize]);
        let want_secondary = SECONDARY_COUNT_BY_RARITY[item.rarity as usize] as usize;
        assert!(item.affixes.len() <= 1 + want_secondary);
        for (i, _a) in item.affixes.iter().enumerate() {
            let (lo, hi) = value_bounds(&item, i);
            assert!((item.affixes[i].value as i64) >= lo && (item.affixes[i].value as i64) <= hi);
        }
    }
}
