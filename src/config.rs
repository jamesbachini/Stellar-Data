/// Configuration for Stellar data sources
pub struct Config {
    #[allow(dead_code)]
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

    /// Get the Soroban RPC URL for contract calls (price queries, balance queries)
    pub fn soroban_rpc_url() -> &'static str {
        "https://rpc.lightsail.network/"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.network_passphrase, "Public Global Stellar Network ; September 2015");
        assert_eq!(config.ledgers_per_batch, 1);
        assert_eq!(config.batches_per_partition, 64000);
        assert_eq!(config.base_url, "https://aws-public-blockchain.s3.us-east-2.amazonaws.com");
        assert_eq!(config.ledgers_path, "v1.1/stellar/ledgers/pubnet");
    }

    #[test]
    fn test_resolve_token_xlm() {
        let result = Config::resolve_token("xlm");
        assert_eq!(result, Some("CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA"));
    }

    #[test]
    fn test_resolve_token_xlm_uppercase() {
        let result = Config::resolve_token("XLM");
        assert_eq!(result, Some("CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA"));
    }

    #[test]
    fn test_resolve_token_usdc() {
        let result = Config::resolve_token("usdc");
        assert_eq!(result, Some("CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75"));
    }

    #[test]
    fn test_resolve_token_unknown() {
        let result = Config::resolve_token("unknown_token");
        assert_eq!(result, None);
    }

    #[test]
    fn test_generate_url_ledger_0() {
        let config = Config::default();
        let url = config.generate_url(0);
        assert_eq!(
            url,
            "https://aws-public-blockchain.s3.us-east-2.amazonaws.com/v1.1/stellar/ledgers/pubnet/FFFFFFFF--0-63999/FFFFFFFF--0.xdr.zst"
        );
    }

    #[test]
    fn test_generate_url_ledger_63864() {
        let config = Config::default();
        let url = config.generate_url(63864);
        // Partition: 0-63999
        // Batch: 63864
        // Hex for 63864: F978
        // Inverted: FFFF0687
        assert_eq!(
            url,
            "https://aws-public-blockchain.s3.us-east-2.amazonaws.com/v1.1/stellar/ledgers/pubnet/FFFFFFFF--0-63999/FFFF0687--63864.xdr.zst"
        );
    }

    #[test]
    fn test_generate_url_ledger_64000() {
        let config = Config::default();
        let url = config.generate_url(64000);
        // Partition: 64000-127999
        // u32::MAX - 64000 = 4294903295 = 0xFFFF05FF
        assert_eq!(
            url,
            "https://aws-public-blockchain.s3.us-east-2.amazonaws.com/v1.1/stellar/ledgers/pubnet/FFFF05FF--64000-127999/FFFF05FF--64000.xdr.zst"
        );
    }

    #[test]
    fn test_generate_url_ledger_50000000() {
        let config = Config::default();
        let url = config.generate_url(50000000);
        // Partition: (50000000 / 64000) * 64000 = 781 * 64000 = 49984000
        // Partition end: 49984000 + 63999 = 50047999
        // u32::MAX - 49984000 = 4244983295 = 0xFD054DFF
        // u32::MAX - 50000000 = 4244967295 = 0xFD050F7F
        assert_eq!(
            url,
            "https://aws-public-blockchain.s3.us-east-2.amazonaws.com/v1.1/stellar/ledgers/pubnet/FD054DFF--49984000-50047999/FD050F7F--50000000.xdr.zst"
        );
    }

    #[test]
    fn test_generate_url_partition_boundaries() {
        let config = Config::default();

        // Test first ledger of second partition
        let url = config.generate_url(64000);
        assert!(url.contains("64000-127999"));
        assert!(url.contains("64000.xdr.zst"));

        // Test last ledger of first partition
        let url = config.generate_url(63999);
        assert!(url.contains("0-63999"));
        assert!(url.contains("63999.xdr.zst"));
    }

    #[test]
    fn test_generate_url_custom_config() {
        let config = Config {
            network_passphrase: "Test Network".to_string(),
            ledgers_per_batch: 10,
            batches_per_partition: 1000,
            base_url: "https://test.example.com".to_string(),
            ledgers_path: "test/path".to_string(),
        };

        let url = config.generate_url(100);
        // Partition: 0-999 (batches) but ledgers_per_batch = 10
        // So partition 0 covers ledgers 0-9999
        // Batch 100 covers ledgers 100-109
        assert!(url.starts_with("https://test.example.com/test/path/"));
        assert!(url.contains("100-109.xdr.zst"));
    }

    #[test]
    fn test_generate_url_hex_inversion() {
        let config = Config::default();

        // Test that hex values are properly inverted
        // For ledger 0: u32::MAX - 0 = 0xFFFFFFFF
        let url = config.generate_url(0);
        assert!(url.contains("FFFFFFFF--0.xdr.zst"));

        // For ledger 1: u32::MAX - 1 = 0xFFFFFFFE
        let url = config.generate_url(1);
        assert!(url.contains("FFFFFFFE--1.xdr.zst"));
    }
}
