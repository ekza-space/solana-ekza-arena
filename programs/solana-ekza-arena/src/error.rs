use anchor_lang::prelude::*;

#[error_code]
pub enum ArenaRegistryError {
    #[msg("Invalid metadata URI or IPFS hash length.")]
    InvalidMetadataLength,
    #[msg("Invalid Arena archetype id.")]
    InvalidArchetypeId,
    #[msg("Invalid Arena skill id list.")]
    InvalidSkillIds,
    #[msg("Invalid Arena slot mask.")]
    InvalidSlotMask,
    #[msg("Invalid Arena stats for this card kind.")]
    InvalidStats,
    #[msg("Numerical overflow occurred.")]
    NumericalOverflow,
    #[msg("Unauthorized action.")]
    Unauthorized,
    #[msg("Invalid Stellar program.")]
    InvalidStellarProgram,
    #[msg("Invalid Stellar universe account.")]
    InvalidStellarUniverse,
    #[msg("Invalid Stellar release account.")]
    InvalidStellarRelease,
    #[msg("Invalid Stellar vault account.")]
    InvalidStellarVault,
    #[msg("Invalid item skin reference.")]
    InvalidSkin,
    #[msg("A Stellar skin was requested but the Stellar accounts were not supplied.")]
    MissingStellarSkinAccounts,
    #[msg("Invalid SlotHashes sysvar account.")]
    InvalidSlotHashes,
    #[msg("Invalid NFT metadata (name/symbol/uri length).")]
    InvalidNftMetadata,
    #[msg("The signer is not the current holder of the item NFT.")]
    NotNftHolder,
    #[msg("Reveal attempted before the committed target slot has passed.")]
    RevealTooEarly,
    #[msg(
        "The canonical slot hash at/after the committed target is not provable from SlotHashes."
    )]
    SlotHashNotFound,
    #[msg("The committed reveal window has expired.")]
    RevealWindowExpired,
    #[msg("The mint commitment has not expired yet.")]
    CommitNotExpired,
    #[msg("The registry has not been configured with a treasury / commit fee.")]
    RegistryNotConfigured,
    #[msg("Only the configured registry authority may use the privileged development mint.")]
    QuickMintRestricted,
    #[msg("The supplied treasury account does not match the configured treasury.")]
    InvalidTreasury,
    #[msg("The supplied sink account does not match the configured protocol sink.")]
    InvalidSink,
    #[msg("Fee split basis points must sum to exactly 10,000.")]
    InvalidFeeSplit,
    #[msg("A fee destination must already exist or remain rent-exempt after its fee slice is transferred.")]
    FeeDestinationNotRentExempt,
    #[msg("The Stellar release supplied at reveal does not match the paid commit.")]
    StellarCommitMismatch,
    #[msg("The registry can only be bootstrapped by a trusted or program upgrade authority.")]
    UnauthorizedRegistryBootstrap,
    #[msg("The supplied program/program-data accounts are invalid.")]
    InvalidProgramData,
    #[msg("The new registry authority cannot be the default public key.")]
    InvalidConfigurationAuthority,
    #[msg("The registry account does not have the supported legacy v1 layout.")]
    InvalidRegistryMigration,
    #[msg("The referenced arena asset is not an Avatar card.")]
    InvalidAvatarAsset,
    #[msg("Invalid avatar name (empty or too long).")]
    InvalidAvatarName,
    #[msg("The avatar does not support this equip slot.")]
    InvalidEquipSlot,
    #[msg("The item's base type cannot be equipped into the requested slot.")]
    ItemSlotMismatch,
    #[msg("The item is already at the maximum enhancement level.")]
    EnhanceLevelMaxed,
    #[msg("An enhancement commit is already pending for this item.")]
    EnhancePending,
    #[msg("The supplied account does not match the enhancement commit.")]
    EnhanceCommitMismatch,
    #[msg("The signer is not the current holder of the enhancement scroll NFT.")]
    NotScrollHolder,
    #[msg("The item is currently equipped; unequip it before enhancing.")]
    ItemEquipped,
    #[msg("Playable fighter symbol must be exactly EKZAF0, EKZAF1, EKZAF2, or EKZAF3.")]
    InvalidFighterSymbol,
    #[msg("The mint commitment is not a valid playable-fighter intent.")]
    InvalidFighterCommit,
    #[msg("Invalid playable-fighter NFT name or metadata URI.")]
    InvalidFighterMetadata,
    #[msg("EKZAF0..3 are reserved for reveal_avatar_mint and cannot be revealed as gear.")]
    FighterSymbolReserved,
    #[msg("The signer is not the current holder of the fighter NFT.")]
    NotFighterHolder,
    #[msg("The supplied fighter mint is not a canonical 1/1 Arena fighter NFT.")]
    InvalidFighterMint,
    #[msg("The supplied minted-fighter Avatar PDA is invalid.")]
    InvalidFighterAvatar,
    #[msg("The v2 equipment record identity is invalid.")]
    InvalidEquipmentRecord,
    #[msg("Protocol-minted fighters must be selected through activate_fighter_v2.")]
    FighterActivationRequired,
}
