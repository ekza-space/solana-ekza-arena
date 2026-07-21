//! The Stellar gate — Arena side.
//!
//! Everything here goes through the `solana-stellar` crate (typed `state::*`
//! accounts + generated `cpi::*` clients), never hand-rolled offsets or
//! discriminators: if the upstream `Release`/`Universe` layout or instruction
//! signatures change, this file fails to COMPILE instead of silently reading
//! garbage. This is the reference consumer implementation of the gate contract
//! (see solana-stellar/docs/INTEGRATION.md).

use anchor_lang::prelude::*;
use solana_stellar::state::{Release, ReleaseStatus, Universe};

use crate::error::ArenaRegistryError;

/// Identity of a validated Stellar release, as read from the typed account.
pub struct StellarReleaseOrigin {
    pub universe: Pubkey,
    pub asset: Pubkey,
    pub vault: Pubkey,
    pub authority: Pubkey,
    pub status: ReleaseStatus,
}

/// Validate a solana-stellar `Release` account and return its identity.
///
/// Checks: the supplied program IS solana-stellar and executable, the release
/// is owned by it, deserializes as a `Release` (discriminator enforced by
/// `try_deserialize`), the stored vault matches, and the status is
/// `Finalized` or `Linked` (the only publishable states).
pub fn validate_stellar_release<'info>(
    stellar_program: &AccountInfo<'info>,
    release: &AccountInfo<'info>,
    vault: &AccountInfo<'info>,
) -> Result<StellarReleaseOrigin> {
    require_keys_eq!(
        *stellar_program.key,
        solana_stellar::ID,
        ArenaRegistryError::InvalidStellarProgram
    );
    require!(
        stellar_program.executable,
        ArenaRegistryError::InvalidStellarProgram
    );
    require_keys_eq!(
        *release.owner,
        solana_stellar::ID,
        ArenaRegistryError::InvalidStellarRelease
    );

    let release_data = release.try_borrow_data()?;
    let release_account = Release::try_deserialize(&mut release_data.as_ref())
        .map_err(|_| ArenaRegistryError::InvalidStellarRelease)?;

    require_keys_eq!(
        release_account.vault,
        *vault.key,
        ArenaRegistryError::InvalidStellarVault
    );
    require!(
        matches!(
            release_account.status,
            ReleaseStatus::Finalized | ReleaseStatus::Linked
        ),
        ArenaRegistryError::InvalidStellarRelease
    );

    Ok(StellarReleaseOrigin {
        universe: release_account.universe,
        asset: release_account.asset,
        vault: release_account.vault,
        authority: release_account.authority,
        status: release_account.status,
    })
}

/// CPI `deposit_revenue`: atomically transfers SOL from the transaction signer
/// into the Stellar release vault and advances Stellar's accounted
/// `total_deposited_lamports`. Paying during commit (while the wallet is still
/// the signer) means an abandoned reveal can never strand creator funds.
pub fn deposit_revenue_to_stellar<'info>(
    amount: u64,
    payer: &AccountInfo<'info>,
    stellar_program: &AccountInfo<'info>,
    release: &AccountInfo<'info>,
    vault: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
) -> Result<()> {
    solana_stellar::cpi::deposit_revenue(
        CpiContext::new(
            stellar_program.clone(),
            solana_stellar::cpi::accounts::DepositRevenue {
                release: release.clone(),
                vault: vault.clone(),
                payer: payer.clone(),
                system_program: system_program.clone(),
            },
        ),
        amount,
    )
}

/// Validate that `universe` is the release's universe, is a real solana-stellar
/// `Universe` account, and is owned by `expected_owner` (the publisher).
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
    let universe_account = Universe::try_deserialize(&mut universe_data.as_ref())
        .map_err(|_| ArenaRegistryError::InvalidStellarUniverse)?;
    require_keys_eq!(
        universe_account.owner,
        expected_owner,
        ArenaRegistryError::Unauthorized
    );

    Ok(())
}

/// CPI `link_avatar_data`: bind the Arena card back into the Stellar release
/// (Finalized → Linked). Signer must be the universe owner.
pub fn link_arena_asset_to_stellar<'info>(
    arena_asset: Pubkey,
    owner: &AccountInfo<'info>,
    stellar_program: &AccountInfo<'info>,
    universe: &AccountInfo<'info>,
    release: &AccountInfo<'info>,
) -> Result<()> {
    solana_stellar::cpi::link_avatar_data(
        CpiContext::new(
            stellar_program.clone(),
            solana_stellar::cpi::accounts::LinkAvatarData {
                universe: universe.clone(),
                release: release.clone(),
                owner: owner.clone(),
            },
        ),
        arena_asset,
    )
}

/// CPI `record_release_deployment`: write the per-project bridge record on the
/// Stellar side (seeds `[b"release_deployment", release, project_slug]`).
/// Signer must be the release authority.
// Anchor CPI account plumbing is intentionally explicit at this boundary.
#[allow(clippy::too_many_arguments)]
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
    solana_stellar::cpi::record_release_deployment(
        CpiContext::new(
            stellar_program.clone(),
            solana_stellar::cpi::accounts::RecordReleaseDeployment {
                release: release.clone(),
                deployment: deployment.clone(),
                authority: owner.clone(),
                system_program: system_program.clone(),
            },
        ),
        project_slug.to_string(),
        registry_program,
        registry_record,
    )
}
