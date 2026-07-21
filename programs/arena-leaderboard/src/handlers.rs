use anchor_lang::prelude::*;

use crate::{
    constants::{
        MAX_CAPACITY, MIN_CAPACITY, RATING_FLOOR, RATING_LOSS_VS_BOT, RATING_LOSS_VS_PLAYER,
        RATING_WIN_VS_BOT, RATING_WIN_VS_PLAYER, STARTING_RATING,
    },
    contexts::{InitLeaderboard, RecordBattle, RegisterSessionKey, RevokeSessionKey, SetProfile},
    error::LeaderboardError,
    state::PlayerStats,
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
    stats.rating = stats.rating.saturating_add(delta).max(RATING_FLOOR);

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
