/**
 * pvp-sim.ts — TypeScript mirror of the on-chain PvP battle sim + seed
 * derivation, used by `arena-pvp.ts` to independently predict the winner of a
 * resolved challenge and assert the chain agrees.
 *
 * This is a faithful port of the web sim `resolveBattle` (combat.ts). Its
 * correctness against the WEB sim is asserted in `arena-pvp.ts` by replaying the
 * shared parity fixtures (`tests/fixtures/pvp-sim-vectors.json`) through it — so
 * the trust chain is: web combat.ts == fixtures == Rust sim (cargo) and
 * fixtures == this helper == on-chain resolve (E2E).
 *
 * Identity fed to the FNV-1a tie-break is the ArenaSnapshot account pubkey as
 * its standard base58 string (design §1) — `PublicKey.toBase58()`.
 */
import { PublicKey } from "@solana/web3.js";

const MASK64 = (1n << 64n) - 1n;

export const SKILL = {
  MOSS_SKIN: 1 << 0,
  STONE_OATH: 1 << 1,
  FIRE_OPENER: 1 << 2,
  GLASS_CANNON: 1 << 3,
  HEAVY_GUARD: 1 << 4,
  QUICKSTEP: 1 << 5,
  JEWELRY_FOCUS: 1 << 6,
} as const;

export type SimSnap = {
  identityBase58: string; // ArenaSnapshot pubkey, base58
  hp: number;
  attack: number;
  armor: number;
  speed: number;
  skillMask: number;
};

const has = (m: number, bit: number) => (m & bit) !== 0;
const effAttack = (c: SimSnap) =>
  c.attack + (has(c.skillMask, SKILL.GLASS_CANNON) ? 3 : 0);
const effMaxHp = (c: SimSnap) =>
  Math.max(1, c.hp - (has(c.skillMask, SKILL.GLASS_CANNON) ? 3 : 0));
const maxMana = (c: SimSnap) =>
  Math.max(1, 3 + c.speed) + (has(c.skillMask, SKILL.JEWELRY_FOCUS) ? 2 : 0);
const effInit = (c: SimSnap) =>
  c.speed + (has(c.skillMask, SKILL.QUICKSTEP) ? 2 : 0);

function tieBreakEven(concat: string, nonce: bigint): boolean {
  let hash = (0xcbf29ce484222325n ^ nonce) & MASK64;
  const prime = 0x00000100000001b3n;
  for (const byte of new TextEncoder().encode(concat)) {
    hash ^= BigInt(byte);
    hash = (hash * prime) & MASK64;
  }
  return hash % 2n === 0n;
}

function strike(
  attacker: SimSnap,
  defender: SimSnap,
  attackerHp: number,
  attackerMana: number,
  defenderHp: number,
  fireReady: boolean
) {
  const canFire = fireReady && attackerMana >= 2;
  const manaCost = canFire ? 2 : attackerMana >= 1 ? 1 : 0;
  const nextAttackerMana = Math.max(0, attackerMana - manaCost);
  const baseDamage = Math.max(1, effAttack(attacker) - defender.armor);
  const beforeGuard =
    Math.max(1, baseDamage - (manaCost === 0 ? 1 : 0)) + (canFire ? 1 : 0);
  const damage = has(defender.skillMask, SKILL.HEAVY_GUARD)
    ? Math.max(1, beforeGuard - 1)
    : beforeGuard;
  const reflected = has(defender.skillMask, SKILL.STONE_OATH) ? 1 : 0;
  return {
    attHp: Math.max(0, attackerHp - reflected),
    defHp: Math.max(0, defenderHp - damage),
    attMana: nextAttackerMana,
  };
}

function recover(c: SimSnap, hp: number, mana: number) {
  return {
    hp: has(c.skillMask, SKILL.MOSS_SKIN) ? Math.min(effMaxHp(c), hp + 1) : hp,
    mana: has(c.skillMask, SKILL.JEWELRY_FOCUS)
      ? Math.min(maxMana(c), mana + 1)
      : mana,
  };
}

export function resolveWinner(
  a: SimSnap,
  b: SimSnap,
  nonce: bigint
): { winnerIsA: boolean; rounds: number } {
  const a58 = a.identityBase58;
  const b58 = b.identityBase58;
  let aHp = effMaxHp(a);
  let bHp = effMaxHp(b);
  let aMana = maxMana(a);
  let bMana = maxMana(b);
  let aFire = has(a.skillMask, SKILL.FIRE_OPENER);
  let bFire = has(b.skillMask, SKILL.FIRE_OPENER);
  const aInit = effInit(a);
  const bInit = effInit(b);
  let rounds = 0;
  for (let round = 1; round <= 10; round += 1) {
    rounds = round;
    const aFirst =
      aInit !== bInit
        ? aInit > bInit
        : tieBreakEven(`${a58}${b58}:${round}`, nonce);
    if (aFirst) {
      let s = strike(a, b, aHp, aMana, bHp, aFire);
      aHp = s.attHp;
      bHp = s.defHp;
      aMana = s.attMana;
      aFire = false;
      if (aHp <= 0 || bHp <= 0) break;
      s = strike(b, a, bHp, bMana, aHp, bFire);
      bHp = s.attHp;
      aHp = s.defHp;
      bMana = s.attMana;
      bFire = false;
    } else {
      let s = strike(b, a, bHp, bMana, aHp, bFire);
      bHp = s.attHp;
      aHp = s.defHp;
      bMana = s.attMana;
      bFire = false;
      if (aHp <= 0 || bHp <= 0) break;
      s = strike(a, b, aHp, aMana, bHp, aFire);
      aHp = s.attHp;
      bHp = s.defHp;
      aMana = s.attMana;
      aFire = false;
    }
    if (aHp <= 0 || bHp <= 0) break;
    const ra = recover(a, aHp, aMana);
    aHp = ra.hp;
    aMana = ra.mana;
    const rb = recover(b, bHp, bMana);
    bHp = rb.hp;
    bMana = rb.mana;
  }
  const winnerIsA =
    aHp !== bHp
      ? aHp > bHp
      : effAttack(a) !== effAttack(b)
      ? effAttack(a) > effAttack(b)
      : tieBreakEven(`${a58}${b58}`, nonce);
  return { winnerIsA, rounds };
}

// --- seed derivation (mirror handlers.rs / affix.rs splitmix64) -------------
const GAMMA = 0x9e3779b97f4a7c15n;
export function splitmix64Mix(x: bigint): bigint {
  let z = (x + GAMMA) & MASK64;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK64;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK64;
  return (z ^ (z >> 31n)) & MASK64;
}
export function first8(pk: PublicKey): bigint {
  return pk.toBuffer().readBigUInt64LE(0);
}
export function deriveSeed(
  slothash: bigint,
  challenger: PublicKey,
  opponentSnapshot: PublicKey,
  commitNonce: bigint
): bigint {
  return splitmix64Mix(
    (slothash ^ first8(challenger) ^ first8(opponentSnapshot) ^ commitNonce) &
      MASK64
  );
}
