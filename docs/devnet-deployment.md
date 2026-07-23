# Ekza Arena devnet deployment

- Deployed: 2026-07-23 16:16 MSK
- Canonical machine-readable record: [`deployments/devnet.json`](../deployments/devnet.json)

## Live addresses

| Component              | Devnet address                                 |
| ---------------------- | ---------------------------------------------- |
| Solana Stellar program | `3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA` |
| Arena program          | `D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ` |
| Leaderboard program    | `9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU` |
| Arena Registry PDA     | `4x25bbbKGuA4p46XXiV5sMb4FV4C1x7zNKtkvbusJXUV` |
| Leaderboard board      | `MfVv4UTLu7cwJxzhMZ25QCg8usjtA4srCvgEKxJ9rto`  |
| Platform treasury      | `Ab5TgPbcB8QVuormXYXHzRVkV7okAbzkS2sU2neKoWvQ` |
| Devnet sink            | `CyiV8EG8gGhT1WCV8YaLJ9raf9DpxZ3g4FdkoXL9yt3u` |

All programs remain upgradeable. Their upgrade authority, registry
configuration authority, pack universe owner and platform treasury are the
public key of `/Users/wotori/.config/solana/id.json`:
`Ab5TgPbcB8QVuormXYXHzRVkV7okAbzkS2sU2neKoWvQ`.

The Registry charges `0.002 SOL` and splits Stellar mint fees `50/40/10`
(creator/platform/sink). Builtin/IPFS mint fees use `90/0/10` because they have
no Stellar creator vault.

The devnet sink is a dedicated, pre-funded SystemAccount. Its keypair is kept
outside git at `../ekza-controll/.state/devnet/arena-sink.json`. This is a
custodial test sink, not an irreversible burn address; choose a production sink
policy before mainnet.

## Verification

```sh
solana program show --url devnet 3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA
solana program show --url devnet D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ
solana program show --url devnet 9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU
```

On-chain dumps were compared byte-for-byte by SHA-256 with the locally built
`.so` files. All three hashes match the values stored in `devnet.json`.

The live `validate:bridge` proof also passed against devnet. It created a
Stellar universe, approved asset and finalized release, published the release
into Arena via CPI, then verified the Arena asset, Stellar links and
`ReleaseDeployment`. The publish signature is
`52NvfqKVYkmAxpvK1T7F9kxZqmWdEkvjz7NU9VN4W3uzTkxS1Hso71CyjcoR4yLDpntVgr4JAeWPTNWEXudWRMmx`;
all proof addresses are stored in `devnet.json`.

The persistent leaderboard smoke also passed: one wallet-signed bot win was
recorded exactly once, creating the player/rate-limit PDAs and placing the
player on the board at rating `1010`. Signature:
`cTqEkHpxWsmrGZUSr4BQiu91yAhr987uKcUPVbTVF5QP2XZo1vGk5FFhnHnZhAbMNbJaJ1pMSKSYZta64HKYskC`.

The full Builtin commit→reveal mint smoke passed from an independently funded
devnet minter. It created a supply-1/decimals-0 SPL NFT and ArenaItem PDA, and
its metadata URI returned HTTP 200 from the public IPFS gateway. Fee accounting
was exact: treasury `+1,800,000` lamports and sink `+200,000` lamports. Commit
signature `3UNinVpboUSUCYyEiHmynd7BRBfmmmXftqcbtoRMH6DDwcgdTmdSP9YhKtqsHEwBFXBqKF8gRZSPX9xZjcEjsZdB`;
reveal signature `4ujnDeKZF2bFhVMvA47G5edPz5SieD9nqA1zRkt73i3aYKV3Y8jyhFSPvZ9JS51yGGkNADNo8qRDZgzCwXuDRyjY`.

Registry/board bootstrap is idempotent and refuses any cluster whose genesis
hash is not canonical devnet:

```sh
RPC_URL=https://api.devnet.solana.com \
WALLET=/path/to/id.json \
ARENA_TREASURY=Ab5TgPbcB8QVuormXYXHzRVkV7okAbzkS2sU2neKoWvQ \
ARENA_SINK=CyiV8EG8gGhT1WCV8YaLJ9raf9DpxZ3g4FdkoXL9yt3u \
BOARD_KEYPAIR=/path/to/arena-leaderboard-board.json \
yarn bootstrap:devnet
```

The board keypair is retained at
`../ekza-controll/.state/devnet/arena-leaderboard-board.json`; after atomic
creation the board authority is the deploy wallet, and the board account itself
does not sign again.

## Deployment procedure

Build each repository at the source commits recorded in `devnet.json`, then
deploy in dependency order: Stellar → Arena → leaderboard. Use explicit program
keypairs and never pass `--final`.

```sh
solana program deploy --url devnet --keypair /path/to/id.json \
  --fee-payer /path/to/id.json --upgrade-authority /path/to/id.json \
  --program-id target/deploy/<program>-keypair.json \
  target/deploy/<program>.so
```

`solana-cli 4.0.0` panicked in its TPU client against the current devnet
`getClusterNodes` response. Deployment succeeded with the locally installed
`solana-cli 3.1.8`. An initial failed RPC-only upload buffer was closed by its
authority and its rent returned; no recovery phrase is stored in repository
files.

## Key material and backups

These files are deliberately ignored and must be backed up securely before any
machine migration:

- `/Users/wotori/.config/solana/id.json` — deploy/upgrade/config authority;
- `../solana-stellar/target/deploy/solana_stellar-keypair.json`;
- `target/deploy/solana_ekza_arena-keypair.json`;
- `target/deploy/arena_leaderboard-keypair.json`;
- `../ekza-controll/.state/devnet/arena-sink.json`;
- `../ekza-controll/.state/devnet/arena-leaderboard-board.json`.
- `../ekza-controll/.state/devnet/arena-smoke-minter.json` (test-only wallet).

Never commit their JSON byte arrays, seed phrases, authenticated RPC URLs or
pinning credentials.

## Content and web configuration

The four pack deployments and all entity/CID mappings are documented in the
Solana Stellar repository at `docs/devnet-arena-packs.md`. The web deployment
uses the public values in `ekza-arena-web/.env.example`; `.env.local` is ignored
and must never contain `NEXT_PUBLIC_DEV_WALLET_SECRET` on devnet.

The initial CIDs are pinned by the local Kubo node under
`ekza-controll/.state/ipfs-devnet`; a public gateway returned HTTP 200 during
deployment verification. Add durable remote pinning before treating the
content as production-available.
