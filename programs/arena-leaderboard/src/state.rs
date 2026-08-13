use anchor_lang::prelude::*;

use crate::{
    constants::{
        MAX_BATTLES_PER_UTC_DAY, MAX_CAPACITY, MAX_RANKED_PER_PAIR_PER_DAY,
        MIN_BATTLE_COOLDOWN_SLOTS, PAIR_COOLDOWN_SLOTS, SECONDS_PER_DAY,
    },
    error::LeaderboardError,
};

// ---------------------------------------------------------------------------
// Leaderboard: fixed-size binary MIN-heap over player ratings (zero-copy).
//
// The heap is stored flat in `entries[0..size]` with the classic index math:
//
//   parent(i)      = (i - 1) / 2
//   left_child(i)  = 2 * i + 1
//   right_child(i) = 2 * i + 2
//
// MIN-heap by `rating`: the ROOT (`entries[0]`) is the WEAKEST player still on
// the board. That makes top-N maintenance O(log n):
//   - board not full  -> push at `size`, sift UP;
//   - board full      -> a challenger beats the board iff their rating beats
//                        the root; replace the root (eviction), sift DOWN.
//
// The account always reserves MAX_CAPACITY entries (~40 KB) so the zero-copy
// type has a fixed layout; `capacity` (100..=1000 in production) limits how
// many are logically in play. At 40 KB the account must be zero_copy — a borsh
// deserialize of 1000 entries would blow the compute budget and heap.
// ---------------------------------------------------------------------------

/// One heap slot. `#[zero_copy]` => `repr(C)` + Pod; field order gives
/// 32 + 4 + 4 = 40 bytes with no implicit padding (align 4).
#[zero_copy]
#[derive(Debug, PartialEq, Eq)]
pub struct HeapEntry {
    pub player: Pubkey,
    pub rating: i32,
    pub wins: u32,
}

/// The board. PDA `["leaderboard", authority]`, one-time init.
///
/// Byte layout (after the 8-byte Anchor discriminator):
///   authority: 32 | capacity: u16 | size: u16 | bump: u8 | _padding: [u8; 3]
///   entries: [HeapEntry; 1000]  (only entries[0..size] are meaningful)
#[account(zero_copy)]
pub struct Leaderboard {
    /// Board creator (PDA seed); the only key allowed to init this board.
    pub authority: Pubkey,
    /// Logical heap capacity (top-N). Fixed at init.
    pub capacity: u16,
    /// Number of live entries in `entries[0..size]`.
    pub size: u16,
    pub bump: u8,
    /// Explicit padding so the struct is Pod with zero implicit padding
    /// (entries need 4-byte alignment after the 37 header bytes).
    pub _padding: [u8; 3],
    pub entries: [HeapEntry; MAX_CAPACITY],
}

impl Leaderboard {
    pub const LEN: usize = 8 + core::mem::size_of::<Leaderboard>();

    #[inline]
    fn rating_at(&self, i: usize) -> i32 {
        self.entries[i].rating
    }

    /// Linear scan for a player among the live entries. O(capacity) — fine on
    /// chain (≤1000 pubkey compares) and unavoidable: sift operations move
    /// OTHER players around, so a stored per-player heap index would go stale
    /// without writing to accounts we don't have in the transaction.
    pub fn find(&self, player: &Pubkey) -> Option<usize> {
        self.entries[..self.size as usize]
            .iter()
            .position(|entry| entry.player == *player)
    }

    /// True while `player` holds a top-list slot (gates `set_profile`).
    pub fn contains(&self, player: &Pubkey) -> bool {
        self.find(player).is_some()
    }

    /// Move `entries[i]` toward the root while it is SMALLER than its parent
    /// (min-heap: smaller ratings bubble up). Returns the final index.
    fn sift_up(&mut self, mut i: usize) -> usize {
        while i > 0 {
            let parent = (i - 1) / 2;
            if self.rating_at(i) >= self.rating_at(parent) {
                break;
            }
            self.entries.swap(i, parent);
            i = parent;
        }
        i
    }

    /// Move `entries[i]` toward the leaves while it is LARGER than its
    /// smallest child. Returns the final index.
    fn sift_down(&mut self, mut i: usize) -> usize {
        let size = self.size as usize;
        loop {
            let left = 2 * i + 1;
            let right = 2 * i + 2;
            let mut smallest = i;
            if left < size && self.rating_at(left) < self.rating_at(smallest) {
                smallest = left;
            }
            if right < size && self.rating_at(right) < self.rating_at(smallest) {
                smallest = right;
            }
            if smallest == i {
                return i;
            }
            self.entries.swap(i, smallest);
            i = smallest;
        }
    }

    /// Insert or refresh `player` on the board. Three cases:
    ///   1. already on the board  -> update in place, re-heapify from its slot
    ///      (sift up then down — only one of them will actually move it);
    ///   2. board not full        -> push at the end, sift up;
    ///   3. board full            -> evict the root (the weakest of the top)
    ///      iff the new rating strictly beats it, then sift down from the
    ///      root. Ties lose to the incumbent.
    ///
    /// Returns true when the player holds a slot after the call.
    pub fn upsert(&mut self, player: Pubkey, rating: i32, wins: u32) -> bool {
        if let Some(i) = self.find(&player) {
            self.entries[i].rating = rating;
            self.entries[i].wins = wins;
            let i = self.sift_up(i);
            self.sift_down(i);
            return true;
        }
        let entry = HeapEntry {
            player,
            rating,
            wins,
        };
        if (self.size as usize) < self.capacity as usize {
            let i = self.size as usize;
            self.entries[i] = entry;
            self.size += 1;
            self.sift_up(i);
            return true;
        }
        if rating > self.rating_at(0) {
            self.entries[0] = entry;
            self.sift_down(0);
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Per-player battle stats (regular borsh account — small and fixed-size).
// ---------------------------------------------------------------------------

/// One per wallet: `["player_stats_v1", player]`. Created lazily by the first
/// `record_battle` / `register_session_key` (init_if_needed); a freshly
/// created account is recognized by `player == Pubkey::default()` and seeded
/// with `STARTING_RATING`.
#[account]
pub struct PlayerStats {
    /// The wallet these stats belong to (PDA seed).
    pub player: Pubkey,
    pub wins: u32,
    pub losses: u32,
    pub games: u32,
    /// Current consecutive-win streak (resets to 0 on a loss).
    pub streak: u16,
    pub best_streak: u16,
    /// Elo-lite ladder rating; starts at 1000, floored at 0.
    pub rating: i32,
    /// Burner key allowed to sign `record_battle` for this player — the web
    /// app's soft auto-confirm flow (battles recorded without wallet popups).
    pub session_key: Option<Pubkey>,
    /// Top-list perk (`set_profile`): display name, utf-8, zero-padded.
    pub profile_name: [u8; 32],
    /// Top-list perk: profile link, utf-8, zero-padded. Can later point at a
    /// solana-users profile (a CPI-verified link is out of scope for now).
    pub profile_uri: [u8; 96],
    pub bump: u8,
}

impl PlayerStats {
    pub const MAX_NAME_LEN: usize = 32;
    pub const MAX_URI_LEN: usize = 96;
    pub const INIT_SPACE: usize = 32 // player
        + 4 // wins
        + 4 // losses
        + 4 // games
        + 2 // streak
        + 2 // best_streak
        + 4 // rating
        + 1 + 32 // session_key: Option<Pubkey>
        + Self::MAX_NAME_LEN
        + Self::MAX_URI_LEN
        + 1; // bump
}

// ---------------------------------------------------------------------------
// Per-player anti-farming state (separate PDA for PlayerStats compatibility).
// ---------------------------------------------------------------------------

/// One per wallet: `["battle_rate_limit_v1", player]`.
///
/// This state is deliberately keyed by the player, not the transaction signer:
/// owner- and session-key-signed battles consume the same cooldown and daily
/// allowance. Session rotation therefore cannot reset either limit.
#[account]
pub struct BattleRateLimit {
    pub player: Pubkey,
    /// Slot of the most recently accepted battle.
    pub last_battle_slot: u64,
    /// `Clock.unix_timestamp / 86_400` for the active counter window.
    pub utc_day: i64,
    pub battles_today: u16,
    /// Explicit first-use marker; slot 0 remains a valid first battle slot.
    pub has_recorded_battle: bool,
    pub bump: u8,
}

impl BattleRateLimit {
    pub const INIT_SPACE: usize = 32 // player
        + 8 // last_battle_slot
        + 8 // utc_day
        + 2 // battles_today
        + 1 // has_recorded_battle
        + 1; // bump

    /// Initialize only a newly-created zeroed PDA. Calling this for an
    /// existing account (including after session-key rotation) preserves the
    /// accumulated allowance.
    pub fn initialize_if_needed(&mut self, player: Pubkey, bump: u8) {
        if self.player == Pubkey::default() {
            self.player = player;
            self.bump = bump;
        }
    }

    /// Validate and consume one battle allowance. This runs before any player
    /// stats or heap mutation; an error therefore leaves every account intact.
    pub fn consume(&mut self, slot: u64, unix_timestamp: i64) -> Result<()> {
        if self.has_recorded_battle {
            let first_allowed_slot = self
                .last_battle_slot
                .checked_add(MIN_BATTLE_COOLDOWN_SLOTS)
                .ok_or(LeaderboardError::NumericalOverflow)?;
            require!(
                slot >= first_allowed_slot,
                LeaderboardError::BattleCooldownActive
            );
        }

        let current_utc_day = unix_timestamp.div_euclid(SECONDS_PER_DAY);
        if !self.has_recorded_battle || current_utc_day > self.utc_day {
            self.utc_day = current_utc_day;
            self.battles_today = 0;
        }
        // A rare backwards Clock adjustment must never grant a fresh quota:
        // retain the latest observed day/counter until time catches up.

        require!(
            self.battles_today < MAX_BATTLES_PER_UTC_DAY,
            LeaderboardError::DailyBattleLimitReached
        );

        self.battles_today = self
            .battles_today
            .checked_add(1)
            .ok_or(LeaderboardError::NumericalOverflow)?;
        self.last_battle_slot = slot;
        self.has_recorded_battle = true;
        Ok(())
    }
}

// ===========================================================================
// Async PvP ladder state (design §2/§3). All ADDITIVE — new PDAs only; the
// PlayerStats / Leaderboard / BattleRateLimit layouts above are untouched.
// ===========================================================================

/// Engine-ready total stats (base + equipment folded), captured at publish.
/// Mirrors the web `Stats` fold `opponentSnapshot()` already uses.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ArenaStatsLite {
    pub hp: i16,
    pub attack: i16,
    pub armor: i16,
    pub speed: i16,
}

/// A player's published build ("ghost"): `["arena_snapshot_v1", owner]`.
/// Self-contained + engine-ready so `resolve_challenge` needs no CPI (design §2.1).
#[account]
pub struct ArenaSnapshot {
    /// PDA seed; the ghost's wallet.
    pub owner: Pubkey,
    /// ArenaAssetData avatar-card pubkey (== PlayerAvatar.avatar_asset); the
    /// per-character key its `CharRecord` accrues to.
    pub avatar_ref: Pubkey,
    /// Display handle, utf-8 zero-padded (e.g. "ember_witch").
    pub archetype_id: [u8; 32],
    /// ENGINE-READY total stats used verbatim by the on-chain sim.
    pub stats: ArenaStatsLite,
    /// Active skills as a bitmask (sim canonical order; see `sim.rs`).
    pub skill_mask: u8,
    /// 0 None, 1 Fire, ... (mirrors ArenaElement); display only, ignored by resolve.
    pub element: u8,
    /// Cosmetic ref (avatar skin) — display only, ignored by resolve.
    pub skin_ref: [u8; 32],
    /// Owner rating snapshot at publish time (matchmaking hint only).
    pub rating_at_publish: i32,
    /// Freshness marker.
    pub published_slot: u64,
    pub bump: u8,
}

impl ArenaSnapshot {
    pub const INIT_SPACE: usize = 32 // owner
        + 32 // avatar_ref
        + 32 // archetype_id
        + 8 // stats (4 * i16)
        + 1 // skill_mask
        + 1 // element
        + 32 // skin_ref
        + 4 // rating_at_publish
        + 8 // published_slot
        + 1; // bump

    /// Sim-ready combatant view. Identity = this snapshot ACCOUNT pubkey (the
    /// design §1 parity requirement), promoted stats to i32.
    pub fn combatant(&self, snapshot_key: &Pubkey) -> crate::sim::Combatant {
        crate::sim::Combatant {
            identity: snapshot_key.to_bytes(),
            hp: self.stats.hp as i32,
            attack: self.stats.attack as i32,
            armor: self.stats.armor as i32,
            speed: self.stats.speed as i32,
            skill_mask: self.skill_mask,
        }
    }
}

/// A commit to fight a specific ghost at a future slot:
/// `["challenge_v1", challenger, nonce]` (design §2.2). Locking the opponent +
/// a future target slot at commit is the revert-grind / re-pick defense.
#[account]
pub struct Challenge {
    pub challenger: Pubkey,
    pub nonce: u64,
    /// The ghost ArenaSnapshot chosen AT COMMIT (locks the pairing).
    pub opponent_snapshot: Pubkey,
    /// commit_slot + PVP_REVEAL_DELAY_SLOTS; its hash seeds the fight.
    pub target_slot: u64,
    pub bump: u8,
}

impl Challenge {
    pub const INIT_SPACE: usize = 32 // challenger
        + 8 // nonce
        + 32 // opponent_snapshot
        + 8 // target_slot
        + 1; // bump
}

/// Per-character W/L: `["char_record_v1", owner, avatar_ref]` (design §3).
/// Persists across avatar swaps; keyed by the avatar-card pubkey.
#[account]
pub struct CharRecord {
    pub owner: Pubkey,
    pub avatar_ref: Pubkey,
    pub wins: u32,
    pub losses: u32,
    pub games: u32,
    pub streak: u16,
    pub best_streak: u16,
    pub last_played_slot: u64,
    pub bump: u8,
}

impl CharRecord {
    pub const INIT_SPACE: usize = 32 // owner
        + 32 // avatar_ref
        + 4 // wins
        + 4 // losses
        + 4 // games
        + 2 // streak
        + 2 // best_streak
        + 8 // last_played_slot
        + 1; // bump

    /// Lazy init on a freshly zeroed PDA (recognized by the unset owner).
    pub fn initialize_if_needed(&mut self, owner: Pubkey, avatar_ref: Pubkey, bump: u8) {
        if self.owner == Pubkey::default() {
            self.owner = owner;
            self.avatar_ref = avatar_ref;
            self.bump = bump;
        }
    }

    /// Record one PvP result for this character.
    pub fn record(&mut self, won: bool, slot: u64) {
        self.games = self.games.saturating_add(1);
        if won {
            self.wins = self.wins.saturating_add(1);
            self.streak = self.streak.saturating_add(1);
            self.best_streak = self.best_streak.max(self.streak);
        } else {
            self.losses = self.losses.saturating_add(1);
            self.streak = 0;
        }
        self.last_played_slot = slot;
    }
}

/// Order-independent per-pair rated-result throttle:
/// `["pair_cd_v1", sort(challenger, opponent_owner)]` (design §4). Repeats
/// within the cooldown / daily cap resolve as no-rating exhibitions.
#[account]
pub struct PairCooldown {
    pub key_lo: Pubkey,
    pub key_hi: Pubkey,
    /// Last slot this pair produced a RATED result (0 = never).
    pub last_ranked_slot: u64,
    pub ranked_today: u16,
    pub utc_day: i64,
    pub bump: u8,
}

impl PairCooldown {
    pub const INIT_SPACE: usize = 32 // key_lo
        + 32 // key_hi
        + 8 // last_ranked_slot
        + 2 // ranked_today
        + 8 // utc_day
        + 1; // bump

    /// Sort two wallets into the order-independent `(lo, hi)` pair key.
    pub fn sort_keys(a: Pubkey, b: Pubkey) -> (Pubkey, Pubkey) {
        if a.to_bytes() <= b.to_bytes() {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// Decide whether THIS fight is rated, and consume the pair allowance when
    /// it is. Returns true for a RATED result, false for an exhibition. A rated
    /// result requires: no prior rated result within `PAIR_COOLDOWN_SLOTS`, and
    /// the per-UTC-day rated cap not yet reached.
    pub fn consume_rated(
        &mut self,
        lo: Pubkey,
        hi: Pubkey,
        bump: u8,
        slot: u64,
        unix_timestamp: i64,
    ) -> bool {
        if self.key_lo == Pubkey::default() && self.key_hi == Pubkey::default() {
            self.key_lo = lo;
            self.key_hi = hi;
            self.bump = bump;
        }

        let day = unix_timestamp.div_euclid(SECONDS_PER_DAY);
        if day > self.utc_day {
            self.utc_day = day;
            self.ranked_today = 0;
        }

        let mut rated = true;
        if self.last_ranked_slot != 0 && slot < self.last_ranked_slot + PAIR_COOLDOWN_SLOTS {
            rated = false; // still cooling down from the last rated fight
        }
        if self.ranked_today >= MAX_RANKED_PER_PAIR_PER_DAY {
            rated = false; // per-day rated cap reached
        }

        if rated {
            // slot.max(1) keeps 0 meaningful as the "never rated" sentinel.
            self.last_ranked_slot = slot.max(1);
            self.ranked_today = self.ranked_today.saturating_add(1);
        }
        rated
    }
}

#[cfg(test)]
mod rate_limit_tests {
    use super::*;

    fn limiter() -> BattleRateLimit {
        let player = Pubkey::new_unique();
        let mut limiter = BattleRateLimit {
            player: Pubkey::default(),
            last_battle_slot: 0,
            utc_day: 0,
            battles_today: 0,
            has_recorded_battle: false,
            bump: 0,
        };
        limiter.initialize_if_needed(player, 255);
        limiter
    }

    #[test]
    fn first_battle_at_slot_zero_is_allowed_then_same_slot_is_blocked() {
        let mut limiter = limiter();
        limiter.consume(0, 100).unwrap();
        assert!(limiter.consume(0, 100).is_err());
        limiter.consume(MIN_BATTLE_COOLDOWN_SLOTS, 100).unwrap();
        assert_eq!(limiter.battles_today, 2);
    }

    #[test]
    fn utc_day_rollover_resets_quota_but_clock_rollback_does_not() {
        let mut limiter = limiter();
        let day = 20_000i64;
        for i in 0..MAX_BATTLES_PER_UTC_DAY {
            limiter
                .consume(i as u64 * MIN_BATTLE_COOLDOWN_SLOTS, day * SECONDS_PER_DAY)
                .unwrap();
        }
        assert!(limiter
            .consume(
                MAX_BATTLES_PER_UTC_DAY as u64 * MIN_BATTLE_COOLDOWN_SLOTS,
                day * SECONDS_PER_DAY
            )
            .is_err());

        // Moving backwards must retain the exhausted quota.
        assert!(limiter
            .consume(
                (MAX_BATTLES_PER_UTC_DAY as u64 + 1) * MIN_BATTLE_COOLDOWN_SLOTS,
                (day - 1) * SECONDS_PER_DAY
            )
            .is_err());

        // The next real UTC day grants a fresh quota.
        limiter
            .consume(
                (MAX_BATTLES_PER_UTC_DAY as u64 + 2) * MIN_BATTLE_COOLDOWN_SLOTS,
                (day + 1) * SECONDS_PER_DAY,
            )
            .unwrap();
        assert_eq!(limiter.utc_day, day + 1);
        assert_eq!(limiter.battles_today, 1);
    }
}
