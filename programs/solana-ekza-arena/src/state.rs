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
    /// v4 god-roll (~1/1000). Only the commit-reveal mint can produce it
    /// (spec §12.2). The 1-tx dev mint is clamped below this.
    Mythic,
}

impl ArenaRarity {
    pub const INIT_SPACE: usize = 1;

    /// Map a raw affix-generator rarity id (spec §6/§12.2 order) to the enum.
    pub fn from_roll(rarity: u8) -> Self {
        match rarity {
            crate::affix::RARITY_COMMON => ArenaRarity::Common,
            crate::affix::RARITY_RARE => ArenaRarity::Rare,
            crate::affix::RARITY_EPIC => ArenaRarity::Epic,
            crate::affix::RARITY_LEGENDARY => ArenaRarity::Legendary,
            crate::affix::RARITY_MYTHIC => ArenaRarity::Mythic,
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

    /// Equip-slot index into `PlayerAvatar::equipped` (and bit position in the
    /// avatar card `slot_mask`).
    pub fn slot_index(self) -> u8 {
        match self {
            ArenaBaseType::Weapon => 0,
            ArenaBaseType::Head => 1,
            ArenaBaseType::Armor => 2,
            ArenaBaseType::Charm => 3,
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
    /// NFT Metadata `name` (e.g. "Ekza Arena Item #12"). Client builds it.
    pub name: String,
    /// NFT Metadata `symbol` (e.g. "EKZAITEM").
    pub symbol: String,
    /// Off-chain metadata JSON URI (skin image/animation + stats mirror, spec
    /// §11.2). Client uploads to IPFS and passes the gateway/ipfs URI.
    pub uri: String,
}

impl MintArenaItemArgs {
    pub const MAX_NAME_LEN: usize = 32;
    pub const MAX_SYMBOL_LEN: usize = 10;
    pub const MAX_URI_LEN: usize = 200;
}

#[account]
pub struct ArenaRegistry {
    pub next_index: u64,
    /// Destination of the non-refundable commit fee (spec §12.1). Set by
    /// `configure_registry`; default (all-zero) until configured.
    pub treasury: Pubkey,
    /// Non-refundable fee charged at `commit_mint`, in lamports (spec §12.1).
    pub commit_fee_lamports: u64,
    pub bump: u8,
}

impl ArenaRegistry {
    pub const INIT_SPACE: usize = 8 + 32 + 8 + 1;
}

/// Args for `configure_registry` — set the treasury + commit fee (spec §12.1).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfigureRegistryArgs {
    pub treasury: Pubkey,
    pub commit_fee_lamports: u64,
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

/// Forward-compat gem socket (spec §12.3/§4 of rng-economy-upgrades). Reserved
/// now (defaulted empty at mint); the `socket_gem` instruction lands in v5.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Socket {
    pub kind: u8,
    pub value: i16,
}

impl Socket {
    pub const INIT_SPACE: usize = 1 + 2;
    /// Max sockets reserved in `ArenaItem::INIT_SPACE` (spec §12.3).
    pub const MAX_SOCKETS: usize = 3;
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
    /// Original creator of the item. NOT the owner — ownership is the current
    /// holder of the `mint` NFT token (spec §11.2). Gameplay reads must resolve
    /// the owner from the token holder, never from `minter`.
    pub minter: Pubkey,
    /// The SPL mint of the tradeable NFT this item is bound to (1:1). The PDA is
    /// seeded by this mint (spec §11.2).
    pub mint: Pubkey,
    pub index: u64,
    /// Forward-compat (spec §12.3): sharpening level. Defaulted 0 at mint; no
    /// instruction mutates it yet (v5 `sharpen_item`).
    pub enhance_level: u8,
    /// Forward-compat (spec §12.3): gem sockets. Defaulted empty at mint, space
    /// reserved for MAX_SOCKETS; no instruction mutates it yet (v5 `socket_gem`).
    pub sockets: Vec<Socket>,
    pub bump: u8,
}

impl ArenaItem {
    pub const MAX_AFFIXES: usize = crate::affix::MAX_AFFIXES;
    pub const AFFIXES_SPACE: usize = 4 + Self::MAX_AFFIXES * ArenaAffix::INIT_SPACE;
    pub const SOCKETS_SPACE: usize = 4 + Socket::MAX_SOCKETS * Socket::INIT_SPACE;
    pub const INIT_SPACE: usize = 8 // seed
        + ArenaBaseType::INIT_SPACE
        + 1 // tier
        + ArenaRarity::INIT_SPACE
        + Self::AFFIXES_SPACE
        + ItemSkin::INIT_SPACE
        + 32 // minter
        + 32 // mint
        + 8 // index
        + 1 // enhance_level
        + Self::SOCKETS_SPACE
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

// ---------------------------------------------------------------------------
// Player avatar (character customization + on-chain equip).
// ---------------------------------------------------------------------------

/// A player's character: one per wallet (`[PLAYER_AVATAR_SEED, owner]`).
///
/// Holds the chosen Avatar card, a display name, a cosmetic skin, and the
/// equipped item NFTs — one slot per `ArenaBaseType` (Weapon/Head/Armor/Charm).
/// A slot stores the item's **NFT mint** (`Pubkey::default()` = empty).
///
/// IMPORTANT for gameplay reads: equipping does NOT lock the NFT — the owner can
/// still trade it. Clients must treat a slot as valid only while the avatar
/// owner still holds the mint's single token (same holder-resolution rule as
/// `ArenaItem.minter` vs owner).
/// Equip slots on a `PlayerAvatar` — one per `ArenaBaseType`.
pub const EQUIP_SLOT_COUNT: usize = 4;

#[account]
pub struct PlayerAvatar {
    pub owner: Pubkey,
    /// The `ArenaAssetData` Avatar card this character is based on.
    pub avatar_asset: Pubkey,
    /// Player-chosen display name (cosmetic only).
    pub name: String,
    /// Cosmetic skin override; defaults to the avatar card's `skin_ref`.
    pub skin_ref: ItemSkin,
    /// Copy of the avatar card's `slot_mask` at create/swap time — which equip
    /// slots this character supports (bit N = `ArenaBaseType::slot_index()` N).
    pub slot_mask: u8,
    /// Equipped item NFT mints, indexed by `ArenaBaseType::slot_index()`.
    pub equipped: [Pubkey; EQUIP_SLOT_COUNT],
    pub bump: u8,
}

impl PlayerAvatar {
    pub const SLOT_COUNT: usize = EQUIP_SLOT_COUNT;
    pub const MAX_NAME_LEN: usize = 32;
    pub const INIT_SPACE: usize = 32 // owner
        + 32 // avatar_asset
        + 4 + Self::MAX_NAME_LEN
        + ItemSkin::INIT_SPACE
        + 1 // slot_mask
        + 32 * Self::SLOT_COUNT
        + 1; // bump
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CreatePlayerAvatarArgs {
    pub name: String,
}

/// Args for `customize_avatar`. All fields optional — only supplied parts
/// change. Passing a new `avatar_asset` account (see context) swaps the base
/// character and clears all equipped slots.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CustomizeAvatarArgs {
    pub name: Option<String>,
    /// Cosmetic skin override. Only `Builtin`/`Ipfs` are accepted here (Stellar
    /// skins enter via the avatar card's own `skin_ref`).
    pub skin: Option<MintSkinArg>,
}

// ---------------------------------------------------------------------------
// Commit-reveal mint (spec §12.1): the canonical, grind-resistant mint path.
// ---------------------------------------------------------------------------

/// Args for `commit_mint` (spec §12.1). The roll is NOT performed here — only
/// the intent + future-slot lock is persisted; `reveal_mint` rolls from the
/// then-unknown slot hash.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommitMintArgs {
    /// Caller-chosen nonce, part of the `MintCommit` PDA seeds (lets one minter
    /// have many concurrent commits).
    pub nonce: u64,
    pub base_type: ArenaBaseType,
    pub skin: MintSkinArg,
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

/// A pending mint, committed to a FUTURE slot (spec §12.1). `reveal_mint` reads
/// the hash of `target_slot` (unknown at commit time) to derive the seed, making
/// the roll unpredictable and thus revert-grind resistant.
#[account]
pub struct MintCommit {
    pub minter: Pubkey,
    pub nonce: u64,
    /// Slot whose hash seeds the roll; reveal must wait until `Clock::slot` has
    /// passed it AND the hash is still in the SlotHashes sysvar (~512 slots).
    pub target_slot: u64,
    pub base_type: ArenaBaseType,
    pub skin: MintSkinArg,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub bump: u8,
}

impl MintCommit {
    /// Largest `MintSkinArg` payload: tag(1) + Ipfs(4 len + MAX_IPFS_LEN).
    pub const SKIN_SPACE: usize = 1 + 4 + ItemSkin::MAX_IPFS_LEN;
    pub const INIT_SPACE: usize = 32 // minter
        + 8 // nonce
        + 8 // target_slot
        + ArenaBaseType::INIT_SPACE
        + Self::SKIN_SPACE
        + 4 + MintArenaItemArgs::MAX_NAME_LEN
        + 4 + MintArenaItemArgs::MAX_SYMBOL_LEN
        + 4 + MintArenaItemArgs::MAX_URI_LEN
        + 1; // bump
}
