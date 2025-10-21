use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::data::{parse_xdr, query_balance, Config};
use crate::data::s3::fetch_and_decompress;
use crate::data::rpc::fetch_from_rpc;
use crate::ledger::{get_latest_ledger, LedgerRange};
use crate::output::to_json;
use crate::stellar::filters::{filter_by_address, filter_by_contract, filter_by_function};
use stellar_xdr::curr::LedgerCloseMeta;

#[derive(Debug, Deserialize)]
pub struct TransactionsQuery {
    ledger: String,
    #[serde(default)]
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AllQuery {
    ledger: String,
}

#[derive(Debug, Deserialize)]
pub struct ContractQuery {
    ledger: String,
    address: String,
}

#[derive(Debug, Deserialize)]
pub struct FunctionQuery {
    ledger: String,
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct BalanceQuery {
    address: String,
    token: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    error: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

/// Handler for /transactions endpoint
/// Supports both filtered (by address) and unfiltered transaction queries
pub async fn transactions_handler(
    Query(params): Query<TransactionsQuery>,
) -> Result<Json<Value>, ErrorResponse> {
    let config = Config::default();

    // Parse ledger range
    let latest_ledger = if params.ledger.trim().starts_with('-') {
        Some(get_latest_ledger().map_err(|e| ErrorResponse {
            error: format!("Failed to get latest ledger: {}", e),
        })?)
    } else {
        None
    };

    let ledger_range = LedgerRange::parse(&params.ledger, latest_ledger).map_err(|e| ErrorResponse {
        error: format!("Invalid ledger range: {}", e),
    })?;

    let mut all_transactions = Vec::new();
    let mut total_processed = 0;

    // Process each ledger in the range
    for ledger_seq in ledger_range.iter() {
        let url = config.generate_url(ledger_seq);

        // Fetch data with RPC fallback
        let decompressed_data = match fetch_and_decompress(&url, true) {
            Ok(data) => data,
            Err(e) => {
                if e.to_string().contains("HTTP 404") {
                    match fetch_from_rpc(ledger_seq, true) {
                        Ok(data) => data,
                        Err(_) => continue,
                    }
                } else {
                    continue;
                }
            }
        };

        let batch = match parse_xdr(&decompressed_data) {
            Ok(batch) => batch,
            Err(_) => continue,
        };

        total_processed += 1;

        // Filter by address if provided, otherwise get all transactions
        if let Some(ref address) = params.address {
            let matching = filter_by_address(&batch, address);
            all_transactions.extend(matching);
        } else {
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
    }

    let result = json!({
        "start_sequence": ledger_range.start,
        "end_sequence": ledger_range.end,
        "ledgers_processed": total_processed,
        "address": params.address,
        "transactions": all_transactions,
        "count": all_transactions.len()
    });

    Ok(Json(result))
}

/// Handler for /all endpoint
/// Returns full ledger metadata
pub async fn all_handler(Query(params): Query<AllQuery>) -> Result<Json<Value>, ErrorResponse> {
    let config = Config::default();

    // Parse ledger range
    let latest_ledger = if params.ledger.trim().starts_with('-') {
        Some(get_latest_ledger().map_err(|e| ErrorResponse {
            error: format!("Failed to get latest ledger: {}", e),
        })?)
    } else {
        None
    };

    let ledger_range = LedgerRange::parse(&params.ledger, latest_ledger).map_err(|e| ErrorResponse {
        error: format!("Invalid ledger range: {}", e),
    })?;

    let mut all_ledgers = Vec::new();
    let mut total_processed = 0;

    // Process each ledger in the range
    for ledger_seq in ledger_range.iter() {
        let url = config.generate_url(ledger_seq);

        let decompressed_data = match fetch_and_decompress(&url, true) {
            Ok(data) => data,
            Err(e) => {
                if e.to_string().contains("HTTP 404") {
                    match fetch_from_rpc(ledger_seq, true) {
                        Ok(data) => data,
                        Err(_) => continue,
                    }
                } else {
                    continue;
                }
            }
        };

        let batch = match parse_xdr(&decompressed_data) {
            Ok(batch) => batch,
            Err(_) => continue,
        };

        total_processed += 1;

        // Collect full ledger metadata
        for meta in batch.ledger_close_metas.as_vec() {
            if let Ok(meta_json) = serde_json::to_value(meta) {
                all_ledgers.push(meta_json);
            }
        }
    }

    let result = json!({
        "start_sequence": ledger_range.start,
        "end_sequence": ledger_range.end,
        "ledgers_processed": total_processed,
        "ledgers": all_ledgers,
        "count": all_ledgers.len()
    });

    Ok(Json(result))
}

/// Handler for /contract endpoint
/// Returns transactions involving a specific contract
pub async fn contract_handler(
    Query(params): Query<ContractQuery>,
) -> Result<Json<Value>, ErrorResponse> {
    let config = Config::default();

    let latest_ledger = if params.ledger.trim().starts_with('-') {
        Some(get_latest_ledger().map_err(|e| ErrorResponse {
            error: format!("Failed to get latest ledger: {}", e),
        })?)
    } else {
        None
    };

    let ledger_range = LedgerRange::parse(&params.ledger, latest_ledger).map_err(|e| ErrorResponse {
        error: format!("Invalid ledger range: {}", e),
    })?;

    let mut all_transactions = Vec::new();
    let mut total_processed = 0;

    for ledger_seq in ledger_range.iter() {
        let url = config.generate_url(ledger_seq);

        let decompressed_data = match fetch_and_decompress(&url, true) {
            Ok(data) => data,
            Err(e) => {
                if e.to_string().contains("HTTP 404") {
                    match fetch_from_rpc(ledger_seq, true) {
                        Ok(data) => data,
                        Err(_) => continue,
                    }
                } else {
                    continue;
                }
            }
        };

        let batch = match parse_xdr(&decompressed_data) {
            Ok(batch) => batch,
            Err(_) => continue,
        };

        total_processed += 1;
        let matching = filter_by_contract(&batch, &params.address);
        all_transactions.extend(matching);
    }

    let result = json!({
        "start_sequence": ledger_range.start,
        "end_sequence": ledger_range.end,
        "ledgers_processed": total_processed,
        "contract": params.address,
        "transactions": all_transactions,
        "count": all_transactions.len()
    });

    Ok(Json(result))
}

/// Handler for /function endpoint
/// Returns transactions calling a specific function
pub async fn function_handler(
    Query(params): Query<FunctionQuery>,
) -> Result<Json<Value>, ErrorResponse> {
    let config = Config::default();

    let latest_ledger = if params.ledger.trim().starts_with('-') {
        Some(get_latest_ledger().map_err(|e| ErrorResponse {
            error: format!("Failed to get latest ledger: {}", e),
        })?)
    } else {
        None
    };

    let ledger_range = LedgerRange::parse(&params.ledger, latest_ledger).map_err(|e| ErrorResponse {
        error: format!("Invalid ledger range: {}", e),
    })?;

    let mut all_transactions = Vec::new();
    let mut total_processed = 0;

    for ledger_seq in ledger_range.iter() {
        let url = config.generate_url(ledger_seq);

        let decompressed_data = match fetch_and_decompress(&url, true) {
            Ok(data) => data,
            Err(e) => {
                if e.to_string().contains("HTTP 404") {
                    match fetch_from_rpc(ledger_seq, true) {
                        Ok(data) => data,
                        Err(_) => continue,
                    }
                } else {
                    continue;
                }
            }
        };

        let batch = match parse_xdr(&decompressed_data) {
            Ok(batch) => batch,
            Err(_) => continue,
        };

        total_processed += 1;
        let matching = filter_by_function(&batch, &params.name);
        all_transactions.extend(matching);
    }

    let result = json!({
        "start_sequence": ledger_range.start,
        "end_sequence": ledger_range.end,
        "ledgers_processed": total_processed,
        "function": params.name,
        "transactions": all_transactions,
        "count": all_transactions.len()
    });

    Ok(Json(result))
}

/// Handler for /balance endpoint
/// Returns token balance for an address
pub async fn balance_handler(
    Query(params): Query<BalanceQuery>,
) -> Result<Json<Value>, ErrorResponse> {
    // Resolve token shortcut to contract address
    let token_contract = Config::resolve_token(&params.token)
        .unwrap_or(&params.token);

    let result = query_balance(&params.address, token_contract).map_err(|e| ErrorResponse {
        error: format!("Failed to query balance: {}", e),
    })?;

    Ok(Json(result))
}

/// Handler for /help endpoint
/// Returns API documentation and usage information
pub async fn help_handler() -> Json<Value> {
    let help_info = json!({
        "name": "Stellar Data API",
        "version": "1.0.0",
        "description": "Query Stellar blockchain data from AWS S3 public data lakes and RPC endpoints",
        "base_url": "http://0.0.0.0:3000",
        "endpoints": [
            {
                "path": "/help",
                "method": "GET",
                "description": "Display this help information",
                "parameters": [],
                "examples": [
                    "/help"
                ]
            },
            {
                "path": "/transactions",
                "method": "GET",
                "description": "Get transactions from specified ledger(s), optionally filtered by address",
                "parameters": [
                    {
                        "name": "ledger",
                        "type": "string",
                        "required": true,
                        "description": "Ledger sequence number, range (e.g. '100-200'), or negative value for recent ledgers (e.g. '-10' for last 10 ledgers)"
                    },
                    {
                        "name": "address",
                        "type": "string",
                        "required": false,
                        "description": "Stellar address to filter transactions (e.g. 'GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB')"
                    }
                ],
                "examples": [
                    "/transactions?ledger=50000000",
                    "/transactions?ledger=50000000-50000005",
                    "/transactions?ledger=-10",
                    "/transactions?ledger=50000000&address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB"
                ]
            },
            {
                "path": "/all",
                "method": "GET",
                "description": "Get complete ledger metadata including all transaction processing details",
                "parameters": [
                    {
                        "name": "ledger",
                        "type": "string",
                        "required": true,
                        "description": "Ledger sequence number, range, or negative value"
                    }
                ],
                "examples": [
                    "/all?ledger=50000000",
                    "/all?ledger=50000000-50000002",
                    "/all?ledger=-5"
                ]
            },
            {
                "path": "/contract",
                "method": "GET",
                "description": "Get transactions involving a specific smart contract",
                "parameters": [
                    {
                        "name": "ledger",
                        "type": "string",
                        "required": true,
                        "description": "Ledger sequence number, range, or negative value"
                    },
                    {
                        "name": "address",
                        "type": "string",
                        "required": true,
                        "description": "Contract address (starts with 'C')"
                    }
                ],
                "examples": [
                    "/contract?ledger=50000000-50000010&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
                    "/contract?ledger=-100&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
                ]
            },
            {
                "path": "/function",
                "method": "GET",
                "description": "Get transactions calling a specific contract function by name",
                "parameters": [
                    {
                        "name": "ledger",
                        "type": "string",
                        "required": true,
                        "description": "Ledger sequence number, range, or negative value"
                    },
                    {
                        "name": "name",
                        "type": "string",
                        "required": true,
                        "description": "Function name (e.g. 'transfer', 'approve', 'mint')"
                    }
                ],
                "examples": [
                    "/function?ledger=50000000-50000100&name=transfer",
                    "/function?ledger=-1000&name=approve"
                ]
            },
            {
                "path": "/balance",
                "method": "GET",
                "description": "Get current token balance for a Stellar address using RPC",
                "parameters": [
                    {
                        "name": "address",
                        "type": "string",
                        "required": true,
                        "description": "Stellar account address"
                    },
                    {
                        "name": "token",
                        "type": "string",
                        "required": true,
                        "description": "Token contract address or shortcut ('xlm', 'usdc', 'usdt', 'aqua', 'btc')"
                    }
                ],
                "examples": [
                    "/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=xlm",
                    "/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=usdc",
                    "/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
                ]
            }
        ],
        "notes": {
            "ledger_ranges": "Ledger ranges are inclusive. Use '-N' for the last N ledgers (e.g. '-10' for most recent 10 ledgers)",
            "data_source": "Data is fetched from AWS S3 public blockchain data lake with automatic fallback to Stellar RPC for recent ledgers",
            "address_format": "Stellar addresses are base32-encoded Ed25519 public keys starting with 'G' (accounts) or 'C' (contracts)",
            "token_shortcuts": "Supported token shortcuts: xlm, usdc, usdt, aqua, btc",
            "response_format": "All responses are JSON with metadata including start_sequence, end_sequence, ledgers_processed, and results",
            "error_handling": "Individual ledger failures in ranges are logged but don't stop processing"
        },
        "response_structure": {
            "transactions_endpoint": {
                "start_sequence": "First ledger in range",
                "end_sequence": "Last ledger in range",
                "ledgers_processed": "Number of successfully processed ledgers",
                "address": "Filter address (if provided)",
                "transactions": "Array of transaction objects",
                "count": "Number of transactions returned"
            },
            "all_endpoint": {
                "start_sequence": "First ledger in range",
                "end_sequence": "Last ledger in range",
                "ledgers_processed": "Number of successfully processed ledgers",
                "ledgers": "Array of full ledger metadata objects",
                "count": "Number of ledgers returned"
            },
            "contract_endpoint": {
                "start_sequence": "First ledger in range",
                "end_sequence": "Last ledger in range",
                "ledgers_processed": "Number of successfully processed ledgers",
                "contract": "Contract address filter",
                "transactions": "Array of matching transaction objects",
                "count": "Number of transactions returned"
            },
            "function_endpoint": {
                "start_sequence": "First ledger in range",
                "end_sequence": "Last ledger in range",
                "ledgers_processed": "Number of successfully processed ledgers",
                "function": "Function name filter",
                "transactions": "Array of matching transaction objects",
                "count": "Number of transactions returned"
            },
            "balance_endpoint": {
                "address": "Account address",
                "token": "Token contract address",
                "balance": "Current balance",
                "decimals": "Token decimals"
            }
        }
    });

    Json(help_info)
}

/// Create and configure the Axum router
pub fn create_router() -> Router {
    Router::new()
        .route("/help", get(help_handler))
        .route("/transactions", get(transactions_handler))
        .route("/all", get(all_handler))
        .route("/contract", get(contract_handler))
        .route("/function", get(function_handler))
        .route("/balance", get(balance_handler))
        .layer(CorsLayer::permissive())
}

/// Start the API server
pub async fn start_server(port: u16) -> anyhow::Result<()> {
    let app = create_router();

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("Stellar Data API Server");
    println!("======================");
    println!("Listening on http://{}", addr);
    println!("\nAvailable endpoints:");
    println!("  GET /help");
    println!("  GET /transactions?ledger=<LEDGER>&address=<ADDRESS>");
    println!("  GET /all?ledger=<LEDGER>");
    println!("  GET /contract?ledger=<LEDGER>&address=<CONTRACT>");
    println!("  GET /function?ledger=<LEDGER>&name=<FUNCTION>");
    println!("  GET /balance?address=<ADDRESS>&token=<TOKEN>");
    println!("\nFor detailed API documentation, visit:");
    println!("  http://127.0.0.1:{}/help\n", port);

    axum::serve(listener, app).await?;

    Ok(())
}
