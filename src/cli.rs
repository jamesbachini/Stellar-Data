use clap::Parser;

/// Stellar blockchain data query tool for S3 public data lake
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = "Query Stellar blockchain data from public data lake.\n\
                  Downloads XDR data, decompresses it, and converts to JSON.\n\n\
                  Examples:\n  \
                    stellar-data --ledger 50000000 --query transactions\n\
                    stellar-data --ledger 63864-63900 --query address --address GABC...\n\
                    stellar-data --ledger -999 --query transactions\n\
                    stellar-data --ledger 59424051-59424060 --query contract --address CAB1...\n\
                    stellar-data --ledger 59424051-59424060 --query function --name work\n\n\

                  For more information: https://github.com/stellar/stellar-public-data"
)]
pub struct Args {
    /// Ledger/block number or range to query
    ///
    /// Formats:
    ///   Single: --ledger 63864
    ///   Range:  --ledger 63864-63900
    ///   Recent: --ledger -999 (last 999 blocks from current)
    ///
    /// Note: Not required for --query balance
    #[arg(
        short,
        long,
        allow_hyphen_values = true,
        value_name = "LEDGER",
        help = "Ledger/block number, range, or negative value for recent blocks"
    )]
    pub ledger: Option<String>,

    /// Query type determines what data to return
    ///
    /// Options:
    ///   all          - Full ledger metadata (default)
    ///   transactions - Just transaction data
    ///   address      - Transactions involving a specific address (requires --address)
    ///   contract     - Transactions involving a specific contract (requires --address)
    ///   function     - Transactions calling a specific function (requires --name)
    ///   balance      - Token balance for an address (requires --address and --token)
    #[arg(
        short,
        long,
        default_value = "all",
        value_name = "TYPE",
        help = "Query type: 'all', 'transactions', 'address', 'contract', 'function', or 'balance'"
    )]
    pub query: String,

    /// Stellar address to filter transactions by
    ///
    /// Required when using --query address or --query contract
    /// For 'address': Searches for transactions where the address appears as:
    ///   - Transaction source account
    ///   - Operation source account
    ///   - Payment destination
    ///   - Asset issuer
    ///   - And other address-related fields
    /// For 'contract': Searches for transactions that invoke the specified contract
    #[arg(
        short,
        long,
        value_name = "ADDRESS",
        help = "Stellar address or contract address to search for"
    )]
    pub address: Option<String>,

    /// Function name to filter transactions by
    ///
    /// Required when using --query function
    /// Searches for transactions that call the specified contract function name
    #[arg(
        short = 'n',
        long,
        value_name = "NAME",
        help = "Function name to search for (required with --query function)"
    )]
    pub name: Option<String>,

    /// Token contract address or shortcut
    ///
    /// Required when using --query balance
    /// Can be a full contract address (C...) or a shortcut: xlm, usdc, kale (case-insensitive)
    #[arg(
        short = 't',
        long,
        value_name = "TOKEN",
        help = "Token contract address or shortcut (xlm, usdc, kale)"
    )]
    pub token: Option<String>,
}

impl Args {
    /// Validate arguments based on query type
    pub fn validate(&self) -> anyhow::Result<()> {
        match self.query.as_str() {
            "address" | "contract" => {
                if self.address.is_none() {
                    anyhow::bail!("--address is required when using --query {}", self.query);
                }
                if self.ledger.is_none() {
                    anyhow::bail!("--ledger is required when using --query {}", self.query);
                }
            }
            "function" => {
                if self.name.is_none() {
                    anyhow::bail!("--name is required when using --query function");
                }
                if self.ledger.is_none() {
                    anyhow::bail!("--ledger is required when using --query function");
                }
            }
            "balance" => {
                if self.address.is_none() {
                    anyhow::bail!("--address is required when using --query balance");
                }
                if self.token.is_none() {
                    anyhow::bail!("--token is required when using --query balance");
                }
                // balance doesn't require ledger
            }
            "all" | "transactions" => {
                if self.ledger.is_none() {
                    anyhow::bail!("--ledger is required when using --query {}", self.query);
                }
            }
            _ => {
                anyhow::bail!(
                    "Unsupported query type: {}. Use 'all', 'transactions', 'address', 'contract', 'function', or 'balance'",
                    self.query
                );
            }
        }
        Ok(())
    }
}
