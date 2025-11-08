use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use crate::data::{parse_xdr, query_balance, Config};
use crate::data::s3::fetch_and_decompress;
use crate::data::rpc::fetch_from_rpc;
use crate::ledger::{get_latest_ledger, LedgerRange};
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
pub async fn help_handler() -> Html<String> {
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Stellar Data API Documentation</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            background: #f5f5f5;
        }
        header {
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 30px;
            border-radius: 8px;
            margin-bottom: 30px;
        }
        header h1 {
            margin: 0 0 10px 0;
            font-size: 2.5em;
        }
        header p {
            margin: 0;
            font-size: 1.1em;
            opacity: 0.95;
        }
        .version {
            display: inline-block;
            background: rgba(255,255,255,0.2);
            padding: 4px 12px;
            border-radius: 4px;
            font-size: 0.9em;
            margin-top: 10px;
        }
        .endpoint {
            background: white;
            padding: 25px;
            margin-bottom: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }
        .endpoint h2 {
            margin-top: 0;
            color: #667eea;
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .method {
            display: inline-block;
            background: #10b981;
            color: white;
            padding: 4px 10px;
            border-radius: 4px;
            font-size: 0.75em;
            font-weight: bold;
        }
        .param-table {
            width: 100%;
            border-collapse: collapse;
            margin: 15px 0;
        }
        .param-table th {
            background: #f8f9fa;
            text-align: left;
            padding: 10px;
            border-bottom: 2px solid #dee2e6;
        }
        .param-table td {
            padding: 10px;
            border-bottom: 1px solid #dee2e6;
        }
        .required {
            color: #dc3545;
            font-weight: bold;
        }
        .optional {
            color: #6c757d;
        }
        .example {
            background: #f8f9fa;
            padding: 15px;
            border-radius: 4px;
            margin: 10px 0;
            border-left: 4px solid #667eea;
        }
        .example-title {
            font-weight: bold;
            margin-bottom: 10px;
            color: #667eea;
        }
        .example code {
            display: block;
            background: #2d3748;
            color: #68d391;
            padding: 10px;
            border-radius: 4px;
            margin: 5px 0;
            overflow-x: auto;
            font-family: 'Courier New', monospace;
        }
        .example a {
            color: #68d391;
            text-decoration: none;
        }
        .example a:hover {
            text-decoration: underline;
        }
        .notes {
            background: #fff3cd;
            border: 1px solid #ffc107;
            padding: 20px;
            border-radius: 8px;
            margin: 20px 0;
        }
        .notes h3 {
            margin-top: 0;
            color: #856404;
        }
        .notes ul {
            margin: 0;
            padding-left: 20px;
        }
        .notes li {
            margin: 10px 0;
        }
        .response-structure {
            background: #e7f3ff;
            border: 1px solid #0066cc;
            padding: 20px;
            border-radius: 8px;
            margin: 20px 0;
        }
        .response-structure h3 {
            margin-top: 0;
            color: #004085;
        }
        .response-structure pre {
            background: #f8f9fa;
            padding: 15px;
            border-radius: 4px;
            overflow-x: auto;
        }
    </style>
</head>
<body>
    <header>
        <h1>Stellar Data API</h1>
        <p>Query Stellar blockchain data from AWS S3 public data lakes and RPC endpoints</p>
        <div class="version">v1.0.0</div>
    </header>

    <div class="endpoint">
        <h2><span class="method">GET</span> /help</h2>
        <p>Display this help information (you are here!)</p>
        <div class="example">
            <div class="example-title">Example:</div>
            <code><a href="/help">/help</a></code>
        </div>
    </div>

    <div class="endpoint">
        <h2><span class="method">GET</span> /transactions</h2>
        <p>Get transactions from specified ledger(s), optionally filtered by address.</p>

        <table class="param-table">
            <thead>
                <tr>
                    <th>Parameter</th>
                    <th>Type</th>
                    <th>Required</th>
                    <th>Description</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td><strong>ledger</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Ledger sequence number, range (e.g. '100-200'), or negative value for recent ledgers (e.g. '-10' for last 10 ledgers)</td>
                </tr>
                <tr>
                    <td><strong>address</strong></td>
                    <td>string</td>
                    <td class="optional">Optional</td>
                    <td>Stellar address to filter transactions (e.g. 'GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB')</td>
                </tr>
            </tbody>
        </table>

        <div class="example">
            <div class="example-title">Examples:</div>
            <code><a href="/transactions?ledger=50000000">/transactions?ledger=50000000</a></code>
            <code><a href="/transactions?ledger=50000000-50000005">/transactions?ledger=50000000-50000005</a></code>
            <code><a href="/transactions?ledger=-10">/transactions?ledger=-10</a></code>
            <code><a href="/transactions?ledger=50000000&address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB">/transactions?ledger=50000000&address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB</a></code>
        </div>
    </div>

    <div class="endpoint">
        <h2><span class="method">GET</span> /all</h2>
        <p>Get complete ledger metadata including all transaction processing details.</p>

        <table class="param-table">
            <thead>
                <tr>
                    <th>Parameter</th>
                    <th>Type</th>
                    <th>Required</th>
                    <th>Description</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td><strong>ledger</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Ledger sequence number, range, or negative value</td>
                </tr>
            </tbody>
        </table>

        <div class="example">
            <div class="example-title">Examples:</div>
            <code><a href="/all?ledger=50000000">/all?ledger=50000000</a></code>
            <code><a href="/all?ledger=50000000-50000002">/all?ledger=50000000-50000002</a></code>
            <code><a href="/all?ledger=-5">/all?ledger=-5</a></code>
        </div>
    </div>

    <div class="endpoint">
        <h2><span class="method">GET</span> /contract</h2>
        <p>Get transactions involving a specific smart contract.</p>

        <table class="param-table">
            <thead>
                <tr>
                    <th>Parameter</th>
                    <th>Type</th>
                    <th>Required</th>
                    <th>Description</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td><strong>ledger</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Ledger sequence number, range, or negative value</td>
                </tr>
                <tr>
                    <td><strong>address</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Contract address (starts with 'C')</td>
                </tr>
            </tbody>
        </table>

        <div class="example">
            <div class="example-title">Examples:</div>
            <code><a href="/contract?ledger=50000000-50000010&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC">/contract?ledger=50000000-50000010&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC</a></code>
            <code><a href="/contract?ledger=-100&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC">/contract?ledger=-100&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC</a></code>
        </div>
    </div>

    <div class="endpoint">
        <h2><span class="method">GET</span> /function</h2>
        <p>Get transactions calling a specific contract function by name.</p>

        <table class="param-table">
            <thead>
                <tr>
                    <th>Parameter</th>
                    <th>Type</th>
                    <th>Required</th>
                    <th>Description</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td><strong>ledger</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Ledger sequence number, range, or negative value</td>
                </tr>
                <tr>
                    <td><strong>name</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Function name (e.g. 'transfer', 'approve', 'mint')</td>
                </tr>
            </tbody>
        </table>

        <div class="example">
            <div class="example-title">Examples:</div>
            <code><a href="/function?ledger=50000000-50000100&name=transfer">/function?ledger=50000000-50000100&name=transfer</a></code>
            <code><a href="/function?ledger=-1000&name=approve">/function?ledger=-1000&name=approve</a></code>
        </div>
    </div>

    <div class="endpoint">
        <h2><span class="method">GET</span> /balance</h2>
        <p>Get current token balance for a Stellar address using RPC.</p>

        <table class="param-table">
            <thead>
                <tr>
                    <th>Parameter</th>
                    <th>Type</th>
                    <th>Required</th>
                    <th>Description</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td><strong>address</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Stellar account address</td>
                </tr>
                <tr>
                    <td><strong>token</strong></td>
                    <td>string</td>
                    <td class="required">Required</td>
                    <td>Token contract address or shortcut ('xlm', 'usdc', 'usdt', 'aqua', 'btc')</td>
                </tr>
            </tbody>
        </table>

        <div class="example">
            <div class="example-title">Examples:</div>
            <code><a href="/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=xlm">/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=xlm</a></code>
            <code><a href="/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=usdc">/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=usdc</a></code>
            <code><a href="/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC">/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC</a></code>
        </div>
    </div>

    <div class="notes">
        <h3>Important Notes</h3>
        <ul>
            <li><strong>Ledger Ranges:</strong> Ledger ranges are inclusive. Use '-N' for the last N ledgers (e.g. '-10' for most recent 10 ledgers)</li>
            <li><strong>Data Source:</strong> Data is fetched from AWS S3 public blockchain data lake with automatic fallback to Stellar RPC for recent ledgers</li>
            <li><strong>Address Format:</strong> Stellar addresses are base32-encoded Ed25519 public keys starting with 'G' (accounts) or 'C' (contracts)</li>
            <li><strong>Token Shortcuts:</strong> Supported token shortcuts: xlm, usdc, usdt, aqua, btc</li>
            <li><strong>Response Format:</strong> All responses (except /help) are JSON with metadata including start_sequence, end_sequence, ledgers_processed, and results</li>
            <li><strong>Error Handling:</strong> Individual ledger failures in ranges are logged but don't stop processing</li>
        </ul>
    </div>

    <div class="response-structure">
        <h3>Response Structure</h3>
        <p>All API endpoints (except /help) return JSON responses with the following general structure:</p>
        <pre>{
  "start_sequence": &lt;first ledger in range&gt;,
  "end_sequence": &lt;last ledger in range&gt;,
  "ledgers_processed": &lt;number of successfully processed ledgers&gt;,
  &lt;endpoint-specific fields&gt;,
  "count": &lt;number of results&gt;
}</pre>
    </div>
</body>
</html>"#;

    Html(html.to_string())
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
