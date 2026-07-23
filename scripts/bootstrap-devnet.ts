/**
 * Idempotent post-deploy bootstrap for the public Ekza Arena devnet.
 *
 * Required environment:
 *   RPC_URL              devnet RPC endpoint
 *   WALLET               deployment/configuration authority keypair
 *   ARENA_TREASURY       existing SystemAccount receiving the platform share
 *   ARENA_SINK           existing rent-safe SystemAccount receiving sink share
 *   BOARD_KEYPAIR        keypair used only to create the leaderboard account
 *
 * Set ALLOW_RECONFIGURE=1 only when intentionally changing an existing
 * registry. The script refuses every cluster except canonical Solana devnet.
 */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import * as fs from "fs";

const { Keypair, PublicKey, SystemProgram } = anchor.web3;

const DEVNET_GENESIS_HASH = "EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
const ARENA_PROGRAM_ID = new PublicKey(
  "D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ"
);
const LEADERBOARD_PROGRAM_ID = new PublicKey(
  "9A5PkCQrsp98SNBfVRiRs5zVdnzxVdRZQFy2ZDDGjaeU"
);
const STELLAR_PROGRAM_ID = new PublicKey(
  "3rVXfq7LLSLqbDzvZuSrQoMytwczLj2Q8Hue62rxPZAA"
);
const UPGRADEABLE_LOADER_ID = new PublicKey(
  "BPFLoaderUpgradeab1e11111111111111111111111"
);

const COMMIT_FEE_LAMPORTS = 2_000_000;
const CREATOR_BPS = 5_000;
const PLATFORM_BPS = 4_000;
const SINK_BPS = 1_000;
const LEADERBOARD_LEN = 8 + 32 + 2 + 2 + 1 + 3 + 1_000 * 40;

function requiredEnv(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function loadKeypair(path: string): anchor.web3.Keypair {
  const bytes = JSON.parse(fs.readFileSync(path, "utf8"));
  return Keypair.fromSecretKey(Uint8Array.from(bytes));
}

function samePubkey(
  actual: anchor.web3.PublicKey,
  expected: anchor.web3.PublicKey
): boolean {
  return actual.equals(expected);
}

async function main() {
  const rpcUrl = requiredEnv("RPC_URL");
  const walletPath = requiredEnv("WALLET");
  const boardKeypairPath = requiredEnv("BOARD_KEYPAIR");
  const treasury = new PublicKey(requiredEnv("ARENA_TREASURY"));
  const sink = new PublicKey(requiredEnv("ARENA_SINK"));
  const capacity = Number(process.env.BOARD_CAPACITY || "1000");

  if (!Number.isInteger(capacity) || capacity < 100 || capacity > 1_000) {
    throw new Error(
      "BOARD_CAPACITY must be an integer in production range 100..1000"
    );
  }

  const connection = new anchor.web3.Connection(rpcUrl, "confirmed");
  const genesisHash = await connection.getGenesisHash();
  if (genesisHash !== DEVNET_GENESIS_HASH) {
    throw new Error(
      `refusing non-devnet cluster: expected ${DEVNET_GENESIS_HASH}, got ${genesisHash}`
    );
  }

  const walletKeypair = loadKeypair(walletPath);
  const boardKeypair = loadKeypair(boardKeypairPath);
  const wallet = new anchor.Wallet(walletKeypair);
  const provider = new anchor.AnchorProvider(connection, wallet, {
    commitment: "confirmed",
    preflightCommitment: "confirmed",
  });
  anchor.setProvider(provider);

  for (const [name, programId] of [
    ["solana_stellar", STELLAR_PROGRAM_ID],
    ["solana_ekza_arena", ARENA_PROGRAM_ID],
    ["arena_leaderboard", LEADERBOARD_PROGRAM_ID],
  ] as const) {
    const info = await connection.getAccountInfo(programId, "confirmed");
    if (!info?.executable) {
      throw new Error(`${name} is not executable at ${programId.toBase58()}`);
    }
  }

  const treasuryInfo = await connection.getAccountInfo(treasury, "confirmed");
  if (!treasuryInfo || !treasuryInfo.owner.equals(SystemProgram.programId)) {
    throw new Error("ARENA_TREASURY must be an existing SystemAccount");
  }
  const sinkInfo = await connection.getAccountInfo(sink, "confirmed");
  if (!sinkInfo || !sinkInfo.owner.equals(SystemProgram.programId)) {
    throw new Error("ARENA_SINK must be an existing SystemAccount");
  }
  const sinkRent = await connection.getMinimumBalanceForRentExemption(
    sinkInfo.data.length
  );
  if (sinkInfo.lamports < sinkRent) {
    throw new Error(
      `ARENA_SINK is not rent-safe: ${sinkInfo.lamports} < ${sinkRent} lamports`
    );
  }

  const arenaIdl = require("../target/idl/solana_ekza_arena.json");
  const leaderboardIdl = require("../target/idl/arena_leaderboard.json");
  const arena = new Program(arenaIdl as anchor.Idl, provider) as Program;
  const leaderboard = new Program(
    leaderboardIdl as anchor.Idl,
    provider
  ) as Program;

  if (!arena.programId.equals(ARENA_PROGRAM_ID)) {
    throw new Error(
      `Arena IDL address mismatch: ${arena.programId.toBase58()}`
    );
  }
  if (!leaderboard.programId.equals(LEADERBOARD_PROGRAM_ID)) {
    throw new Error(
      `Leaderboard IDL address mismatch: ${leaderboard.programId.toBase58()}`
    );
  }

  const [registry] = PublicKey.findProgramAddressSync(
    [Buffer.from("arena_registry")],
    ARENA_PROGRAM_ID
  );
  const [programData] = PublicKey.findProgramAddressSync(
    [ARENA_PROGRAM_ID.toBuffer()],
    UPGRADEABLE_LOADER_ID
  );

  const expectedRegistry = {
    configurationAuthority: wallet.publicKey,
    treasury,
    sink,
    commitFeeLamports: new anchor.BN(COMMIT_FEE_LAMPORTS),
    creatorBps: CREATOR_BPS,
    platformBps: PLATFORM_BPS,
    sinkBps: SINK_BPS,
  };

  const arenaAccounts = (arena as any).account;
  let registrySignature: string | null = null;
  const registryInfo = await connection.getAccountInfo(registry, "confirmed");
  if (registryInfo) {
    const current = await arenaAccounts.arenaRegistry.fetch(registry);
    const matches =
      samePubkey(current.configurationAuthority, wallet.publicKey) &&
      samePubkey(current.treasury, treasury) &&
      samePubkey(current.sink, sink) &&
      current.commitFeeLamports.eq(expectedRegistry.commitFeeLamports) &&
      current.creatorBps === CREATOR_BPS &&
      current.platformBps === PLATFORM_BPS &&
      current.sinkBps === SINK_BPS;
    if (!matches && process.env.ALLOW_RECONFIGURE !== "1") {
      throw new Error(
        "registry exists with different configuration; set ALLOW_RECONFIGURE=1 for an intentional update"
      );
    }
    if (!matches) {
      registrySignature = await (arena.methods as any)
        .configureRegistry({
          treasury,
          sink,
          commitFeeLamports: expectedRegistry.commitFeeLamports,
          creatorBps: CREATOR_BPS,
          platformBps: PLATFORM_BPS,
          sinkBps: SINK_BPS,
        })
        .accountsStrict({
          registry,
          payer: wallet.publicKey,
          arenaProgram: ARENA_PROGRAM_ID,
          programData,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
    }
  } else {
    registrySignature = await (arena.methods as any)
      .configureRegistry({
        treasury,
        sink,
        commitFeeLamports: expectedRegistry.commitFeeLamports,
        creatorBps: CREATOR_BPS,
        platformBps: PLATFORM_BPS,
        sinkBps: SINK_BPS,
      })
      .accountsStrict({
        registry,
        payer: wallet.publicKey,
        arenaProgram: ARENA_PROGRAM_ID,
        programData,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
  }

  let boardSignature: string | null = null;
  const boardInfo = await connection.getAccountInfo(
    boardKeypair.publicKey,
    "confirmed"
  );
  if (boardInfo) {
    const current = await (leaderboard as any).account.leaderboard.fetch(
      boardKeypair.publicKey
    );
    if (
      !samePubkey(current.authority, wallet.publicKey) ||
      current.capacity !== capacity
    ) {
      throw new Error(
        "leaderboard account exists with unexpected authority/capacity"
      );
    }
  } else {
    const lamports = await connection.getMinimumBalanceForRentExemption(
      LEADERBOARD_LEN
    );
    const createInstruction = SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: boardKeypair.publicKey,
      lamports,
      space: LEADERBOARD_LEN,
      programId: LEADERBOARD_PROGRAM_ID,
    });
    boardSignature = await (leaderboard.methods as any)
      .initLeaderboard(capacity)
      .accountsStrict({
        leaderboard: boardKeypair.publicKey,
        authority: wallet.publicKey,
      })
      .preInstructions([createInstruction])
      .signers([boardKeypair])
      .rpc();
  }

  const finalRegistry = await arenaAccounts.arenaRegistry.fetch(registry);
  const finalBoard = await (leaderboard as any).account.leaderboard.fetch(
    boardKeypair.publicKey
  );
  console.log(
    JSON.stringify(
      {
        cluster: "devnet",
        genesisHash,
        rpcUrl,
        authority: wallet.publicKey.toBase58(),
        registry: {
          address: registry.toBase58(),
          signature: registrySignature,
          treasury: finalRegistry.treasury.toBase58(),
          sink: finalRegistry.sink.toBase58(),
          commitFeeLamports: finalRegistry.commitFeeLamports.toString(),
          creatorBps: finalRegistry.creatorBps,
          platformBps: finalRegistry.platformBps,
          sinkBps: finalRegistry.sinkBps,
        },
        leaderboard: {
          address: boardKeypair.publicKey.toBase58(),
          signature: boardSignature,
          authority: finalBoard.authority.toBase58(),
          capacity: finalBoard.capacity,
          size: finalBoard.size,
          accountBytes: LEADERBOARD_LEN,
        },
      },
      null,
      2
    )
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
