use anyhow::{Context, Result};

/// Configuration from the S3 data lake
pub struct Config {
    pub network_passphrase: String,
    pub ledgers_per_batch: u32,
    pub batches_per_partition: u32,
    pub base_url: String,
    pub ledgers_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            network_passphrase: "Public Global Stellar Network ; September 2015".to_string(),
            ledgers_per_batch: 1,
            batches_per_partition: 64000,
            base_url: "https://aws-public-blockchain.s3.us-east-2.amazonaws.com".to_string(),
            ledgers_path: "v1.1/stellar/ledgers/pubnet".to_string(),
        }
    }
}

impl Config {
    /// Get the RPC URL for fallback queries
    pub fn rpc_url() -> &'static str {
        "https://archive-rpc.lightsail.network/"
    }

    /// Resolve token shortcut to contract address
    pub fn resolve_token(token: &str) -> Option<&'static str> {
        match token.to_lowercase().as_str() {
            "xlm" => Some("CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA"),
            "usdc" => Some("CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75"),
            "kale" => Some("CB23WRDQWGSP6YPMY4UV5C4OW5CBTXKYN3XEATG7KJEZCXMJBYEHOUOV"),
            _ => None,
        }
    }

    /// Generate the S3 URL for a given ledger sequence number
    pub fn generate_url(&self, ledger_seq: u32) -> String {
        let batch_start = ledger_seq;
        let batch_end = batch_start + self.ledgers_per_batch - 1;

        // Calculate partition boundaries
        let partition_start = (batch_start / self.batches_per_partition) * self.batches_per_partition;
        let partition_end = partition_start + self.batches_per_partition - 1;

        let partition_key = format!(
            "{:08X}--{}-{}",
            u32::MAX - partition_start,
            partition_start,
            partition_end
        );

        let batch_key = if self.ledgers_per_batch == 1 {
            format!("{:08X}--{}.xdr.zst", u32::MAX - batch_start, batch_start)
        } else {
            format!(
                "{:08X}--{}-{}.xdr.zst",
                u32::MAX - batch_start,
                batch_start,
                batch_end
            )
        };

        format!(
            "{}/{}/{}/{}",
            self.base_url, self.ledgers_path, partition_key, batch_key
        )
    }
}

/// Download and decompress XDR data from S3
pub fn fetch_and_decompress(url: &str, silent: bool) -> Result<Vec<u8>> {
    if !silent {
        println!("Fetching data from: {}", url);
    }

    let response = reqwest::blocking::get(url)
        .context("Failed to download data from S3")?;

    // Check HTTP status code before processing
    let status = response.status();
    if !status.is_success() {
        if status.as_u16() == 404 {
            anyhow::bail!("Ledger data not found (HTTP 404). The ledger may not be available in the S3 bucket yet.");
        } else {
            anyhow::bail!("HTTP error {}: {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown error"));
        }
    }

    let bytes = response.bytes()
        .context("Failed to read response bytes")?;

    if !silent {
        println!("Downloaded {} bytes (compressed)", bytes.len());
    }

    // Decompress using zstd
    let decompressed = zstd::decode_all(&bytes[..])
        .context("Failed to decompress zstd data")?;

    if !silent {
        println!("Decompressed to {} bytes", decompressed.len());
    }

    Ok(decompressed)
}
