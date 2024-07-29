use crate::oracle_job::OracleJob;
use base64::prelude::*;
use prost::Message;
use reqwest::header::CONTENT_TYPE;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedEvalResponse {
    pub oracle_pubkey: String,
    pub queue_pubkey: String,
    pub oracle_signing_pubkey: String,
    pub feed_hash: String,
    pub recent_hash: String,
    pub failure_error: String,
    pub success_value: String,
    pub msg: String,
    pub signature: String,
    pub recovery_id: i32,
    pub recent_successes_if_failed: Vec<FeedEvalResponse>,
    pub timestamp: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedEvalResponseSingle {
    pub responses: Vec<FeedEvalResponse>,
    pub caller: String,
    pub failures: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FeedEvalManyResponse {
    pub feed_responses: Vec<FeedEvalResponse>,
    pub signature: String,
    pub recovery_id: i32,
    pub errors: Vec<Option<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchSignaturesMultiResponse {
    pub oracle_responses: Vec<FeedEvalManyResponse>,
    pub errors: Vec<Option<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RandomnessRevealResponse {
    pub signature: String,
    pub recovery_id: i32,
    pub value: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AttestEnclaveResponse {
    pub guardian: String,
    pub signature: String,
    pub recovery_id: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PingResponse {
    pub oracle_pubkey: String,
    pub oracle_authority: String,
    pub queue: String,
    pub rate_limit: i32,
    pub version: String,
    pub mr_enclave: String,
    pub is_push_oracle: bool,
    pub is_pull_oracle: bool,
    pub is_gateway: bool,
    pub is_guardian: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchQuoteResponse {
    pub oracle_pubkey: String,
    pub queue: String,
    pub now: i64,
    pub mr_enclave: String,
    pub ed25519_pubkey: String,
    pub secp256k1_pubkey: String,
    pub quote: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BridgeEnclaveResponse {
    pub guardian: String,
    pub oracle: String,
    pub queue: String,
    pub mr_enclave: String,
    pub chain_hash: String,
    pub oracle_ed25519_enclave_signer: String,
    pub oracle_secp256k1_enclave_signer: String,
    pub msg: String,
    pub msg_prehash: String,
    pub signature: String,
    pub recovery_id: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Gateway {
    gateway_url: String,
    client: Arc<Client>,
}

impl Gateway {
    pub fn new(gateway_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            // Switchboard does its own keypair authentication
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        Self {
            gateway_url,
            client: Arc::new(client),
        }
    }

    /// Fetches signatures from the gateway
    /// # Arguments
    /// * `params` - FetchSignaturesParams
    /// * `params.recent_hash` - The recent hash of the feed
    /// * `params.encoded_jobs` - The encoded jobs
    /// * `params.num_signatures` - The number of signatures to fetch
    /// * `params.max_variance` - The maximum variance
    /// * `params.min_responses` - The minimum number of responses
    /// * `params.use_timestamp` - Whether to use the timestamp
    /// # Returns
    /// * `Result<FeedEvalResponseSingle, reqwest::Error>`
    pub async fn fetch_signatures_from_encoded(
        &self,
        params: FetchSignaturesParams,
    ) -> Result<FeedEvalResponseSingle, reqwest::Error> {
        let url = format!("{}/gateway/api/v1/fetch_signatures", self.gateway_url);
        let body = serde_json::json!({
            "api_version": "1.0.0",
            "jobs_b64_encoded": params.encoded_jobs,
            "recent_chainhash": params.recent_hash.unwrap_or_else(|| bs58::encode(vec![0; 32]).into_string()),
            "signature_scheme": "Secp256k1",
            "hash_scheme": "Sha256",
            "num_oracles": params.num_signatures,
            "max_variance": (params.max_variance.unwrap_or(1) as f64 * 1e9) as u64,
            "min_responses": params.min_responses.unwrap_or(1),
            "use_timestamp": params.use_timestamp.unwrap_or(false),
        });

        let res = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;

        let raw = res.text().await?;
        let res = serde_json::from_str::<FeedEvalResponseSingle>(&raw).unwrap();

        Ok(res)
    }

    /// Fetches signatures from the gateway using the multi-feed method
    /// # Arguments
    /// * `params` - FetchSignaturesMultiParams
    /// * `params.recent_hash` - The recent hash of the feed
    /// * `params.feed_configs` - The feed configurations
    /// * `params.num_signatures` - The number of signatures to fetch
    /// * `params.use_timestamp` - Whether to use the timestamp
    /// # Returns
    /// * `Result<FetchSignaturesMultiResponse, reqwest::Error>`
    pub async fn fetch_signatures_multi(
        &self,
        params: FetchSignaturesMultiParams,
    ) -> Result<FetchSignaturesMultiResponse, reqwest::Error> {
        let url = format!("{}/gateway/api/v1/fetch_signatures_multi", self.gateway_url);
        let mut feed_requests = vec![];

        for config in params.feed_configs {
            let max_variance = (config.max_variance.unwrap_or(1) as f64 * 1e9) as u64;
            feed_requests.push(serde_json::json!({
                "jobs_b64_encoded": config.encoded_jobs,
                "max_variance": max_variance,
                "min_responses": config.min_responses.unwrap_or(1),
                "use_timestamp": params.use_timestamp.unwrap_or(false),
            }));
        }

        let body = serde_json::json!({
            "api_version": "1.0.0",
            "num_oracles": params.num_signatures.unwrap_or(1),
            "recent_hash": params.recent_hash.unwrap_or_else(|| bs58::encode(vec![0; 32]).into_string()),
            "signature_scheme": "Secp256k1",
            "hash_scheme": "Sha256",
            "feed_requests": feed_requests,
        });

        let res = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;
        let res = res.json::<FetchSignaturesMultiResponse>().await?;

        Ok(res)
    }

    pub async fn test_gateway(&self) -> bool {
        let client = &self.client;
        // Make HTTP request
        let url = format!("{}/gateway/api/v1/test", self.gateway_url);
        let response = client.get(&url).send().await;

        // Process response
        if let Ok(resp) = response {
            if let Ok(text) = resp.text().await {
                !text.is_empty()
            } else {
                false
            }
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub struct FetchSignaturesParams {
    pub recent_hash: Option<String>,
    pub encoded_jobs: Vec<String>,
    pub num_signatures: u32,
    pub max_variance: Option<u32>,
    pub min_responses: Option<u32>,
    pub use_timestamp: Option<bool>,
}

#[derive(Debug)]
pub struct FeedConfig {
    pub encoded_jobs: Vec<String>,
    pub max_variance: Option<u32>,
    pub min_responses: Option<u32>,
}

#[derive(Debug)]
pub struct FetchSignaturesMultiParams {
    pub recent_hash: Option<String>,
    pub feed_configs: Vec<FeedConfig>,
    pub num_signatures: Option<u32>,
    pub use_timestamp: Option<bool>,
}

pub fn encode_jobs(job_array: Vec<OracleJob>) -> Vec<String> {
    job_array
        .into_iter()
        .map(|job| BASE64_STANDARD.encode(&job.encode_length_delimited_to_vec()))
        .collect()
}
