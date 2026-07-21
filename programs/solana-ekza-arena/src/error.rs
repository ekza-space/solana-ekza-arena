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
    #[msg("The committed target slot's hash is no longer in the SlotHashes sysvar.")]
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
}
