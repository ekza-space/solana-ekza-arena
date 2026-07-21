use anchor_lang::prelude::*;

#[error_code]
pub enum LeaderboardError {
    #[msg("Leaderboard capacity out of range (min 2, max 1000; production boards use 100..=1000).")]
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
}
