/**
 * validate-bridge.ts — standalone, repeatable end-to-end proof of the
 * Stellar → Arena publish loop against a RUNNING localnet.
 *
 * What it proves (the full round-trip, not just a compile):
 *   1. A Stellar universe + asset + finalized release is created on-chain via
 *      the solana-stellar program (create_universe → create_asset → submit →
 *      approve → create_release → add_release_share → finalize_release).
 *   2. That release is published into Arena with the bridge instruction
 *      `register_arena_asset_from_stellar` on solana-ekza-arena.
 *   3. The resulting ArenaAssetData PDA is READ BACK and asserted:
 *        - card_kind        == { avatar: {} }
 *        - archetype_id     == "arena_bridge_avatar"
 *        - slot_mask        == 3
 *        - skin_ref         == { stellarAsset: [ <stellar asset pubkey> ] }
 *        - base/stat deltas forced to neutral (identity-only publish)
 *   4. A Stellar ReleaseDeployment PDA (project slug "arena") was recorded,
 *      pointing back at the Arena program + the ArenaAssetData record, and the
 *      Release flipped to status "linked".
 *
 * Prerequisites: a localnet with BOTH programs deployed and the Metaplex
 * Token Metadata program present (the Arena Anchor.toml genesis-clones both
 * ../solana-stellar and tests/fixtures/token_metadata.so). See docs/bridge-loop.md.
 *
 * Run:
 *   node_modules/.bin/ts-node scripts/validate-bridge.ts
 *   (optionally: RPC_URL=http://127.0.0.1:8899 WALLET=~/.config/solana/id.json)
 *
 * Exit code 0 = PASS, 1 = FAIL.
 */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

// Program ids (also carried in the IDL `address` field).
const STELLAR_PROGRAM_ID = new anchor.web3.PublicKey(
  "3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA"
);
const ARENA_PROGRAM_ID = new anchor.web3.PublicKey(
  "D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ"
);

const ARENA_IDL = require("../target/idl/solana_ekza_arena.json");
const STELLAR_IDL = require("../../solana-stellar/target/idl/solana_stellar.json");

const RPC_URL = process.env.RPC_URL || "http://127.0.0.1:8899";
const WALLET_PATH = (
  process.env.WALLET ||
  process.env.ANCHOR_WALLET ||
  path.join(os.homedir(), ".config/solana/id.json")
).replace(/^~(?=$|\/)/, os.homedir());

// ---- tiny assertion harness (no test framework needed) --------------------
let failures = 0;
function check(label: string, ok: boolean, detail?: string) {
  const tag = ok ? "PASS" : "FAIL";
  if (!ok) failures++;
  console.log(`  [${tag}] ${label}${detail ? " — " + detail : ""}`);
}

function loadWallet(): anchor.web3.Keypair {
  const raw = JSON.parse(fs.readFileSync(WALLET_PATH, "utf8"));
  return anchor.web3.Keypair.fromSecretKey(Uint8Array.from(raw));
}

const u64Bytes = (value: number) =>
  new anchor.BN(value).toArrayLike(Buffer, "le", 8);

async function main() {
  console.log("=== Ekza Stellar → Arena bridge round-trip validation ===");
  console.log("RPC:    " + RPC_URL);
  console.log("WALLET: " + WALLET_PATH);

  const connection = new anchor.web3.Connection(RPC_URL, "confirmed");
  const wallet = new anchor.Wallet(loadWallet());
  const provider = new anchor.AnchorProvider(connection, wallet, {
    commitment: "confirmed",
  });
  anchor.setProvider(provider);

  const owner = wallet.publicKey;
  console.log("PAYER:  " + owner.toBase58());

  // Sanity: both programs must be deployed on this localnet.
  for (const [name, id] of [
    ["solana_stellar", STELLAR_PROGRAM_ID],
    ["solana_ekza_arena", ARENA_PROGRAM_ID],
  ] as const) {
    const info = await connection.getAccountInfo(id);
    if (!info || !info.executable) {
      console.error(
        `FATAL: program ${name} (${id.toBase58()}) is not deployed/executable ` +
          `on ${RPC_URL}. Bring up localnet with both programs first ` +
          `(see docs/bridge-loop.md).`
      );
      process.exit(1);
    }
  }

  const arena = new Program(ARENA_IDL as anchor.Idl, provider) as Program;
  const stellar = new Program(STELLAR_IDL as anchor.Idl, provider) as Program;
  const arenaAcc = (arena as any).account;
  const stellarAcc = (stellar as any).account;

  // ---- PDA helpers (mirror the on-chain seeds) ----------------------------
  const arenaRegistryPda = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("arena_registry")],
    ARENA_PROGRAM_ID
  )[0];
  const arenaAssetPda = (index: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("arena_asset_v1"), u64Bytes(index)],
      ARENA_PROGRAM_ID
    )[0];
  const stellarArenaLinkPda = (arenaAsset: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("stellar_arena_link"), arenaAsset.toBuffer()],
      ARENA_PROGRAM_ID
    )[0];
  const stellarReleaseLinkPda = (release: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("stellar_release_link"), release.toBuffer()],
      ARENA_PROGRAM_ID
    )[0];

  const stellarRegistryPda = anchor.web3.PublicKey.findProgramAddressSync(
    [Buffer.from("registry")],
    STELLAR_PROGRAM_ID
  )[0];
  const universeIndexPda = (globalIndex: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("universe_index"), u64Bytes(globalIndex)],
      STELLAR_PROGRAM_ID
    )[0];
  const universePda = (ownerIndex: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("universe"), owner.toBuffer(), u64Bytes(ownerIndex)],
      STELLAR_PROGRAM_ID
    )[0];
  const assetPda = (universe: anchor.web3.PublicKey, index: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("asset"), universe.toBuffer(), u64Bytes(index)],
      STELLAR_PROGRAM_ID
    )[0];
  const releasePda = (universe: anchor.web3.PublicKey, index: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("release"), universe.toBuffer(), u64Bytes(index)],
      STELLAR_PROGRAM_ID
    )[0];
  const vaultPda = (release: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("release_vault"), release.toBuffer()],
      STELLAR_PROGRAM_ID
    )[0];
  const sharePda = (
    release: anchor.web3.PublicKey,
    contributor: anchor.web3.PublicKey
  ) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("share"), release.toBuffer(), contributor.toBuffer()],
      STELLAR_PROGRAM_ID
    )[0];
  const releaseDeploymentPda = (release: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("release_deployment"), release.toBuffer(), Buffer.from("arena")],
      STELLAR_PROGRAM_ID
    )[0];

  // ---- read next on-chain indices (green regardless of ledger state) -------
  async function nextArenaAssetIndex(): Promise<number> {
    try {
      const reg = await arenaAcc.arenaRegistry.fetch(arenaRegistryPda);
      return reg.nextIndex.toNumber();
    } catch {
      return 0;
    }
  }
  async function nextStellarGlobalIndex(): Promise<number> {
    try {
      const reg = await stellarAcc.registry.fetch(stellarRegistryPda);
      return reg.universeCount.toNumber();
    } catch {
      return 0;
    }
  }

  // =========================================================================
  // STEP 1 — create a finalized Stellar release (the "character/asset").
  // =========================================================================
  console.log("\n[1] Creating Stellar universe + asset + finalized release…");
  const label = "BridgeValidate";
  const ownerIndex = Date.now(); // unique per-owner slot
  const globalIndex = await nextStellarGlobalIndex();
  const universe = universePda(ownerIndex);
  const universeLookup = universeIndexPda(globalIndex);
  const asset = assetPda(universe, 0);
  const release = releasePda(universe, 0);
  const vault = vaultPda(release);

  await stellar.methods
    .createUniverse(new anchor.BN(ownerIndex), `Qm${label}UniverseHash`, { model3D: {} }, true)
    .accountsStrict({
      registry: stellarRegistryPda,
      universe,
      universeLookup,
      owner,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .rpc();

  await stellar.methods
    .createAsset(
      new anchor.BN(0),
      { model3D: {} },
      { final: {} },
      { ccBy4: {} },
      `Qm${label}AssetHash`,
      `Qm${label}PreviewHash`,
      true,
      { custom: {} }
    )
    .accountsStrict({ universe, asset, creator: owner, systemProgram: anchor.web3.SystemProgram.programId })
    .rpc();

  await stellar.methods.submitAsset().accountsStrict({ asset, creator: owner }).rpc();
  await stellar.methods.approveAsset().accountsStrict({ universe, asset, owner }).rpc();

  await stellar.methods
    .createRelease(new anchor.BN(0), `Qm${label}ReleaseHash`)
    .accountsStrict({ universe, asset, release, vault, owner, systemProgram: anchor.web3.SystemProgram.programId })
    .rpc();

  await stellar.methods
    .addReleaseShare(10_000)
    .accountsStrict({
      universe,
      release,
      share: sharePda(release, owner),
      contributor: owner,
      owner,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .rpc();

  await stellar.methods
    .finalizeRelease()
    .accountsStrict({ universe, release, asset, owner })
    .rpc();

  console.log("    stellar universe = " + universe.toBase58());
  console.log("    stellar asset    = " + asset.toBase58());
  console.log("    stellar release  = " + release.toBase58());

  // =========================================================================
  // STEP 2 — publish the release into Arena (the bridge instruction).
  // =========================================================================
  console.log("\n[2] Publishing into Arena via register_arena_asset_from_stellar…");
  const arenaAssetIndex = await nextArenaAssetIndex();
  const arenaAsset = arenaAssetPda(arenaAssetIndex);
  const stellarLink = stellarArenaLinkPda(arenaAsset);
  const stellarReleaseLink = stellarReleaseLinkPda(release);
  const stellarReleaseDeployment = releaseDeploymentPda(release);

  const publishArgs = {
    metadataIpfsHash: "QmArenaBridgeValidateMetadata",
    cardKind: { avatar: {} },
    archetypeId: "arena_bridge_avatar",
    slotMask: 3,
    skillIds: ["moss_skin"],
  };

  const sig = await arena.methods
    .registerArenaAssetFromStellar(publishArgs)
    .accountsStrict({
      registry: arenaRegistryPda,
      arenaAsset,
      payer: owner,
      stellarLink,
      stellarProgram: STELLAR_PROGRAM_ID,
      stellarUniverse: universe,
      stellarRelease: release,
      stellarVault: vault,
      stellarReleaseDeployment,
      stellarReleaseLink,
      systemProgram: anchor.web3.SystemProgram.programId,
    })
    .rpc();

  console.log("    tx               = " + sig);
  console.log("    arena asset PDA  = " + arenaAsset.toBase58());
  console.log("    stellar link PDA = " + stellarLink.toBase58());
  console.log("    deployment PDA   = " + stellarReleaseDeployment.toBase58());

  // =========================================================================
  // STEP 3 — READ BACK + ASSERT the round-trip.
  // =========================================================================
  console.log("\n[3] Reading ArenaAssetData back and asserting the round-trip…");
  const a = await arenaAcc.arenaAssetData.fetch(arenaAsset);

  check("card_kind == avatar", JSON.stringify(a.cardKind) === JSON.stringify({ avatar: {} }), JSON.stringify(a.cardKind));
  check("archetype_id == arena_bridge_avatar", a.archetypeId === "arena_bridge_avatar", a.archetypeId);
  check("slot_mask == 3", a.slotMask === 3, String(a.slotMask));
  check("metadata_ipfs_hash preserved", a.metadataIpfsHash === "QmArenaBridgeValidateMetadata", a.metadataIpfsHash);
  check("index == expected", a.index.toNumber() === arenaAssetIndex, String(a.index.toNumber()));

  const skinAsset = a.skinRef && a.skinRef.stellarAsset ? a.skinRef.stellarAsset[0] : undefined;
  check(
    "skin_ref == StellarAsset(stellar asset pubkey)",
    !!skinAsset && skinAsset.toBase58() === asset.toBase58(),
    skinAsset ? skinAsset.toBase58() : "undefined"
  );

  // Identity-only publish: caller stats must NOT leak into balance.
  const zeroStats =
    a.baseStats.hp === 0 && a.baseStats.attack === 0 && a.baseStats.armor === 0 && a.baseStats.speed === 0 &&
    a.statDelta.hp === 0 && a.statDelta.attack === 0 && a.statDelta.armor === 0 && a.statDelta.speed === 0;
  check("base_stats + stat_delta forced neutral (identity-only)", zeroStats);

  // ---- Arena-side link ----------------------------------------------------
  const link = await arenaAcc.stellarArenaAssetLink.fetch(stellarLink);
  check("link.arena_asset == arenaAsset", link.arenaAsset.toBase58() === arenaAsset.toBase58());
  check("link.release == stellar release", link.release.toBase58() === release.toBase58());
  check("link.asset == stellar asset", link.asset.toBase58() === asset.toBase58());

  const releaseLink = await arenaAcc.stellarReleaseLink.fetch(stellarReleaseLink);
  check("release_link.arena_asset == arenaAsset", releaseLink.arenaAsset.toBase58() === arenaAsset.toBase58());

  // ---- Stellar-side deployment record (slug "arena") ----------------------
  const deployment = await stellarAcc.releaseDeployment.fetch(stellarReleaseDeployment);
  check("ReleaseDeployment.project_slug == 'arena'", deployment.projectSlug === "arena", deployment.projectSlug);
  check("ReleaseDeployment.registry_program == Arena program", deployment.registryProgram.toBase58() === ARENA_PROGRAM_ID.toBase58());
  check("ReleaseDeployment.registry_record == arenaAsset", deployment.registryRecord.toBase58() === arenaAsset.toBase58());

  const linkedRelease = await stellarAcc.release.fetch(release);
  check("Release.status == linked", JSON.stringify(linkedRelease.status) === JSON.stringify({ linked: {} }), JSON.stringify(linkedRelease.status));
  check("Release.linked_avatar_data == arenaAsset", linkedRelease.linkedAvatarData.toBase58() === arenaAsset.toBase58());

  // =========================================================================
  // VERDICT
  // =========================================================================
  console.log("\n=== PROOF (on-chain addresses) ===");
  console.log("stellar_universe   = " + universe.toBase58());
  console.log("stellar_asset      = " + asset.toBase58());
  console.log("stellar_release    = " + release.toBase58());
  console.log("arena_asset_data   = " + arenaAsset.toBase58());
  console.log("skin_ref_asset     = " + (skinAsset ? skinAsset.toBase58() : "undefined"));
  console.log("release_deployment = " + stellarReleaseDeployment.toBase58());
  console.log("publish_tx         = " + sig);

  if (failures === 0) {
    console.log("\n==================== ROUND-TRIP: PASS ====================");
    process.exit(0);
  } else {
    console.log(`\n==================== ROUND-TRIP: FAIL (${failures}) ====================`);
    process.exit(1);
  }
}

main().catch((e) => {
  console.error("\nROUND-TRIP: FAIL (exception)");
  console.error(e);
  process.exit(1);
});
