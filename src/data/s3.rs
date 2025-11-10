use anyhow::{Context, Result};

/// Download and decompress XDR data from S3
pub fn fetch_and_decompress(url: &str, silent: bool) -> Result<Vec<u8>> {
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
