use anchor_lang::prelude::*;

/// `Leaderboard` PDA seed: `["leaderboard", authority]`. Per-authority (not a
/// singleton) so one deployment can host several boards (e.g. seasons, or the
/// throwaway small-capacity boards the test suite uses to exercise eviction).
#[constant]
pub const LEADERBOARD_SEED: &[u8] = b"leaderboard";

/// `PlayerStats` PDA seed: `["player_stats_v1", player]` — one per wallet.
#[constant]
pub const PLAYER_STATS_SEED: &[u8] = b"player_stats_v1";

/// `BattleRateLimit` PDA seed: `["battle_rate_limit_v1", player]`. Keeping
/// throttle state in a separate PDA lets existing `PlayerStats` accounts keep
/// their original binary layout. Registering a session key creates this PDA
/// up-front (wallet-funded); owner-signed first battles create it lazily.
#[constant]
pub const BATTLE_RATE_LIMIT_SEED: &[u8] = b"battle_rate_limit_v1";

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

/// A second battle for the same wallet cannot land in the same slot. This
/// blocks transaction batching / same-slot replay while leaving the client UX
/// responsive; the daily cap below is the hard bound against unattended
/// gasless farming.
#[constant]
pub const MIN_BATTLE_COOLDOWN_SLOTS: u64 = 1;

/// Maximum accepted battle records per player per UTC day. The limit belongs
/// to the player PDA, not to the signer, so switching between the owner and a
/// session key (or rotating the session key) cannot reset it.
#[constant]
pub const MAX_BATTLES_PER_UTC_DAY: u16 = 20;

/// Used to derive a stable UTC-day index from the on-chain Clock timestamp.
pub const SECONDS_PER_DAY: i64 = 86_400;

// ---------------------------------------------------------------------------
// Async PvP ladder (ghost snapshots + commit/reveal challenges). All additive;
// no existing constant changes.
// ---------------------------------------------------------------------------

/// `ArenaSnapshot` PDA seed: `["arena_snapshot_v1", owner]` — one live ghost
/// per wallet; republishing overwrites in place.
#[constant]
pub const ARENA_SNAPSHOT_SEED: &[u8] = b"arena_snapshot_v1";

/// `Challenge` PDA seed: `["challenge_v1", challenger, nonce]`.
#[constant]
pub const CHALLENGE_SEED: &[u8] = b"challenge_v1";

/// `CharRecord` PDA seed: `["char_record_v1", owner, avatar_ref]` — per-character
/// W/L that persists across avatar swaps.
#[constant]
pub const CHAR_RECORD_SEED: &[u8] = b"char_record_v1";

/// `PairCooldown` PDA seed: `["pair_cd_v1", key_lo, key_hi]` with
/// `(key_lo, key_hi) = sort(challenger, opponent_owner)` (order-independent).
#[constant]
pub const PAIR_COOLDOWN_SEED: &[u8] = b"pair_cd_v1";

/// Slots between `commit_challenge` and the earliest `resolve_challenge`. The
/// first produced slot hash at/after the target (unknown at commit) seeds the
/// fight — skip-safe and revert-grind resistant, mirroring the mint path.
#[constant]
pub const PVP_REVEAL_DELAY_SLOTS: u64 = 5;

/// Lifetime of a committed challenge after its target slot. Past this, the
/// canonical post-target hash has aged out of SlotHashes; `close_expired_challenge`
/// reclaims the rent. Mirrors `COMMIT_REVEAL_WINDOW_SLOTS`.
#[constant]
/// Roughly two minutes for an external-wallet resolve approval while the
/// target hash remains comfortably inside SlotHashes retention.
pub const PVP_COMMIT_WINDOW_SLOTS: u64 = 300;

/// Elo K-factor for the opponent-scaled PvP rating delta (design §4).
pub const PVP_ELO_K: i32 = 24;

/// A wallet is hidden from the ranked heap / matchmaking pool until it has this
/// many PvP games (anti-sybil heap-spam gate, design §4).
pub const MIN_RANKED_GAMES: u32 = 3;

/// A given pair yields a RATED result at most once per this many slots; sooner
/// repeats resolve as no-rating exhibitions (self-play throttle, design §4).
#[constant]
pub const PAIR_COOLDOWN_SLOTS: u64 = 150;

/// Hard cap on RATED results per pair per UTC day (secondary self-play throttle).
#[constant]
pub const MAX_RANKED_PER_PAIR_PER_DAY: u16 = 5;
