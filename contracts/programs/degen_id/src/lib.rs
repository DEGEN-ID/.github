use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{
        self, spl_token::instruction::AuthorityType, FreezeAccount, Mint, MintTo, SetAuthority,
        Token, TokenAccount,
    },
};

// Replace with your deployed program id (run `anchor keys sync`).
declare_id!("ADnrpikhh6f13ZcqWVenEqQzo5TrNj5Xbo4w8L5Jx4pZ");

/// Minimum $DIDID a wallet must hold (in whole tokens, before decimals) to mint
/// its Degen Id. Mirrors the client-side gate in the web app (lib/didid.ts).
pub const HOLD_REQUIREMENT: u64 = 500_000;

pub const AUTHORITY_SEED: &[u8] = b"authority";
pub const CARD_SEED: &[u8] = b"card";
pub const CONFIG_SEED: &[u8] = b"config";

#[program]
pub mod degen_id {
    use super::*;

    /// One-time setup: create the program config. The caller becomes `admin`.
    /// Run this once right after deploy (before the $DIDID token exists). The
    /// $DIDID mint starts unset, so minting is closed until `set_didid_mint`.
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let c = &mut ctx.accounts.config;
        c.admin = ctx.accounts.admin.key();
        c.didid_mint = Pubkey::default(); // unset → mint closed
        c.bump = ctx.bumps.config;
        emit!(Initialized { admin: c.admin });
        Ok(())
    }

    /// Admin-only: point the gate at the official $DIDID mint. Call this when the
    /// token launches — no redeploy needed. (Admin may update it; transfer admin
    /// to a multisig / accept it's a trust point until then.)
    pub fn set_didid_mint(ctx: Context<SetDididMint>, mint: Pubkey) -> Result<()> {
        ctx.accounts.config.didid_mint = mint;
        emit!(DididMintSet { mint });
        Ok(())
    }

    /// Admin: close the config account and refund its rent (wind-down). The
    /// program account itself (the bulk of the deploy cost) is reclaimed
    /// separately via `solana program close <id>` — which requires the upgrade
    /// authority to still be active. See README → "Reclaiming rent".
    pub fn close_config(_ctx: Context<CloseConfig>) -> Result<()> {
        Ok(())
    }

    /// Mint a soulbound Degen Id identity card to `owner`.
    ///
    /// - Requires the official $DIDID mint to be configured (token launched).
    /// - Verifies `didid_mint` IS that official mint, then re-verifies the
    ///   caller holds ≥ HOLD_REQUIREMENT on-chain (never trusts the client).
    /// - Mints 1 token (0 decimals), freezes the owner's account (soulbound),
    ///   drops the mint authority (supply fixed at 1), and records the archetype
    ///   + score in a per-wallet PDA (one card per wallet).
    pub fn mint_identity(ctx: Context<MintIdentity>, args: CardArgs) -> Result<()> {
        require!(args.archetype.len() <= 16, DegenIdError::ArchetypeTooLong);
        require!(args.score <= 1000, DegenIdError::ScoreOutOfRange);

        // ── gate must be configured, and the passed mint must be the real one ──
        let official = ctx.accounts.config.didid_mint;
        require!(official != Pubkey::default(), DegenIdError::MintNotConfigured);
        require_keys_eq!(
            ctx.accounts.didid_mint.key(),
            official,
            DegenIdError::WrongDididMint
        );

        // ── on-chain hold-gate ──────────────────────────────────────────────
        let decimals = ctx.accounts.didid_mint.decimals as u32;
        let required = HOLD_REQUIREMENT
            .checked_mul(10u64.checked_pow(decimals).ok_or(DegenIdError::MathOverflow)?)
            .ok_or(DegenIdError::MathOverflow)?;
        require!(
            ctx.accounts.holder_didid_ata.amount >= required,
            DegenIdError::InsufficientHold
        );

        // ── record the card (PDA seeds enforce one-per-wallet) ──────────────
        let card = &mut ctx.accounts.card;
        card.owner = ctx.accounts.owner.key();
        card.mint = ctx.accounts.identity_mint.key();
        card.archetype = args.archetype.clone();
        card.score = args.score;
        card.minted_at = Clock::get()?.unix_timestamp;
        card.bump = ctx.bumps.card;

        let signer: &[&[&[u8]]] = &[&[AUTHORITY_SEED, &[ctx.bumps.mint_authority]]];

        // ── mint exactly 1 (0 decimals) → NFT ───────────────────────────────
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.identity_mint.to_account_info(),
                    to: ctx.accounts.owner_identity_ata.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
                signer,
            ),
            1,
        )?;

        // ── soulbound: freeze the holder's token account forever ────────────
        token::freeze_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            FreezeAccount {
                account: ctx.accounts.owner_identity_ata.to_account_info(),
                mint: ctx.accounts.identity_mint.to_account_info(),
                authority: ctx.accounts.mint_authority.to_account_info(),
            },
            signer,
        ))?;

        // ── lock supply: drop the mint authority ────────────────────────────
        token::set_authority(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                SetAuthority {
                    current_authority: ctx.accounts.mint_authority.to_account_info(),
                    account_or_mint: ctx.accounts.identity_mint.to_account_info(),
                },
                signer,
            ),
            AuthorityType::MintTokens,
            None,
        )?;

        emit!(IdentityMinted {
            owner: card.owner,
            mint: card.mint,
            archetype: card.archetype.clone(),
            score: card.score,
            minted_at: card.minted_at,
        });
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetDididMint<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = admin)]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct CloseConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin,
        close = admin, // refund the config rent to admin
    )]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct MintIdentity<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Program config — holds the official $DIDID mint the gate is pinned to.
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    /// The $DIDID mint used for the hold-gate (verified == config.didid_mint).
    pub didid_mint: Account<'info, Mint>,

    /// The owner's $DIDID token account — its balance is the hold-gate check.
    #[account(
        associated_token::mint = didid_mint,
        associated_token::authority = owner,
    )]
    pub holder_didid_ata: Account<'info, TokenAccount>,

    /// PDA that is both mint authority and freeze authority for identity NFTs.
    /// CHECK: address is derived from seeds; used only as a CPI signer.
    #[account(seeds = [AUTHORITY_SEED], bump)]
    pub mint_authority: UncheckedAccount<'info>,

    /// The fresh identity-card mint (client passes a new keypair).
    #[account(
        init,
        payer = owner,
        mint::decimals = 0,
        mint::authority = mint_authority,
        mint::freeze_authority = mint_authority,
    )]
    pub identity_mint: Account<'info, Mint>,

    /// The owner's token account for the identity NFT.
    #[account(
        init,
        payer = owner,
        associated_token::mint = identity_mint,
        associated_token::authority = owner,
    )]
    pub owner_identity_ata: Account<'info, TokenAccount>,

    /// One card per wallet — these seeds make a second mint fail.
    #[account(
        init,
        payer = owner,
        space = 8 + IdentityCard::INIT_SPACE,
        seeds = [CARD_SEED, owner.key().as_ref()],
        bump,
    )]
    pub card: Account<'info, IdentityCard>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub didid_mint: Pubkey,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct IdentityCard {
    pub owner: Pubkey,
    pub mint: Pubkey,
    #[max_len(16)]
    pub archetype: String,
    pub score: u16,
    pub minted_at: i64,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CardArgs {
    pub archetype: String,
    pub score: u16,
}

#[event]
pub struct Initialized {
    pub admin: Pubkey,
}

#[event]
pub struct DididMintSet {
    pub mint: Pubkey,
}

#[event]
pub struct IdentityMinted {
    pub owner: Pubkey,
    pub mint: Pubkey,
    pub archetype: String,
    pub score: u16,
    pub minted_at: i64,
}

#[error_code]
pub enum DegenIdError {
    #[msg("$DIDID mint not configured yet — minting is closed")]
    MintNotConfigured,
    #[msg("Provided mint is not the official $DIDID mint")]
    WrongDididMint,
    #[msg("Wallet does not hold enough $DIDID to mint")]
    InsufficientHold,
    #[msg("Archetype string too long (max 16)")]
    ArchetypeTooLong,
    #[msg("Score must be 0..=1000")]
    ScoreOutOfRange,
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
