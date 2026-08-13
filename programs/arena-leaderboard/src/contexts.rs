use anchor_lang::prelude::*;

use crate::{
    constants::{
        ARENA_SNAPSHOT_SEED, BATTLE_RATE_LIMIT_SEED, CHALLENGE_SEED, CHAR_RECORD_SEED,
        PAIR_COOLDOWN_SEED, PLAYER_STATS_SEED,
    },
    handlers::PublishSnapshotArgs,
    state::{
        ArenaSnapshot, BattleRateLimit, Challenge, CharRecord, Leaderboard, PairCooldown,
        PlayerStats,
    },
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

// ===========================================================================
// Async PvP ladder contexts (design §2). New PDAs + instructions only.
// ===========================================================================

/// Capture the caller's current build as an on-chain ghost.
/// `["arena_snapshot_v1", owner]`, one per wallet; republishing overwrites.
#[derive(Accounts)]
#[instruction(args: PublishSnapshotArgs)]
pub struct PublishSnapshot<'info> {
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + ArenaSnapshot::INIT_SPACE,
        seeds = [ARENA_SNAPSHOT_SEED, owner.key().as_ref()],
        bump
    )]
    pub arena_snapshot: Account<'info, ArenaSnapshot>,

    #[account(mut)]
    pub owner: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// Leave the ghost pool; rent returns to the owner.
#[derive(Accounts)]
pub struct UnpublishSnapshot<'info> {
    #[account(
        mut,
        close = owner,
        has_one = owner,
        seeds = [ARENA_SNAPSHOT_SEED, owner.key().as_ref()],
        bump = arena_snapshot.bump,
    )]
    pub arena_snapshot: Account<'info, ArenaSnapshot>,

    /// `mut` because `close = owner` credits the reclaimed rent to this wallet.
    #[account(mut)]
    pub owner: Signer<'info>,
}

/// Lock an opponent ghost + a future target slot. `["challenge_v1", challenger,
/// nonce]` — the nonce lets one wallet keep several open challenges.
#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct CommitChallenge<'info> {
    #[account(
        init,
        payer = challenger,
        space = 8 + Challenge::INIT_SPACE,
        seeds = [CHALLENGE_SEED, challenger.key().as_ref(), &nonce.to_le_bytes()],
        bump
    )]
    pub challenge: Account<'info, Challenge>,

    /// The challenger's own published ghost must exist — it is the build that
    /// fights. The PDA seed binds `challenger_snapshot.owner == challenger`.
    #[account(
        seeds = [ARENA_SNAPSHOT_SEED, challenger.key().as_ref()],
        bump = challenger_snapshot.bump,
    )]
    pub challenger_snapshot: Account<'info, ArenaSnapshot>,

    /// The chosen opponent ghost, locked into the challenge at commit time.
    pub opponent_snapshot: Account<'info, ArenaSnapshot>,

    #[account(mut)]
    pub challenger: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// PERMISSIONLESS resolve (design §2.7). `payer` is the only signer, trusted for
/// nothing but funding lazy inits + pushing the tx; every value is program-computed.
#[derive(Accounts)]
#[instruction(nonce: u64, pair_lo: Pubkey, pair_hi: Pubkey)]
pub struct ResolveChallenge<'info> {
    #[account(
        mut,
        close = challenger,
        has_one = challenger,
        seeds = [CHALLENGE_SEED, challenger.key().as_ref(), &nonce.to_le_bytes()],
        bump = challenge.bump,
    )]
    pub challenge: Box<Account<'info, Challenge>>,

    /// CHECK: rent destination for the closed challenge; bound by `has_one` on
    /// `challenge.challenger` and by the challenge PDA seeds. Does not sign.
    #[account(mut)]
    pub challenger: UncheckedAccount<'info>,

    // The typed accounts below are `Box`ed to keep `try_accounts` off the 4 KB
    // BPF stack — five `init_if_needed` accounts otherwise overflow the frame.
    #[account(
        seeds = [ARENA_SNAPSHOT_SEED, challenger.key().as_ref()],
        bump = challenger_snapshot.bump,
    )]
    pub challenger_snapshot: Box<Account<'info, ArenaSnapshot>>,

    /// Bound to the pairing locked at commit; the opponent's own signed build.
    #[account(address = challenge.opponent_snapshot)]
    pub opponent_snapshot: Box<Account<'info, ArenaSnapshot>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + PlayerStats::INIT_SPACE,
        seeds = [PLAYER_STATS_SEED, challenger.key().as_ref()],
        bump
    )]
    pub challenger_stats: Box<Account<'info, PlayerStats>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + PlayerStats::INIT_SPACE,
        seeds = [PLAYER_STATS_SEED, opponent_snapshot.owner.as_ref()],
        bump
    )]
    pub opponent_stats: Box<Account<'info, PlayerStats>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + CharRecord::INIT_SPACE,
        seeds = [CHAR_RECORD_SEED, challenger.key().as_ref(), challenger_snapshot.avatar_ref.as_ref()],
        bump
    )]
    pub challenger_char: Box<Account<'info, CharRecord>>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + CharRecord::INIT_SPACE,
        seeds = [CHAR_RECORD_SEED, opponent_snapshot.owner.as_ref(), opponent_snapshot.avatar_ref.as_ref()],
        bump
    )]
    pub opponent_char: Box<Account<'info, CharRecord>>,

    /// Order-independent pair throttle. Seeds come from the `pair_lo`/`pair_hi`
    /// args; the handler enforces they equal `sort(challenger, opponent_owner)`.
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + PairCooldown::INIT_SPACE,
        seeds = [PAIR_COOLDOWN_SEED, pair_lo.as_ref(), pair_hi.as_ref()],
        bump
    )]
    pub pair_cooldown: Box<Account<'info, PairCooldown>>,

    /// The ranked board; the min-games-gated heap upsert writes here.
    #[account(mut)]
    pub leaderboard: AccountLoader<'info, Leaderboard>,

    /// CHECK: Address-constrained to the SlotHashes sysvar; read as raw data.
    #[account(address = anchor_lang::solana_program::sysvar::slot_hashes::ID)]
    pub slot_hashes: AccountInfo<'info>,

    /// Any key may push the resolve and fund lazy inits (permissionless).
    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// Reclaim rent from a challenge whose reveal window aged out. Permissionless.
#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct CloseExpiredChallenge<'info> {
    #[account(
        mut,
        close = challenger,
        has_one = challenger,
        seeds = [CHALLENGE_SEED, challenger.key().as_ref(), &nonce.to_le_bytes()],
        bump = challenge.bump,
    )]
    pub challenge: Account<'info, Challenge>,

    /// CHECK: rent destination; bound by `has_one` on `challenge.challenger`.
    #[account(mut)]
    pub challenger: UncheckedAccount<'info>,

    /// Any fee payer may clean up an expired challenge.
    pub closer: Signer<'info>,
}
