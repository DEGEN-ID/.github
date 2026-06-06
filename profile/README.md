![DEGEN ID](https://raw.githubusercontent.com/DEGEN-ID/.github/main/assets/banner.png)

# DEGEN ID

On-chain identity for Solana.

Connect a wallet, get scored across 12 archetypes, and mint a soulbound identity
card. The card is a single NFT (0 decimals, supply 1) that records your archetype
and score on-chain. It is frozen on mint, so it cannot be transferred or sold.
Minting requires holding $DIDID.

The web app does the read-only wallet scan and scoring. The on-chain program does
the mint and re-checks the $DIDID hold requirement itself, so the gate does not
depend on the client.

- App: https://degenidentity.com
- Contracts: [degen_id Anchor program](https://github.com/DEGEN-ID/.github/tree/main/contracts)

Status: draft, unaudited, not deployed to mainnet. Do not use with real funds
before a review.
