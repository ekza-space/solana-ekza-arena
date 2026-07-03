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
    #[msg("The registry has not been configured with a treasury / commit fee.")]
    RegistryNotConfigured,
    #[msg("The supplied treasury account does not match the configured treasury.")]
    InvalidTreasury,
    #[msg("The referenced arena asset is not an Avatar card.")]
    InvalidAvatarAsset,
    #[msg("Invalid avatar name (empty or too long).")]
    InvalidAvatarName,
    #[msg("The avatar does not support this equip slot.")]
    InvalidEquipSlot,
}
