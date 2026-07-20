import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { expect } from "chai";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
  getMint,
  createAssociatedTokenAccountInstruction,
  createTransferInstruction,
} from "@solana/spl-token";
import { SolanaEkzaArena } from "../target/types/solana_ekza_arena";
import stellarIdl from "../../solana-stellar/target/idl/solana_stellar.json";

const STELLAR_PROGRAM_ID = new anchor.web3.PublicKey(
  "3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA"
);

// Metaplex Token Metadata (cloned on localnet / genesis in anchor test).
const TOKEN_METADATA_PROGRAM_ID = new anchor.web3.PublicKey(
  "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s"
);

const metadataPda = (mint: anchor.web3.PublicKey) =>
  anchor.web3.PublicKey.findProgramAddressSync(
    [
      Buffer.from("metadata"),
      TOKEN_METADATA_PROGRAM_ID.toBuffer(),
      mint.toBuffer(),
    ],
    TOKEN_METADATA_PROGRAM_ID
  )[0];

const masterEditionPda = (mint: anchor.web3.PublicKey) =>
  anchor.web3.PublicKey.findProgramAddressSync(
    [
      Buffer.from("metadata"),
      TOKEN_METADATA_PROGRAM_ID.toBuffer(),
      mint.toBuffer(),
      Buffer.from("edition"),
    ],
    TOKEN_METADATA_PROGRAM_ID
  )[0];

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

  // v3 (spec §11.2): ArenaItem PDA is seeded by the NFT mint pubkey (1:1).
  const arenaItemPda = (mint: anchor.web3.PublicKey) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("arena_item_v1"), mint.toBuffer()],
      program.programId
    )[0];

  // v4 (spec §12.1): MintCommit PDA = [mint_commit, minter, nonce_le].
  const mintCommitPda = (minter: anchor.web3.PublicKey, nonce: number) =>
    anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("mint_commit"), minter.toBuffer(), u64Bytes(nonce)],
      program.programId
    )[0];

  // Treasury that receives the non-refundable commit fee (spec §12.1).
  const treasury = anchor.web3.Keypair.generate();
  const COMMIT_FEE_LAMPORTS = 10_000_000; // 0.01 SOL
  let nextCommitNonce = Date.now();

  const waitForSlotAfter = async (target: number) => {
    for (;;) {
      const slot = await provider.connection.getSlot();
      if (slot > target) return slot;
      await new Promise((r) => setTimeout(r, 200));
    }
  };

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

  // Mint a real, tradeable item NFT (spec §11). Generates a fresh mint keypair,
  // wires the full Metaplex account set, and returns everything the asserts need.
  async function mintArenaItemNft(opts: {
    baseType: any;
    skin: any;
    name?: string;
    symbol?: string;
    uri?: string;
    stellar?: {
      program: anchor.web3.PublicKey;
      release: anchor.web3.PublicKey;
      vault: anchor.web3.PublicKey;
    };
  }) {
    const mint = anchor.web3.Keypair.generate();
    const payer = provider.wallet.publicKey;
    const ata = getAssociatedTokenAddressSync(mint.publicKey, payer);
    const arenaItem = arenaItemPda(mint.publicKey);

    await program.methods
      .mintArenaItem({
        baseType: opts.baseType,
        skin: opts.skin,
        name: opts.name ?? "Ekza Arena Item",
        symbol: opts.symbol ?? "EKZAITM",
        uri: opts.uri ?? "https://meta.ekza.space/arena/item.json",
      })
      .accountsStrict({
        registry: registryPda(),
        mint: mint.publicKey,
        arenaItem,
        minterTokenAccount: ata,
        payer,
        slotHashes: SLOT_HASHES_SYSVAR,
        metadataAccount: metadataPda(mint.publicKey),
        masterEdition: masterEditionPda(mint.publicKey),
        stellarProgram: opts.stellar ? opts.stellar.program : null,
        stellarRelease: opts.stellar ? opts.stellar.release : null,
        stellarVault: opts.stellar ? opts.stellar.vault : null,
        tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([mint])
      .rpc();

    return { mint: mint.publicKey, ata, arenaItem };
  }

  it("configures the registry treasury + commit fee (spec §12.1)", async () => {
    await program.methods
      .configureRegistry({
        treasury: treasury.publicKey,
        commitFeeLamports: new anchor.BN(COMMIT_FEE_LAMPORTS),
      })
      .accountsStrict({
        registry: registryPda(),
        payer: provider.wallet.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const reg = await program.account.arenaRegistry.fetch(registryPda());
    expect(reg.treasury.toBase58()).to.equal(treasury.publicKey.toBase58());
    expect(reg.commitFeeLamports.toNumber()).to.equal(COMMIT_FEE_LAMPORTS);
    console.log("E2E_PROOF registry_treasury=" + reg.treasury.toBase58());
    console.log(
      "E2E_PROOF registry_commit_fee_lamports=" + reg.commitFeeLamports.toString()
    );
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

  it("mints a real tradeable item NFT with rolled affixes (builtin skin, spec §11)", async () => {
    const index = await nextArenaAssetIndex();
    const { mint, ata, arenaItem } = await mintArenaItemNft({
      baseType: { weapon: {} },
      skin: { builtin: [7] },
      name: "Ekza Weapon #1",
      uri: "https://meta.ekza.space/arena/weapon-1.json",
    });

    console.log("E2E_PROOF minted_nft_mint=" + mint.toBase58());

    // SPL mint: supply == 1, decimals == 0 (true non-fungible).
    const mintInfo = await getMint(provider.connection, mint);
    expect(mintInfo.supply.toString()).to.equal("1");
    expect(mintInfo.decimals).to.equal(0);

    // Minter ATA holds exactly the 1 token.
    const ataInfo = await getAccount(provider.connection, ata);
    expect(ataInfo.amount.toString()).to.equal("1");
    expect(ataInfo.owner.equals(provider.wallet.publicKey)).to.equal(true);

    // Metadata account exists with the supplied name/uri.
    const metaInfo = await provider.connection.getAccountInfo(metadataPda(mint));
    expect(metaInfo).to.not.equal(null);
    expect(metaInfo!.owner.equals(TOKEN_METADATA_PROGRAM_ID)).to.equal(true);
    const metaStr = metaInfo!.data.toString("utf8");
    expect(metaStr).to.include("Ekza Weapon #1");
    expect(metaStr).to.include("weapon-1.json");

    // Master Edition exists (true 1-of-1 non-fungible).
    const meInfo = await provider.connection.getAccountInfo(
      masterEditionPda(mint)
    );
    expect(meInfo).to.not.equal(null);
    expect(meInfo!.owner.equals(TOKEN_METADATA_PROGRAM_ID)).to.equal(true);

    // ArenaItem PDA (seeded by mint) carries the rolled stats + the mint.
    const itemAccount = await program.account.arenaItem.fetch(arenaItem);
    expect(itemAccount.index.toNumber()).to.equal(index);
    expect(itemAccount.minter.equals(provider.wallet.publicKey)).to.equal(true);
    expect(itemAccount.mint.equals(mint)).to.equal(true);
    expect(itemAccount.baseType).to.deep.equal({ weapon: {} });
    expect(itemAccount.skinRef).to.deep.equal({ builtin: { "0": 7 } });
    expect(itemAccount.seed.toString()).to.not.equal("0");
    expect(itemAccount.tier).to.be.greaterThan(0);
    expect(itemAccount.tier).to.be.lessThan(5);
    expect(itemAccount.affixes.length).to.be.greaterThan(0);
    expect(itemAccount.affixes.length).to.be.lessThan(5);
    for (const affix of itemAccount.affixes) {
      expect(affix.value).to.be.greaterThan(0);
      expect(affix.kind).to.be.greaterThan(-1);
      expect(affix.kind).to.be.lessThan(9);
    }
  });

  it("mints an item NFT from a Stellar skin (skin_ref == StellarAsset, spec §11.2)", async () => {
    const stellar = await createFinalizedStellarRelease("ArenaNftSkin");

    const { mint, ata, arenaItem } = await mintArenaItemNft({
      baseType: { armor: {} },
      skin: { stellar: {} },
      name: "Ekza Stellar Armor",
      stellar: {
        program: STELLAR_PROGRAM_ID,
        release: stellar.release,
        vault: stellar.vault,
      },
    });

    console.log("E2E_PROOF stellar_skin_nft_mint=" + mint.toBase58());
    console.log(
      "E2E_PROOF stellar_skin_asset=" + stellar.asset.toBase58()
    );

    // NFT really minted with stats.
    const mintInfo = await getMint(provider.connection, mint);
    expect(mintInfo.supply.toString()).to.equal("1");
    const ataInfo = await getAccount(provider.connection, ata);
    expect(ataInfo.amount.toString()).to.equal("1");

    const itemAccount = await program.account.arenaItem.fetch(arenaItem);
    // skin_ref points at the validated Stellar asset pubkey.
    expect(itemAccount.skinRef.stellarAsset).to.not.equal(undefined);
    expect(itemAccount.skinRef.stellarAsset[0].toBase58()).to.equal(
      stellar.asset.toBase58()
    );
    // And it carries the rolled stats.
    expect(itemAccount.affixes.length).to.be.greaterThan(0);
    expect(itemAccount.tier).to.be.greaterThan(0);
    expect(itemAccount.mint.equals(mint)).to.equal(true);
  });

  it("rejects minting an item with an out-of-range builtin skin id", async () => {
    try {
      await mintArenaItemNft({
        baseType: { charm: {} },
        skin: { builtin: [200] }, // >= MAX_BUILTIN_SKINS (64)
      });
      expect.fail("mintArenaItem should reject an out-of-range builtin skin id");
    } catch (error) {
      expect(String(error)).to.include("Invalid item skin reference");
    }
  });

  it("TRADEABILITY: transfers the item NFT to a second wallet then scraps by the new owner (spec §11.3/§11.5)", async () => {
    // Mint an item NFT to wallet #1 (the provider wallet).
    const { mint, ata, arenaItem } = await mintArenaItemNft({
      baseType: { head: {} },
      skin: { builtin: [3] },
      name: "Ekza Tradeable Head",
    });

    // Second wallet (the buyer).
    const buyer = anchor.web3.Keypair.generate();
    const air = await provider.connection.requestAirdrop(
      buyer.publicKey,
      anchor.web3.LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(air);
    const buyerAta = getAssociatedTokenAddressSync(mint, buyer.publicKey);

    // Standard SPL transfer: wallet #1 -> wallet #2.
    const tx = new anchor.web3.Transaction()
      .add(
        createAssociatedTokenAccountInstruction(
          provider.wallet.publicKey, // payer
          buyerAta,
          buyer.publicKey, // owner
          mint
        )
      )
      .add(
        createTransferInstruction(
          ata, // source
          buyerAta, // destination
          provider.wallet.publicKey, // authority
          1
        )
      );
    await provider.sendAndConfirm(tx, []);

    // New wallet now holds the NFT; the first wallet is empty.
    const oldInfo = await getAccount(provider.connection, ata);
    const newInfo = await getAccount(provider.connection, buyerAta);
    expect(oldInfo.amount.toString()).to.equal("0");
    expect(newInfo.amount.toString()).to.equal("1");
    expect(newInfo.owner.equals(buyer.publicKey)).to.equal(true);

    console.log("E2E_PROOF transfer_old_owner=" + provider.wallet.publicKey.toBase58());
    console.log("E2E_PROOF transfer_new_owner=" + buyer.publicKey.toBase58());
    console.log("E2E_PROOF transfer_mint=" + mint.toBase58());

    // The original minter can NO LONGER scrap — they are not the holder.
    let oldOwnerRejected = false;
    try {
      await program.methods
        .scrapArenaItem()
        .accountsStrict({
          arenaItem,
          mint,
          tokenAccount: ata, // empty account of the old owner
          metadataAccount: metadataPda(mint),
          masterEdition: masterEditionPda(mint),
          owner: provider.wallet.publicKey,
          tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
    } catch {
      oldOwnerRejected = true;
    }
    expect(oldOwnerRejected).to.equal(true);

    // Now the NEW owner scraps: burns the NFT + closes the ArenaItem PDA.
    const buyerLamportsBefore = await provider.connection.getBalance(
      buyer.publicKey
    );

    await program.methods
      .scrapArenaItem()
      .accountsStrict({
        arenaItem,
        mint,
        tokenAccount: buyerAta,
        metadataAccount: metadataPda(mint),
        masterEdition: masterEditionPda(mint),
        owner: buyer.publicKey,
        tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([buyer])
      .rpc();

    // NFT burned: the holder's token account is closed and the mint supply is 0.
    const burnedToken = await provider.connection.getAccountInfo(buyerAta);
    expect(burnedToken).to.equal(null);
    const burnedMint = await getMint(provider.connection, mint);
    expect(burnedMint.supply.toString()).to.equal("0");

    // Metadata + master edition are torn down by Metaplex `BurnNft`: the master
    // edition account is fully closed (null) and the metadata account is drained
    // to a 1-byte `Key::Uninitialized` husk — neither is a live NFT account.
    const burnedMeta = await provider.connection.getAccountInfo(
      metadataPda(mint)
    );
    const burnedEdition = await provider.connection.getAccountInfo(
      masterEditionPda(mint)
    );
    expect(burnedEdition).to.equal(null);
    expect(burnedMeta === null || burnedMeta.data.length <= 1).to.equal(true);

    // ArenaItem PDA closed, rent returned to the new owner.
    const closedItem = await provider.connection.getAccountInfo(arenaItem);
    expect(closedItem).to.equal(null);
    const buyerLamportsAfter = await provider.connection.getBalance(
      buyer.publicKey
    );
    expect(buyerLamportsAfter).to.be.greaterThan(buyerLamportsBefore);

    console.log("E2E_PROOF burned_and_closed_mint=" + mint.toBase58());
  });

  it("commit_mint charges the non-refundable fee to treasury + creates the MintCommit PDA (spec §12.1)", async () => {
    const minter = provider.wallet.publicKey;
    const nonce = nextCommitNonce++;
    const commitPda = mintCommitPda(minter, nonce);

    const payerBefore = await provider.connection.getBalance(minter);
    const treasuryBefore = await provider.connection.getBalance(
      treasury.publicKey
    );

    await program.methods
      .commitMint({
        nonce: new anchor.BN(nonce),
        baseType: { weapon: {} },
        skin: { builtin: [5] },
        name: "Ekza Commit Weapon",
        symbol: "EKZAITM",
        uri: "https://meta.ekza.space/arena/commit-weapon.json",
      })
      .accountsStrict({
        registry: registryPda(),
        mintCommit: commitPda,
        minter,
        treasury: treasury.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    // Fee left the payer and arrived at the treasury, exactly.
    const treasuryAfter = await provider.connection.getBalance(
      treasury.publicKey
    );
    const payerAfter = await provider.connection.getBalance(minter);
    expect(treasuryAfter - treasuryBefore).to.equal(COMMIT_FEE_LAMPORTS);
    // Payer paid the fee + the commit-account rent + tx fee.
    expect(payerBefore - payerAfter).to.be.greaterThan(COMMIT_FEE_LAMPORTS);

    const commit = await program.account.mintCommit.fetch(commitPda);
    expect(commit.minter.toBase58()).to.equal(minter.toBase58());
    expect(commit.nonce.toNumber()).to.equal(nonce);
    expect(commit.targetSlot.toNumber()).to.be.greaterThan(0);
    expect(commit.baseType).to.deep.equal({ weapon: {} });

    console.log("E2E_PROOF commit_pda=" + commitPda.toBase58());
    console.log("E2E_PROOF commit_target_slot=" + commit.targetSlot.toString());
    console.log(
      "E2E_PROOF commit_fee_to_treasury=" + (treasuryAfter - treasuryBefore)
    );
  });

  it("reveal_mint: rejected BEFORE target slot, then mints NFT + ArenaItem and closes the commit (spec §12.1)", async () => {
    const minter = provider.wallet.publicKey;
    const nonce = nextCommitNonce++;
    const commitPda = mintCommitPda(minter, nonce);

    await program.methods
      .commitMint({
        nonce: new anchor.BN(nonce),
        baseType: { head: {} },
        skin: { builtin: [9] },
        name: "Ekza Reveal Head",
        symbol: "EKZAITM",
        uri: "https://meta.ekza.space/arena/reveal-head.json",
      })
      .accountsStrict({
        registry: registryPda(),
        mintCommit: commitPda,
        minter,
        treasury: treasury.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const commit = await program.account.mintCommit.fetch(commitPda);
    const targetSlot = commit.targetSlot.toNumber();

    const mint = anchor.web3.Keypair.generate();
    const ata = getAssociatedTokenAddressSync(mint.publicKey, minter);
    const arenaItem = arenaItemPda(mint.publicKey);
    const revealAccounts = {
      registry: registryPda(),
      mintCommit: commitPda,
      mint: mint.publicKey,
      arenaItem,
      minterTokenAccount: ata,
      minter,
      slotHashes: SLOT_HASHES_SYSVAR,
      metadataAccount: metadataPda(mint.publicKey),
      masterEdition: masterEditionPda(mint.publicKey),
      stellarProgram: null,
      stellarRelease: null,
      stellarVault: null,
      tokenMetadataProgram: TOKEN_METADATA_PROGRAM_ID,
      tokenProgram: TOKEN_PROGRAM_ID,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: anchor.web3.SystemProgram.programId,
      rent: anchor.web3.SYSVAR_RENT_PUBKEY,
    };

    // EARLY reveal (current slot still <= target) must be rejected. The 5-slot
    // delay gives a comfortable window for this to land before the target.
    let earlyRejected = false;
    try {
      await program.methods
        .revealMint(new anchor.BN(nonce))
        .accountsStrict(revealAccounts)
        .signers([mint])
        .rpc();
    } catch (error) {
      earlyRejected = true;
      expect(String(error)).to.include("Reveal attempted before");
    }
    expect(earlyRejected).to.equal(true);
    console.log("E2E_PROOF reveal_early_rejected=true target_slot=" + targetSlot);

    // Advance past the target slot, then reveal succeeds (single-shot).
    await waitForSlotAfter(targetSlot);
    await program.methods
      .revealMint(new anchor.BN(nonce))
      .accountsStrict(revealAccounts)
      .signers([mint])
      .rpc();

    // NFT minted (supply 1, decimals 0) and the minter holds it.
    const mintInfo = await getMint(provider.connection, mint.publicKey);
    expect(mintInfo.supply.toString()).to.equal("1");
    expect(mintInfo.decimals).to.equal(0);
    const ataInfo = await getAccount(provider.connection, ata);
    expect(ataInfo.amount.toString()).to.equal("1");

    // ArenaItem written with rolled stats + forward-compat defaults (spec §12.3).
    const item = await program.account.arenaItem.fetch(arenaItem);
    expect(item.mint.toBase58()).to.equal(mint.publicKey.toBase58());
    expect(item.minter.toBase58()).to.equal(minter.toBase58());
    expect(item.baseType).to.deep.equal({ head: {} });
    expect(item.affixes.length).to.be.greaterThan(0);
    expect(item.tier).to.be.greaterThan(0);
    expect(item.enhanceLevel).to.equal(0);
    expect(item.sockets.length).to.equal(0);

    // Commit closed (rent → minter); single-shot.
    const closed = await provider.connection.getAccountInfo(commitPda);
    expect(closed).to.equal(null);

    console.log("E2E_PROOF reveal_minted_nft=" + mint.publicKey.toBase58());
    console.log("E2E_PROOF reveal_arena_item=" + arenaItem.toBase58());
    console.log("E2E_PROOF reveal_commit_closed=" + commitPda.toBase58());
  });

  it("1-tx mint_arena_item NEVER rolls Mythic (clamped at Legendary) + writes forward-compat defaults (spec §12.1/§12.3)", async () => {
    // The same-tx seed is revert-grindable, so the jackpot is fenced off: this
    // path is hard-capped at Legendary. Sample several mints.
    for (let i = 0; i < 6; i++) {
      const { arenaItem } = await mintArenaItemNft({
        baseType: { weapon: {} },
        skin: { builtin: [1] },
        name: "Ekza Clamp Weapon",
      });
      const item = await program.account.arenaItem.fetch(arenaItem);
      expect(item.rarity).to.not.deep.equal({ mythic: {} });
      expect(item.tier).to.be.lessThanOrEqual(4); // Legendary tier or below
      expect(item.enhanceLevel).to.equal(0);
      expect(item.sockets.length).to.equal(0);
    }
    console.log("E2E_PROOF mint_arena_item_clamped_never_mythic=true (6 samples)");
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

  // -------------------------------------------------------------------------
  // Player avatar: character customization + on-chain equip.
  // -------------------------------------------------------------------------

  describe("player avatar", () => {
    const playerAvatarPda = (owner: anchor.web3.PublicKey) =>
      anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("player_avatar_v1"), owner.toBuffer()],
        program.programId
      )[0];

    // Fresh keypairs so the one-avatar-per-wallet PDA is always creatable,
    // even on a shared/accumulated validator.
    const avatarOwner = anchor.web3.Keypair.generate();
    const secondOwner = anchor.web3.Keypair.generate();

    // Registered in the setup test below.
    let avatarCard: anchor.web3.PublicKey; // slotMask 3 (weapon+head)
    let fullCard: anchor.web3.PublicKey; // slotMask 15 (all slots)
    let modifierCard: anchor.web3.PublicKey;

    const registerCard = async (args: any) => {
      const arenaAsset = arenaAssetPda(await nextArenaAssetIndex());
      await program.methods
        .registerArenaAsset(args)
        .accountsStrict({
          registry: registryPda(),
          arenaAsset,
          payer: provider.wallet.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();
      return arenaAsset;
    };

    // Mint an item NFT with the provider wallet, then hand it to `to` (the
    // same holder-transfer pattern the tradeability test uses).
    const mintItemTo = async (
      to: anchor.web3.Keypair,
      baseType: any,
      name: string
    ) => {
      const { mint, ata } = await mintArenaItemNft({
        baseType,
        skin: { builtin: [0] },
        name,
      });
      const destAta = getAssociatedTokenAddressSync(mint, to.publicKey);
      const tx = new anchor.web3.Transaction().add(
        createAssociatedTokenAccountInstruction(
          provider.wallet.publicKey,
          destAta,
          to.publicKey,
          mint
        ),
        createTransferInstruction(ata, destAta, provider.wallet.publicKey, 1)
      );
      await provider.sendAndConfirm(tx);
      return { mint, ata: destAta, arenaItem: arenaItemPda(mint) };
    };

    const equip = (owner: anchor.web3.Keypair, item: { mint: anchor.web3.PublicKey; ata: anchor.web3.PublicKey; arenaItem: anchor.web3.PublicKey }) =>
      program.methods
        .equipItem()
        .accountsStrict({
          playerAvatar: playerAvatarPda(owner.publicKey),
          arenaItem: item.arenaItem,
          mint: item.mint,
          tokenAccount: item.ata,
          owner: owner.publicKey,
        })
        .signers([owner])
        .rpc();

    it("setup: funds owners + registers avatar/modifier cards", async () => {
      const fund = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.transfer({
          fromPubkey: provider.wallet.publicKey,
          toPubkey: avatarOwner.publicKey,
          lamports: anchor.web3.LAMPORTS_PER_SOL,
        }),
        anchor.web3.SystemProgram.transfer({
          fromPubkey: provider.wallet.publicKey,
          toPubkey: secondOwner.publicKey,
          lamports: anchor.web3.LAMPORTS_PER_SOL,
        })
      );
      await provider.sendAndConfirm(fund);

      avatarCard = await registerCard({
        ...baseArgs(),
        archetypeId: "sprout_ipfs",
      }); // slotMask 3
      fullCard = await registerCard({
        ...baseArgs(),
        archetypeId: "toka_tempest",
        slotMask: 15,
      });
      modifierCard = await registerCard({
        ...baseArgs(),
        cardKind: { modifier: {} },
        archetypeId: "burning_hat",
      });
    });

    it("creates the player avatar from an Avatar card (defaults copied)", async () => {
      const pda = playerAvatarPda(avatarOwner.publicKey);
      await program.methods
        .createPlayerAvatar({ name: "Wotori the Brave" })
        .accountsStrict({
          playerAvatar: pda,
          avatarAsset: avatarCard,
          owner: avatarOwner.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([avatarOwner])
        .rpc();

      const avatar = await program.account.playerAvatar.fetch(pda);
      expect(avatar.owner.equals(avatarOwner.publicKey)).to.equal(true);
      expect(avatar.avatarAsset.equals(avatarCard)).to.equal(true);
      expect(avatar.name).to.equal("Wotori the Brave");
      expect(avatar.slotMask).to.equal(3);
      expect(avatar.skinRef).to.deep.equal({ builtin: { "0": 0 } });
      for (const slot of avatar.equipped) {
        expect(slot.equals(anchor.web3.PublicKey.default)).to.equal(true);
      }
      console.log("E2E_PROOF player_avatar=" + pda.toBase58());
    });

    it("rejects creating an avatar from a Modifier card", async () => {
      try {
        await program.methods
          .createPlayerAvatar({ name: "Not An Avatar" })
          .accountsStrict({
            playerAvatar: playerAvatarPda(secondOwner.publicKey),
            avatarAsset: modifierCard,
            owner: secondOwner.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
          })
          .signers([secondOwner])
          .rpc();
        expect.fail("createPlayerAvatar should reject a Modifier card");
      } catch (error) {
        expect(String(error)).to.include("not an Avatar card");
      }
    });

    it("equips an owned item NFT into its base-type slot", async () => {
      const weapon = await mintItemTo(avatarOwner, { weapon: {} }, "Equip Sword");
      await equip(avatarOwner, weapon);

      const avatar = await program.account.playerAvatar.fetch(
        playerAvatarPda(avatarOwner.publicKey)
      );
      expect(avatar.equipped[0].equals(weapon.mint)).to.equal(true);
      console.log("E2E_PROOF equipped_weapon=" + weapon.mint.toBase58());
    });

    it("rejects equipping into a slot the avatar card does not support", async () => {
      // avatarCard slotMask = 3 (weapon+head): Armor (slot 2) must be rejected.
      const armor = await mintItemTo(avatarOwner, { armor: {} }, "Illegal Plate");
      try {
        await equip(avatarOwner, armor);
        expect.fail("equipItem should reject an unsupported slot");
      } catch (error) {
        expect(String(error)).to.include("does not support this equip slot");
      }
    });

    it("rejects equip after the NFT was traded away (holder rule)", async () => {
      const head = await mintItemTo(avatarOwner, { head: {} }, "Traded Cap");
      // avatarOwner ships the NFT to secondOwner…
      const destAta = getAssociatedTokenAddressSync(
        head.mint,
        secondOwner.publicKey
      );
      const tx = new anchor.web3.Transaction().add(
        createAssociatedTokenAccountInstruction(
          avatarOwner.publicKey,
          destAta,
          secondOwner.publicKey,
          head.mint
        ),
        createTransferInstruction(
          head.ata,
          destAta,
          avatarOwner.publicKey,
          1
        )
      );
      await provider.sendAndConfirm(tx, [avatarOwner]);

      // …and can no longer equip it (amount == 0 in their ATA).
      try {
        await equip(avatarOwner, head);
        expect.fail("equipItem should reject a non-holder");
      } catch (error) {
        expect(String(error)).to.include("not the current holder");
      }
    });

    it("unequips a slot and rejects an out-of-range slot index", async () => {
      await program.methods
        .unequipItem(0)
        .accountsStrict({
          playerAvatar: playerAvatarPda(avatarOwner.publicKey),
          owner: avatarOwner.publicKey,
        })
        .signers([avatarOwner])
        .rpc();
      const avatar = await program.account.playerAvatar.fetch(
        playerAvatarPda(avatarOwner.publicKey)
      );
      expect(avatar.equipped[0].equals(anchor.web3.PublicKey.default)).to.equal(
        true
      );

      try {
        await program.methods
          .unequipItem(9)
          .accountsStrict({
            playerAvatar: playerAvatarPda(avatarOwner.publicKey),
            owner: avatarOwner.publicKey,
          })
          .signers([avatarOwner])
          .rpc();
        expect.fail("unequipItem should reject slot 9");
      } catch (error) {
        expect(String(error)).to.include("does not support this equip slot");
      }
    });

    it("customizes name + cosmetic skin (Stellar skin arg rejected)", async () => {
      const pda = playerAvatarPda(avatarOwner.publicKey);
      await program.methods
        .customizeAvatar({ name: "Wotori Reborn", skin: { builtin: [2] } })
        .accountsStrict({
          playerAvatar: pda,
          newAvatarAsset: null,
          owner: avatarOwner.publicKey,
        })
        .signers([avatarOwner])
        .rpc();
      const avatar = await program.account.playerAvatar.fetch(pda);
      expect(avatar.name).to.equal("Wotori Reborn");
      expect(avatar.skinRef).to.deep.equal({ builtin: { "0": 2 } });

      try {
        await program.methods
          .customizeAvatar({ name: null, skin: { stellar: {} } })
          .accountsStrict({
            playerAvatar: pda,
            newAvatarAsset: null,
            owner: avatarOwner.publicKey,
          })
          .signers([avatarOwner])
          .rpc();
        expect.fail("customizeAvatar should reject a Stellar skin arg");
      } catch (error) {
        expect(String(error)).to.include("Invalid item skin");
      }
    });

    it("swaps the base Avatar card: slot mask updates, equips clear", async () => {
      const pda = playerAvatarPda(avatarOwner.publicKey);
      // Re-equip something first so the clear is observable.
      const weapon = await mintItemTo(avatarOwner, { weapon: {} }, "Swap Sword");
      await equip(avatarOwner, weapon);

      await program.methods
        .customizeAvatar({ name: null, skin: null })
        .accountsStrict({
          playerAvatar: pda,
          newAvatarAsset: fullCard,
          owner: avatarOwner.publicKey,
        })
        .signers([avatarOwner])
        .rpc();

      const avatar = await program.account.playerAvatar.fetch(pda);
      expect(avatar.avatarAsset.equals(fullCard)).to.equal(true);
      expect(avatar.slotMask).to.equal(15);
      for (const slot of avatar.equipped) {
        expect(slot.equals(anchor.web3.PublicKey.default)).to.equal(true);
      }
      console.log("E2E_PROOF avatar_swap_cleared_equips=true");
    });

    // -----------------------------------------------------------------------
    // v2 equip ("Lineage tribute"): EquipmentRecord PDA, 7 explicit slots.
    // Slot order mirrors the web EQUIPMENT_SLOTS:
    //   0=Weapon 1=Head 2=Body 3=Gloves 4=Boots 5=Amulet 6=Ring
    // -----------------------------------------------------------------------

    describe("equipment record (v2 slots)", () => {
      const SLOT = {
        Weapon: 0,
        Head: 1,
        Body: 2,
        Gloves: 3,
        Boots: 4,
        Amulet: 5,
        Ring: 6,
      } as const;

      const equipmentPda = (owner: anchor.web3.PublicKey) =>
        anchor.web3.PublicKey.findProgramAddressSync(
          [Buffer.from("equipment"), playerAvatarPda(owner).toBuffer()],
          program.programId
        )[0];

      const equipV2 = (
        owner: anchor.web3.Keypair,
        item: {
          mint: anchor.web3.PublicKey;
          ata: anchor.web3.PublicKey;
          arenaItem: anchor.web3.PublicKey;
        },
        slot: number
      ) =>
        program.methods
          .equipItemV2(slot)
          .accountsStrict({
            playerAvatar: playerAvatarPda(owner.publicKey),
            equipmentRecord: equipmentPda(owner.publicKey),
            arenaItem: item.arenaItem,
            mint: item.mint,
            tokenAccount: item.ata,
            owner: owner.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
          })
          .signers([owner])
          .rpc();

      const unequipV2 = (owner: anchor.web3.Keypair, slot: number) =>
        program.methods
          .unequipItemV2(slot)
          .accountsStrict({
            playerAvatar: playerAvatarPda(owner.publicKey),
            equipmentRecord: equipmentPda(owner.publicKey),
            owner: owner.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
          })
          .signers([owner])
          .rpc();

      // avatarOwner now wears the fullCard (slotMask 15) from the swap test.
      it("equips the full 7-slot set (Armor→Body/Gloves/Boots, Charm→Amulet/Ring)", async () => {
        const weapon = await mintItemTo(avatarOwner, { weapon: {} }, "V2 Sword");
        const head = await mintItemTo(avatarOwner, { head: {} }, "V2 Helm");
        const body = await mintItemTo(avatarOwner, { armor: {} }, "V2 Plate");
        const gloves = await mintItemTo(avatarOwner, { armor: {} }, "V2 Gloves");
        const boots = await mintItemTo(avatarOwner, { armor: {} }, "V2 Boots");
        const amulet = await mintItemTo(avatarOwner, { charm: {} }, "V2 Amulet");
        const ring = await mintItemTo(avatarOwner, { charm: {} }, "V2 Ring");

        await equipV2(avatarOwner, weapon, SLOT.Weapon);
        await equipV2(avatarOwner, head, SLOT.Head);
        await equipV2(avatarOwner, body, SLOT.Body);
        await equipV2(avatarOwner, gloves, SLOT.Gloves);
        await equipV2(avatarOwner, boots, SLOT.Boots);
        await equipV2(avatarOwner, amulet, SLOT.Amulet);
        await equipV2(avatarOwner, ring, SLOT.Ring);

        // FULL-SET READ: the record is the single battle-relevant account.
        const record = await program.account.equipmentRecord.fetch(
          equipmentPda(avatarOwner.publicKey)
        );
        expect(
          record.avatar.equals(playerAvatarPda(avatarOwner.publicKey))
        ).to.equal(true);
        expect(record.owner.equals(avatarOwner.publicKey)).to.equal(true);
        expect(record.slots.length).to.equal(16); // 16 reserved slots
        expect(record.slots[SLOT.Weapon].equals(weapon.mint)).to.equal(true);
        expect(record.slots[SLOT.Head].equals(head.mint)).to.equal(true);
        expect(record.slots[SLOT.Body].equals(body.mint)).to.equal(true);
        expect(record.slots[SLOT.Gloves].equals(gloves.mint)).to.equal(true);
        expect(record.slots[SLOT.Boots].equals(boots.mint)).to.equal(true);
        expect(record.slots[SLOT.Amulet].equals(amulet.mint)).to.equal(true);
        expect(record.slots[SLOT.Ring].equals(ring.mint)).to.equal(true);
        for (let i = 7; i < 16; i++) {
          expect(
            record.slots[i].equals(anchor.web3.PublicKey.default)
          ).to.equal(true);
        }

        // Legacy mirror: canonical slots reflected into PlayerAvatar.equipped.
        const avatar = await program.account.playerAvatar.fetch(
          playerAvatarPda(avatarOwner.publicKey)
        );
        expect(avatar.equipped[0].equals(weapon.mint)).to.equal(true);
        expect(avatar.equipped[1].equals(head.mint)).to.equal(true);
        expect(avatar.equipped[2].equals(body.mint)).to.equal(true);
        expect(avatar.equipped[3].equals(amulet.mint)).to.equal(true);
        console.log(
          "E2E_PROOF equipment_record=" +
            equipmentPda(avatarOwner.publicKey).toBase58()
        );
      });

      it("rejects a base-type/slot mismatch (Weapon into Head, Armor into Ring)", async () => {
        const weapon = await mintItemTo(avatarOwner, { weapon: {} }, "Bad Fit");
        try {
          await equipV2(avatarOwner, weapon, SLOT.Head);
          expect.fail("equipItemV2 should reject Weapon in the Head slot");
        } catch (error) {
          expect(String(error)).to.include("cannot be equipped into");
        }
        const armor = await mintItemTo(avatarOwner, { armor: {} }, "Bad Ring");
        try {
          await equipV2(avatarOwner, armor, SLOT.Ring);
          expect.fail("equipItemV2 should reject Armor in the Ring slot");
        } catch (error) {
          expect(String(error)).to.include("cannot be equipped into");
        }
      });

      it("rejects reserved/out-of-range slots (7..)", async () => {
        const weapon = await mintItemTo(avatarOwner, { weapon: {} }, "Slot Nine");
        try {
          await equipV2(avatarOwner, weapon, 9);
          expect.fail("equipItemV2 should reject slot 9");
        } catch (error) {
          expect(String(error)).to.include("does not support this equip slot");
        }
      });

      it("rejects equip when the avatar card lacks the item class (slot_mask)", async () => {
        // secondOwner gets an avatar from avatarCard (slotMask 3: weapon+head
        // only) — Armor into Body must be rejected by the mask check.
        await program.methods
          .createPlayerAvatar({ name: "Masked" })
          .accountsStrict({
            playerAvatar: playerAvatarPda(secondOwner.publicKey),
            avatarAsset: avatarCard,
            owner: secondOwner.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
          })
          .signers([secondOwner])
          .rpc();
        const armor = await mintItemTo(secondOwner, { armor: {} }, "No Mask");
        try {
          await equipV2(secondOwner, armor, SLOT.Body);
          expect.fail("equipItemV2 should reject a masked-out item class");
        } catch (error) {
          expect(String(error)).to.include("does not support this equip slot");
        }
      });

      it("rejects v2 equip after the NFT was traded away (holder rule)", async () => {
        const ring = await mintItemTo(avatarOwner, { charm: {} }, "Traded Ring");
        const destAta = getAssociatedTokenAddressSync(
          ring.mint,
          secondOwner.publicKey
        );
        const tx = new anchor.web3.Transaction().add(
          createAssociatedTokenAccountInstruction(
            avatarOwner.publicKey,
            destAta,
            secondOwner.publicKey,
            ring.mint
          ),
          createTransferInstruction(
            ring.ata,
            destAta,
            avatarOwner.publicKey,
            1
          )
        );
        await provider.sendAndConfirm(tx, [avatarOwner]);

        try {
          await equipV2(avatarOwner, ring, SLOT.Ring);
          expect.fail("equipItemV2 should reject a non-holder");
        } catch (error) {
          expect(String(error)).to.include("not the current holder");
        }
      });

      it("unequips a v2 slot (record + legacy mirror both clear)", async () => {
        await unequipV2(avatarOwner, SLOT.Body);
        await unequipV2(avatarOwner, SLOT.Gloves);

        const record = await program.account.equipmentRecord.fetch(
          equipmentPda(avatarOwner.publicKey)
        );
        expect(
          record.slots[SLOT.Body].equals(anchor.web3.PublicKey.default)
        ).to.equal(true);
        expect(
          record.slots[SLOT.Gloves].equals(anchor.web3.PublicKey.default)
        ).to.equal(true);
        // Boots untouched.
        expect(
          record.slots[SLOT.Boots].equals(anchor.web3.PublicKey.default)
        ).to.equal(false);

        // Legacy mirror of Body (index 2) cleared too.
        const avatar = await program.account.playerAvatar.fetch(
          playerAvatarPda(avatarOwner.publicKey)
        );
        expect(
          avatar.equipped[2].equals(anchor.web3.PublicKey.default)
        ).to.equal(true);

        try {
          await unequipV2(avatarOwner, 12);
          expect.fail("unequipItemV2 should reject slot 12");
        } catch (error) {
          expect(String(error)).to.include("does not support this equip slot");
        }
        console.log("E2E_PROOF equipment_v2_unequip=true");
      });
    });
  });
});
