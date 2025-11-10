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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_ledger() {
        let range = LedgerRange::parse("63864", None).unwrap();
        assert_eq!(range.start, 63864);
        assert_eq!(range.end, 63864);
        assert!(!range.is_range());
    }

    #[test]
    fn test_parse_single_ledger_with_whitespace() {
        let range = LedgerRange::parse("  63864  ", None).unwrap();
        assert_eq!(range.start, 63864);
        assert_eq!(range.end, 63864);
    }

    #[test]
    fn test_parse_range() {
        let range = LedgerRange::parse("100-200", None).unwrap();
        assert_eq!(range.start, 100);
        assert_eq!(range.end, 200);
        assert!(range.is_range());
    }

    #[test]
    fn test_parse_range_with_whitespace() {
        let range = LedgerRange::parse(" 100 - 200 ", None).unwrap();
        assert_eq!(range.start, 100);
        assert_eq!(range.end, 200);
    }

    #[test]
    fn test_parse_negative_single() {
        let range = LedgerRange::parse("-10", Some(1000)).unwrap();
        assert_eq!(range.start, 991);
        assert_eq!(range.end, 1000);
        assert!(range.is_range());
    }

    #[test]
    fn test_parse_negative_larger_count() {
        let range = LedgerRange::parse("-999", Some(10000)).unwrap();
        assert_eq!(range.start, 9002);
        assert_eq!(range.end, 10000);
    }

    #[test]
    fn test_parse_negative_without_latest_fails() {
        let result = LedgerRange::parse("-10", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Could not determine latest ledger"));
    }

    #[test]
    fn test_parse_negative_zero_fails() {
        let result = LedgerRange::parse("-0", Some(1000));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be greater than 0"));
    }

    #[test]
    fn test_parse_negative_exceeds_latest_fails() {
        let result = LedgerRange::parse("-10000", Some(100));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds available ledgers"));
    }

    #[test]
    fn test_parse_invalid_number_fails() {
        let result = LedgerRange::parse("abc", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_range_start_greater_than_end_fails() {
        let result = LedgerRange::parse("200-100", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Start ledger must be less than or equal to end ledger"));
    }

    #[test]
    fn test_parse_range_equal_values() {
        let range = LedgerRange::parse("100-100", None).unwrap();
        assert_eq!(range.start, 100);
        assert_eq!(range.end, 100);
        assert!(!range.is_range()); // Same start and end = not a range
    }

    #[test]
    fn test_iter_single_ledger() {
        let range = LedgerRange::parse("100", None).unwrap();
        let values: Vec<u32> = range.iter().collect();
        assert_eq!(values, vec![100]);
    }

    #[test]
    fn test_iter_range() {
        let range = LedgerRange::parse("100-105", None).unwrap();
        let values: Vec<u32> = range.iter().collect();
        assert_eq!(values, vec![100, 101, 102, 103, 104, 105]);
    }

    #[test]
    fn test_iter_negative() {
        let range = LedgerRange::parse("-3", Some(1000)).unwrap();
        let values: Vec<u32> = range.iter().collect();
        assert_eq!(values, vec![998, 999, 1000]);
    }

    #[test]
    fn test_is_range_single() {
        let range = LedgerRange { start: 100, end: 100 };
        assert!(!range.is_range());
    }

    #[test]
    fn test_is_range_multiple() {
        let range = LedgerRange { start: 100, end: 200 };
        assert!(range.is_range());
    }
}
