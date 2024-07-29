#![allow(non_snake_case)]
use anyhow_ext::anyhow;
use anyhow_ext::Context;
use anyhow_ext::Error as AnyhowError;
use base58::ToBase58;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct StoreResponse {
    pub cid: String,
    pub feedHash: String,
    pub queueHex: String,
}

#[derive(Serialize, Deserialize)]
pub struct FetchSolanaUpdatesResponse {
    pub success: bool,
    pub pullIx: String,
    pub responses: Vec<Response>,
    pub lookupTables: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub oracle: String,
    pub result: Option<f64>,
    pub errors: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimulateSolanaFeedsResponse {
    pub feed: String,
    pub feedHash: String,
    pub results: Vec<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimulateFeedsResponse {
    pub feedHash: String,
    pub results: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct CrossbarClient {
    crossbar_url: String,
    verbose: bool,
    client: Client,
}

impl Default for CrossbarClient {
    fn default() -> Self {
        Self::new("https://crossbar.switchboard.xyz", false)
    }
}

impl CrossbarClient {
    pub fn default(verbose: Option<bool>) -> Self {
        Self::new("https://crossbar.switchboard.xyz", verbose.unwrap_or(false))
    }

    pub fn new(crossbar_url: &str, verbose: bool) -> Self {
        Self {
            crossbar_url: crossbar_url.to_string(),
            verbose,
            client: Client::new(),
        }
    }

    /// Fetch feed jobs from the crossbar gateway
    /// # Arguments
    /// * `feed_hash` - The feed hash of the jobs it performs
    /// # Returns
    /// * `Result<serde_json::Value>` - The response from the crossbar gateway,
    ///   containing the json formatted oracle jobs
    pub async fn fetch(&self, feed_hash: &str) -> Result<serde_json::Value, AnyhowError> {
        let url = format!("{}/fetch/{}", self.crossbar_url, feed_hash);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send fetch request")?;

        let status = resp.status();
        if !status.is_success() {
            if self.verbose {
                eprintln!("{}", resp.text().await.context("Failed to fetch response")?);
            }
            return Err(anyhow!("Bad status code {}", status.as_u16()));
        }

        Ok(resp.json().await.context("Failed to parse response")?)
    }

    /// Store feed jobs in the crossbar gateway to a pinned IPFS address
    pub async fn store(
        &self,
        queue_address: &str,
        jobs: &[serde_json::Value],
    ) -> Result<StoreResponse, AnyhowError> {
        let queue = bs58::decode(queue_address)
            .into_vec()
            .context("Failed to decode queue address")?;
        let queue_hex = queue.to_base58();
        let payload = serde_json::json!({ "queue": queue_hex, "jobs": jobs });

        let url = format!("{}/store", self.crossbar_url);
        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .header("Content-Type", "application/json")
            .send()
            .await
            .context("Failed to send store request")?;

        let status = resp.status();
        if !status.is_success() {
            if self.verbose {
                eprintln!(
                    "{}: {}",
                    status,
                    resp.text().await.context("Failed to fetch response")?
                );
            }
            return Err(anyhow!("Bad status code {}", status.as_u16()));
        }

        Ok(resp.json().await.context("Failed to parse response")?)
    }

    pub async fn fetch_solana_updates(
        &self,
        network: &str,
        feed_pubkeys: &[&str],
        num_signatures: Option<usize>,
    ) -> Result<Vec<FetchSolanaUpdatesResponse>, AnyhowError> {
        if feed_pubkeys.is_empty() {
            return Err(anyhow!("Feed pubkeys are empty"));
        }

        let feeds_param = feed_pubkeys.join(",");
        let mut url = format!(
            "{}/updates/solana/{}/{}",
            self.crossbar_url, network, feeds_param
        );
        if let Some(num_signatures) = num_signatures {
            url.push_str(&format!("?numSignatures={}", num_signatures));
        }

        let resp = self.client.get(&url).send().await?;

        let status = resp.status();
        if !status.is_success() {
            if self.verbose {
                eprintln!(
                    "{}: {}",
                    status,
                    resp.text().await.context("Failed to fetch response")?
                );
            }
            return Err(anyhow!("Bad status code {}", status.as_u16()));
        }

        Ok(resp.json().await.context("Failed to parse response")?)
    }

    /// Simulate feed responses from the crossbar gateway
    pub async fn simulate_solana_feeds(
        &self,
        network: &str,
        feed_pubkeys: &[&str],
    ) -> Result<Vec<SimulateSolanaFeedsResponse>, AnyhowError> {
        if feed_pubkeys.is_empty() {
            return Err(anyhow!("Feed pubkeys are empty"));
        }

        let feeds_param = feed_pubkeys.join(",");
        let url = format!(
            "{}/simulate/solana/{}/{}",
            self.crossbar_url, network, feeds_param
        );
        let resp = self.client.get(&url).send().await?;

        let status = resp.status();
        if !status.is_success() {
            if self.verbose {
                eprintln!(
                    "{}: {}",
                    status,
                    resp.text().await.context("Failed to fetch response")?
                );
            }
            return Err(anyhow!("Bad status code {}", status.as_u16()));
        }

        Ok(resp.json().await.context("Failed to parse response")?)
    }

    /// Simulate feed responses from the crossbar gateway
    /// # Arguments
    /// * `feed_hashes` - The feed hashes to simulate
    pub async fn simulate_feeds(
        &self,
        feed_hashes: &[&str],
    ) -> Result<Vec<SimulateFeedsResponse>, AnyhowError> {
        if feed_hashes.is_empty() {
            return Err(anyhow!("Feed hashes are empty"));
        }

        let feeds_param = feed_hashes.join(",");
        let url = format!("{}/simulate/{}", self.crossbar_url, feeds_param);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send simulate feeds request")?;

        let status = resp.status();
        if !status.is_success() {
            if self.verbose {
                eprintln!(
                    "{}: {}",
                    status,
                    resp.text().await.context("Failed to fetch response")?
                );
            }
            return Err(anyhow!("Bad status code {}", status.as_u16()));
        }

        Ok(resp.json().await.context("Failed to parse response")?)
    }
}
