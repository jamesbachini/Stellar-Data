# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust CLI tool that queries Stellar blockchain data from AWS S3 public data lakes, decompresses XDR (External Data Representation) binary format, and converts it to JSON. The tool is specifically designed to work with the Stellar public blockchain data archive at `s3://aws-public-blockchain/v1.1/stellar/ledgers/`.

## Build Commands

```bash
# Build release binary
cargo build --release

# Build for development (faster compilation, no optimizations)
cargo build

# Run the tool directly (development)
cargo run -- --ledger 50000000 --query transactions

# Run release binary
./target/release/stellar-data --ledger 50000000 --query transactions
```

The compiled binary is located at `./target/release/stellar-data` after building.

## Code Architecture

### Single-File Design
All code is in `src/main.rs` (~610 lines). This is intentional - the tool is a focused CLI application without complex module boundaries.

### Key Data Flow

1. **CLI Parsing** → `Args` struct (clap-based, lines 12-72)
2. **Ledger Range Resolution** → `LedgerRange::parse()` (lines 82-122)
   - Handles single ledgers, ranges (e.g., "100-200"), and negative values (e.g., "-999")
   - Negative values trigger Horizon API call to get latest ledger
3. **S3 URL Generation** → `Config::generate_url()` (lines 177-208)
   - Uses partition/batch naming scheme: `FFFFFFFF--{start}-{end}`
   - Hex values are `0xFFFFFFFF - ledger_sequence`
4. **Data Fetching** → `fetch_and_decompress()` (lines 212-235)
   - Downloads from S3 using reqwest blocking client
   - Decompresses Zstandard (.zst) files
5. **XDR Parsing** → `parse_xdr()` (lines 238-241)
   - Uses `stellar-xdr` crate's `ReadXdr` trait
   - Parses into `LedgerCloseMetaBatch` structure
6. **JSON Conversion** → `to_json()` (lines 411-470)
   - Handles three query types: "all", "transactions", "address"
7. **Address Filtering** (when using `--query address`)
   - `transaction_involves_address()` (lines 265-323) - Main filter logic
   - `operation_involves_address()` (lines 326-370) - Checks operation-level addresses
   - Covers all Stellar operation types and address fields

### Critical Architecture Details

**Ledger Range Parsing Logic (lines 82-122)**
- Negative values (e.g., "-999") are detected and converted to absolute ranges using Horizon API
- Range format "X-Y" is split on hyphen, but must avoid collision with negative number prefix
- Uses `allow_hyphen_values = true` in clap to accept negative numbers

**S3 URL Construction (lines 177-208)**
- Partition size: 64,000 ledgers per partition
- Batch size: 1 ledger per batch (configurable in `Config`)
- URL format: `{base_url}/{ledgers_path}/{partition_key}/{batch_key}`
- Partition example: `FFFFFFFF--0-63999` (for ledgers 0-63999)
- Batch example: `FFFF0687--63864.xdr.zst` (for ledger 63864)

**Address Filtering Implementation**
- V0 ledgers: Filter on `TransactionEnvelope` directly from `tx_set.txs`
- V1/V2 ledgers: Currently returns all transactions (see lines 387-403)
  - TODO: Proper filtering requires cross-referencing tx_set with tx_processing
  - This is a known limitation documented in code comments
- Checks multiple address fields per transaction:
  - Transaction source accounts (both TxV0 and Tx envelopes)
  - Operation source accounts
  - Payment destinations, asset issuers, trustors, sponsorship targets, etc.
  - Fee bump inner transactions (recursive check)

**XDR Type Handling**
- Uses `stellar-xdr` v24.0.0 with "curr" feature for current protocol
- Three `LedgerCloseMeta` versions: V0, V1, V2
- Must handle all three in pattern matching
- V0: Basic transaction set in `tx_set.txs`
- V1/V2: Transaction processing results in `tx_processing`
- Uses `VecM` type (limited vector) - access via `.as_vec()`

## Stellar-Specific Context

### XDR Structure Hierarchy
```
LedgerCloseMetaBatch
  ├── start_sequence: u32
  ├── end_sequence: u32
  └── ledger_close_metas: Vec<LedgerCloseMeta>
        ├── V0 (early ledgers)
        │   ├── tx_set (TransactionSet with txs)
        │   └── tx_processing (empty)
        ├── V1 (intermediate)
        │   ├── tx_set
        │   └── tx_processing (TransactionResultMeta)
        └── V2 (current)
            └── tx_processing (TransactionResultMetaV1)
```

### Address Encoding
- Uses `stellar-strkey` for address encoding/decoding
- Stellar addresses are Ed25519 public keys encoded in base32
- Format: `G...` for accounts (e.g., `GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB`)
- Two account types in XDR: `MuxedAccount` and `AccountId`
  - MuxedAccount: Can be Ed25519 or MuxedEd25519 (with ID)
  - AccountId: Wraps PublicKey enum

### Stellar Data Lake Format
- Each batch is a single ledger (batch size = 1)
- Partitions contain 64,000 batches
- Files are Zstandard compressed
- Configuration in S3: `{"networkPassphrase":"Public Global Stellar Network ; September 2015","version":"1.0","compression":"zstd","ledgersPerBatch":1,"batchesPerPartition":64000}`
- Network: Public Global Stellar Network (mainnet)

## External Dependencies

### Horizon API Integration
- URL: `https://horizon.stellar.org/ledgers?order=desc&limit=1`
- Used only for negative ledger values to fetch latest ledger number
- Response format: `{"_embedded": {"records": [{"sequence": 59423261}]}}`
- See `get_latest_ledger()` at lines 130-153

### S3 Bucket
- Base URL: `https://aws-public-blockchain.s3.us-east-2.amazonaws.com`
- Path: `v1.1/stellar/ledgers/pubnet`
- Public bucket, no authentication required
- Sequential file access for range queries (not parallelized)

## Common Patterns

### Adding New Operation Type Checks
When Stellar adds new operation types, update `operation_involves_address()` (lines 326-370):
1. Add new match arm for operation type
2. Extract relevant addresses from operation struct
3. Use `account_id_to_string()` for AccountId or `muxed_account_to_string()` for MuxedAccount
4. Return boolean comparison with target address

### Modifying Query Types
To add new query types, modify:
1. `to_json()` function (lines 411-470) - Add new match arm
2. Update help text in Args struct (lines 41-54)
3. Update README.md examples

### Handling New LedgerCloseMeta Versions
If Stellar adds V3/V4 ledger versions:
1. Add match arm in `to_json()` for all three query types
2. Add match arm in `filter_by_address()`
3. Update data structure documentation in README

## Known Limitations

1. **V1/V2 Address Filtering**: Currently returns all transactions for V1/V2 ledgers when using address filter (lines 387-403). Proper filtering requires cross-referencing tx_set with tx_processing.

2. **Sequential Processing**: Range queries process ledgers sequentially. Could be parallelized for better performance on large ranges.

3. **Memory Usage**: Large ranges with `--query transactions` load all transactions into memory before output. For very large ranges, consider streaming output.

4. **Error Handling**: Individual ledger fetch failures in ranges are logged but don't stop processing. This is intentional for resilience but may miss data.

## Testing Strategy

No formal test suite exists. Manual testing pattern:
```bash
# Test single ledger
./target/release/stellar-data --ledger 63864 --query all

# Test range
./target/release/stellar-data --ledger 50000000-50000005 --query transactions

# Test negative (recent blocks)
./target/release/stellar-data --ledger -10 --query transactions

# Test address filtering
./target/release/stellar-data --ledger 50000000 --query address --address GCWGA2XKBSKVAAPN3UKG2V4TA2O4UDOQEVNNND5GPRLBC63DDEUM3G2I
```

When making changes, verify:
- Early ledgers (V0): 63864
- Recent ledgers (V1/V2): 50000000+
- Negative values: -10
- Address with known activity
