# PvP Ladder MVP — Build Notes (interruption-safe progress log)

Design source: `docs/pvp-ladder-design.md` (LOCKED build decisions at bottom).
Program: `programs/arena-leaderboard`. Devnet only. No deploy. No commit.

## Plan / status

- [x] Study `combat.ts` + `skills.ts` + `types.ts` (TS sim ground truth).
- [x] Study leaderboard program (state/handlers/contexts/constants/error).
- [x] Study mint commit/reveal + splitmix64 + SlotHashes pattern in `solana-ekza-arena`.
- [x] SIM PARITY: TS vector generator `ekza-arena-web/scripts/gen-pvp-sim-vectors.ts`.
- [x] SIM PARITY: shared fixtures `tests/fixtures/pvp-sim-vectors.json`.
- [x] SIM PARITY: Rust port `programs/arena-leaderboard/src/sim.rs` + cargo vector test.
      RESULT: 336/336 vectors reproduce winner AND rounds. PARITY LOCKED.
- [x] PDAs: ArenaSnapshot / Challenge / CharRecord / PairCooldown in `state.rs`.
- [x] Instructions: publish/unpublish snapshot, commit/resolve/close challenge.
- [x] Scaled elo (opponent-scaled, near-zero-sum) + PairCooldown + min-games gate.
- [x] Anchor E2E `tests/arena-pvp.ts`.
- [x] cargo fmt / clippy / anchor build / anchor test.

## Key parity facts (the single correctness gotcha)

- FNV-1a tie-break identity = the **ArenaSnapshot account pubkey**, fed to FNV
  as its **standard base58 string** (matches TS `CombatantSnapshot.mint` which is
  base58). Rust `sim.rs` implements standard base58 (Bitcoin alphabet, leading-zero
  `1` handling) so on-chain == host-test == TS byte-for-byte.
- FNV-1a: `hash = 0xcbf29ce484222325 ^ nonce`; per byte `hash ^= b; hash *= 0x100000001b3`
  (wrapping u64); winner-tie true => A wins. firstAttacker tie hashes
  `a58 ++ b58 ++ ":" ++ round`; final tie hashes `a58 ++ b58`.
- `nonce` fed to the sim == the resolve seed. Seed derivation (E2E only):
  `splitmix64_mix(slothash(target_slot) ^ first8(challenger) ^ first8(opp_snapshot) ^ commit_nonce)`.
- Skill bitmask order: 0 moss_skin, 1 stone_oath, 2 fire_opener, 3 glass_cannon,
  4 heavy_guard, 5 quickstep, 6 jewelry_focus (bit 7 reserved).
- COMBAT_SKILL_VALUES baked in: mossHeal 1, stoneReflect 1, fireDamage 1,
  glassAttack 3, glassHpPenalty 3, guardReduction 1, quickstepInitiative 2,
  jewelryMaxMana 2, jewelryManaRecovery 1.

## Tunables chosen (design §9 open questions)

- `PVP_ELO_K = 24`; expected score via integer logistic (21-entry 10^(d/400) table,
  scale 1e6, linear interp, symmetric for negative diff), diff clamped [-800,800].
- `MIN_RANKED_GAMES = 3` (heap/matchmaking gate on the PvP path).
- `PAIR_COOLDOWN_SLOTS = 150`, `MAX_RANKED_PER_PAIR_PER_DAY = 5` (repeat pair => exhibition).
- `PVP_REVEAL_DELAY_SLOTS = 5`, `PVP_COMMIT_WINDOW_SLOTS = 128` (mirror mint).
- Exhibition (cooldown-exceeded) fights: update W/L + CharRecord, NO rating, NO heap.

## New PDAs (seeds -> fields)

- `ArenaSnapshot` ["arena_snapshot_v1", owner] : owner, avatar_ref, archetype_id[32],
  stats{hp,attack,armor,speed:i16}, skill_mask:u8, element:u8, skin_ref[32],
  rating_at_publish:i32, published_slot:u64, bump. init_if_needed (republish overwrites).
- `Challenge` ["challenge_v1", challenger, nonce_le8] : challenger, nonce, opponent_snapshot,
  target_slot, bump. init.
- `CharRecord` ["char_record_v1", owner, avatar_ref] : owner, avatar_ref, wins,losses,games:u32,
  streak,best_streak:u16, last_played_slot:u64, bump. init_if_needed.
- `PairCooldown` ["pair_cd_v1", key_lo, key_hi=sort(challenger,opp_owner)] : key_lo,key_hi,
  last_ranked_slot:u64, ranked_today:u16, utc_day:i64, bump. init_if_needed.

## New instruction signatures (lib.rs #[program])

- publish_snapshot(ctx, args: PublishSnapshotArgs{avatar_ref, archetype_id[32], stats,
  skill_mask, element, skin_ref[32], rating_at_publish})  — owner-signed.
- unpublish_snapshot(ctx)  — owner-signed, close=owner.
- commit_challenge(ctx, nonce: u64)  — challenger-signed; target_slot = slot +
  PVP_REVEAL_DELAY_SLOTS; rejects self-snapshot. (No args struct — see deviations.)
- resolve_challenge(ctx, nonce: u64, pair_lo: Pubkey, pair_hi: Pubkey)  — PERMISSIONLESS
  (payer any signer). ~13 accounts (challenge, challenger[unchecked], both snapshots,
  both PlayerStats, both CharRecord, pair_cooldown, leaderboard, slot_hashes, payer, sysprog).
  Boxed accounts to fit the BPF stack frame.
- close_expired_challenge(ctx, nonce: u64)  — permissionless (closer signs), close=challenger.

## Resolve trust flow (trustless re-resolution)

1. Reveal-window gate: now > target_slot AND now <= target_slot + PVP_COMMIT_WINDOW_SLOTS.
2. seed = splitmix64_mix(slothash(target_slot) ^ first8(challenger) ^
   first8(opp_snapshot) ^ commit_nonce).
3. winner = sim::resolve_onchain(challenger_snapshot, opponent_snapshot, seed) — identity =
   each ArenaSnapshot ACCOUNT pubkey. NOBODY submits a result; it is computed.
4. PairCooldown.consume_rated -> rated | exhibition.
5. Dual PlayerStats (games/W-L/streak; rating only if rated, opponent-scaled elo on
   PRE-fight ratings). Dual CharRecord (always). Heap upsert only if rated AND
   games >= MIN_RANKED_GAMES. Challenge closed (rent -> challenger).

Permissionless = anti-loss-dodge: a challenger cannot withhold a losing reveal because
any third party (defender / keeper) pushes the identical computed result.

## Elo formula (opponent-scaled, near-zero-sum)

expected_a = 1 / (1 + 10^((rating_b - rating_a)/400))  [fixed-point, 21-entry 10^(d/400)
table @1e6 + linear interp, diff clamped ±800]. delta = round(K*(score - expected)), K=24.
delta_b computed symmetrically -> |delta_a + delta_b| <= 1 (exactly 0 at equal ratings:
winner +12 / loser -12). Rating floored at RATING_FLOOR. Crushing a much weaker ghost pays
~0; losing to it costs a lot.

## Files touched

NEW: programs/arena-leaderboard/src/sim.rs; tests/fixtures/pvp-sim-vectors.json (336 vectors);
tests/pvp-sim.ts; tests/arena-pvp.ts; ekza-arena-web/scripts/gen-pvp-sim-vectors.ts.
EDITED (additive only): arena-leaderboard {constants,error,state,contexts,handlers,lib}.rs,
Cargo.toml (dev-deps serde/serde_json). PlayerStats/Leaderboard/record_battle layout UNCHANGED.

## Deviations from the design doc

- Seed formula: used the design §2.6 formula verbatim (splitmix64_mix over the 4-way xor).
- resolve_challenge takes (nonce, pair_lo, pair_hi) args: the sorted pair keys cannot be
  expressed in Anchor seed macros, so they are passed and validated on-chain against
  sort(challenger, opponent_owner) (InvalidPairKeys otherwise). Closest to existing patterns.
- ResolveChallenge accounts are Box-ed (5x init_if_needed overflowed the 4 KB BPF stack).
- PairCooldown "never rated" uses last_ranked_slot==0 sentinel (slot stored as slot.max(1)).
- Dropped the design's `CommitChallengeArgs` (it was an empty MVP struct): Anchor's TS coder
  mishandles zero-field struct instruction args -> InstructionDidNotDeserialize. commit_challenge
  now takes just `nonce: u64`; future flags can be added additively when they carry fields.
- UnpublishSnapshot.owner is `#[account(mut)]` (it receives the closed snapshot's rent).
</content>
