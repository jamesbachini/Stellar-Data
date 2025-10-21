use anyhow::{Context, Result};

/// Ledger range parsed from input
#[derive(Debug)]
pub struct LedgerRange {
    pub start: u32,
    pub end: u32,
}

impl LedgerRange {
    /// Parse a ledger range string into start and end values
    ///
    /// Supports:
    /// - Single ledger: "63864"
    /// - Range: "63864-63900"
    /// - Negative (recent): "-999" (requires latest_ledger parameter)
    pub fn parse(input: &str, latest_ledger: Option<u32>) -> Result<Self> {
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

    /// Create an iterator over ledger sequence numbers in the range
    pub fn iter(&self) -> impl Iterator<Item = u32> {
        self.start..=self.end
    }

    /// Check if this represents a range (multiple ledgers) or a single ledger
    pub fn is_range(&self) -> bool {
        self.start != self.end
    }
}
