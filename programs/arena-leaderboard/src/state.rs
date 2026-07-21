use anchor_lang::prelude::*;

use crate::constants::MAX_CAPACITY;

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
    /// Returns true when the player holds a slot after the call.
    pub fn upsert(&mut self, player: Pubkey, rating: i32, wins: u32) -> bool {
        if let Some(i) = self.find(&player) {
            self.entries[i].rating = rating;
            self.entries[i].wins = wins;
            let i = self.sift_up(i);
            self.sift_down(i);
            return true;
        }
        let entry = HeapEntry { player, rating, wins };
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
