pub mod instructions;
pub use instructions::*;
pub mod crossbar;
pub use crossbar::*;
pub mod gateway;
pub use gateway::*;
pub mod pull_feed;
pub use pull_feed::*;
pub mod associated_token_account;
pub mod oracle_job;
pub use associated_token_account::*;
pub mod recent_slothashes;
pub use recent_slothashes::*;
pub mod accounts;
pub use accounts::*;
pub mod lut_owner;
pub mod address_lookup_table;
use crate::oracle_job::OracleJob;
use anyhow_ext::Error as AnyhowError;
use lazy_static::lazy_static;
pub use lut_owner::*;
use solana_sdk::hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;

lazy_static! {
    pub static ref SWITCHBOARD_ON_DEMAND_PROGRAM_ID: Pubkey =
        Pubkey::from_str("SBondMDrcV3K4kxZR1HNVT7osZxAHVHgYXL5Ze1oMUv").unwrap();
}

pub const STATE_SEED: &[u8] = b"STATE";
pub const ORACLE_FEED_STATS_SEED: &[u8] = b"OracleFeedStats";
pub const ORACLE_RANDOMNESS_STATS_SEED: &[u8] = b"OracleRandomnessStats";
pub const ORACLE_STATS_SEED: &[u8] = b"OracleStats";
pub const LUT_SIGNER_SEED: &[u8] = b"LutSigner";
pub const DELEGATION_SEED: &[u8] = b"Delegation";
pub const DELEGATION_GROUP_SEED: &[u8] = b"Group";
pub const REWARD_POOL_VAULT_SEED: &[u8] = b"RewardPool";

pub fn ix_to_tx(
    ixs: &[Instruction],
    signers: &[&Keypair],
    blockhash: hash::Hash,
) -> Result<Transaction, AnyhowError> {
    let msg = Message::new(ixs, Some(&signers[0].pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&signers.to_vec(), blockhash)?;
    Ok(tx)
}
