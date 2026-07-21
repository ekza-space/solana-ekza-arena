# Stellar → Arena bridge loop (validated round-trip)

This documents the **end-to-end publish loop** that takes a character/asset
created on `solana-stellar` and publishes it into `solana-ekza-arena` as a
readable Arena card. It is the loop exercised by:

- `tests/solana-ekza-arena.ts` → test _"registers a Stellar release as an Arena
  asset and records deployment"_ (genesis-clones stellar, self-contained), and
- `scripts/validate-bridge.ts` → standalone, repeatable proof against a running
  localnet with **both** programs deployed.

Both were run and **PASS** (see "Re-run" below).

## Programs

| Program | ID |
| --- | --- |
| `solana_stellar` (publish source) | `3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA` |
| `solana_ekza_arena` (publish target) | `D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ` |
| Metaplex Token Metadata (dependency) | `metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s` |

## The loop, step by step

### 1. Build a finalized Stellar release (the asset/character)

All instructions on `solana_stellar`, signed by the owner wallet:

| # | Instruction | Key accounts (PDA seeds) |
| --- | --- | --- |
| 1 | `create_universe(owner_index, ipfs, kind, public)` | `registry` = `["registry"]`; `universe` = `["universe", owner, owner_index_le]`; `universe_lookup` = `["universe_index", global_index_le]` |
| 2 | `create_asset(0, kind, maturity, license, ipfs, preview, …)` | `asset` = `["asset", universe, 0_le]` |
| 3 | `submit_asset()` | `asset`, `creator` |
| 4 | `approve_asset()` | `universe`, `asset`, `owner` |
| 5 | `create_release(0, ipfs)` | `release` = `["release", universe, 0_le]`; `vault` = `["release_vault", release]` |
| 6 | `add_release_share(10_000)` | `share` = `["share", release, contributor]` (100% to owner) |
| 7 | `finalize_release()` | `universe`, `release`, `asset`, `owner` |

After step 7 the `Release` is `Finalized` and eligible to be published.

### 2. Publish into Arena (the bridge instruction)

One instruction on `solana_ekza_arena`:

```
register_arena_asset_from_stellar({
  metadataIpfsHash,          // Arena card metadata
  cardKind: { avatar: {} },  // Avatar | Modifier | …
  archetypeId,               // e.g. "arena_bridge_avatar"
  slotMask,                  // equip-slot bitmask (must be non-zero)
  skillIds,                  // e.g. ["moss_skin"]
})
```

Accounts (`accountsStrict`):

| Account | PDA seeds / value | Program |
| --- | --- | --- |
| `registry` | `["arena_registry"]` | arena |
| `arenaAsset` | `["arena_asset_v1", next_index_le]` | arena |
| `payer` | owner wallet | — |
| `stellarLink` | `["stellar_arena_link", arenaAsset]` | arena |
| `stellarProgram` | `3rVXfq…rxPZAA` | — |
| `stellarUniverse` | `["universe", owner, owner_index_le]` | stellar |
| `stellarRelease` | `["release", universe, 0_le]` | stellar |
| `stellarVault` | `["release_vault", release]` | stellar |
| `stellarReleaseDeployment` | `["release_deployment", release, "arena"]` | stellar |
| `stellarReleaseLink` | `["stellar_release_link", release]` | arena |
| `systemProgram` | `1111…1111` | — |

This is an **identity-only** publish (spec §8b): the Stellar release carries the
skin/identity, not the balance. `base_stats` / `stat_delta` / `rarity` /
`element` are forced to neutral/zero on-chain even though the caller cannot
supply them here. The instruction also CPIs into `solana_stellar` to record a
`ReleaseDeployment` and flip the `Release` to `Linked`.

### 3. Read back (the proof)

Fetch and decode the `ArenaAssetData` PDA (`["arena_asset_v1", index_le]`).
This is the same decode used by the read side
(`ekza-stellar-sdk` chain module `decodeArenaAssetData` /
`ekza-arena-web/src/lib/chain`). The round-trip asserts:

- `card_kind` == `{ avatar: {} }`
- `archetype_id` == the published archetype (`"arena_bridge_avatar"`)
- `slot_mask` == `3`
- `metadata_ipfs_hash` preserved
- `skin_ref` == `StellarAsset(<stellar asset pubkey>)` — the Arena card points
  back at the exact Stellar asset it was published from
- `base_stats` + `stat_delta` are all zero (identity-only)

Cross-account proof:

- Arena `StellarArenaAssetLink` (`["stellar_arena_link", arenaAsset]`):
  `arena_asset` / `release` / `asset` all match.
- Arena `StellarReleaseLink` (`["stellar_release_link", release]`):
  `arena_asset` matches.
- Stellar `ReleaseDeployment` (`["release_deployment", release, "arena"]`):
  `project_slug == "arena"`, `registry_program == D3a99…SM8iZ`,
  `registry_record == arenaAsset`.
- Stellar `Release`: `status == Linked`, `linked_avatar_data == arenaAsset`.

## Re-run

### Standalone script (against a live localnet)

Bring up a localnet with **both** programs + Metaplex Token Metadata. The Arena
`Anchor.toml` already genesis-clones `../solana-stellar` and
`tests/fixtures/token_metadata.so`, so `anchor localnet` from this repo is enough:

```sh
# terminal 1 — from solana-ekza-arena/
anchor build            # once, if target/ is stale
anchor localnet --skip-build   # validator with arena + stellar + metaplex

# terminal 2 — from solana-ekza-arena/
yarn validate:bridge
#   or: TS_NODE_TRANSPILE_ONLY=1 node_modules/.bin/ts-node scripts/validate-bridge.ts
```

Overridable via env: `RPC_URL` (default `http://127.0.0.1:8899`), `WALLET`
(default `~/.config/solana/id.json`). Exit code `0` = PASS, `1` = FAIL; the
script prints a per-assertion PASS/FAIL table and the on-chain addresses.

### Reference test (fully self-contained)

```sh
anchor test    # genesis-clones stellar + metaplex, runs the whole suite
```

The relevant case is _"registers a Stellar release as an Arena asset and records
deployment"_.

## Last validated run (localnet, both programs deployed)

```
[1] Stellar release created
    stellar universe = HikzYTWj5mTusUGhoudgqWaDD7aCxNeL6gda7AfV2Fc5
    stellar asset    = 5n8tk88hxRUW4cmjQtLe5bJYU6WP2SvHFgF2PMKmjMmr
    stellar release  = 3xk8EfdNKUMCRYDR3iz1oaEVekjYc4ktie5nwxwgH6R8
[2] register_arena_asset_from_stellar
    arena asset PDA  = EfUskYYwLwac5QrFyTmPB7CtLv6C7UBBcf9HesZgq9ZM
    release depl PDA = ERPu2oakDdh38f3RVcLtYTuaqG1mgLDnDPNk3RRM5Zrs
    publish tx       = 4fAAG7LkWLsEmwqZifZ7KyDcZvdJH1CWMrL7v9CDuXwXPYS99DD1aYEEvH2aPUDNZm2ub69ncUz6yEWFCSLn9di5
[3] ArenaAssetData read back — 16/16 assertions PASS
    skin_ref_asset   = 5n8tk88hxRUW4cmjQtLe5bJYU6WP2SvHFgF2PMKmjMmr  (== stellar asset)
==================== ROUND-TRIP: PASS ====================
```
