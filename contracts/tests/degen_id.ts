import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { DegenId } from '../target/types/degen_id';
import {
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  getAccount,
} from '@solana/spl-token';
import { Keypair, PublicKey } from '@solana/web3.js';
import { assert } from 'chai';

// Keep in sync with HOLD_REQUIREMENT in the program.
const HOLD_REQUIREMENT = 500_000;
const DIDID_DECIMALS = 6;

describe('degen_id', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.DegenId as Program<DegenId>;
  const payer = (provider.wallet as anchor.Wallet).payer;

  const [config] = PublicKey.findProgramAddressSync([Buffer.from('config')], program.programId);

  it('initializes config, sets $DIDID mint, then mints a soulbound card', async () => {
    // 1. one-time setup — config with admin = payer (mint starts unset)
    await program.methods.initialize().accounts({ admin: payer.publicKey }).rpc();

    // 2. the $DIDID token launches later → create it, then point the gate at it
    const dididMint = await createMint(provider.connection, payer, payer.publicKey, null, DIDID_DECIMALS);
    await program.methods.setDididMint(dididMint).accounts({ admin: payer.publicKey, config }).rpc();

    // 3. fund the wallet over the threshold
    const holderAta = await getOrCreateAssociatedTokenAccount(provider.connection, payer, dididMint, payer.publicKey);
    await mintTo(
      provider.connection,
      payer,
      dididMint,
      holderAta.address,
      payer,
      BigInt(HOLD_REQUIREMENT) * BigInt(10) ** BigInt(DIDID_DECIMALS),
    );

    // 4. mint the identity card
    const identityMint = Keypair.generate();
    const ownerIdentityAta = anchor.utils.token.associatedAddress({
      mint: identityMint.publicKey,
      owner: payer.publicKey,
    });
    const [card] = PublicKey.findProgramAddressSync(
      [Buffer.from('card'), payer.publicKey.toBuffer()],
      program.programId,
    );

    await program.methods
      .mintIdentity({ archetype: 'jupiter', score: 580 })
      .accounts({
        owner: payer.publicKey,
        config,
        dididMint,
        holderDididAta: holderAta.address,
        identityMint: identityMint.publicKey,
        ownerIdentityAta,
        card,
      })
      .signers([identityMint])
      .rpc();

    // 5. card recorded on-chain
    const cardAcc = await program.account.identityCard.fetch(card);
    assert.equal(cardAcc.archetype, 'jupiter');
    assert.equal(cardAcc.score, 580);
    assert.ok(cardAcc.owner.equals(payer.publicKey));

    // 6. exactly 1 token, and the account is frozen (soulbound)
    const ata = await getAccount(provider.connection, ownerIdentityAta);
    assert.equal(ata.amount.toString(), '1');
    assert.isTrue(ata.isFrozen);

    // 7. wind-down: admin closes the config and reclaims its rent
    await program.methods.closeConfig().accounts({ admin: payer.publicKey, config }).rpc();
    let closed = false;
    try {
      await program.account.config.fetch(config);
    } catch {
      closed = true;
    }
    assert.isTrue(closed);
  });
});
