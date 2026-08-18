use anchor_lang::prelude::*;

use crate::{
    constants::{
        MAX_CAPACITY, MIN_CAPACITY, MIN_RANKED_GAMES, PVP_COMMIT_WINDOW_SLOTS, PVP_ELO_K,
        PVP_REVEAL_DELAY_SLOTS, RATING_FLOOR, RATING_LOSS_VS_BOT, RATING_LOSS_VS_PLAYER,
        RATING_WIN_VS_BOT, RATING_WIN_VS_PLAYER, STARTING_RATING,
    },
    contexts::{
        CloseExpiredChallenge, CommitChallenge, InitLeaderboard, PublishSnapshot, RecordBattle,
        RegisterSessionKey, ResolveChallenge, RevokeSessionKey, SetProfile, UnpublishSnapshot,
    },
    error::LeaderboardError,
    state::{ArenaStatsLite, BattleRateLimit, PairCooldown, PlayerStats},
};

/// A `PlayerStats` fresh out of `init_if_needed` is all-zero; recognize it by
/// the unset `player` field and seed the defaults (rating starts at 1000).
fn ensure_stats_initialized(stats: &mut PlayerStats, player: Pubkey, bump: u8) {
    if stats.player == Pubkey::default() {
        stats.player = player;
        stats.rating = STARTING_RATING;
        stats.bump = bump;
    }
}

fn ensure_rate_limit_initialized(rate_limit: &mut BattleRateLimit, player: Pubkey, bump: u8) {
    rate_limit.initialize_if_needed(player, bump);
}

#[inline]
fn apply_rating_delta(rating: i32, delta: i32) -> i32 {
    rating.saturating_add(delta).max(RATING_FLOOR)
}

pub fn init_leaderboard(ctx: Context<InitLeaderboard>, capacity: u16) -> Result<()> {
    require!(
        capacity >= MIN_CAPACITY && capacity as usize <= MAX_CAPACITY,
        LeaderboardError::CapacityOutOfRange
    );

    // `zero` guarantees a brand-new zeroed account (a second init on the same
    // account fails the constraint — that IS the one-time gate).
    let mut board = ctx.accounts.leaderboard.load_init()?;
    board.authority = ctx.accounts.authority.key();
    board.capacity = capacity;
    board.size = 0;
    board.bump = 0; // not a PDA — the board is a client-created account
    Ok(())
}

pub fn register_session_key(ctx: Context<RegisterSessionKey>, session_key: Pubkey) -> Result<()> {
    let stats = &mut ctx.accounts.player_stats;
    ensure_stats_initialized(stats, ctx.accounts.player.key(), ctx.bumps.player_stats);
    ensure_rate_limit_initialized(
        &mut ctx.accounts.battle_rate_limit,
        ctx.accounts.player.key(),
        ctx.bumps.battle_rate_limit,
    );
    stats.session_key = Some(session_key);
    Ok(())
}

pub fn revoke_session_key(ctx: Context<RevokeSessionKey>) -> Result<()> {
    ctx.accounts.player_stats.session_key = None;
    Ok(())
}

pub fn record_battle(ctx: Context<RecordBattle>, win: bool, opponent_is_bot: bool) -> Result<()> {
    let stats = &mut ctx.accounts.player_stats;
    let player = ctx.accounts.player.key();
    ensure_stats_initialized(stats, player, ctx.bumps.player_stats);

    // Soft auto-confirm: either the wallet itself signs, or the burner key it
    // registered via `register_session_key` (no wallet popup per battle).
    let signer = ctx.accounts.signer.key();
    require!(
        signer == stats.player || stats.session_key == Some(signer),
        LeaderboardError::SessionKeyMismatch
    );

    // Both authorization paths consume the same player-keyed allowance. The
    // throttle PDA is created by the wallet during session registration, or
    // lazily by the signer for a direct owner-signed first battle.
    let rate_limit = &mut ctx.accounts.battle_rate_limit;
    ensure_rate_limit_initialized(rate_limit, player, ctx.bumps.battle_rate_limit);
    let clock = Clock::get()?;
    rate_limit.consume(clock.slot, clock.unix_timestamp)?;

    // Battle tally + streaks.
    stats.games = stats
        .games
        .checked_add(1)
        .ok_or(LeaderboardError::NumericalOverflow)?;
    if win {
        stats.wins = stats
            .wins
            .checked_add(1)
            .ok_or(LeaderboardError::NumericalOverflow)?;
        stats.streak = stats.streak.saturating_add(1);
        stats.best_streak = stats.best_streak.max(stats.streak);
    } else {
        stats.losses = stats
            .losses
            .checked_add(1)
            .ok_or(LeaderboardError::NumericalOverflow)?;
        stats.streak = 0;
    }

    // Elo-lite: fixed deltas (PvP swings harder than PvE), floored at 0.
    let delta = match (win, opponent_is_bot) {
        (true, false) => RATING_WIN_VS_PLAYER,
        (false, false) => RATING_LOSS_VS_PLAYER,
        (true, true) => RATING_WIN_VS_BOT,
        (false, true) => RATING_LOSS_VS_BOT,
    };
    stats.rating = apply_rating_delta(stats.rating, delta);

    // Min-heap upsert: in-heap -> update + re-heapify; room -> push + sift up;
    // full -> evict the root iff the new rating beats the weakest of the top.
    let mut board = ctx.accounts.leaderboard.load_mut()?;
    board.upsert(player, stats.rating, stats.wins);
    Ok(())
}

pub fn set_profile(ctx: Context<SetProfile>, name: String, uri: String) -> Result<()> {
    require!(
        !name.is_empty() && name.len() <= PlayerStats::MAX_NAME_LEN,
        LeaderboardError::InvalidProfileName
    );
    require!(
        uri.len() <= PlayerStats::MAX_URI_LEN,
        LeaderboardError::InvalidProfileUri
    );

    // The top-list perk gate: only players currently holding a heap slot may
    // set a profile. Eviction does not clear an already-set profile, but the
    // evicted player can no longer edit it.
    let board = ctx.accounts.leaderboard.load()?;
    require!(
        board.contains(&ctx.accounts.player.key()),
        LeaderboardError::NotInTopList
    );
    drop(board);

    let stats = &mut ctx.accounts.player_stats;
    stats.profile_name = [0u8; PlayerStats::MAX_NAME_LEN];
    stats.profile_name[..name.len()].copy_from_slice(name.as_bytes());
    stats.profile_uri = [0u8; PlayerStats::MAX_URI_LEN];
    stats.profile_uri[..uri.len()].copy_from_slice(uri.as_bytes());
    Ok(())
}

// ===========================================================================
// Async PvP ladder handlers (design §2). PvE `record_battle` above is untouched;
// the PvP path is a separate rating model (opponent-scaled elo) writing the SAME
// `PlayerStats.rating` field, plus new PDAs.
// ===========================================================================

/// Args for `publish_snapshot`: the engine-ready build to capture as a ghost.
/// MVP trusts these client-captured stats (design decision §2, LOCKED) — the
/// same trust boundary as today's `/api/battle`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PublishSnapshotArgs {
    pub avatar_ref: Pubkey,
    pub archetype_id: [u8; 32],
    pub stats: ArenaStatsLite,
    pub skill_mask: u8,
    pub element: u8,
    pub skin_ref: [u8; 32],
    pub rating_at_publish: i32,
}

pub fn publish_snapshot(ctx: Context<PublishSnapshot>, args: PublishSnapshotArgs) -> Result<()> {
    let snapshot = &mut ctx.accounts.arena_snapshot;
    snapshot.owner = ctx.accounts.owner.key();
    snapshot.avatar_ref = args.avatar_ref;
    snapshot.archetype_id = args.archetype_id;
    snapshot.stats = args.stats;
    // Only 7 skills are defined; bit 7 is reserved (sim ignores it, but mask it
    // off so the stored ghost never carries phantom bits).
    snapshot.skill_mask = args.skill_mask & 0x7f;
    snapshot.element = args.element;
    snapshot.skin_ref = args.skin_ref;
    snapshot.rating_at_publish = args.rating_at_publish;
    snapshot.published_slot = Clock::get()?.slot;
    snapshot.bump = ctx.bumps.arena_snapshot;
    Ok(())
}

pub fn unpublish_snapshot(_ctx: Context<UnpublishSnapshot>) -> Result<()> {
    // `close = owner` returns the ghost's rent to the wallet. No state to touch.
    Ok(())
}

pub fn commit_challenge(ctx: Context<CommitChallenge>, nonce: u64) -> Result<()> {
    let challenger = ctx.accounts.challenger.key();
    // No-record vs your own snapshot (design §4): reject at commit.
    require!(
        ctx.accounts.opponent_snapshot.owner != challenger,
        LeaderboardError::SelfSnapshotNotAllowed
    );

    // Lock a FUTURE target slot: its (currently unknown) hash seeds the fight,
    // making the outcome unknowable at commit — mirrors the mint commit path.
    let target_slot = Clock::get()?
        .slot
        .checked_add(PVP_REVEAL_DELAY_SLOTS)
        .ok_or(LeaderboardError::NumericalOverflow)?;

    let challenge = &mut ctx.accounts.challenge;
    challenge.challenger = challenger;
    challenge.nonce = nonce;
    challenge.opponent_snapshot = ctx.accounts.opponent_snapshot.key();
    challenge.target_slot = target_slot;
    challenge.bump = ctx.bumps.challenge;
    Ok(())
}

pub fn resolve_challenge(
    ctx: Context<ResolveChallenge>,
    _nonce: u64,
    pair_lo: Pubkey,
    pair_hi: Pubkey,
) -> Result<()> {
    let challenger = ctx.accounts.challenger.key();
    let opponent_owner = ctx.accounts.opponent_snapshot.owner;

    // No-record vs your own snapshot (defense in depth; also blocked at commit).
    require!(
        opponent_owner != challenger,
        LeaderboardError::SelfSnapshotNotAllowed
    );

    // The permissionless caller must supply the correct order-independent pair
    // keys; the PDA seeds bind them and this check pins them to the real pair.
    let (lo, hi) = PairCooldown::sort_keys(challenger, opponent_owner);
    require!(
        pair_lo == lo && pair_hi == hi,
        LeaderboardError::InvalidPairKeys
    );

    // Reveal window: the target slot must have PASSED and its hash still be live
    // in SlotHashes (past the window -> use `close_expired_challenge`).
    let target_slot = ctx.accounts.challenge.target_slot;
    let clock = Clock::get()?;
    let now = clock.slot;
    require!(now > target_slot, LeaderboardError::RevealTooEarly);
    require!(
        now <= target_slot
            .checked_add(PVP_COMMIT_WINDOW_SLOTS)
            .ok_or(LeaderboardError::NumericalOverflow)?,
        LeaderboardError::ChallengeWindowExpired
    );

    // Seed = splitmix64_mix(slothash(first produced slot >= target) ^ first8(challenger) ^
    //                       first8(opp_snapshot) ^ commit_nonce)  (design §2.6).
    let opp_snapshot_key = ctx.accounts.challenge.opponent_snapshot;
    let commit_nonce = ctx.accounts.challenge.nonce;
    let slothash = slothash_at_or_after(&ctx.accounts.slot_hashes, target_slot)?;
    let seed =
        splitmix64_mix(slothash ^ first8(&challenger) ^ first8(&opp_snapshot_key) ^ commit_nonce);

    // Recompute the winner ON CHAIN — nobody submits a result (trustless).
    // A = challenger, B = opponent; identity = each ArenaSnapshot pubkey (§1).
    let challenger_snapshot_key = ctx.accounts.challenger_snapshot.key();
    let a = ctx
        .accounts
        .challenger_snapshot
        .combatant(&challenger_snapshot_key);
    let b = ctx.accounts.opponent_snapshot.combatant(&opp_snapshot_key);
    let (challenger_won, _rounds) = crate::sim::resolve_onchain(&a, &b, seed);

    // Pair throttle: a rated result at most once per cooldown / daily cap; a
    // repeat pairing resolves as a no-rating exhibition (design §4 self-play).
    let rated = ctx.accounts.pair_cooldown.consume_rated(
        lo,
        hi,
        ctx.bumps.pair_cooldown,
        now,
        clock.unix_timestamp,
    );

    // Ensure both PlayerStats exist and read PRE-fight ratings (both deltas use
    // the pre-fight pair so the exchange is ~zero-sum).
    ensure_stats_initialized(
        &mut ctx.accounts.challenger_stats,
        challenger,
        ctx.bumps.challenger_stats,
    );
    ensure_stats_initialized(
        &mut ctx.accounts.opponent_stats,
        opponent_owner,
        ctx.bumps.opponent_stats,
    );
    let ra = ctx.accounts.challenger_stats.rating;
    let rb = ctx.accounts.opponent_stats.rating;
    let (delta_a, delta_b) = if rated {
        (
            elo_delta(ra, rb, challenger_won),
            elo_delta(rb, ra, !challenger_won),
        )
    } else {
        (0, 0)
    };

    // Dual PlayerStats write (design §2.7). Rating only on a rated result.
    apply_pvp_stats(
        &mut ctx.accounts.challenger_stats,
        challenger_won,
        rated.then_some(delta_a),
    )?;
    apply_pvp_stats(
        &mut ctx.accounts.opponent_stats,
        !challenger_won,
        rated.then_some(delta_b),
    )?;

    // Dual CharRecord write (design §3) — always (exhibitions still count W/L).
    let challenger_avatar = ctx.accounts.challenger_snapshot.avatar_ref;
    let opponent_avatar = ctx.accounts.opponent_snapshot.avatar_ref;
    let challenger_char = &mut ctx.accounts.challenger_char;
    challenger_char.initialize_if_needed(challenger, challenger_avatar, ctx.bumps.challenger_char);
    challenger_char.record(challenger_won, now);
    let opponent_char = &mut ctx.accounts.opponent_char;
    opponent_char.initialize_if_needed(opponent_owner, opponent_avatar, ctx.bumps.opponent_char);
    opponent_char.record(!challenger_won, now);

    // Ranked heap: RATED results only, and only for wallets past the min-games
    // gate (design §4 anti-sybil). Exhibitions never touch the board.
    if rated {
        let mut board = ctx.accounts.leaderboard.load_mut()?;
        let challenger_stats = &ctx.accounts.challenger_stats;
        if challenger_stats.games >= MIN_RANKED_GAMES {
            board.upsert(challenger, challenger_stats.rating, challenger_stats.wins);
        }
        let opponent_stats = &ctx.accounts.opponent_stats;
        if opponent_stats.games >= MIN_RANKED_GAMES {
            board.upsert(opponent_owner, opponent_stats.rating, opponent_stats.wins);
        }
    }

    Ok(())
}

pub fn close_expired_challenge(ctx: Context<CloseExpiredChallenge>, _nonce: u64) -> Result<()> {
    let expires_after = ctx
        .accounts
        .challenge
        .target_slot
        .checked_add(PVP_COMMIT_WINDOW_SLOTS)
        .ok_or(LeaderboardError::NumericalOverflow)?;
    require!(
        Clock::get()?.slot > expires_after,
        LeaderboardError::ChallengeNotExpired
    );
    // `close = challenger` refunds the commit rent to the wallet that paid it;
    // the challenge holds no other value, so a permissionless closer gains nothing.
    Ok(())
}

// --- PvP helpers ------------------------------------------------------------

/// Apply one PvP battle result to a PlayerStats (tally + streaks, optional
/// rating delta). `ensure_stats_initialized` must have run first.
fn apply_pvp_stats(stats: &mut PlayerStats, won: bool, rating_delta: Option<i32>) -> Result<()> {
    stats.games = stats
        .games
        .checked_add(1)
        .ok_or(LeaderboardError::NumericalOverflow)?;
    if won {
        stats.wins = stats
            .wins
            .checked_add(1)
            .ok_or(LeaderboardError::NumericalOverflow)?;
        stats.streak = stats.streak.saturating_add(1);
        stats.best_streak = stats.best_streak.max(stats.streak);
    } else {
        stats.losses = stats
            .losses
            .checked_add(1)
            .ok_or(LeaderboardError::NumericalOverflow)?;
        stats.streak = 0;
    }
    if let Some(delta) = rating_delta {
        stats.rating = apply_rating_delta(stats.rating, delta);
    }
    Ok(())
}

/// First 8 bytes (LE u64) of a pubkey — the entropy-mixing handle used in the
/// seed, mirroring the mint path in `solana-ekza-arena`.
#[inline]
fn first8(key: &Pubkey) -> u64 {
    u64::from_le_bytes(key.to_bytes()[0..8].try_into().unwrap())
}

/// Single-shot splitmix64 mix (identical to `solana-ekza-arena` affix::splitmix64_mix).
#[inline]
fn splitmix64_mix(x: u64) -> u64 {
    const SPLITMIX64_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut z = x.wrapping_add(SPLITMIX64_GAMMA);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// First 8 bytes (LE u64) of the first produced slot hash at or after the
/// committed lower bound. SlotHashes is newest-first and may omit skipped
/// slots. A skipped-target candidate is accepted only after an older entry
/// below the target proves the canonical boundary; this prevents rerolling
/// against a later retained hash after the original candidate has aged out.
fn slothash_at_or_after(slot_hashes: &AccountInfo, target_slot: u64) -> Result<u64> {
    let data = slot_hashes.try_borrow_data()?;
    slothash_at_or_after_data(&data, target_slot)
}

fn slothash_at_or_after_data(data: &[u8], target_slot: u64) -> Result<u64> {
    require!(data.len() >= 8, LeaderboardError::InvalidSlotHashes);
    let len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
    let required_len = len
        .checked_mul(40)
        .and_then(|entries| entries.checked_add(8))
        .ok_or(LeaderboardError::InvalidSlotHashes)?;
    require!(
        data.len() >= required_len,
        LeaderboardError::InvalidSlotHashes
    );

    let mut candidate = None;
    for i in 0..len {
        let base = 8 + i * 40;
        let slot = u64::from_le_bytes(data[base..base + 8].try_into().unwrap());
        if slot == target_slot {
            let hash_off = base + 8;
            return Ok(u64::from_le_bytes(
                data[hash_off..hash_off + 8].try_into().unwrap(),
            ));
        }
        if slot > target_slot {
            let hash_off = base + 8;
            candidate = Some(u64::from_le_bytes(
                data[hash_off..hash_off + 8].try_into().unwrap(),
            ));
        } else {
            return candidate.ok_or(LeaderboardError::SlotHashNotFound.into());
        }
    }
    Err(LeaderboardError::SlotHashNotFound.into())
}

// --- Opponent-scaled elo (design §4) ---------------------------------------
//
// expected_a = 1 / (1 + 10^((rating_b - rating_a)/400)); delta = round(K*(score
// - expected)). Fully deterministic integer math: a 21-entry fixed-point table
// of 10^(d/400) (scale 1e6) over d=0..800 step 40, linear interpolation, and
// symmetry 10^(-x)=1/10^x. `diff` clamped to [-800,800].

const ELO_FP: i64 = 1_000_000;

/// 10^(k/400) * 1e6 for k = 0, 40, ..., 800.
const POW10_TABLE: [i64; 21] = [
    1_000_000,
    1_258_925,
    1_584_893,
    1_995_262,
    2_511_886,
    3_162_278,
    3_981_072,
    5_011_872,
    6_309_573,
    7_943_282,
    10_000_000,
    12_589_254,
    15_848_932,
    19_952_623,
    25_118_864,
    31_622_777,
    39_810_717,
    50_118_723,
    63_095_734,
    79_432_823,
    100_000_000,
];

fn pow10_pos(d: i64) -> i64 {
    let idx = (d / 40) as usize;
    if idx >= 20 {
        return POW10_TABLE[20];
    }
    let lo = POW10_TABLE[idx];
    let hi = POW10_TABLE[idx + 1];
    let frac = d - (idx as i64) * 40;
    lo + (hi - lo) * frac / 40
}

fn pow10_ratio(diff: i32) -> i64 {
    let d = diff.clamp(-800, 800) as i64;
    if d < 0 {
        ELO_FP * ELO_FP / pow10_pos(-d)
    } else {
        pow10_pos(d)
    }
}

/// Expected score for A vs B, scale 1e6.
fn expected_score_a(ra: i32, rb: i32) -> i64 {
    let q = pow10_ratio(rb - ra);
    ELO_FP * ELO_FP / (ELO_FP + q)
}

fn round_div(num: i64, den: i64) -> i64 {
    if num >= 0 {
        (num + den / 2) / den
    } else {
        -(((-num) + den / 2) / den)
    }
}

/// Scaled-elo rating delta for A given the result. `won` in {win, loss}.
fn elo_delta(ra: i32, rb: i32, won: bool) -> i32 {
    let expected_a = expected_score_a(ra, rb);
    let score_a: i64 = if won { ELO_FP } else { 0 };
    round_div(PVP_ELO_K as i64 * (score_a - expected_a), ELO_FP) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_hashes_data(entries: &[(u64, u64)]) -> Vec<u8> {
        let mut data = Vec::with_capacity(8 + entries.len() * 40);
        data.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        for (slot, hash_first8) in entries {
            data.extend_from_slice(&slot.to_le_bytes());
            data.extend_from_slice(&hash_first8.to_le_bytes());
            data.extend_from_slice(&[0u8; 24]);
        }
        data
    }

    #[test]
    fn challenge_entropy_is_skip_safe_and_timing_invariant() {
        let exact = slot_hashes_data(&[(110, 11), (105, 22), (104, 33)]);
        assert_eq!(slothash_at_or_after_data(&exact, 105).unwrap(), 22);

        let skipped = slot_hashes_data(&[(111, 44), (108, 55), (104, 66)]);
        assert_eq!(slothash_at_or_after_data(&skipped, 105).unwrap(), 55);

        let too_early = slot_hashes_data(&[(104, 77), (103, 88)]);
        assert!(slothash_at_or_after_data(&too_early, 105).is_err());

        let aged_out = slot_hashes_data(&[(700, 99), (600, 111)]);
        assert!(slothash_at_or_after_data(&aged_out, 100).is_err());

        let mut truncated = slot_hashes_data(&[(110, 1), (104, 2)]);
        truncated.pop();
        assert!(slothash_at_or_after_data(&truncated, 105).is_err());
    }

    #[test]
    fn rating_delta_never_crosses_the_floor() {
        assert_eq!(apply_rating_delta(5, RATING_LOSS_VS_PLAYER), RATING_FLOOR);
        assert_eq!(apply_rating_delta(0, RATING_LOSS_VS_BOT), RATING_FLOOR);
    }

    #[test]
    fn elo_is_near_zero_sum_and_opponent_scaled() {
        // Equal ratings: winner +12, loser -12 (K=24, expected 0.5). Zero-sum.
        let d_win = elo_delta(1000, 1000, true);
        let d_loss = elo_delta(1000, 1000, false);
        assert_eq!(d_win, 12);
        assert_eq!(d_loss, -12);
        assert_eq!(d_win + d_loss, 0);

        // Beating a MUCH weaker opponent pays ~0; losing to it costs a lot.
        let strong_beats_weak = elo_delta(1600, 800, true);
        let strong_loses_weak = elo_delta(1600, 800, false);
        assert!(strong_beats_weak <= 1, "crushing a weak ghost must pay ~0");
        assert!(
            strong_loses_weak <= -20,
            "losing to a weak ghost must sting"
        );

        // Symmetric exchange stays within 1 of zero-sum for any pairing.
        for (ra, rb) in [(1000, 1400), (1200, 900), (1500, 1500), (800, 2000)] {
            let a = elo_delta(ra, rb, true);
            let b = elo_delta(rb, ra, false);
            assert!((a + b).abs() <= 1, "near-zero-sum for {ra}/{rb}: {a}+{b}");
        }
    }
}
