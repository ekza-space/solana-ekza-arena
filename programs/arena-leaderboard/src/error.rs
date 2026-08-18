use anchor_lang::prelude::*;

#[error_code]
pub enum LeaderboardError {
    #[msg(
        "Leaderboard capacity out of range (min 2, max 1000; production boards use 100..=1000)."
    )]
    CapacityOutOfRange,
    #[msg("The leaderboard is already initialized.")]
    AlreadyInitialized,
    #[msg("Signer is neither the player wallet nor the registered session key.")]
    SessionKeyMismatch,
    #[msg("Player is not currently in the top list; profiles are a top-list perk.")]
    NotInTopList,
    #[msg("Invalid profile name (empty or longer than 32 bytes).")]
    InvalidProfileName,
    #[msg("Invalid profile uri (longer than 96 bytes).")]
    InvalidProfileUri,
    #[msg("Numerical overflow occurred.")]
    NumericalOverflow,
    #[msg("Unauthorized action.")]
    Unauthorized,
    #[msg("Battle recording is on cooldown for this player; wait for the next slot.")]
    BattleCooldownActive,
    #[msg("This player has reached the maximum number of battle records for the current UTC day.")]
    DailyBattleLimitReached,
    // --- Async PvP (ghost snapshots + commit/reveal challenges) ---
    #[msg("A challenge cannot target your own published snapshot.")]
    SelfSnapshotNotAllowed,
    #[msg("The challenge's target slot has not passed yet; reveal is too early.")]
    RevealTooEarly,
    #[msg("The challenge's reveal window has expired; close it to reclaim rent.")]
    ChallengeWindowExpired,
    #[msg("The challenge is not expired yet.")]
    ChallengeNotExpired,
    #[msg("Invalid SlotHashes sysvar account.")]
    InvalidSlotHashes,
    #[msg(
        "The canonical slot hash at/after the committed target is not provable from SlotHashes."
    )]
    SlotHashNotFound,
    #[msg("Snapshot/PDA account does not match the challenge it is resolved against.")]
    SnapshotMismatch,
    #[msg("The provided pair-cooldown key pair is not the sorted (challenger, opponent) pair.")]
    InvalidPairKeys,
}
