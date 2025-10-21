use anyhow::{Context, Result};
use stellar_xdr::curr::{LedgerCloseMeta, LedgerCloseMetaBatch, Limits, ReadXdr, WriteXdr, ScVal};
use stellar_strkey::Strkey;
use crate::data::Config;

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
        .post(Config::rpc_url())
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
                // Convert i128 parts to a readable balance
                let balance = i128::from(parts.hi) << 64 | i128::from(parts.lo as u64);
                return Ok(serde_json::json!({
                    "address": address,
                    "token": token_contract,
                    "balance": balance.to_string(),
                    "raw_balance": balance
                }));
            }
            Ok(ScVal::U128(parts)) => {
                // Convert u128 parts to a readable balance
                let balance = u128::from(parts.hi) << 64 | u128::from(parts.lo);
                return Ok(serde_json::json!({
                    "address": address,
                    "token": token_contract,
                    "balance": balance.to_string(),
                    "raw_balance": balance
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
