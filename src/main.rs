use anyhow::Result;
use clap::Parser;
use stellar_xdr::curr::LedgerCloseMeta;

mod cli;
mod ledger;
mod data;
mod stellar;
mod output;
mod server;

use cli::{Args, LONG_ABOUT};
use ledger::{LedgerRange, get_latest_ledger};
use data::{Config, parse_xdr, query_balance};
use data::s3::fetch_and_decompress;
use data::rpc::fetch_from_rpc;
use stellar::filters::{filter_by_address, filter_by_contract, filter_by_function};
use output::to_json;

#[tokio::main]
async fn main() -> Result<()> {
    // If no arguments provided, show just the long_about and exit
    if std::env::args().len() == 1 {
        println!("{}", LONG_ABOUT);
        std::process::exit(0);
    }

    let args = Args::parse();

    // Validate arguments
    args.validate()?;

    // If server mode is enabled, start the API server
    if args.server {
        return server::start_server(args.port).await;
    }

    // Handle balance query separately (doesn't need ledger data)
    if args.query == "balance" {
        let address = args.address.as_ref().unwrap();
        let token_input = args.token.as_ref().unwrap();

        // Resolve token shortcut to contract address
        let token_contract = Config::resolve_token(token_input)
            .unwrap_or(token_input.as_str());

        println!("Querying balance for address: {}", address);
        println!("Token: {} ({})", token_input, token_contract);

        let result = query_balance(address, token_contract)?;
        println!("\n{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let config = Config::default();

    // Get ledger string, required for non-balance queries
    let ledger_str = args.ledger.as_ref()
        .ok_or_else(|| anyhow::anyhow!("--ledger is required for this query type"))?;

    // Fetch latest ledger if we need it (for negative ledger values)
    let latest_ledger = if ledger_str.trim().starts_with('-') {
        Some(get_latest_ledger()?)
    } else {
        None
    };

    // Parse ledger range
    let ledger_range = LedgerRange::parse(ledger_str, latest_ledger)?;

    let is_range = ledger_range.is_range();
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
