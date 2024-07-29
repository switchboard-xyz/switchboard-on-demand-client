use crate::Gateway;
use crate::OracleAccountData;
use crate::State;
use crate::*;
use anyhow_ext::anyhow;
use anyhow_ext::Context;
use anyhow_ext::Error as AnyhowError;
use associated_token_account::get_associated_token_address;
use associated_token_account::NATIVE_MINT;
use associated_token_account::SPL_TOKEN_PROGRAM_ID;
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use bs58;
use bytemuck;
use futures::future::try_join_all;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::instruction::AccountMeta;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program;
use std::future::Future;
use std::pin::Pin;
use std::result::Result;

#[derive(Clone, Debug)]
pub struct OracleResponse {
    pub value: Decimal,
    pub error: String,
    pub oracle: Pubkey,
    pub signature: [u8; 64],
    pub recovery_id: u8,
}

#[derive(Clone, Debug, Default)]
pub struct FetchUpdateParams {
    pub feed: Pubkey,
    pub payer: Pubkey,
    pub gateway: Gateway,
    pub crossbar: Option<CrossbarClient>,
    pub num_signatures: Option<u32>,
    pub debug: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct FetchUpdateManyParams {
    pub feeds: Vec<Pubkey>,
    pub payer: Pubkey,
    pub gateway: Gateway,
    pub crossbar: Option<CrossbarClient>,
    pub num_signatures: Option<u32>,
    pub debug: Option<bool>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct SolanaSubmitSignaturesParams {
    pub queue: Pubkey,
    pub feed: Pubkey,
    pub payer: Pubkey,
}

pub struct PullFeed;
impl PullFeed {
    /// Load the data from the PullFeed account
    /// * `client` - The RPC client
    /// * `key` - The account public key
    pub async fn load_data(
        client: &RpcClient,
        key: &Pubkey,
    ) -> Result<PullFeedAccountData, AnyhowError> {
        let account = client
            .get_account_data(key)
            .await
            .map_err(|_| anyhow!("PullFeed.load_data: Account not found"))?;
        let account = account[8..].to_vec();
        let data = bytemuck::try_from_bytes::<PullFeedAccountData>(&account)
            .map_err(|_| anyhow!("PullFeed.load_data: Failed to parse data"))?;
        Ok(data.clone())
    }

    /// Translate the Switchboard On-Demand oracle responses to a Solana instruction
    /// * `slot` - The slot of the responses
    /// * `responses` - The oracle responses
    /// * `params` - The parameters for the instruction
    /// * `params.feed` - The feed account
    /// * `params.queue` - The queue account
    /// * `params.payer` - The payer account
    fn get_solana_submit_signatures_ix(
        slot: u64,
        responses: Vec<OracleResponse>,
        params: SolanaSubmitSignaturesParams,
    ) -> Result<Instruction, AnyhowError> {
        let mut submissions = Vec::new();
        for resp in &responses {
            let mut value = resp.value;
            value.rescale(18);
            submissions.push(Submission {
                value: value.mantissa(),
                signature: resp.signature,
                recovery_id: resp.recovery_id,
                offset: 0,
            });
        }
        let mut remaining_accounts = Vec::new();
        for resp in &responses {
            remaining_accounts.push(AccountMeta::new_readonly(resp.oracle, false));
        }
        for resp in responses {
            let stats_key = OracleAccountData::stats_key(&resp.oracle);
            remaining_accounts.push(AccountMeta::new(stats_key, false));
        }
        let mut submit_ix = Instruction {
            program_id: *SWITCHBOARD_ON_DEMAND_PROGRAM_ID,
            data: PullFeedSubmitResponseParams { slot, submissions }.data(),
            accounts: PullFeedSubmitResponse {
                feed: params.feed,
                queue: params.queue,
                program_state: State::key(),
                recent_slothashes: solana_sdk::sysvar::slot_hashes::ID,
                payer: params.payer,
                system_program: system_program::ID,
                reward_vault: get_associated_token_address(&params.queue, &NATIVE_MINT),
                token_program: *SPL_TOKEN_PROGRAM_ID,
                token_mint: *NATIVE_MINT,
            }
            .to_account_metas(None),
        };
        submit_ix.accounts.extend(remaining_accounts);
        Ok(submit_ix)
    }

    /// Fetch the oracle responses and format them into a Solana instruction.
    /// Also fetches relevant lookup tables for the instruction.
    /// * `client` - The RPC client
    /// * `params` - The parameters for the fetch
    /// * `params.feed` - The feed account
    /// * `params.payer` - The payer account
    /// * `params.gateway` - The gateway object
    /// * `params.num_signatures` - The number of signatures to fetch
    /// * `params.debug` - Whether to print debug information
    /// Returns the Solana instruction, the oracle responses, the number of successful responses, and the lookup tables
    pub async fn fetch_update_ix(
        client: &RpcClient,
        params: FetchUpdateParams,
    ) -> Result<
        (
            Instruction,
            Vec<OracleResponse>,
            usize,
            Vec<AddressLookupTableAccount>,
        ),
        AnyhowError,
    > {
        let latest_slot = SlotHashSysvar::get_latest_slothash(&client)
            .await
            .context("PullFeed.fetchUpdateIx: Failed to fetch latest slot")?;

        let feed_data = PullFeed::load_data(client, &params.feed).await?;
        let crossbar = params.crossbar.unwrap_or_default();
        let jobs = crossbar
            .fetch(&hex::encode(feed_data.feed_hash))
            .await
            .context("PullFeed.fetchUpdateIx: Failed to fetch jobs")?;
        let jobs: Vec<OracleJob> =
            serde_json::from_value(jobs.get("jobs").unwrap().clone()).unwrap();
        let encoded_jobs = encode_jobs(jobs);
        // ""https://xoracle-1-mn.switchboard.xyz""
        let gateway = params.gateway;

        // println!("max_variance: {:?}", data.max_variance);
        // println!("max_variance: {:?}", data.max_variance / 1_000_000_000);
        // println!("max_variance: {:?}", data.max_variance as u32);
        let num_signatures = if params.num_signatures.is_none() {
            feed_data.min_responses + ((feed_data.min_responses as f64) / 3.0).ceil() as u32
        } else {
            params.num_signatures.unwrap()
        };
        let price_signatures = gateway
            .fetch_signatures_from_encoded(FetchSignaturesParams {
                recent_hash: Some(bs58::encode(latest_slot.hash.clone()).into_string()),
                encoded_jobs: encoded_jobs.clone(),
                num_signatures: num_signatures,
                max_variance: Some((feed_data.max_variance / 1_000_000_000) as u32),
                min_responses: Some(feed_data.min_responses),
                use_timestamp: Some(false),
            })
            .await
            .context("PullFeed.fetchUpdateIx: Failed to fetch signatures")?;

        let mut num_successes = 0;
        let oracle_responses: Vec<OracleResponse> = price_signatures
            .responses
            .iter()
            .map(|x| {
                let value = x.success_value.parse::<i128>();
                if value.is_ok() {
                    num_successes += 1;
                }
                let value = Decimal::from_i128_with_scale(value.unwrap(), 18);
                OracleResponse {
                    value,
                    error: x.failure_error.clone(),
                    oracle: Pubkey::new_from_array(
                        hex::decode(x.oracle_pubkey.clone())
                            .unwrap()
                            .try_into()
                            .unwrap(),
                    ),
                    recovery_id: x.recovery_id as u8,
                    signature: base64
                        .decode(x.signature.clone())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                }
            })
            .collect();

        if params.debug.unwrap_or(false) {
            println!("priceSignatures: {:?}", price_signatures);
        }

        if num_successes == 0 {
            return Err(anyhow_ext::Error::msg(format!(
                "PullFeed.fetchUpdateIx Failure: {:?}",
                oracle_responses
                    .iter()
                    .map(|x| &x.error)
                    .collect::<Vec<&String>>()
            )));
        }
        let submit_signatures_ix = PullFeed::get_solana_submit_signatures_ix(
            latest_slot.slot,
            oracle_responses.clone(),
            SolanaSubmitSignaturesParams {
                feed: params.feed,
                queue: feed_data.queue,
                payer: params.payer,
            },
        )
        .context("PullFeed.fetchUpdateIx: Failed to create submit signatures instruction")?;

        let oracle_keys: Vec<Pubkey> = oracle_responses.iter().map(|x| x.oracle).collect();
        let feed_key = params.feed;
        let feed = feed_data.clone();
        let queue_key = feed.queue;
        let mut luts: Vec<
            Pin<Box<dyn Future<Output = Result<AddressLookupTableAccount, AnyhowError>> + Send>>,
        > = Vec::new();
        let oracle_luts = load_lookup_tables::<OracleAccountData>(client, &oracle_keys);
        luts.push(Box::pin(load_lookup_table::<PullFeedAccountData>(
            client, feed_key,
        )));
        luts.push(Box::pin(load_lookup_table::<QueueAccountData>(
            client, queue_key,
        )));
        let mut luts = try_join_all(luts).await?;
        luts.extend(oracle_luts.await?);

        Ok((submit_signatures_ix, oracle_responses, num_successes, luts))
    }

    /// Fetch the oracle responses and format them into a Solana instruction.
    /// Also fetches relevant lookup tables for the instruction.
    /// This is much like fetch_update_ix method, but for multiple feeds at once.
    /// # Arguments
    /// * `client` - The RPC client
    /// * `params` - The parameters for the fetch
    pub async fn fetch_update_many_ix(
        client: &RpcClient,
        params: FetchUpdateManyParams,
    ) -> Result<(Instruction, Vec<AddressLookupTableAccount>), AnyhowError> {
        let crossbar = params.crossbar.unwrap_or_default();
        let gateway = params.gateway;
        let num_signatures = params.num_signatures.unwrap_or(1);
        let mut feed_configs = Vec::new();
        let mut queue = Pubkey::default();

        for feed in &params.feeds {
            let data = PullFeed::load_data(client, &feed).await?;
            queue = data.queue;
            let jobs = crossbar
                .fetch(&hex::encode(data.feed_hash))
                .await
                .context("PullFeed.fetchUpdateIx: Failed to fetch jobs")?;
            let jobs: Vec<OracleJob> =
                serde_json::from_value(jobs.get("jobs").unwrap().clone()).unwrap();
            let encoded_jobs = encode_jobs(jobs);
            let max_variance = (data.max_variance / 1_000_000_000) as u32;
            let min_responses = data.min_responses;
            let feed_config = FeedConfig {
                encoded_jobs,
                max_variance: Some(max_variance),
                min_responses: Some(min_responses),
            };
            feed_configs.push(feed_config);
        }
        let latest_slot = SlotHashSysvar::get_latest_slothash(&client)
            .await
            .context("PullFeed.fetchUpdateIx: Failed to fetch latest slot")?;
        let price_signatures = gateway
            .fetch_signatures_multi(FetchSignaturesMultiParams {
                recent_hash: Some(bs58::encode(latest_slot.hash.clone()).into_string()),
                num_signatures: Some(num_signatures),
                feed_configs,
                use_timestamp: Some(false),
            })
            .await
            .context("PullFeed.fetchUpdateIx: fetch signatures failure")?;
        if params.debug.unwrap_or(false) {
            println!("priceSignatures: {:?}", price_signatures);
        }

        let mut submissions: Vec<MultiSubmission> = Vec::new();
        for x in &price_signatures.oracle_responses {
            submissions.push(MultiSubmission {
                values: x
                    .feed_responses
                    .iter()
                    .map(|x| x.success_value.parse().unwrap_or(i128::MAX))
                    .collect(),
                signature: base64
                    .decode(x.signature.clone())
                    .context("base64:decode failure")?
                    .try_into()
                    .map_err(|_| anyhow!("base64:decode failure"))?,
                recovery_id: x.recovery_id as u8,
            });
        }
        let ix_data = PullFeedSubmitResponseManyParams {
            slot: latest_slot.slot,
            submissions,
        };
        let mut remaining_accounts = Vec::new();
        let oracle_keys: Vec<Pubkey> = price_signatures
            .oracle_responses
            .iter()
            .map(|x| {
                Pubkey::new_from_array(
                    hex::decode(x.feed_responses.get(0).unwrap().oracle_pubkey.clone())
                        .unwrap_or_default()
                        .try_into()
                        .unwrap(),
                )
            })
            .collect();
        for feed in &params.feeds {
            remaining_accounts.push(AccountMeta::new(*feed, false));
        }
        for oracle in oracle_keys.iter() {
            remaining_accounts.push(AccountMeta::new_readonly(*oracle, false));
            let stats_key = OracleAccountData::stats_key(&oracle);
            remaining_accounts.push(AccountMeta::new(stats_key, false));
        }

        let mut luts: Vec<
            Pin<Box<dyn Future<Output = Result<AddressLookupTableAccount, AnyhowError>> + Send>>,
        > = Vec::new();
        let oracle_luts = load_lookup_tables::<OracleAccountData>(client, &oracle_keys);
        let feed_luts = load_lookup_tables::<PullFeedAccountData>(client, &params.feeds);
        luts.push(Box::pin(load_lookup_table::<QueueAccountData>(
            client, queue,
        )));
        let mut luts = try_join_all(luts).await?;
        luts.extend(oracle_luts.await?);
        luts.extend(feed_luts.await?);

        let mut submit_ix = Instruction {
            program_id: *SWITCHBOARD_ON_DEMAND_PROGRAM_ID,
            data: ix_data.data(),
            accounts: PullFeedSubmitResponseMany {
                queue: queue,
                program_state: State::key(),
                recent_slothashes: solana_sdk::sysvar::slot_hashes::ID,
                payer: params.payer,
                system_program: system_program::ID,
                reward_vault: get_associated_token_address(&queue, &NATIVE_MINT),
                token_program: *SPL_TOKEN_PROGRAM_ID,
                token_mint: *NATIVE_MINT,
            }
            .to_account_metas(None),
        };
        submit_ix.accounts.extend(remaining_accounts);

        Ok((submit_ix, luts))
    }
}
