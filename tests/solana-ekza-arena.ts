import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { expect } from "chai";
import { SolanaEkzaArena } from "../target/types/solana_ekza_arena";
import stellarIdl from "../../solana-stellar/target/idl/solana_stellar.json";

const STELLAR_PROGRAM_ID = new anchor.web3.PublicKey(
  "3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA"
);

describe("solana-ekza-arena", () => {
  anchor.setProvider(anchor.AnchorProvider.env());

  const provider = anchor.getProvider() as anchor.AnchorProvider;
  const program = anchor.workspace.solanaEkzaArena as Program<SolanaEkzaArena>;
  const stellarProgram = new anchor.Program(
    stellarIdl as anchor.Idl,
    provider
  ) as Program;
  const stellarAccounts = (stellarProgram as any).account;
  let nextOwnerUniverseIndex = Date.now();

  const registryPda = () =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("arena_registry")],
      program.programId
    )[0];

  const u64Bytes = (value: number) =>
    new anchor.BN(value).toArrayLike(Buffer, "le", 8);

  const arenaAssetPda = (index: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("arena_asset_v1"), u64Bytes(index)],
      program.programId
    )[0];

  const arenaItemPda = (index: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("arena_item_v1"), u64Bytes(index)],
      program.programId
    )[0];

  const SLOT_HASHES_SYSVAR = new anchor.web3.PublicKey(
    "SysvarS1otHashes111111111111111111111111111"
  );

  const stellarRegistryPda = () =>
    anchor.web3.PublicKey.findProgramAddressSync(
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
      [
        Buffer.from("universe"),
        provider.wallet.publicKey.toBuffer(),
        u64Bytes(ownerIndex),
      ],
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

  const stellarArenaLinkPda = (arenaAsset: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("stellar_arena_link"), arenaAsset.toBuffer()],
      program.programId
    )[0];

  const stellarReleaseLinkPda = (release: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("stellar_release_link"), release.toBuffer()],
      program.programId
    )[0];

  const stellarReleaseDeploymentPda = (release: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("release_deployment"),
        release.toBuffer(),
        Buffer.from("arena"),
      ],
      STELLAR_PROGRAM_ID
    )[0];

  const baseArgs = () => ({
    metadataIpfsHash: "QmArenaCardMetadata",
    cardKind: { avatar: {} },
    archetypeId: "sprout_ipfs",
    baseStats: {
      hp: 10,
      attack: 1,
      armor: 0,
      speed: 1,
    },
    statDelta: {
      hp: 0,
      attack: 0,
      armor: 0,
      speed: 0,
    },
    slotMask: 3,
    rarity: { common: {} },
    element: { none: {} },
    skillIds: ["moss_skin"],
  });

  async function nextArenaAssetIndex() {
    try {
      const registryAccount = await program.account.arenaRegistry.fetch(
        registryPda()
      );
      return registryAccount.nextIndex.toNumber();
    } catch {
      return 0;
    }
  }

  async function nextStellarUniverseIndex() {
    try {
      const registryAccount = await stellarAccounts.registry.fetch(
        stellarRegistryPda()
      );
      return registryAccount.universeCount.toNumber();
    } catch {
      return 0;
    }
  }

  async function createFinalizedStellarRelease(label: string) {
    const ownerIndex = nextOwnerUniverseIndex++;
    const globalIndex = await nextStellarUniverseIndex();
    const universe = universePda(ownerIndex);
    const universeLookup = universeIndexPda(globalIndex);
    const asset = assetPda(universe, 0);
    const release = releasePda(universe, 0);
    const vault = vaultPda(release);
    const owner = provider.wallet.publicKey;

    await stellarProgram.methods
      .createUniverse(
        new anchor.BN(ownerIndex),
        `Qm${label}UniverseHash`,
        { model3D: {} },
        true
      )
      .accountsStrict({
        registry: stellarRegistryPda(),
        universe,
        universeLookup,
        owner,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    await stellarProgram.methods
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
      .accountsStrict({
        universe,
        asset,
        creator: owner,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    await stellarProgram.methods
      .submitAsset()
      .accountsStrict({ asset, creator: owner })
      .rpc();

    await stellarProgram.methods
      .approveAsset()
      .accountsStrict({ universe, asset, owner })
      .rpc();

    await stellarProgram.methods
      .createRelease(new anchor.BN(0), `Qm${label}ReleaseHash`)
      .accountsStrict({
        universe,
        asset,
        release,
        vault,
        owner,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    await stellarProgram.methods
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

    await stellarProgram.methods
      .finalizeRelease()
      .accountsStrict({ universe, release, asset, owner })
      .rpc();

    return { universe, asset, release, vault };
  }

  it("Is initialized!", async () => {
    const tx = await program.methods.initialize().rpc();
    expect(tx).to.be.a("string");
  });

  it("registers a direct Arena asset record", async () => {
    const registry = registryPda();
    // Derive the index from on-chain registry.next_index so the test is green
    // regardless of ledger state (fresh OR accumulated/shared validator).
    const index = await nextArenaAssetIndex();
    const arenaAsset = arenaAssetPda(index);

    await program.methods
      .registerArenaAsset(baseArgs())
      .accountsStrict({
        registry,
        arenaAsset,
        payer: provider.wallet.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const registryAccount = await program.account.arenaRegistry.fetch(registry);
    const assetAccount = await program.account.arenaAssetData.fetch(arenaAsset);

    // The counter advanced by exactly one from the index we consumed.
    expect(registryAccount.nextIndex.toNumber()).to.equal(index + 1);
    expect(assetAccount.metadataIpfsHash).to.equal("QmArenaCardMetadata");
    expect(assetAccount.creator.equals(provider.wallet.publicKey)).to.equal(
      true
    );
    expect(assetAccount.index.toNumber()).to.equal(index);
    expect(assetAccount.archetypeId).to.equal("sprout_ipfs");
    expect(assetAccount.baseStats.hp).to.equal(10);
    expect(assetAccount.skillIds).to.deep.equal(["moss_skin"]);
    // Direct/manual cards default to a builtin skin (spec §8b).
    expect(assetAccount.skinRef).to.deep.equal({ builtin: { "0": 0 } });
  });

  it("registers a Stellar release as an Arena asset and records deployment", async () => {
    const stellarRelease = await createFinalizedStellarRelease("ArenaBridge");
    const registry = registryPda();
    const arenaAssetIndex = await nextArenaAssetIndex();
    const arenaAsset = arenaAssetPda(arenaAssetIndex);
    const stellarLink = stellarArenaLinkPda(arenaAsset);
    const stellarReleaseLink = stellarReleaseLinkPda(stellarRelease.release);
    const stellarReleaseDeployment = stellarReleaseDeploymentPda(
      stellarRelease.release
    );

    // Skin-only args (spec §8b): no base_stats/stat_delta/rarity/element — the
    // Stellar publish carries identity only, balance is rolled later.
    await program.methods
      .registerArenaAssetFromStellar({
        metadataIpfsHash: "QmArenaBridgeMetadata",
        cardKind: { avatar: {} },
        archetypeId: "arena_bridge_avatar",
        slotMask: 3,
        skillIds: ["moss_skin"],
      })
      .accountsStrict({
        registry,
        arenaAsset,
        payer: provider.wallet.publicKey,
        stellarLink,
        stellarProgram: STELLAR_PROGRAM_ID,
        stellarUniverse: stellarRelease.universe,
        stellarRelease: stellarRelease.release,
        stellarVault: stellarRelease.vault,
        stellarReleaseDeployment,
        stellarReleaseLink,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const assetAccount = await program.account.arenaAssetData.fetch(arenaAsset);
    expect(assetAccount.metadataIpfsHash).to.equal("QmArenaBridgeMetadata");
    expect(assetAccount.index.toNumber()).to.equal(arenaAssetIndex);

    console.log("E2E_PROOF arena_asset_pda=" + arenaAsset.toBase58());
    console.log(
      "E2E_PROOF stellar_asset_pubkey=" + stellarRelease.asset.toBase58()
    );
    console.log(
      "E2E_PROOF skin_ref_stellar_asset=" +
        assetAccount.skinRef.stellarAsset[0].toBase58()
    );

    // SKIN-ONLY bridge: skin_ref points at the Stellar asset pubkey.
    expect(assetAccount.skinRef.stellarAsset).to.not.equal(undefined);
    expect(assetAccount.skinRef.stellarAsset[0].toBase58()).to.equal(
      stellarRelease.asset.toBase58()
    );
    // Caller-supplied stats must NOT leak into balance on this path — the args
    // struct omits them and the account is forced to neutral/zero.
    expect(assetAccount.baseStats.hp).to.equal(0);
    expect(assetAccount.baseStats.attack).to.equal(0);
    expect(assetAccount.baseStats.armor).to.equal(0);
    expect(assetAccount.baseStats.speed).to.equal(0);
    expect(assetAccount.statDelta.hp).to.equal(0);
    expect(assetAccount.statDelta.attack).to.equal(0);
    expect(assetAccount.statDelta.armor).to.equal(0);
    expect(assetAccount.statDelta.speed).to.equal(0);
    expect(assetAccount.rarity).to.deep.equal({ common: {} });
    expect(assetAccount.element).to.deep.equal({ none: {} });

    const linkAccount = await program.account.stellarArenaAssetLink.fetch(
      stellarLink
    );
    expect(linkAccount.arenaAsset.toBase58()).to.equal(arenaAsset.toBase58());
    expect(linkAccount.release.toBase58()).to.equal(
      stellarRelease.release.toBase58()
    );
    expect(linkAccount.asset.toBase58()).to.equal(
      stellarRelease.asset.toBase58()
    );

    const releaseLinkAccount = await program.account.stellarReleaseLink.fetch(
      stellarReleaseLink
    );
    expect(releaseLinkAccount.arenaAsset.toBase58()).to.equal(
      arenaAsset.toBase58()
    );

    const deploymentAccount = await stellarAccounts.releaseDeployment.fetch(
      stellarReleaseDeployment
    );
    expect(deploymentAccount.projectSlug).to.equal("arena");
    expect(deploymentAccount.registryProgram.toBase58()).to.equal(
      program.programId.toBase58()
    );
    expect(deploymentAccount.registryRecord.toBase58()).to.equal(
      arenaAsset.toBase58()
    );

    const linkedRelease = await stellarAccounts.release.fetch(
      stellarRelease.release
    );
    expect(linkedRelease.status).to.deep.equal({ linked: {} });
    expect(linkedRelease.linkedAvatarData.toBase58()).to.equal(
      arenaAsset.toBase58()
    );
  });

  it("mints a deterministic Arena item with rolled affixes (builtin skin)", async () => {
    const registry = registryPda();
    const index = await nextArenaAssetIndex();
    const arenaItem = arenaItemPda(index);

    await program.methods
      .mintArenaItem({
        baseType: { weapon: {} },
        skin: { builtin: [7] },
      })
      .accountsStrict({
        registry,
        arenaItem,
        payer: provider.wallet.publicKey,
        slotHashes: SLOT_HASHES_SYSVAR,
        stellarProgram: null,
        stellarRelease: null,
        stellarVault: null,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const itemAccount = await program.account.arenaItem.fetch(arenaItem);

    // Account created with sane fields.
    expect(itemAccount.index.toNumber()).to.equal(index);
    expect(itemAccount.minter.equals(provider.wallet.publicKey)).to.equal(true);
    expect(itemAccount.baseType).to.deep.equal({ weapon: {} });
    expect(itemAccount.skinRef).to.deep.equal({ builtin: { "0": 7 } });
    // seed is non-zero entropy off the slothash.
    expect(itemAccount.seed.toString()).to.not.equal("0");
    // tier in [1..4], at least one affix, at most the legendary cap of 4.
    expect(itemAccount.tier).to.be.greaterThan(0);
    expect(itemAccount.tier).to.be.lessThan(5);
    expect(itemAccount.affixes.length).to.be.greaterThan(0);
    expect(itemAccount.affixes.length).to.be.lessThan(5);
    // every affix value is positive and kind is a valid id (0..8).
    for (const affix of itemAccount.affixes) {
      expect(affix.value).to.be.greaterThan(0);
      expect(affix.kind).to.be.greaterThan(-1);
      expect(affix.kind).to.be.lessThan(9);
    }
  });

  it("rejects minting an item with an out-of-range builtin skin id", async () => {
    const registry = registryPda();
    const index = await nextArenaAssetIndex();
    const arenaItem = arenaItemPda(index);

    try {
      await program.methods
        .mintArenaItem({
          baseType: { charm: {} },
          skin: { builtin: [200] }, // >= MAX_BUILTIN_SKINS (64)
        })
        .accountsStrict({
          registry,
          arenaItem,
          payer: provider.wallet.publicKey,
          slotHashes: SLOT_HASHES_SYSVAR,
          stellarProgram: null,
          stellarRelease: null,
          stellarVault: null,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();
      expect.fail("mintArenaItem should reject an out-of-range builtin skin id");
    } catch (error) {
      expect(String(error)).to.include("Invalid item skin reference");
    }
  });

  async function mintItem(baseType: any) {
    const registry = registryPda();
    const index = await nextArenaAssetIndex();
    const arenaItem = arenaItemPda(index);

    await program.methods
      .mintArenaItem({ baseType, skin: { builtin: [3] } })
      .accountsStrict({
        registry,
        arenaItem,
        payer: provider.wallet.publicKey,
        slotHashes: SLOT_HASHES_SYSVAR,
        stellarProgram: null,
        stellarRelease: null,
        stellarVault: null,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    return { index, arenaItem };
  }

  it("scraps a minted Arena item and closes the account (owner, spec §10.5)", async () => {
    const { arenaItem } = await mintItem({ head: {} });

    // Account exists before scrap.
    const before = await program.account.arenaItem.fetch(arenaItem);
    expect(before.minter.equals(provider.wallet.publicKey)).to.equal(true);

    await program.methods
      .scrapArenaItem()
      .accountsStrict({
        arenaItem,
        minter: provider.wallet.publicKey,
      })
      .rpc();

    // Account closed: the on-chain account is gone (rent returned to owner).
    const closed = await provider.connection.getAccountInfo(arenaItem);
    expect(closed).to.equal(null);

    let fetchThrew = false;
    try {
      await program.account.arenaItem.fetch(arenaItem);
    } catch {
      fetchThrew = true;
    }
    expect(fetchThrew).to.equal(true);
  });

  it("rejects scrapping an Arena item by a non-owner", async () => {
    const { arenaItem } = await mintItem({ armor: {} });

    const attacker = anchor.web3.Keypair.generate();
    const sig = await provider.connection.requestAirdrop(
      attacker.publicKey,
      anchor.web3.LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(sig);

    try {
      await program.methods
        .scrapArenaItem()
        .accountsStrict({
          arenaItem,
          minter: attacker.publicKey,
        })
        .signers([attacker])
        .rpc();
      expect.fail("scrapArenaItem should reject a non-owner signer");
    } catch (error) {
      // has_one = minter @ Unauthorized guards ownership.
      expect(String(error)).to.match(/Unauthorized|has one|ConstraintHasOne/i);
    }

    // The item must still exist after a rejected scrap.
    const still = await program.account.arenaItem.fetch(arenaItem);
    expect(still.minter.equals(provider.wallet.publicKey)).to.equal(true);
  });

  it("rejects Arena assets without an equip slot mask", async () => {
    const registry = registryPda();
    const arenaAsset = arenaAssetPda(await nextArenaAssetIndex());
    const args = { ...baseArgs(), slotMask: 0 };

    try {
      await program.methods
        .registerArenaAsset(args)
        .accountsStrict({
          registry,
          arenaAsset,
          payer: provider.wallet.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();
      expect.fail("registerArenaAsset should reject a zero slot mask");
    } catch (error) {
      expect(String(error)).to.include("Invalid Arena slot mask");
    }
  });
});
