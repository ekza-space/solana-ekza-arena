use anchor_lang::prelude::*;

declare_id!("D3a99Wj3eLLn4jbXU5rLDbaFT6giQiUbmcPkiyQSM8iZ");

#[program]
pub mod solana_ekza_arena {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
