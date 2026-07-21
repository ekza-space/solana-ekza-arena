use anchor_lang::prelude::*;

/// `Leaderboard` PDA seed: `["leaderboard", authority]`. Per-authority (not a
/// singleton) so one deployment can host several boards (e.g. seasons, or the
/// throwaway small-capacity boards the test suite uses to exercise eviction).
#[constant]
pub const LEADERBOARD_SEED: &[u8] = b"leaderboard";

/// `PlayerStats` PDA seed: `["player_stats_v1", player]` — one per wallet.
#[constant]
pub const PLAYER_STATS_SEED: &[u8] = b"player_stats_v1";

/// Hard upper bound on `Leaderboard.capacity`. The zero-copy account always
/// reserves space for this many `HeapEntry`s (~40 KB) so the array layout is a
/// fixed type; `capacity` only limits how many are logically used.
pub const MAX_CAPACITY: usize = 1000;

/// Lower bound on `Leaderboard.capacity`. Production boards are expected to
/// run 100..=1000; the minimum is kept tiny (2) so localnet tests can exercise
/// eviction on a 4-slot board without recording hundreds of battles.
pub const MIN_CAPACITY: u16 = 2;

/// Every player starts here (elo-lite anchor point).
pub const STARTING_RATING: i32 = 1000;

/// Ratings never drop below this floor.
pub const RATING_FLOOR: i32 = 0;

// Elo-lite deltas. PvP swings harder than PvE so grinding bots can't keep up
// with actual ladder play; bot losses still sting more than bot wins pay.
pub const RATING_WIN_VS_PLAYER: i32 = 25;
pub const RATING_LOSS_VS_PLAYER: i32 = -20;
pub const RATING_WIN_VS_BOT: i32 = 10;
pub const RATING_LOSS_VS_BOT: i32 = -15;
