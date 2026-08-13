//! Pure, self-contained port of the web battle sim `resolveBattle`
//! (`ekza-arena-web/src/domain/ekza/combat.ts`). BYTE-IDENTICAL by construction:
//! integer-only math, the same bounded 10-round loop, and the same FNV-1a
//! tie-break over the combatant identity encoded as its STANDARD base58 string
//! (design §1 — the identity is the ArenaSnapshot account pubkey, and the web
//! `CombatantSnapshot.mint` is that pubkey's base58).
//!
//! No Anchor / Solana imports here so the module is host-testable in plain
//! `cargo test`; the on-chain handler builds `Combatant`s from account data and
//! calls [`resolve_onchain`]. `alloc` (String/Vec) is used only inside the rare
//! base58 tie-break path — never for the per-strike hot loop.

// Canonical skill bitmask (design §2.6). bit N set => skill N active.
pub const SKILL_MOSS_SKIN: u8 = 1 << 0;
pub const SKILL_STONE_OATH: u8 = 1 << 1;
pub const SKILL_FIRE_OPENER: u8 = 1 << 2;
pub const SKILL_GLASS_CANNON: u8 = 1 << 3;
pub const SKILL_HEAVY_GUARD: u8 = 1 << 4;
pub const SKILL_QUICKSTEP: u8 = 1 << 5;
pub const SKILL_JEWELRY_FOCUS: u8 = 1 << 6;

// COMBAT_SKILL_VALUES (skills.ts:3), ported verbatim.
const MOSS_HEAL: i32 = 1;
const STONE_REFLECT: i32 = 1;
const FIRE_DAMAGE: i32 = 1;
const GLASS_ATTACK: i32 = 3;
const GLASS_HP_PENALTY: i32 = 3;
const GUARD_REDUCTION: i32 = 1;
const QUICKSTEP_INITIATIVE: i32 = 2;
const JEWELRY_MAX_MANA: i32 = 2;
const JEWELRY_MANA_RECOVERY: i32 = 1;

/// Engine-ready combatant: total stats (base + equipment folded) + skill mask +
/// identity (the 32 ArenaSnapshot pubkey bytes). Stats are `i32` for headroom;
/// on-chain they arrive as `i16` and are promoted.
#[derive(Clone, Copy)]
pub struct Combatant {
    pub identity: [u8; 32],
    pub hp: i32,
    pub attack: i32,
    pub armor: i32,
    pub speed: i32,
    pub skill_mask: u8,
}

#[inline]
fn has(mask: u8, bit: u8) -> bool {
    mask & bit != 0
}

// --- effective stats (combat.ts effectiveAttack/effectiveMaxHp/maxMana/effectiveInitiative) ---
#[inline]
fn eff_attack(c: &Combatant) -> i32 {
    c.attack
        + if has(c.skill_mask, SKILL_GLASS_CANNON) {
            GLASS_ATTACK
        } else {
            0
        }
}
#[inline]
fn eff_max_hp(c: &Combatant) -> i32 {
    (c.hp
        - if has(c.skill_mask, SKILL_GLASS_CANNON) {
            GLASS_HP_PENALTY
        } else {
            0
        })
    .max(1)
}
#[inline]
fn max_mana(c: &Combatant) -> i32 {
    (3 + c.speed).max(1)
        + if has(c.skill_mask, SKILL_JEWELRY_FOCUS) {
            JEWELRY_MAX_MANA
        } else {
            0
        }
}
#[inline]
fn eff_initiative(c: &Combatant) -> i32 {
    c.speed
        + if has(c.skill_mask, SKILL_QUICKSTEP) {
            QUICKSTEP_INITIATIVE
        } else {
            0
        }
}

/// One directed strike. Mirrors `strike()` (combat.ts:260). Returns
/// `(next_attacker_hp, next_defender_hp, next_attacker_mana)`; the defender's
/// mana is unchanged (as in the TS sim).
#[inline]
fn strike(
    attacker: &Combatant,
    defender: &Combatant,
    attacker_hp: i32,
    attacker_mana: i32,
    defender_hp: i32,
    fire_ready: bool,
) -> (i32, i32, i32) {
    let can_fire = fire_ready && attacker_mana >= 2;
    let mana_cost = if can_fire {
        2
    } else if attacker_mana >= 1 {
        1
    } else {
        0
    };
    let next_attacker_mana = (attacker_mana - mana_cost).max(0);
    let base_damage = (eff_attack(attacker) - defender.armor).max(1);
    let before_guard = (base_damage - if mana_cost == 0 { 1 } else { 0 }).max(1)
        + if can_fire { FIRE_DAMAGE } else { 0 };
    let damage = if has(defender.skill_mask, SKILL_HEAVY_GUARD) {
        (before_guard - GUARD_REDUCTION).max(1)
    } else {
        before_guard
    };
    let reflected = if has(defender.skill_mask, SKILL_STONE_OATH) {
        STONE_REFLECT
    } else {
        0
    };
    let next_defender_hp = (defender_hp - damage).max(0);
    let next_attacker_hp = (attacker_hp - reflected).max(0);
    (next_attacker_hp, next_defender_hp, next_attacker_mana)
}

/// End-of-round recovery (combat.ts:349). Returns `(hp, mana)`.
#[inline]
fn recover(c: &Combatant, hp: i32, mana: i32) -> (i32, i32) {
    let next_hp = if has(c.skill_mask, SKILL_MOSS_SKIN) {
        (hp + MOSS_HEAL).min(eff_max_hp(c))
    } else {
        hp
    };
    let next_mana = if has(c.skill_mask, SKILL_JEWELRY_FOCUS) {
        (mana + JEWELRY_MANA_RECOVERY).min(max_mana(c))
    } else {
        mana
    };
    (next_hp, next_mana)
}

/// Standard Bitcoin/Solana base58 encode (matches JS `bs58` and
/// `Pubkey::to_string`). Preserves leading zero bytes as `'1'`. Used only for
/// the FNV tie-break identity string.
fn base58_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let zeros = input.iter().take_while(|&&b| b == 0).count();
    let mut digits: Vec<u8> = Vec::with_capacity(input.len() * 138 / 100 + 1);
    for &byte in input {
        let mut carry = byte as u32;
        for d in digits.iter_mut() {
            carry += (*d as u32) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out = String::with_capacity(zeros + digits.len());
    for _ in 0..zeros {
        out.push('1');
    }
    for &d in digits.iter().rev() {
        out.push(ALPHABET[d as usize] as char);
    }
    out
}

/// FNV-1a 64-bit over `left ++ right`, seeded by `nonce`; true when the final
/// hash is even. Identical to `deterministicTieBreak` (combat.ts:497).
#[inline]
fn tie_break_even(concatenated: &str, nonce: u64) -> bool {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET ^ nonce;
    for byte in concatenated.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash % 2 == 0
}

/// Resolve the deterministic battle. `nonce` is the resolve seed. Returns
/// `(winner_is_a, rounds_played)` where `rounds_played` is the highest round
/// index in which a strike occurred (matches the web sim's max battle-log round).
pub fn resolve_onchain(a: &Combatant, b: &Combatant, nonce: u64) -> (bool, u32) {
    // base58 of both identities computed once and reused across rounds (the FNV
    // path is otherwise the only allocator in this function).
    let a58 = base58_encode(&a.identity);
    let b58 = base58_encode(&b.identity);

    let mut a_hp = eff_max_hp(a);
    let mut b_hp = eff_max_hp(b);
    let mut a_mana = max_mana(a);
    let mut b_mana = max_mana(b);
    let mut a_fire = has(a.skill_mask, SKILL_FIRE_OPENER);
    let mut b_fire = has(b.skill_mask, SKILL_FIRE_OPENER);
    let a_init = eff_initiative(a);
    let b_init = eff_initiative(b);

    let mut rounds: u32 = 0;
    for round in 1..=10u32 {
        rounds = round;

        // firstAttackerIsA (combat.ts:248): initiative, then FNV tie over
        // `a58 ++ b58 ++ ":" ++ round`.
        let a_goes_first = if a_init != b_init {
            a_init > b_init
        } else {
            let mut concat = String::with_capacity(a58.len() + b58.len() + 4);
            concat.push_str(&a58);
            concat.push_str(&b58);
            concat.push(':');
            concat.push_str(itoa(round).as_str());
            tie_break_even(&concat, nonce)
        };

        if a_goes_first {
            let (nah, nbh, nam) = strike(a, b, a_hp, a_mana, b_hp, a_fire);
            a_hp = nah;
            b_hp = nbh;
            a_mana = nam;
            a_fire = false;
            if a_hp <= 0 || b_hp <= 0 {
                break;
            }
            let (nbh2, nah2, nbm) = strike(b, a, b_hp, b_mana, a_hp, b_fire);
            b_hp = nbh2;
            a_hp = nah2;
            b_mana = nbm;
            b_fire = false;
        } else {
            let (nbh, nah, nbm) = strike(b, a, b_hp, b_mana, a_hp, b_fire);
            b_hp = nbh;
            a_hp = nah;
            b_mana = nbm;
            b_fire = false;
            if a_hp <= 0 || b_hp <= 0 {
                break;
            }
            let (nah2, nbh2, nam) = strike(a, b, a_hp, a_mana, b_hp, a_fire);
            a_hp = nah2;
            b_hp = nbh2;
            a_mana = nam;
            a_fire = false;
        }

        if a_hp <= 0 || b_hp <= 0 {
            break;
        }

        let (rah, ram) = recover(a, a_hp, a_mana);
        a_hp = rah;
        a_mana = ram;
        let (rbh, rbm) = recover(b, b_hp, b_mana);
        b_hp = rbh;
        b_mana = rbm;
    }

    // Winner rule (combat.ts:179): remaining HP -> effective ATK -> FNV over
    // `a58 ++ b58`.
    let a_wins = if a_hp != b_hp {
        a_hp > b_hp
    } else if eff_attack(a) != eff_attack(b) {
        eff_attack(a) > eff_attack(b)
    } else {
        let mut concat = String::with_capacity(a58.len() + b58.len());
        concat.push_str(&a58);
        concat.push_str(&b58);
        tie_break_even(&concat, nonce)
    };
    (a_wins, rounds)
}

/// Tiny allocation-free `u32` -> decimal for the round suffix (1..=10).
#[inline]
fn itoa(mut n: u32) -> ArrayStr {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    if n == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    ArrayStr { buf, start: i }
}

/// Stack-only string view for the small round decimal.
struct ArrayStr {
    buf: [u8; 10],
    start: usize,
}
impl ArrayStr {
    fn as_str(&self) -> &str {
        // SAFETY: buf[start..] contains only ASCII digits written by `itoa`.
        core::str::from_utf8(&self.buf[self.start..]).unwrap()
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Snap {
        hp: i32,
        attack: i32,
        armor: i32,
        speed: i32,
        #[serde(rename = "skillMask")]
        skill_mask: u8,
        identity: Vec<u8>,
    }

    #[derive(Deserialize)]
    struct Vector {
        note: String,
        #[serde(rename = "seedHex")]
        seed_hex: String,
        a: Snap,
        b: Snap,
        #[serde(rename = "expectedWinner")]
        expected_winner: String,
        #[serde(rename = "expectedRounds")]
        expected_rounds: u32,
    }

    #[derive(Deserialize)]
    struct Fixture {
        count: usize,
        vectors: Vec<Vector>,
    }

    fn combatant(s: &Snap) -> Combatant {
        let mut identity = [0u8; 32];
        identity.copy_from_slice(&s.identity);
        Combatant {
            identity,
            hp: s.hp,
            attack: s.attack,
            armor: s.armor,
            speed: s.speed,
            skill_mask: s.skill_mask,
        }
    }

    #[test]
    fn base58_matches_reference_vectors() {
        // Standard base58 sanity (leading-zero handling included).
        assert_eq!(base58_encode(&[0, 0, 0]), "111");
        assert_eq!(base58_encode(&[0]), "1");
        assert_eq!(base58_encode(&[]), "");
        assert_eq!(base58_encode(&[1, 0]), "5R");
        assert_eq!(base58_encode(&[0, 1, 0]), "15R");
    }

    #[test]
    fn reproduces_every_web_sim_vector() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/pvp-sim-vectors.json"
        );
        let raw =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read fixtures {path}: {e}"));
        let fixture: Fixture = serde_json::from_str(&raw).expect("parse fixtures");
        assert_eq!(fixture.count, fixture.vectors.len(), "count mismatch");
        assert!(
            fixture.vectors.len() >= 300,
            "expected a large vector suite"
        );

        let mut mismatches = 0usize;
        for v in &fixture.vectors {
            let a = combatant(&v.a);
            let b = combatant(&v.b);
            let nonce = u64::from_str_radix(&v.seed_hex, 16)
                .unwrap_or_else(|e| panic!("bad seedHex {}: {e}", v.seed_hex));
            let (winner_is_a, rounds) = resolve_onchain(&a, &b, nonce);
            let expected_a = v.expected_winner == "A";
            if winner_is_a != expected_a || rounds != v.expected_rounds {
                mismatches += 1;
                eprintln!(
                    "MISMATCH [{}] seed={} winner rust={} ts={} rounds rust={} ts={}",
                    v.note,
                    v.seed_hex,
                    if winner_is_a { "A" } else { "B" },
                    v.expected_winner,
                    rounds,
                    v.expected_rounds,
                );
            }
        }
        assert_eq!(
            mismatches, 0,
            "{mismatches} vector(s) diverged from the web sim"
        );
    }
}
