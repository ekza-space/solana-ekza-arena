# Solana Ekza Arena

Anchor program for Ekza Arena: on-chain item NFTs with fair random rolls,
player avatars with equip slots, and a skin-only bridge from Ekza Stellar
releases.

Program ID (localnet/dev): `D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ`

## Concepts

- **Arena card** (`ArenaAssetData`) — a template record: an Avatar or Modifier
  archetype with base stats, skills, equip `slot_mask`, and a cosmetic
  `skin_ref`. Created directly or bridged from a finalized Stellar release
  (skin only — bridged cards carry zero stats).
- **Arena item** (`ArenaItem` + Metaplex NFT) — rolled gear. Every item is a
  real tradeable NFT (SPL mint supply 1 / decimals 0 + Metadata + Master
  Edition); the `ArenaItem` PDA seeded by the mint holds the immutable rolled
  stats. Ownership = current NFT holder, never `minter`.
- **Player avatar** (`PlayerAvatar`) — one per wallet. The player's character:
  chosen Avatar card, display name, cosmetic skin, and four equip slots
  (Weapon/Head/Armor/Charm) holding equipped item NFT mints.
- **Fair randomness** — two mint paths:
  - `mint_arena_item` (1 tx): a registry-authority-only development/admin
    path, seeded from the recent SlotHash and hard-capped at Legendary. Public
    wallets cannot use it to bypass commit-mint economics.
  - `commit_mint` → `reveal_mint` (2 tx): commit locks a *future* slot and
    charges and immediately distributes a non-refundable fee; reveal rolls from
    that slot's then-unknown hash. Grind-resistant — the only path that can roll
    **Mythic (1/1000)**.
  - A commit expires 128 slots after its target. Anyone may clean it up after
    expiry, but all PDA rent returns to the original minter; the fee was already
    distributed and is never refundable or held in the commit.
- **Creator economics** — recommended fee is 0.02 SOL, governed as basis points
  (default 50% creator / 40% platform / 10% sink). Stellar-backed commits send
  the creator slice through `solana_stellar::deposit_revenue` to that release's
  `ReleaseVault`; Builtin/IPFS commits fold the creator slice into platform.
  Distribution happens atomically during commit, so abandoning or missing the
  reveal window cannot strand the paid fee. The Stellar release/vault/asset are
  bound into the commit and cannot be swapped at reveal.
- **Secondary royalty metadata** — item NFTs advertise 5% Metaplex royalties.
  Stellar items name the release authority as creator/distributor; other items
  name the platform treasury. This is legacy marketplace-honored metadata, not
  transfer-hook enforcement.
- **Scrap** (`scrap_arena_item`) — the economic sink: burns the NFT and closes
  the `ArenaItem` PDA, rent back to the holder.

## Instructions

| Instruction | Purpose |
|---|---|
| `configure_registry` | Set authority-guarded fee, creator/platform/sink bps, treasury and sink. |
| `rotate_registry_authority` | Transfer registry governance (current-authority signed). |
| `migrate_registry_v1` | Upgrade-authority/genesis-authority migration of the legacy registry PDA. |
| `register_arena_asset` | Create a direct Arena card. |
| `register_arena_asset_from_stellar` | Bridge a finalized Stellar release as a skin-only card. |
| `mint_arena_item` | 1-tx mint of a rolled item NFT (≤ Legendary). |
| `commit_mint` / `reveal_mint` | Commit-reveal mint (full ladder incl. Mythic). |
| `close_expired_commit` | Permissionlessly close an expired commit; rent returns to its minter. |
| `scrap_arena_item` | Burn item NFT + close its PDA (holder only). |
| `create_player_avatar` | Create the wallet's character from an Avatar card. |
| `customize_avatar` | Rename, change cosmetic skin, or swap the base card (swap clears equips). |
| `equip_item` | Equip an owned item NFT into the slot implied by its base type. |
| `unequip_item` | Clear one equip slot. |

Equipping does not lock the NFT — clients must treat a slot as valid only
while the avatar owner still holds the mint's token.

## Randomness / affix engine

`src/affix.rs` is the canonical deterministic generator (splitmix64):
rarity ladder Common/Rare/Epic/Legendary/Mythic with weights
`[600, 280, 90, 29, 1]`, guaranteed primary affix per base type, rarity-gated
secondary pool. The frontend mirrors it in TypeScript; the golden-vector test
writes `tests/fixtures/affix-golden-vectors.json` (override the location with
`AFFIX_GOLDEN_PATH`) for cross-language assertion.

## Checks

```sh
cargo test -p solana-ekza-arena   # unit tests (affix engine, golden vector)
anchor build
anchor test                       # needs ../solana-stellar built (genesis clone)
yarn lint
```

`Anchor.toml` genesis-clones the sibling `../solana-stellar` program and the
Metaplex Token Metadata program for localnet tests.

The registry account layout now includes configuration authority and fee-split
fields. Existing deployments created with the older 57-byte account use
`migrate_registry_v1`, which validates the exact legacy discriminator/layout,
preserves `next_index`, tops up rent, reallocates to 127 bytes and writes the
current configuration. Bootstrap/migration requires either the deployed
ProgramData upgrade authority or the compile-time `GENESIS_REGISTRY_AUTHORITY`.
That genesis key is a project trust root and **must be reviewed/replaced before
every production deployment**. After bootstrap it has no override: only the
authority stored in the registry may configure or rotate governance.
