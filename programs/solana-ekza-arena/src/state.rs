use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaCardKind {
    Avatar,
    Modifier,
}

impl ArenaCardKind {
    pub const INIT_SPACE: usize = 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaRarity {
    Common,
    Rare,
    Legendary,
    Unique,
    Epic,
}

impl ArenaRarity {
    pub const INIT_SPACE: usize = 1;

    /// Map a raw affix-generator rarity id (spec §6 order) to the enum.
    pub fn from_roll(rarity: u8) -> Self {
        match rarity {
            crate::affix::RARITY_COMMON => ArenaRarity::Common,
            crate::affix::RARITY_RARE => ArenaRarity::Rare,
            crate::affix::RARITY_EPIC => ArenaRarity::Epic,
            crate::affix::RARITY_LEGENDARY => ArenaRarity::Legendary,
            _ => ArenaRarity::Common,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaBaseType {
    Weapon,
    Head,
    Armor,
    Charm,
}

impl ArenaBaseType {
    pub const INIT_SPACE: usize = 1;

    /// Affix-generator base-type id (spec §2/§5 ordering: Weapon..Charm).
    pub fn to_roll(self) -> u8 {
        match self {
            ArenaBaseType::Weapon => crate::affix::BASE_WEAPON,
            ArenaBaseType::Head => crate::affix::BASE_HEAD,
            ArenaBaseType::Armor => crate::affix::BASE_ARMOR,
            ArenaBaseType::Charm => crate::affix::BASE_CHARM,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaElement {
    None,
    Fire,
    Ice,
    Poison,
    Holy,
}

impl ArenaElement {
    pub const INIT_SPACE: usize = 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArenaStats {
    pub hp: i16,
    pub attack: i16,
    pub armor: i16,
    pub speed: i16,
}

impl ArenaStats {
    pub const INIT_SPACE: usize = 2 + 2 + 2 + 2;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterArenaAssetArgs {
    pub metadata_ipfs_hash: String,
    pub card_kind: ArenaCardKind,
    pub archetype_id: String,
    pub base_stats: ArenaStats,
    pub stat_delta: ArenaStats,
    pub slot_mask: u8,
    pub rarity: ArenaRarity,
    pub element: ArenaElement,
    pub skill_ids: Vec<String>,
}

/// Skin-only args for `register_arena_asset_from_stellar` (spec §2/§7/§8b).
///
/// A Stellar publish enters Arena as a SKIN ONLY — it carries zero balance.
/// Deliberately drops `base_stats/stat_delta/rarity/element` so the Stellar
/// caller cannot inject stats; balance comes from a later `mint_arena_item`
/// roll. Only the cosmetic/identity fields cross the bridge.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisterArenaAssetFromStellarArgs {
    pub metadata_ipfs_hash: String,
    pub card_kind: ArenaCardKind,
    pub archetype_id: String,
    pub slot_mask: u8,
    pub skill_ids: Vec<String>,
}

/// Requested skin source for a minted item (cosmetic only, spec §2).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum MintSkinArg {
    /// Built-in skin id (validated against MAX_BUILTIN_SKINS).
    Builtin(u8),
    /// IPFS hash for a self-hosted skin.
    Ipfs(String),
    /// Resolve the skin from the supplied Stellar release accounts.
    Stellar,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct MintArenaItemArgs {
    pub base_type: ArenaBaseType,
    pub skin: MintSkinArg,
}

#[account]
pub struct ArenaRegistry {
    pub next_index: u64,
    pub bump: u8,
}

impl ArenaRegistry {
    pub const INIT_SPACE: usize = 8 + 1;
}

#[account]
pub struct ArenaAssetData {
    pub metadata_ipfs_hash: String,
    pub creator: Pubkey,
    pub index: u64,
    pub card_kind: ArenaCardKind,
    pub archetype_id: String,
    pub base_stats: ArenaStats,
    pub stat_delta: ArenaStats,
    pub slot_mask: u8,
    pub rarity: ArenaRarity,
    pub element: ArenaElement,
    pub skill_ids: Vec<String>,
    /// Cosmetic skin source (spec §2/§8b). Stellar-bridged assets store
    /// `StellarAsset(<asset pubkey>)`; direct/manual cards default to a builtin.
    pub skin_ref: ItemSkin,
    pub bump: u8,
}

impl ArenaAssetData {
    pub const MAX_METADATA_HASH_LEN: usize = 200;
    pub const MAX_ARCHETYPE_ID_LEN: usize = 64;
    pub const MAX_SKILL_IDS: usize = 8;
    pub const MAX_SKILL_ID_LEN: usize = 40;
    pub const SKILL_IDS_SPACE: usize = 4 + Self::MAX_SKILL_IDS * (4 + Self::MAX_SKILL_ID_LEN);
    pub const INIT_SPACE: usize = 4
        + Self::MAX_METADATA_HASH_LEN
        + 32
        + 8
        + ArenaCardKind::INIT_SPACE
        + 4
        + Self::MAX_ARCHETYPE_ID_LEN
        + ArenaStats::INIT_SPACE
        + ArenaStats::INIT_SPACE
        + 1
        + ArenaRarity::INIT_SPACE
        + ArenaElement::INIT_SPACE
        + Self::SKILL_IDS_SPACE
        + ItemSkin::INIT_SPACE
        + 1;
}

#[account]
pub struct StellarArenaAssetLink {
    pub arena_asset: Pubkey,
    pub stellar_program: Pubkey,
    pub universe: Pubkey,
    pub asset: Pubkey,
    pub release: Pubkey,
    pub vault: Pubkey,
    pub bump: u8,
}

impl StellarArenaAssetLink {
    pub const INIT_SPACE: usize = 32 + 32 + 32 + 32 + 32 + 32 + 1;
}

#[account]
pub struct StellarReleaseLink {
    pub release: Pubkey,
    pub stellar_program: Pubkey,
    pub universe: Pubkey,
    pub asset: Pubkey,
    pub vault: Pubkey,
    pub arena_asset: Pubkey,
    pub bump: u8,
}

impl StellarReleaseLink {
    pub const INIT_SPACE: usize = 32 + 32 + 32 + 32 + 32 + 32 + 1;
}

// ---------------------------------------------------------------------------
// Rolled gear (spec §7): deterministic, seed-driven ArenaItem PDA.
// ---------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaAffix {
    pub kind: u8,
    pub value: i16,
    pub element: u8,
}

impl ArenaAffix {
    pub const INIT_SPACE: usize = 1 + 2 + 1;
}

/// Skin source — purely cosmetic, carries zero balance impact (spec §2).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum ItemSkin {
    Ipfs(String),
    StellarAsset(Pubkey),
    Builtin(u8),
}

impl ItemSkin {
    pub const MAX_IPFS_LEN: usize = 96;
    /// enum tag (1) + largest variant: Ipfs => 4 (len prefix) + MAX_IPFS_LEN.
    pub const INIT_SPACE: usize = 1 + 4 + Self::MAX_IPFS_LEN;
}

#[account]
pub struct ArenaItem {
    pub seed: u64,
    pub base_type: ArenaBaseType,
    pub tier: u8,
    pub rarity: ArenaRarity,
    pub affixes: Vec<ArenaAffix>,
    pub skin_ref: ItemSkin,
    pub minter: Pubkey,
    pub index: u64,
    pub bump: u8,
}

impl ArenaItem {
    pub const MAX_AFFIXES: usize = crate::affix::MAX_AFFIXES;
    pub const AFFIXES_SPACE: usize = 4 + Self::MAX_AFFIXES * ArenaAffix::INIT_SPACE;
    pub const INIT_SPACE: usize = 8 // seed
        + ArenaBaseType::INIT_SPACE
        + 1 // tier
        + ArenaRarity::INIT_SPACE
        + Self::AFFIXES_SPACE
        + ItemSkin::INIT_SPACE
        + 32 // minter
        + 8 // index
        + 1; // bump

    /// Derived view: fold the flat affixes into the legacy stat block so the
    /// off-chain battle math (`with_delta` semantics) is preserved (spec §7).
    pub fn stat_delta(&self) -> ArenaStats {
        let mut delta = ArenaStats::default();
        for affix in &self.affixes {
            match affix.kind {
                crate::affix::KIND_FLAT_HP => delta.hp = delta.hp.saturating_add(affix.value),
                crate::affix::KIND_FLAT_ATK => {
                    delta.attack = delta.attack.saturating_add(affix.value)
                }
                crate::affix::KIND_FLAT_ARMOR => {
                    delta.armor = delta.armor.saturating_add(affix.value)
                }
                crate::affix::KIND_FLAT_SPEED => {
                    delta.speed = delta.speed.saturating_add(affix.value)
                }
                _ => {}
            }
        }
        delta
    }

    /// First element-bearing affix, mapped to the engine `ArenaElement`.
    pub fn element(&self) -> ArenaElement {
        let raw = self
            .affixes
            .iter()
            .map(|a| a.element)
            .find(|&e| e != crate::affix::ELEM_NONE)
            .unwrap_or(crate::affix::ELEM_NONE);
        match raw {
            crate::affix::ELEM_FIRE => ArenaElement::Fire,
            crate::affix::ELEM_ICE => ArenaElement::Ice,
            crate::affix::ELEM_POISON => ArenaElement::Poison,
            _ => ArenaElement::None,
        }
    }
}
