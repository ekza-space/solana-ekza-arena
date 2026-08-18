//! Ekza Arena leaderboard — a SEPARATE program from the arena registry.
//!
//! Design (owner spec):
//! - `Leaderboard`: zero-copy binary MIN-heap of the top-N players by rating
//!   (root = weakest of the top), with O(log n) upsert and auto-eviction when
//!   full. See `state.rs` for the index math.
//! - `PlayerStats`: per-wallet battle tally + elo-lite rating + an optional
//!   session (burner) key so the web app can record battles without wallet
//!   popups.
//! - `set_profile`: name/link storage as a TOP-LIST PERK — only players
//!   currently holding a heap slot may set it.
#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;

pub mod constants;
pub mod contexts;
pub mod error;
pub mod handlers;
pub mod sim;
pub mod state;

pub use contexts::*;
pub use error::*;
pub use state::*;

declare_id!("9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU");

#[program]
pub mod arena_leaderboard {
    use super::*;

    /// One-time board creation (`["leaderboard", authority]`).
    /// `capacity` is the top-N size; production boards use 100..=1000.
    pub fn init_leaderboard(ctx: Context<InitLeaderboard>, capacity: u16) -> Result<()> {
        handlers::init_leaderboard(ctx, capacity)
    }

    /// Grant (or rotate) the burner key allowed to sign `record_battle` for
    /// this wallet — the web app's soft auto-confirm flow. Wallet-signed.
    pub fn register_session_key(
        ctx: Context<RegisterSessionKey>,
        session_key: Pubkey,
    ) -> Result<()> {
        handlers::register_session_key(ctx, session_key)
    }

    /// Drop the registered session key. Wallet-signed.
    pub fn revoke_session_key(ctx: Context<RevokeSessionKey>) -> Result<()> {
        handlers::revoke_session_key(ctx)
    }

    /// Record one battle result for `player`: updates wins/losses/streaks,
    /// applies the elo-lite rating delta (+25/-20 vs player, +10/-15 vs bot,
    /// floor 0) and upserts the player into the min-heap top list. A
    /// player-keyed throttle enforces one accepted write per slot and at most
    /// 20 accepted writes per UTC day. Signed by the player wallet OR its
    /// registered session key; both consume the same allowance.
    pub fn record_battle(
        ctx: Context<RecordBattle>,
        win: bool,
        opponent_is_bot: bool,
    ) -> Result<()> {
        handlers::record_battle(ctx, win, opponent_is_bot)
    }

    /// Top-list perk: set display name (≤32 bytes) + profile link (≤96 bytes,
    /// e.g. a future solana-users profile). Rejected with `NotInTopList` when
    /// the player does not currently hold a heap slot. Wallet-signed.
    pub fn set_profile(ctx: Context<SetProfile>, name: String, uri: String) -> Result<()> {
        handlers::set_profile(ctx, name, uri)
    }

    // --- Async PvP ladder (ghost snapshots + trustless commit/reveal) -------

    /// Publish (or overwrite) the caller's build as an on-chain ghost snapshot
    /// (`["arena_snapshot_v1", owner]`). MVP trusts the client-captured stats.
    pub fn publish_snapshot(
        ctx: Context<PublishSnapshot>,
        args: handlers::PublishSnapshotArgs,
    ) -> Result<()> {
        handlers::publish_snapshot(ctx, args)
    }

    /// Leave the ghost pool; the snapshot's rent returns to the owner.
    pub fn unpublish_snapshot(ctx: Context<UnpublishSnapshot>) -> Result<()> {
        handlers::unpublish_snapshot(ctx)
    }

    /// Commit to fight a specific opponent ghost at a future slot. Locks the
    /// pairing + `target_slot` (the first produced hash at/after it seeds the fight). Wallet or
    /// session-key signed.
    pub fn commit_challenge(ctx: Context<CommitChallenge>, nonce: u64) -> Result<()> {
        handlers::commit_challenge(ctx, nonce)
    }

    /// PERMISSIONLESS resolve: recompute the winner on-chain from both snapshots
    /// + the slot-hash seed, dual-write both PlayerStats + both CharRecord,
    /// apply opponent-scaled elo (honoring PairCooldown exhibitions + the
    /// min-games heap gate), and close the challenge. Anyone can push it —
    /// that is the anti-loss-dodge mechanism. `pair_lo`/`pair_hi` are the sorted
    /// `(challenger, opponent_owner)` pair (validated on-chain).
    pub fn resolve_challenge(
        ctx: Context<ResolveChallenge>,
        nonce: u64,
        pair_lo: Pubkey,
        pair_hi: Pubkey,
    ) -> Result<()> {
        handlers::resolve_challenge(ctx, nonce, pair_lo, pair_hi)
    }

    /// Permissionless: reclaim the rent of a challenge whose reveal window aged
    /// out of SlotHashes without a resolve.
    pub fn close_expired_challenge(ctx: Context<CloseExpiredChallenge>, nonce: u64) -> Result<()> {
        handlers::close_expired_challenge(ctx, nonce)
    }
}
