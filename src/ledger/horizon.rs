use anyhow::{Context, Result};

#[allow(dead_code)]
#[derive(serde::Deserialize)]
struct HorizonLedger {
    sequence: u32,
}

/// Fetch the latest ledger number from Stellar Horizon API
pub fn get_latest_ledger() -> Result<u32> {
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
