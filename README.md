# Solana Ekza Arena

Anchor program for publishing finalized Ekza Stellar releases into Arena-owned
card records.

## Current Base

- `register_arena_asset` creates a direct Arena card record.
- `register_arena_asset_from_stellar` validates a Stellar release, stores the
  Arena card record, writes release/link PDAs, and records the Stellar
  `ReleaseDeployment` under project slug `arena`.

The program does not mint Arena NFTs yet. NFT mint and equip accounts should use
`ArenaAssetData` as the source template for avatar and modifier entities.

## Checks

```sh
anchor build
anchor test
yarn lint
```
