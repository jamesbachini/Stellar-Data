use anyhow::{Context, Result};
use stellar_xdr::curr::{LedgerCloseMeta, LedgerCloseMetaBatch, Limits, ReadXdr, WriteXdr};
use stellar_strkey::Strkey;
use crate::config::Config;

/// Reflector oracle contract addresses
const REFLECTOR_STELLAR_CONTRACT: &str = "CALI2BYU2JE6WVRUFYTS6MSBNEHGJ35P4AVCZYF3B6QOE3QKOB2PLE6M";
const REFLECTOR_CRYPTO_CONTRACT: &str = "CAFJZQWSED6YAWZU3GWRTOCNPPCGBN32L7QV43XX5LZLFTK6JLN34DLN";
const REFLECTOR_FIAT_CONTRACT: &str = "CBKGPWGKSKZF52CFHMTRR23TBWTPMRDIYZ4O2P5VS65BMHYH4DXMCJZC";
const REFLECTOR_DECIMALS: u32 = 14;

/// Crypto assets that use the crypto oracle
const CRYPTO_ASSETS: &[&str] = &[
    "BTC", "ETH", "USDT", "XRP", "SOL", "USDC", "ADA", "AVAX", "DOT",
    "MATIC", "LINK", "DAI", "ATOM", "XLM", "UNI", "EURC"
];

/// Fiat assets that use the fiat oracle
const FIAT_ASSETS: &[&str] = &[
    "EUR", "GBP", "CAD", "BRL", "JPY", "CNY", "MXN", "KRW", "TRY", "ARS",
    "PEN", "VES", "CLP", "CRC", "CDF", "COP", "HKD", "INR", "NGN", "PHP",
    "RUB", "ZAR", "XAU"
];

/// Determine which Reflector oracle to use based on the asset
fn get_oracle_for_asset(asset: &str) -> &'static str {
    let asset_upper = asset.to_uppercase();

    if CRYPTO_ASSETS.contains(&asset_upper.as_str()) {
        REFLECTOR_CRYPTO_CONTRACT
    } else if FIAT_ASSETS.contains(&asset_upper.as_str()) {
        REFLECTOR_FIAT_CONTRACT
    } else {
        // Default to Stellar contract for contract addresses and other assets
        REFLECTOR_STELLAR_CONTRACT
    }
}

/// RPC response structures (currently unused, kept for reference)
#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct RpcLedgerResponse {
    ledgers: Vec<RpcLedger>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcLedger {
    sequence: u32,
    metadata_xdr: String,
}

/// Fetch ledger data from RPC when S3 doesn't have it yet
pub fn fetch_from_rpc(ledger_seq: u32, silent: bool) -> Result<Vec<u8>> {
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
    let xdr_bytes = batch.to_xdr(Limits::none())
        .context("Failed to serialize batch to XDR")?;

    if !silent {
        println!("Fetched {} bytes from RPC", xdr_bytes.len());
    }

    Ok(xdr_bytes)
}

/// Query token balance for an address using Soroban RPC
///
/// This uses the simulateTransaction RPC method with a minimal transaction envelope
/// that calls the "balance" function on the token contract.
pub fn query_balance(address: &str, token_contract: &str) -> Result<serde_json::Value> {
    use stellar_xdr::curr::*;

    let client = reqwest::blocking::Client::new();

    // Decode the Stellar address to get the account ID bytes
    let address_bytes = match Strkey::from_string(address) {
        Ok(Strkey::PublicKeyEd25519(pk)) => pk.0,
        _ => anyhow::bail!("Invalid Stellar address format"),
    };

    // Decode the contract address
    let contract_bytes = match Strkey::from_string(token_contract) {
        Ok(Strkey::Contract(contract)) => contract.0,
        _ => anyhow::bail!("Invalid contract address format"),
    };

    // Create the ScVal for the address parameter (Address type with Account variant)
    let address_scval = ScVal::Address(ScAddress::Account(
        AccountId(PublicKey::PublicKeyTypeEd25519(
            Uint256(address_bytes),
        )),
    ));

    // Create function name as ScSymbol
    let function_name = ScSymbol("balance".as_bytes().to_vec().try_into()
        .map_err(|_| anyhow::anyhow!("Function name too long"))?);

    // Create contract address
    use stellar_xdr::curr::ContractId;
    let contract_address = ScAddress::Contract(
        ContractId(Hash(contract_bytes)),
    );

    // Build InvokeContractArgs
    let invoke_args = InvokeContractArgs {
        contract_address,
        function_name,
        args: vec![address_scval].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create args vec"))?,
    };

    // Build HostFunction::InvokeContract
    let host_function = HostFunction::InvokeContract(invoke_args);

    // Build InvokeHostFunctionOp
    let invoke_op = InvokeHostFunctionOp {
        host_function,
        auth: vec![].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create auth vec"))?,
    };

    // Build Operation
    let operation = Operation {
        source_account: None,
        body: OperationBody::InvokeHostFunction(invoke_op),
    };

    // Build a minimal transaction envelope
    // Use a dummy source account
    let source_account = MuxedAccount::Ed25519(Uint256([0u8; 32]));

    let tx = Transaction {
        source_account,
        fee: 100,
        seq_num: SequenceNumber(0),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: vec![operation].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create operations vec"))?,
        ext: TransactionExt::V0,
    };

    let tx_envelope = TransactionEnvelope::Tx(TransactionV1Envelope {
        tx,
        signatures: vec![].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create signatures vec"))?,
    });

    // Encode transaction to base64 XDR
    let tx_xdr = tx_envelope.to_xdr_base64(Limits::none())
        .context("Failed to encode transaction to XDR")?;

    // Make the simulateTransaction RPC call
    let rpc_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateTransaction",
        "params": {
            "transaction": tx_xdr
        }
    });

    let response = client
        .post(Config::soroban_rpc_url())
        .json(&rpc_request)
        .send()
        .context("Failed to call RPC for balance query")?;

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

    // Try to extract and decode the balance from the result XDR
    // The result is in result.results[0].xdr
    if let Some(result_xdr) = result.get("results")
        .and_then(|r| r.get(0))
        .and_then(|r| r.get("xdr"))
        .and_then(|x| x.as_str())
    {
        match ScVal::from_xdr_base64(result_xdr, Limits::none()) {
            Ok(ScVal::I128(parts)) => {
                // Convert i128 parts to get raw balance in stroops
                let raw_balance = i128::from(parts.hi) << 64 | i128::from(parts.lo as u64);
                // Convert to human-readable format (7 decimals for Stellar tokens)
                let balance = raw_balance as f64 / 10_f64.powi(7);
                return Ok(serde_json::json!({
                    "address": address,
                    "token": token_contract,
                    "balance": balance,
                    "raw_balance": raw_balance.to_string()
                }));
            }
            Ok(ScVal::U128(parts)) => {
                // Convert u128 parts to get raw balance in stroops
                let raw_balance = u128::from(parts.hi) << 64 | u128::from(parts.lo);
                // Convert to human-readable format (7 decimals for Stellar tokens)
                let balance = raw_balance as f64 / 10_f64.powi(7);
                return Ok(serde_json::json!({
                    "address": address,
                    "token": token_contract,
                    "balance": balance,
                    "raw_balance": raw_balance.to_string()
                }));
            }
            Ok(val) => {
                // Return the ScVal as-is if it's not a number
                return Ok(serde_json::json!({
                    "address": address,
                    "token": token_contract,
                    "result": format!("{:?}", val)
                }));
            }
            Err(e) => {
                return Ok(serde_json::json!({
                    "address": address,
                    "token": token_contract,
                    "error": format!("Failed to decode result: {}", e),
                    "raw_xdr": result_xdr
                }));
            }
        }
    }

    // If we can't extract the XDR, return the full result
    Ok(serde_json::json!({
        "address": address,
        "token": token_contract,
        "result": result
    }))
}

/// Query price data from Reflector oracle
///
/// This uses the simulateTransaction RPC method to call the "lastprice" function
/// on the Reflector oracle contract. The asset can be:
/// - External asset symbol (e.g., "btc", "eth", "xlm") -> uses ReflectorAsset::Other variant with appropriate oracle
/// - Stellar contract address (starts with 'C') -> uses ReflectorAsset::Stellar variant with Stellar DEX oracle
pub fn query_price(asset_input: &str) -> Result<serde_json::Value> {
    use stellar_xdr::curr::*;

    let client = reqwest::blocking::Client::new();

    // Determine if this is a Stellar asset (contract address) or external asset (symbol)
    // And which oracle to use
    let (asset_type, asset_value, reflector_contract) = if asset_input.starts_with('C') && asset_input.len() == 56 {
        // It's a Stellar contract address - use Stellar DEX oracle
        ("Stellar", asset_input, REFLECTOR_STELLAR_CONTRACT)
    } else {
        // It's an asset symbol (including token shortcuts like xlm, usdc, btc, eth, etc.)
        // Treat all symbols as external assets and determine oracle based on asset type
        let oracle = get_oracle_for_asset(asset_input);
        ("Other", asset_input, oracle)
    };

    // Build the ReflectorAsset ScVal
    // ReflectorAsset is an enum with two variants:
    // - Stellar(Address): for Stellar tokens
    // - Other(Symbol): for external assets
    //
    // In Soroban XDR, enums with data are encoded as Vec where:
    // - First element: variant name as Symbol
    // - Remaining elements: the variant's associated data
    let asset_scval = if asset_type == "Stellar" {
        // Decode the contract address
        let contract_bytes = match Strkey::from_string(asset_value) {
            Ok(Strkey::Contract(contract)) => contract.0,
            _ => anyhow::bail!("Invalid contract address format: {}", asset_value),
        };

        // Create Stellar variant: Vec[Symbol("Stellar"), Address(Contract)]
        ScVal::Vec(Some(
            vec![
                ScVal::Symbol(ScSymbol("Stellar".as_bytes().to_vec().try_into()
                    .map_err(|_| anyhow::anyhow!("Variant name too long"))?)),
                ScVal::Address(ScAddress::Contract(
                    ContractId(Hash(contract_bytes)),
                )),
            ]
            .try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create vec"))?,
        ))
    } else {
        // Create Other variant: Vec[Symbol("Other"), Symbol(asset)]
        ScVal::Vec(Some(
            vec![
                ScVal::Symbol(ScSymbol("Other".as_bytes().to_vec().try_into()
                    .map_err(|_| anyhow::anyhow!("Variant name too long"))?)),
                ScVal::Symbol(ScSymbol(asset_value.to_uppercase().as_bytes().to_vec().try_into()
                    .map_err(|_| anyhow::anyhow!("Asset symbol too long"))?)),
            ]
            .try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create vec"))?,
        ))
    };

    // Decode the Reflector contract address
    let reflector_contract_bytes = match Strkey::from_string(reflector_contract) {
        Ok(Strkey::Contract(contract)) => contract.0,
        _ => anyhow::bail!("Invalid Reflector contract address"),
    };

    // Create function name as ScSymbol
    let function_name = ScSymbol("lastprice".as_bytes().to_vec().try_into()
        .map_err(|_| anyhow::anyhow!("Function name too long"))?);

    // Create contract address
    let contract_address = ScAddress::Contract(
        ContractId(Hash(reflector_contract_bytes)),
    );

    // Build InvokeContractArgs
    let invoke_args = InvokeContractArgs {
        contract_address,
        function_name,
        args: vec![asset_scval].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create args vec"))?,
    };

    // Build HostFunction::InvokeContract
    let host_function = HostFunction::InvokeContract(invoke_args);

    // Build InvokeHostFunctionOp
    let invoke_op = InvokeHostFunctionOp {
        host_function,
        auth: vec![].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create auth vec"))?,
    };

    // Build Operation
    let operation = Operation {
        source_account: None,
        body: OperationBody::InvokeHostFunction(invoke_op),
    };

    // Build a minimal transaction envelope
    // Use a dummy source account
    let source_account = MuxedAccount::Ed25519(Uint256([0u8; 32]));

    let tx = Transaction {
        source_account,
        fee: 100,
        seq_num: SequenceNumber(0),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: vec![operation].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create operations vec"))?,
        ext: TransactionExt::V0,
    };

    let tx_envelope = TransactionEnvelope::Tx(TransactionV1Envelope {
        tx,
        signatures: vec![].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to create signatures vec"))?,
    });

    // Encode transaction to base64 XDR
    let tx_xdr = tx_envelope.to_xdr_base64(Limits::none())
        .context("Failed to encode transaction to XDR")?;

    // Make the simulateTransaction RPC call
    let rpc_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "simulateTransaction",
        "params": {
            "transaction": tx_xdr
        }
    });

    let response = client
        .post(Config::soroban_rpc_url())
        .json(&rpc_request)
        .send()
        .context("Failed to call RPC for price query")?;

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

    // Check for RPC error in result
    if let Some(error) = result.get("error") {
        return Ok(serde_json::json!({
            "asset": asset_input,
            "asset_type": asset_type,
            "error": "Contract execution failed",
            "contract_error": error,
            "result": result
        }));
    }

    // Try to extract and decode the price data from the result XDR
    // The result should be in result.results[0].xdr
    // Expected structure: { price: i128, timestamp: u64 }
    if let Some(result_xdr) = result.get("results")
        .and_then(|r| r.get(0))
        .and_then(|r| r.get("xdr"))
        .and_then(|x| x.as_str())
    {
        match ScVal::from_xdr_base64(result_xdr, Limits::none()) {
            Ok(ScVal::Map(Some(map))) => {
                // Parse the map to extract price and timestamp
                let mut price_i128: Option<i128> = None;
                let mut timestamp_u64: Option<u64> = None;

                for entry in map.as_vec() {
                    if let ScVal::Symbol(ref key_sym) = entry.key {
                        let key_str = String::from_utf8_lossy(key_sym.as_vec());

                        match key_str.as_ref() {
                            "price" => {
                                if let ScVal::I128(parts) = &entry.val {
                                    price_i128 = Some(i128::from(parts.hi) << 64 | i128::from(parts.lo as u64));
                                }
                            }
                            "timestamp" => {
                                if let ScVal::U64(ts) = &entry.val {
                                    timestamp_u64 = Some(*ts);
                                }
                            }
                            _ => {}
                        }
                    }
                }

                if let (Some(price), Some(timestamp)) = (price_i128, timestamp_u64) {
                    // Format price with decimals
                    let price_float = price as f64 / 10_f64.powi(REFLECTOR_DECIMALS as i32);

                    return Ok(serde_json::json!({
                        "asset": asset_input,
                        "asset_type": asset_type,
                        "price": price_float,
                        "price_raw": price.to_string(),
                        "timestamp": timestamp,
                        "decimals": REFLECTOR_DECIMALS,
                        "source": "reflector"
                    }));
                }
            }
            Ok(val) => {
                // Return the ScVal as-is if it's not the expected structure
                return Ok(serde_json::json!({
                    "asset": asset_input,
                    "asset_type": asset_type,
                    "error": "Unexpected result format",
                    "result": format!("{:?}", val)
                }));
            }
            Err(e) => {
                return Ok(serde_json::json!({
                    "asset": asset_input,
                    "asset_type": asset_type,
                    "error": format!("Failed to decode result: {}", e),
                    "raw_xdr": result_xdr
                }));
            }
        }
    }

    // If we can't extract the XDR, return the full result
    Ok(serde_json::json!({
        "asset": asset_input,
        "asset_type": asset_type,
        "error": "No result XDR found in response",
        "result": result
    }))
}
