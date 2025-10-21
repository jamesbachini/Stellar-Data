use anyhow::{Context, Result};
use stellar_xdr::curr::{LedgerCloseMetaBatch, LedgerCloseMeta};
use crate::stellar::filters::{filter_by_address, filter_by_contract, filter_by_function};

/// Convert LedgerCloseMetaBatch to JSON based on query type
pub fn to_json(
    batch: &LedgerCloseMetaBatch,
    query_type: &str,
    address_filter: Option<&str>,
    name_filter: Option<&str>
) -> Result<String> {
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
                    LedgerCloseMeta::V0(v0) => {
                        for tx in v0.tx_set.txs.as_vec() {
                            transactions.push(serde_json::to_value(tx)
                                .context("Failed to serialize transaction")?);
                        }
                    }
                    LedgerCloseMeta::V1(v1) => {
                        for tx_processing in v1.tx_processing.as_vec() {
                            transactions.push(serde_json::to_value(tx_processing)
                                .context("Failed to serialize transaction processing")?);
                        }
                    }
                    LedgerCloseMeta::V2(v2) => {
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
