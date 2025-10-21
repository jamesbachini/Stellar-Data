use anyhow::{Context, Result};
use stellar_xdr::curr::{LedgerCloseMetaBatch, ReadXdr, Limits};

/// Parse XDR data into LedgerCloseMetaBatch
pub fn parse_xdr(data: &[u8]) -> Result<LedgerCloseMetaBatch> {
    LedgerCloseMetaBatch::from_xdr(data, Limits::none())
        .context("Failed to parse XDR data")
}
