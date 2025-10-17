# Stellar Data Query Tool

A command-line tool for querying Stellar blockchain data from AWS public data lakes and converting XDR to JSON.

## Overview

This tool downloads Stellar ledger data from the public S3 bucket at `s3://aws-public-blockchain/v1.1/stellar/ledgers/`, decompresses the Zstandard-compressed XDR data, and converts it to JSON format.

## Features

- Downloads ledger data directly from AWS S3 public data lake
- Automatically calculates correct S3 paths using partition and batch logic
- Decompresses Zstandard (.zst) compressed files
- Parses XDR (External Data Representation) format
- Converts to human-readable JSON
- Supports filtering for specific data types (transactions, all data, address-based)
- Query single ledgers or ranges of ledgers
- Query recent ledgers using negative values (e.g., `-999` for last 999 blocks)
- Filter transactions by Stellar address

## Installation

### Build from source

```bash
cargo build --release
```

The binary will be available at `./target/release/stellar-data`

## Usage

Basic syntax:

```bash
stellar-data --ledger <LEDGER_NUMBER | RANGE> --query <QUERY_TYPE> [--address <ADDRESS>]
```

### Options

- `--ledger, -l`: The ledger/block number or range to query (required)
  - Single ledger: `--ledger 63864`
  - Ledger range: `--ledger 63864-63900`
  - Recent ledgers (negative): `--ledger -999` (queries last 999 blocks from current)
- `--query, -q`: Query type - `all`, `transactions`, or `address` (default: `all`)
- `--address, -a`: Stellar address to filter by (required when `--query address`)

### Examples

#### Get all data for a specific ledger

```bash
./target/release/stellar-data --ledger 63864 --query all
```

Output includes the full ledger close metadata:
- Ledger header information
- Transaction set
- Transaction processing details
- SCP (Stellar Consensus Protocol) info
- Upgrade processing

#### Get only transactions from a ledger

```bash
./target/release/stellar-data --ledger 50000000 --query transactions
```

Output format:
```json
{
  "start_sequence": 50000000,
  "end_sequence": 50000000,
  "count": 721,
  "transactions": [
    {
      "tx": {
        "signatures": [...],
        "tx": {
          "source_account": "...",
          "fee": 5000,
          "seq_num": "...",
          "operations": [...],
          "memo": "none",
          "cond": {...}
        }
      }
    }
  ]
}
```

#### Get early Stellar ledger (may have no transactions)

```bash
./target/release/stellar-data --ledger 63864 --query transactions
```

#### Query a range of ledgers for all transactions

```bash
./target/release/stellar-data --ledger 50000000-50000010 --query transactions
```

Output format:
```json
{
  "start_sequence": 50000000,
  "end_sequence": 50000010,
  "ledgers_processed": 11,
  "address": null,
  "count": 7945,
  "transactions": [...]
}
```

#### Query the most recent N ledgers

Get transactions from the last 10 blocks:
```bash
./target/release/stellar-data --ledger -10 --query transactions
```

The tool will automatically fetch the current latest ledger from Horizon and work backwards.

Output format:
```json
{
  "start_sequence": 59423252,
  "end_sequence": 59423261,
  "ledgers_processed": 10,
  "address": null,
  "count": 5621,
  "transactions": [...]
}
```

#### Search for transactions involving a specific address

Single ledger:
```bash
./target/release/stellar-data --ledger 50000000 --query address --address GCWGA2XKBSKVAAPN3UKG2V4TA2O4UDOQEVNNND5GPRLBC63DDEUM3G2I
```

Ledger range:
```bash
./target/release/stellar-data --ledger 63864-638900 --query address --address GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB
```

Recent blocks with address filter:
```bash
./target/release/stellar-data --ledger -999 --query address --address GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB
```

The address filter searches for:
- Source accounts in transactions
- Source accounts in operations
- Destination accounts in payments
- Asset issuers in trust lines
- Trustors in trust operations
- And other address-related fields

Output format:
```json
{
  "start_sequence": 63864,
  "end_sequence": 638900,
  "ledgers_processed": 575037,
  "address": "GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB",
  "count": 42,
  "transactions": [...]
}
```

## How It Works

### URL Generation

The tool calculates the S3 URL based on the ledger sequence number using the following format:

```
https://aws-public-blockchain.s3.us-east-2.amazonaws.com/v1.1/stellar/ledgers/pubnet/{PARTITION}/{BATCH}.xdr.zst
```

Where:
- **Partition**: Groups of 64,000 ledgers formatted as `FFFFFFFF--{start}-{end}`
- **Batch**: Individual ledgers (batch size = 1) formatted as `FFFFFFFF--{ledger}.xdr.zst`

Example for ledger 63864:
```
FFFFFFFF--0-63999/FFFF0687--63864.xdr.zst
```

The hexadecimal values are calculated as `0xFFFFFFFF - ledger_sequence`.

### Data Processing Pipeline

1. **Fetch Latest Ledger** (if using negative values): Queries Stellar Horizon API for current ledger
2. **Download**: Fetches compressed XDR data from S3
3. **Decompress**: Uses Zstandard decompression
4. **Parse**: Decodes XDR into `LedgerCloseMetaBatch` structure
5. **Convert**: Serializes to JSON using the stellar-xdr crate's serde support

## Configuration

The tool uses these default values from the S3 data lake configuration:

- **Network**: Public Global Stellar Network (mainnet)
- **Compression**: Zstandard
- **Ledgers per batch**: 1
- **Batches per partition**: 64,000
- **Base URL**: `https://aws-public-blockchain.s3.us-east-2.amazonaws.com`

## Dependencies

- `stellar-xdr` v24.0.0 - Stellar XDR encoding/decoding
- `stellar-strkey` v0.0.13 - Stellar address encoding/decoding
- `reqwest` - HTTP client for downloading from S3 and Horizon API
- `zstd` - Zstandard decompression
- `clap` - Command-line argument parsing
- `anyhow` - Error handling
- `serde` - Serialization framework
- `serde_json` - JSON serialization

## Data Structure

The XDR files contain `LedgerCloseMetaBatch` structures with:

```rust
struct LedgerCloseMetaBatch {
    start_sequence: u32,
    end_sequence: u32,
    ledger_close_metas: Vec<LedgerCloseMeta>,
}
```

Each `LedgerCloseMeta` can be V0, V1, or V2 format, containing:
- Ledger header with sequence number, timestamps, hashes
- Transaction set with all transactions in the ledger
- Transaction processing results
- Upgrade processing information
- SCP consensus information

## References

- [Stellar Public Data Documentation](https://github.com/stellar/stellar-public-data)
- [stellar-xdr Rust Crate](https://docs.rs/stellar-xdr/latest/stellar_xdr/)
- [AWS Public Blockchain Data](https://aws.amazon.com/public-datasets/blockchain/)

## License

See LICENSE file for details.
