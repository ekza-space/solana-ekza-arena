use anchor_lang::prelude::*;

use crate::{
    constants::{BATTLE_RATE_LIMIT_SEED, PLAYER_STATS_SEED},
    state::{BattleRateLimit, Leaderboard, PlayerStats},
};

#[derive(Accounts)]
pub struct InitLeaderboard<'info> {
    /// The board is ~40 KB (1000 zero-copy heap slots). A single-CPI allocation
    /// is capped at 10 KB on-chain, so `init` cannot create it. Instead the
    /// client pre-creates the account at full `Leaderboard::LEN` with a
    /// top-level `SystemProgram.createAccount` (no CPI cap) and hands it in
    /// zeroed + program-owned; `zero` verifies the discriminator is unset and
    /// claims it. This is the standard pattern for large zero-copy accounts
    /// (order books, etc). A second init on an already-claimed account fails
    /// the `zero` constraint — that IS the one-time gate.
    #[account(zero)]
    pub leaderboard: AccountLoader<'info, Leaderboard>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct RegisterSessionKey<'info> {
    /// Created lazily on first use — registering a session key is a natural
    /// first touch for a new player (the burner then records battles).
    #[account(
        init_if_needed,
        payer = player,
        space = 8 + PlayerStats::INIT_SPACE,
        seeds = [PLAYER_STATS_SEED, player.key().as_ref()],
        bump
    )]
    pub player_stats: Account<'info, PlayerStats>,

    /// Pre-created by the wallet so the normal session-key flow never makes
    /// an unfunded burner pay the one-time throttle-account rent.
    #[account(
        init_if_needed,
        payer = player,
        space = 8 + BattleRateLimit::INIT_SPACE,
        seeds = [BATTLE_RATE_LIMIT_SEED, player.key().as_ref()],
        bump
    )]
    pub battle_rate_limit: Account<'info, BattleRateLimit>,

    /// Only the real wallet may grant (or rotate) its session key.
    #[account(mut)]
    pub player: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevokeSessionKey<'info> {
    #[account(
        mut,
        seeds = [PLAYER_STATS_SEED, player.key().as_ref()],
        bump = player_stats.bump,
        has_one = player
    )]
    pub player_stats: Account<'info, PlayerStats>,

    /// Only the real wallet may revoke — a leaked burner cannot lock the
    /// player out of revoking it.
    pub player: Signer<'info>,
}

#[derive(Accounts)]
pub struct RecordBattle<'info> {
    /// The board to record into, referenced by address (not a PDA — see
    /// `InitLeaderboard`). `AccountLoader` still enforces program ownership and
    /// the correct discriminator, so only a real `Leaderboard` is accepted.
    #[account(mut)]
    pub leaderboard: AccountLoader<'info, Leaderboard>,

    /// Created lazily on the player's first recorded battle. The handler
    /// enforces that `signer` is the player wallet OR its registered session
    /// key (a fresh account has no session key, so the wallet must sign the
    /// first battle unless `register_session_key` ran first).
    #[account(
        init_if_needed,
        payer = signer,
        space = 8 + PlayerStats::INIT_SPACE,
        seeds = [PLAYER_STATS_SEED, player.key().as_ref()],
        bump
    )]
    pub player_stats: Account<'info, PlayerStats>,

    /// Separate PDA preserves the binary layout of already-created local
    /// PlayerStats accounts. Normally wallet-funded during session-key
    /// registration; lazily signer-funded for an owner's first direct battle.
    #[account(
        init_if_needed,
        payer = signer,
        space = 8 + BattleRateLimit::INIT_SPACE,
        seeds = [BATTLE_RATE_LIMIT_SEED, player.key().as_ref()],
        bump
    )]
    pub battle_rate_limit: Account<'info, BattleRateLimit>,

    /// CHECK: the wallet the battle is recorded FOR. Not required to sign —
    /// authorization is `signer == player || signer == stats.session_key`,
    /// checked in the handler; the PDA seeds bind `player_stats` to this key.
    pub player: UncheckedAccount<'info>,

    /// The player wallet itself, or the registered session (burner) key.
    #[account(mut)]
    pub signer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetProfile<'info> {
    /// Read-only membership check: profiles are a top-list perk, so the
    /// handler scans the heap for `player` and rejects outsiders. Referenced
    /// by address; `AccountLoader` enforces program ownership + discriminator.
    pub leaderboard: AccountLoader<'info, Leaderboard>,

    #[account(
        mut,
        seeds = [PLAYER_STATS_SEED, player.key().as_ref()],
        bump = player_stats.bump,
        has_one = player
    )]
    pub player_stats: Account<'info, PlayerStats>,

    /// Profile edits are wallet-only (deliberately NOT session-key signable:
    /// a leaked burner must not be able to deface a top player's profile).
    pub player: Signer<'info>,
}
