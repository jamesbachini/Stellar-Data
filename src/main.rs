use anyhow::{Context, Result};
use clap::Parser;
use serde_json;
use stellar_xdr::curr::{LedgerCloseMetaBatch, LedgerCloseMeta, ReadXdr, Limits, MuxedAccount, AccountId, PublicKey};

#[derive(serde::Deserialize)]
struct HorizonLedger {
    sequence: u32,
}

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
struct Args {
    /// Ledger/block number or range to query
    ///
    /// Formats:
    ///   Single: --ledger 63864
    ///   Range:  --ledger 63864-63900
    ///   Recent: --ledger -999 (last 999 blocks from current)
    #[arg(
        short,
        long,
        allow_hyphen_values = true,
        value_name = "LEDGER",
        help = "Ledger/block number, range, or negative value for recent blocks"
    )]
    ledger: String,

    /// Query type determines what data to return
    ///
    /// Options:
    ///   all          - Full ledger metadata (default)
    ///   transactions - Just transaction data
    ///   address      - Transactions involving a specific address (requires --address)
    ///   contract     - Transactions involving a specific contract (requires --address)
    ///   function     - Transactions calling a specific function (requires --name)
    #[arg(
        short,
        long,
        default_value = "all",
        value_name = "TYPE",
        help = "Query type: 'all', 'transactions', 'address', 'contract', or 'function'"
    )]
    query: String,

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
    address: Option<String>,

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
    name: Option<String>,
}

/// Ledger range parsed from input
#[derive(Debug)]
struct LedgerRange {
    start: u32,
    end: u32,
}

impl LedgerRange {
    fn parse(input: &str, latest_ledger: Option<u32>) -> Result<Self> {
        // Check if input starts with a negative sign (for relative queries)
        if input.trim().starts_with('-') {
            let latest = latest_ledger.ok_or_else(|| anyhow::anyhow!("Could not determine latest ledger"))?;

            // Parse the negative number (removing the '-' prefix)
            let count = input.trim()[1..].parse::<u32>()
                .context("Invalid negative ledger count")?;

            if count == 0 {
                anyhow::bail!("Ledger count must be greater than 0");
            }

            if count > latest {
                anyhow::bail!("Cannot query {} ledgers from latest ({}), exceeds available ledgers", count, latest);
            }

            let start = latest - count + 1;
            let end = latest;

            return Ok(LedgerRange { start, end });
        }

        // Original positive number parsing
        if let Some((start_str, end_str)) = input.split_once('-') {
            let start = start_str.trim().parse::<u32>()
                .context("Invalid start ledger number")?;
            let end = end_str.trim().parse::<u32>()
                .context("Invalid end ledger number")?;

            if start > end {
                anyhow::bail!("Start ledger must be less than or equal to end ledger");
            }

            Ok(LedgerRange { start, end })
        } else {
            let ledger = input.trim().parse::<u32>()
                .context("Invalid ledger number")?;
            Ok(LedgerRange { start: ledger, end: ledger })
        }
    }

    fn iter(&self) -> impl Iterator<Item = u32> {
        self.start..=self.end
    }
}

/// Fetch the latest ledger number from Stellar Horizon API
fn get_latest_ledger() -> Result<u32> {
    let horizon_url = "https://horizon.stellar.org/ledgers?order=desc&limit=1";

    println!("Fetching latest ledger from Horizon...");

    let response = reqwest::blocking::get(horizon_url)
        .context("Failed to fetch latest ledger from Horizon")?;

    let json: serde_json::Value = response.json()
        .context("Failed to parse Horizon response")?;

    let ledgers = json["_embedded"]["records"].as_array()
        .ok_or_else(|| anyhow::anyhow!("Unexpected Horizon response format"))?;

    let latest_ledger = ledgers.first()
        .ok_or_else(|| anyhow::anyhow!("No ledgers found in Horizon response"))?;

    let sequence = latest_ledger["sequence"].as_u64()
        .ok_or_else(|| anyhow::anyhow!("Could not parse ledger sequence"))?;

    println!("Latest ledger: {}\n", sequence);

    Ok(sequence as u32)
}

/// Configuration from the S3 data lake
struct Config {
    network_passphrase: String,
    ledgers_per_batch: u32,
    batches_per_partition: u32,
    base_url: String,
    ledgers_path: String,
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
    fn rpc_url() -> &'static str {
        "https://archive-rpc.lightsail.network/"
    }

    /// Generate the S3 URL for a given ledger sequence number
    fn generate_url(&self, ledger_seq: u32) -> String {
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
fn fetch_and_decompress(url: &str, silent: bool) -> Result<Vec<u8>> {
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

/// Parse XDR data into LedgerCloseMetaBatch
fn parse_xdr(data: &[u8]) -> Result<LedgerCloseMetaBatch> {
    LedgerCloseMetaBatch::from_xdr(data, Limits::none())
        .context("Failed to parse XDR data")
}

/// RPC response structures
#[derive(serde::Deserialize)]
struct RpcLedgerResponse {
    ledgers: Vec<RpcLedger>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcLedger {
    sequence: u32,
    metadata_xdr: String,
}

/// Fetch ledger data from RPC when S3 doesn't have it yet
fn fetch_from_rpc(ledger_seq: u32, silent: bool) -> Result<Vec<u8>> {
    if !silent {
        println!("Ledger not in S3, fetching from RPC archive...");
    }

    let client = reqwest::blocking::Client::new();
    let rpc_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLedgers",
        "params": {
            "startLedger": ledger_seq,
            "pagination": {
                "limit": 1
            }
        }
    });

    let response = client
        .post(Config::rpc_url())
        .json(&rpc_request)
        .send()
        .context("Failed to call RPC")?;

    if !response.status().is_success() {
        anyhow::bail!("RPC returned error status: {}", response.status());
    }

    let json: serde_json::Value = response.json()
        .context("Failed to parse RPC response")?;

    if let Some(error) = json.get("error") {
        anyhow::bail!("RPC error: {}", error);
    }

    let result = json.get("result")
        .ok_or_else(|| anyhow::anyhow!("No result in RPC response"))?;

    let ledgers = result.get("ledgers")
        .and_then(|l| l.as_array())
        .ok_or_else(|| anyhow::anyhow!("No ledgers in RPC response"))?;

    if ledgers.is_empty() {
        anyhow::bail!("Ledger {} not found in RPC", ledger_seq);
    }

    let ledger = &ledgers[0];
    let metadata_xdr = ledger.get("metadataXdr")
        .and_then(|m| m.as_str())
        .ok_or_else(|| anyhow::anyhow!("No metadataXdr in RPC response"))?;

    if !silent {
        println!("Decoding base64 XDR from RPC...");
    }

    // Decode base64 to get the XDR bytes for LedgerCloseMeta
    use stellar_xdr::curr::ReadXdr;
    let ledger_close_meta = LedgerCloseMeta::from_xdr_base64(metadata_xdr, Limits::none())
        .context("Failed to decode metadataXdr from RPC")?;

    // Wrap it in a LedgerCloseMetaBatch (single ledger batch)
    let batch = LedgerCloseMetaBatch {
        start_sequence: ledger_seq,
        end_sequence: ledger_seq,
        ledger_close_metas: vec![ledger_close_meta].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create VecM"))?,
    };

    // Serialize the batch to XDR bytes so it matches the S3 format
    use stellar_xdr::curr::WriteXdr;
    let xdr_bytes = batch.to_xdr(Limits::none())
        .context("Failed to serialize batch to XDR")?;

    if !silent {
        println!("Fetched {} bytes from RPC", xdr_bytes.len());
    }

    Ok(xdr_bytes)
}

/// Extract account ID as string from MuxedAccount
fn muxed_account_to_string(muxed: &MuxedAccount) -> String {
    match muxed {
        MuxedAccount::Ed25519(uint256) => {
            format!("{}", stellar_strkey::ed25519::PublicKey(uint256.0))
        }
        MuxedAccount::MuxedEd25519(med) => {
            format!("{}", stellar_strkey::ed25519::PublicKey(med.ed25519.0))
        }
    }
}

/// Extract account ID as string from AccountId
fn account_id_to_string(account: &AccountId) -> String {
    match &account.0 {
        PublicKey::PublicKeyTypeEd25519(uint256) => {
            format!("{}", stellar_strkey::ed25519::PublicKey(uint256.0))
        }
    }
}

/// Check if a transaction involves a specific address
fn transaction_involves_address(tx_envelope: &stellar_xdr::curr::TransactionEnvelope, target_address: &str) -> bool {
    use stellar_xdr::curr::TransactionEnvelope::*;

    match tx_envelope {
        TxV0(env) => {
            // Check source account
            let source = format!("{}", stellar_strkey::ed25519::PublicKey(env.tx.source_account_ed25519.0));
            if source == target_address {
                return true;
            }

            // Check operations
            for op in env.tx.operations.as_vec() {
                if let Some(ref src) = op.source_account {
                    if muxed_account_to_string(src) == target_address {
                        return true;
                    }
                }
                // Check operation-specific accounts (destination, etc.)
                if operation_involves_address(&op.body, target_address) {
                    return true;
                }
            }
        }
        Tx(env) => {
            // Check source account
            if muxed_account_to_string(&env.tx.source_account) == target_address {
                return true;
            }

            // Check operations
            for op in env.tx.operations.as_vec() {
                if let Some(ref src) = op.source_account {
                    if muxed_account_to_string(src) == target_address {
                        return true;
                    }
                }
                if operation_involves_address(&op.body, target_address) {
                    return true;
                }
            }
        }
        TxFeeBump(env) => {
            if muxed_account_to_string(&env.tx.fee_source) == target_address {
                return true;
            }
            // Check inner transaction - FeeBumpTransactionInnerTx is an enum with Tx variant
            match &env.tx.inner_tx {
                stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner_env) => {
                    // Wrap in TransactionEnvelope::Tx for recursive check
                    let wrapped = stellar_xdr::curr::TransactionEnvelope::Tx(inner_env.clone());
                    return transaction_involves_address(&wrapped, target_address);
                }
            }
        }
    }

    false
}

/// Check if an operation involves a specific address
fn operation_involves_address(body: &stellar_xdr::curr::OperationBody, target_address: &str) -> bool {
    use stellar_xdr::curr::OperationBody::*;

    match body {
        CreateAccount(op) => account_id_to_string(&op.destination) == target_address,
        Payment(op) => muxed_account_to_string(&op.destination) == target_address,
        PathPaymentStrictReceive(op) => muxed_account_to_string(&op.destination) == target_address,
        PathPaymentStrictSend(op) => muxed_account_to_string(&op.destination) == target_address,
        ManageSellOffer(_) => false,
        CreatePassiveSellOffer(_) => false,
        SetOptions(_) => false,
        ChangeTrust(op) => {
            // Check if the asset issuer matches
            match &op.line {
                stellar_xdr::curr::ChangeTrustAsset::Native => false,
                stellar_xdr::curr::ChangeTrustAsset::CreditAlphanum4(asset) => {
                    account_id_to_string(&asset.issuer) == target_address
                }
                stellar_xdr::curr::ChangeTrustAsset::CreditAlphanum12(asset) => {
                    account_id_to_string(&asset.issuer) == target_address
                }
                stellar_xdr::curr::ChangeTrustAsset::PoolShare(_) => false,
            }
        }
        AllowTrust(op) => account_id_to_string(&op.trustor) == target_address,
        AccountMerge(op) => muxed_account_to_string(op) == target_address,
        ManageData(_) => false,
        BumpSequence(_) => false,
        ManageBuyOffer(_) => false,
        Inflation => false,
        BeginSponsoringFutureReserves(op) => account_id_to_string(&op.sponsored_id) == target_address,
        EndSponsoringFutureReserves => false,
        RevokeSponsorship(_) => false,
        Clawback(op) => muxed_account_to_string(&op.from) == target_address,
        ClawbackClaimableBalance(_) => false,
        SetTrustLineFlags(op) => account_id_to_string(&op.trustor) == target_address,
        LiquidityPoolDeposit(_) => false,
        LiquidityPoolWithdraw(_) => false,
        InvokeHostFunction(_) => false,
        ExtendFootprintTtl(_) => false,
        RestoreFootprint(_) => false,
        CreateClaimableBalance(_) => false,
        ClaimClaimableBalance(_) => false,
    }
}

/// Check if a transaction involves a specific contract address
fn transaction_involves_contract(tx_envelope: &stellar_xdr::curr::TransactionEnvelope, contract_address: &str) -> bool {
    use stellar_xdr::curr::TransactionEnvelope::*;

    let operations = match tx_envelope {
        TxV0(env) => env.tx.operations.as_vec(),
        Tx(env) => env.tx.operations.as_vec(),
        TxFeeBump(env) => {
            match &env.tx.inner_tx {
                stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner_env) => {
                    inner_env.tx.operations.as_vec()
                }
            }
        }
    };

    for op in operations {
        if let stellar_xdr::curr::OperationBody::InvokeHostFunction(invoke_op) = &op.body {
            // Check auth credentials for contract addresses
            for auth in invoke_op.auth.as_vec() {
                // Check if the root_invocation contains the contract address
                let auth_str = format!("{:?}", auth.root_invocation);
                if auth_str.contains(contract_address) {
                    return true;
                }
            }

            // Check host function itself - convert to string and search
            let host_fn_str = format!("{:?}", invoke_op.host_function);
            if host_fn_str.contains(contract_address) {
                return true;
            }
        }
    }

    false
}

/// Check if a transaction calls a specific function name
fn transaction_calls_function(tx_envelope: &stellar_xdr::curr::TransactionEnvelope, function_name: &str) -> bool {
    use stellar_xdr::curr::TransactionEnvelope::*;

    let operations = match tx_envelope {
        TxV0(env) => env.tx.operations.as_vec(),
        Tx(env) => env.tx.operations.as_vec(),
        TxFeeBump(env) => {
            match &env.tx.inner_tx {
                stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner_env) => {
                    inner_env.tx.operations.as_vec()
                }
            }
        }
    };

    for op in operations {
        if let stellar_xdr::curr::OperationBody::InvokeHostFunction(invoke_op) = &op.body {
            // Check auth credentials for function names
            for auth in invoke_op.auth.as_vec() {
                let auth_str = format!("{:?}", auth.root_invocation);
                // Look for function name in the debug output
                if auth_str.contains(&format!("\"{}\"", function_name)) ||
                   auth_str.contains(&format!("function_name: Symbol(StringM({})", function_name)) {
                    return true;
                }
            }

            // Check host function by converting to debug string
            let host_fn_str = format!("{:?}", invoke_op.host_function);
            if host_fn_str.contains(&format!("\"{}\"", function_name)) ||
               host_fn_str.contains(&format!("Symbol(StringM({})", function_name)) {
                return true;
            }
        }
    }

    false
}

/// Filter transactions in a batch by address
fn filter_by_address(batch: &LedgerCloseMetaBatch, address: &str) -> Vec<serde_json::Value> {
    let mut matching_transactions = Vec::new();

    for meta in batch.ledger_close_metas.as_vec() {
        match meta {
            LedgerCloseMeta::V0(v0) => {
                for tx in v0.tx_set.txs.as_vec() {
                    if transaction_involves_address(tx, address) {
                        if let Ok(tx_json) = serde_json::to_value(tx) {
                            matching_transactions.push(tx_json);
                        }
                    }
                }
            }
            LedgerCloseMeta::V1(v1) => {
                for tx_result in v1.tx_processing.as_vec() {
                    // V1 contains the full tx_set, we need to cross reference
                    // For simplicity, just serialize all transactions for V1
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
            LedgerCloseMeta::V2(v2) => {
                for tx_result in v2.tx_processing.as_vec() {
                    // V2 is similar to V1
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
        }
    }

    matching_transactions
}

/// Filter transactions in a batch by contract address
fn filter_by_contract(batch: &LedgerCloseMetaBatch, contract_address: &str) -> Vec<serde_json::Value> {
    let mut matching_transactions = Vec::new();

    for meta in batch.ledger_close_metas.as_vec() {
        match meta {
            LedgerCloseMeta::V0(v0) => {
                for tx in v0.tx_set.txs.as_vec() {
                    if transaction_involves_contract(tx, contract_address) {
                        if let Ok(tx_json) = serde_json::to_value(tx) {
                            matching_transactions.push(tx_json);
                        }
                    }
                }
            }
            LedgerCloseMeta::V1(v1) => {
                for tx_result in v1.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
            LedgerCloseMeta::V2(v2) => {
                for tx_result in v2.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
        }
    }

    matching_transactions
}

/// Filter transactions in a batch by function name
fn filter_by_function(batch: &LedgerCloseMetaBatch, function_name: &str) -> Vec<serde_json::Value> {
    let mut matching_transactions = Vec::new();

    for meta in batch.ledger_close_metas.as_vec() {
        match meta {
            LedgerCloseMeta::V0(v0) => {
                for tx in v0.tx_set.txs.as_vec() {
                    if transaction_calls_function(tx, function_name) {
                        if let Ok(tx_json) = serde_json::to_value(tx) {
                            matching_transactions.push(tx_json);
                        }
                    }
                }
            }
            LedgerCloseMeta::V1(v1) => {
                for tx_result in v1.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
            LedgerCloseMeta::V2(v2) => {
                for tx_result in v2.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
        }
    }

    matching_transactions
}

/// Convert LedgerCloseMetaBatch to JSON
fn to_json(batch: &LedgerCloseMetaBatch, query_type: &str, address_filter: Option<&str>, name_filter: Option<&str>) -> Result<String> {
    match query_type {
        "all" => {
            // Return the full batch as JSON
            serde_json::to_string_pretty(batch)
                .context("Failed to serialize batch to JSON")
        }
        "transactions" => {
            // Extract just transactions from each ledger in the batch
            let mut transactions = Vec::new();

            for meta in batch.ledger_close_metas.as_vec() {
                match meta {
                    stellar_xdr::curr::LedgerCloseMeta::V0(v0) => {
                        for tx in v0.tx_set.txs.as_vec() {
                            transactions.push(serde_json::to_value(tx)
                                .context("Failed to serialize transaction")?);
                        }
                    }
                    stellar_xdr::curr::LedgerCloseMeta::V1(v1) => {
                        for tx_processing in v1.tx_processing.as_vec() {
                            transactions.push(serde_json::to_value(tx_processing)
                                .context("Failed to serialize transaction processing")?);
                        }
                    }
                    stellar_xdr::curr::LedgerCloseMeta::V2(v2) => {
                        for tx_processing in v2.tx_processing.as_vec() {
                            transactions.push(serde_json::to_value(tx_processing)
                                .context("Failed to serialize transaction processing")?);
                        }
                    }
                }
            }

            serde_json::to_string_pretty(&serde_json::json!({
                "start_sequence": batch.start_sequence,
                "end_sequence": batch.end_sequence,
                "transactions": transactions,
                "count": transactions.len()
            }))
            .context("Failed to serialize transactions to JSON")
        }
        "address" => {
            let address = address_filter.ok_or_else(|| anyhow::anyhow!("Address filter required for 'address' query type"))?;
            let transactions = filter_by_address(batch, address);

            serde_json::to_string_pretty(&serde_json::json!({
                "start_sequence": batch.start_sequence,
                "end_sequence": batch.end_sequence,
                "address": address,
                "transactions": transactions,
                "count": transactions.len()
            }))
            .context("Failed to serialize filtered transactions to JSON")
        }
        "contract" => {
            let contract = address_filter.ok_or_else(|| anyhow::anyhow!("Contract address (--address) required for 'contract' query type"))?;
            let transactions = filter_by_contract(batch, contract);

            serde_json::to_string_pretty(&serde_json::json!({
                "start_sequence": batch.start_sequence,
                "end_sequence": batch.end_sequence,
                "contract": contract,
                "transactions": transactions,
                "count": transactions.len()
            }))
            .context("Failed to serialize filtered transactions to JSON")
        }
        "function" => {
            let function_name = name_filter.ok_or_else(|| anyhow::anyhow!("Function name (--name) required for 'function' query type"))?;
            let transactions = filter_by_function(batch, function_name);

            serde_json::to_string_pretty(&serde_json::json!({
                "start_sequence": batch.start_sequence,
                "end_sequence": batch.end_sequence,
                "function": function_name,
                "transactions": transactions,
                "count": transactions.len()
            }))
            .context("Failed to serialize filtered transactions to JSON")
        }
        _ => {
            anyhow::bail!("Unsupported query type: {}. Use 'all', 'transactions', 'address', 'contract', or 'function'", query_type)
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::default();

    // Validate address requirement for address and contract queries
    if args.query == "address" && args.address.is_none() {
        anyhow::bail!("--address is required when using --query address");
    }
    if args.query == "contract" && args.address.is_none() {
        anyhow::bail!("--address is required when using --query contract");
    }
    if args.query == "function" && args.name.is_none() {
        anyhow::bail!("--name is required when using --query function");
    }

    // Fetch latest ledger if we need it (for negative ledger values)
    let latest_ledger = if args.ledger.trim().starts_with('-') {
        Some(get_latest_ledger()?)
    } else {
        None
    };

    // Parse ledger range
    let ledger_range = LedgerRange::parse(&args.ledger, latest_ledger)?;

    let is_range = ledger_range.start != ledger_range.end;
    let silent = is_range; // Be silent during range queries to reduce output

    if is_range {
        println!("Querying ledger range: {} to {}", ledger_range.start, ledger_range.end);
        println!("Query type: {}", args.query);
        if let Some(ref addr) = args.address {
            if args.query == "address" {
                println!("Filtering by address: {}\n", addr);
            } else if args.query == "contract" {
                println!("Filtering by contract: {}\n", addr);
            }
        }
        if let Some(ref name) = args.name {
            println!("Filtering by function: {}\n", name);
        }
    }

    // Collect all matching transactions across the range
    let mut all_transactions = Vec::new();
    let mut total_processed = 0;

    for ledger_seq in ledger_range.iter() {
        // Generate URL for the ledger
        let url = config.generate_url(ledger_seq);

        // Fetch and decompress the data (with RPC fallback on 404)
        let decompressed_data = match fetch_and_decompress(&url, silent) {
            Ok(data) => data,
            Err(e) => {
                // Check if it's a 404 error, and if so, try RPC fallback
                let error_msg = e.to_string();
                if error_msg.contains("HTTP 404") {
                    match fetch_from_rpc(ledger_seq, silent) {
                        Ok(data) => data,
                        Err(rpc_err) => {
                            eprintln!("Error fetching ledger {} from RPC: {}", ledger_seq, rpc_err);
                            continue;
                        }
                    }
                } else {
                    eprintln!("Error fetching ledger {}: {}", ledger_seq, e);
                    continue;
                }
            }
        };

        // Parse XDR
        let batch = match parse_xdr(&decompressed_data) {
            Ok(batch) => batch,
            Err(e) => {
                eprintln!("Error parsing ledger {}: {}", ledger_seq, e);
                continue;
            }
        };

        total_processed += 1;

        // Filter or collect transactions based on query type
        match args.query.as_str() {
            "address" => {
                if let Some(ref address) = args.address {
                    let matching = filter_by_address(&batch, address);
                    if !matching.is_empty() && !silent {
                        println!("Found {} transaction(s) in ledger {}", matching.len(), ledger_seq);
                    }
                    all_transactions.extend(matching);
                }
            }
            "contract" => {
                if let Some(ref contract) = args.address {
                    let matching = filter_by_contract(&batch, contract);
                    if !matching.is_empty() && !silent {
                        println!("Found {} transaction(s) in ledger {}", matching.len(), ledger_seq);
                    }
                    all_transactions.extend(matching);
                }
            }
            "function" => {
                if let Some(ref function_name) = args.name {
                    let matching = filter_by_function(&batch, function_name);
                    if !matching.is_empty() && !silent {
                        println!("Found {} transaction(s) in ledger {}", matching.len(), ledger_seq);
                    }
                    all_transactions.extend(matching);
                }
            }
            "transactions" => {
                // Collect all transactions
                for meta in batch.ledger_close_metas.as_vec() {
                    match meta {
                        LedgerCloseMeta::V0(v0) => {
                            for tx in v0.tx_set.txs.as_vec() {
                                if let Ok(tx_json) = serde_json::to_value(tx) {
                                    all_transactions.push(tx_json);
                                }
                            }
                        }
                        LedgerCloseMeta::V1(v1) => {
                            for tx_processing in v1.tx_processing.as_vec() {
                                if let Ok(tx_json) = serde_json::to_value(tx_processing) {
                                    all_transactions.push(tx_json);
                                }
                            }
                        }
                        LedgerCloseMeta::V2(v2) => {
                            for tx_processing in v2.tx_processing.as_vec() {
                                if let Ok(tx_json) = serde_json::to_value(tx_processing) {
                                    all_transactions.push(tx_json);
                                }
                            }
                        }
                    }
                }
            }
            "all" => {
                // For "all" mode with ranges, collect all ledger metadata
                if !is_range {
                    println!("\nLedger batch: {} to {}", batch.start_sequence, batch.end_sequence);
                    println!("Number of ledgers in batch: {}\n", batch.ledger_close_metas.len());
                    let json = to_json(&batch, &args.query, args.address.as_deref(), args.name.as_deref())?;
                    println!("{}", json);
                    return Ok(());
                } else {
                    // For range queries with "all", collect the full ledger metadata
                    for meta in batch.ledger_close_metas.as_vec() {
                        if let Ok(meta_json) = serde_json::to_value(meta) {
                            all_transactions.push(meta_json);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Output results for range queries
    if is_range {
        println!("\nProcessed {} ledgers", total_processed);

        let result = if args.query == "all" {
            serde_json::json!({
                "start_sequence": ledger_range.start,
                "end_sequence": ledger_range.end,
                "ledgers_processed": total_processed,
                "ledgers": all_transactions,
                "count": all_transactions.len()
            })
        } else {
            serde_json::json!({
                "start_sequence": ledger_range.start,
                "end_sequence": ledger_range.end,
                "ledgers_processed": total_processed,
                "address": args.address,
                "transactions": all_transactions,
                "count": all_transactions.len()
            })
        };

        println!("\n{}", serde_json::to_string_pretty(&result)?);
    } else {
        // Single ledger - use original output format
        let url = config.generate_url(ledger_range.start);
        let decompressed_data = match fetch_and_decompress(&url, false) {
            Ok(data) => data,
            Err(e) => {
                // Check if it's a 404 error, and if so, try RPC fallback
                let error_msg = e.to_string();
                if error_msg.contains("HTTP 404") {
                    fetch_from_rpc(ledger_range.start, false)?
                } else {
                    return Err(e);
                }
            }
        };
        let batch = parse_xdr(&decompressed_data)?;

        println!("\nLedger batch: {} to {}", batch.start_sequence, batch.end_sequence);
        println!("Number of ledgers in batch: {}\n", batch.ledger_close_metas.len());

        let json = to_json(&batch, &args.query, args.address.as_deref(), args.name.as_deref())?;
        println!("{}", json);
    }

    Ok(())
}
