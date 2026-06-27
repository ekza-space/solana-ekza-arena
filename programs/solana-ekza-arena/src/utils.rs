use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke,
    },
};

use crate::{
    constants::{
        LINK_AVATAR_DATA_DISCRIMINATOR, RECORD_RELEASE_DEPLOYMENT_DISCRIMINATOR,
        RELEASE_ACCOUNT_DISCRIMINATOR, RELEASE_ASSET_OFFSET, RELEASE_STATUS_FINALIZED,
        RELEASE_STATUS_LINKED, RELEASE_STATUS_OFFSET, RELEASE_UNIVERSE_OFFSET,
        RELEASE_VAULT_OFFSET, SOLANA_STELLAR_PROGRAM_ID, UNIVERSE_ACCOUNT_DISCRIMINATOR,
        UNIVERSE_OWNER_OFFSET,
    },
    error::ArenaRegistryError,
};

pub struct StellarReleaseOrigin {
    pub universe: Pubkey,
    pub asset: Pubkey,
    pub vault: Pubkey,
    pub status: u8,
}

fn read_pubkey(data: &[u8], offset: usize, error: ArenaRegistryError) -> Result<Pubkey> {
    if data.len() < offset + 32 {
        return Err(error.into());
    }
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&data[offset..offset + 32]);
    Ok(Pubkey::new_from_array(bytes))
}

pub fn validate_stellar_release<'info>(
    stellar_program: &AccountInfo<'info>,
    release: &AccountInfo<'info>,
    vault: &AccountInfo<'info>,
) -> Result<StellarReleaseOrigin> {
    require_keys_eq!(
        *stellar_program.key,
        SOLANA_STELLAR_PROGRAM_ID,
        ArenaRegistryError::InvalidStellarProgram
    );
    require!(
        stellar_program.executable,
        ArenaRegistryError::InvalidStellarProgram
    );
    require_keys_eq!(
        *release.owner,
        *stellar_program.key,
        ArenaRegistryError::InvalidStellarRelease
    );

    let release_data = release.try_borrow_data()?;
    require!(
        release_data.len() > RELEASE_STATUS_OFFSET,
        ArenaRegistryError::InvalidStellarRelease
    );
    require!(
        release_data.get(..8) == Some(RELEASE_ACCOUNT_DISCRIMINATOR.as_ref()),
        ArenaRegistryError::InvalidStellarRelease
    );

    let stored_universe = read_pubkey(
        &release_data,
        RELEASE_UNIVERSE_OFFSET,
        ArenaRegistryError::InvalidStellarRelease,
    )?;
    let stored_asset = read_pubkey(
        &release_data,
        RELEASE_ASSET_OFFSET,
        ArenaRegistryError::InvalidStellarRelease,
    )?;
    let stored_vault = read_pubkey(
        &release_data,
        RELEASE_VAULT_OFFSET,
        ArenaRegistryError::InvalidStellarRelease,
    )?;
    require_keys_eq!(
        stored_vault,
        *vault.key,
        ArenaRegistryError::InvalidStellarVault
    );

    let status = release_data[RELEASE_STATUS_OFFSET];
    require!(
        status == RELEASE_STATUS_FINALIZED || status == RELEASE_STATUS_LINKED,
        ArenaRegistryError::InvalidStellarRelease
    );

    Ok(StellarReleaseOrigin {
        universe: stored_universe,
        asset: stored_asset,
        vault: stored_vault,
        status,
    })
}

pub fn validate_stellar_universe_owner<'info>(
    stellar_program: &AccountInfo<'info>,
    universe: &AccountInfo<'info>,
    expected_universe: Pubkey,
    expected_owner: Pubkey,
) -> Result<()> {
    require_keys_eq!(
        *universe.key,
        expected_universe,
        ArenaRegistryError::InvalidStellarUniverse
    );
    require_keys_eq!(
        *universe.owner,
        *stellar_program.key,
        ArenaRegistryError::InvalidStellarUniverse
    );

    let universe_data = universe.try_borrow_data()?;
    require!(
        universe_data.get(..8) == Some(UNIVERSE_ACCOUNT_DISCRIMINATOR.as_ref()),
        ArenaRegistryError::InvalidStellarUniverse
    );
    let stored_owner = read_pubkey(
        &universe_data,
        UNIVERSE_OWNER_OFFSET,
        ArenaRegistryError::InvalidStellarUniverse,
    )?;
    require_keys_eq!(
        stored_owner,
        expected_owner,
        ArenaRegistryError::Unauthorized
    );

    Ok(())
}

pub fn link_arena_asset_to_stellar<'info>(
    arena_asset: Pubkey,
    owner: &AccountInfo<'info>,
    stellar_program: &AccountInfo<'info>,
    universe: &AccountInfo<'info>,
    release: &AccountInfo<'info>,
) -> Result<()> {
    let mut data = Vec::with_capacity(40);
    data.extend_from_slice(&LINK_AVATAR_DATA_DISCRIMINATOR);
    data.extend_from_slice(arena_asset.as_ref());

    let ix = Instruction {
        program_id: *stellar_program.key,
        accounts: vec![
            AccountMeta::new_readonly(*universe.key, false),
            AccountMeta::new(*release.key, false),
            AccountMeta::new_readonly(*owner.key, true),
        ],
        data,
    };

    invoke(
        &ix,
        &[
            universe.clone(),
            release.clone(),
            owner.clone(),
            stellar_program.clone(),
        ],
    )?;

    Ok(())
}

pub fn record_release_deployment_to_stellar<'info>(
    project_slug: &str,
    registry_program: Pubkey,
    registry_record: Pubkey,
    owner: &AccountInfo<'info>,
    stellar_program: &AccountInfo<'info>,
    release: &AccountInfo<'info>,
    deployment: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
) -> Result<()> {
    let slug_bytes = project_slug.as_bytes();
    let mut data = Vec::with_capacity(8 + 4 + slug_bytes.len() + 32 + 32);
    data.extend_from_slice(&RECORD_RELEASE_DEPLOYMENT_DISCRIMINATOR);
    data.extend_from_slice(&(slug_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(slug_bytes);
    data.extend_from_slice(registry_program.as_ref());
    data.extend_from_slice(registry_record.as_ref());

    let ix = Instruction {
        program_id: *stellar_program.key,
        accounts: vec![
            AccountMeta::new_readonly(*release.key, false),
            AccountMeta::new(*deployment.key, false),
            AccountMeta::new(*owner.key, true),
            AccountMeta::new_readonly(*system_program.key, false),
        ],
        data,
    };

    invoke(
        &ix,
        &[
            release.clone(),
            deployment.clone(),
            owner.clone(),
            system_program.clone(),
            stellar_program.clone(),
        ],
    )?;

    Ok(())
}
