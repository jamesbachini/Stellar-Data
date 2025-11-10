# Stellar Data Tool

A command line tool and REST API for querying the Stellar blockchain using public data lakes and RPC nodes, providing JSON formatted responses to simplify data availability.

## Quick Start

```bash
cargo install --locked stellar-data
stellar-data --help
```

## Overview

This tool downloads Stellar ledger data from the public S3 bucket at `s3://aws-public-blockchain/v1.1/stellar/ledgers/`, decompresses the Zstandard compressed XDR data, and converts it to JSON format. If data isn't available (Most recent blocks) it falls back to querying an RPC node.

Available currently for Stellar mainnet only. Testnet coming soon (hopefully)

Published to: https://crates.io/crates/stellar-data

## Features

- **Dual Mode Operation**: Use as CLI tool or REST API server
- Downloads ledger data directly from AWS S3 public data lake
- Automatically calculates correct S3 paths using partition and batch logic
- Decompresses Zstandard (.zst) compressed files
- Parses XDR (External Data Representation) format
- Converts to human-readable JSON
- Supports filtering for specific data types (transactions, all data, address-based)
- Query single ledgers or ranges of ledgers
- Query recent ledgers using negative values (e.g., `-999` for last 999 blocks)
- Filter transactions by Stellar address
- **REST API**: HTTP endpoints for web application integration
- **Smart Contract Queries**: Filter by contract address or function name
- **Token Balances**: Query current balances with built-in token shortcuts
- **Automatic RPC Fallback**: Seamless fallback to RPC for most recent ledgers

## Installation

### Build from source

```bash
cargo build --release
```

The binary will be available at `./target/release/stellar-data`

## Usage

The tool can be used in two modes:
1. **CLI Mode**: Direct command-line queries with output to stdout
2. **REST API Mode**: HTTP server exposing API endpoints

### CLI Mode

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

### REST API Mode

Start the API server to enable HTTP access to Stellar blockchain data:

```bash
./target/release/stellar-data --server --port 3000
```

Or use the default port (80):
```bash
./target/release/stellar-data --server
```

Once started, the server will display:
```
Stellar Data API Server
======================
Listening on http://0.0.0.0:3000

Available endpoints:
  GET /help
  GET /transactions?ledger=<LEDGER>&address=<ADDRESS>
  GET /all?ledger=<LEDGER>
  GET /contract?ledger=<LEDGER>&address=<CONTRACT>
  GET /function?ledger=<LEDGER>&name=<FUNCTION>
  GET /balance?address=<ADDRESS>&token=<TOKEN>
```

#### REST API Endpoints

All endpoints return JSON responses (except `/help` which returns HTML documentation).

##### `GET /help`

Returns an interactive HTML documentation page with detailed information about all endpoints.

```bash
curl http://localhost:3000/help
```

Or visit `http://localhost:3000/help` in your browser for a formatted documentation page.

##### `GET /transactions`

Get transactions from specified ledger(s), optionally filtered by address.

**Parameters:**
- `ledger` (required): Ledger sequence number, range, or negative value
- `address` (optional): Stellar address to filter transactions

**Examples:**

```bash
# Single ledger
curl "http://localhost:3000/transactions?ledger=50000000"

# Ledger range
curl "http://localhost:3000/transactions?ledger=50000000-50000005"

# Recent ledgers
curl "http://localhost:3000/transactions?ledger=-10"

# Filter by address
curl "http://localhost:3000/transactions?ledger=50000000&address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB"
```

**Response:**
```json
{
  "start_sequence": 50000000,
  "end_sequence": 50000005,
  "ledgers_processed": 6,
  "address": null,
  "transactions": [...],
  "count": 4523
}
```

##### `GET /all`

Get complete ledger metadata including all transaction processing details.

**Parameters:**
- `ledger` (required): Ledger sequence number, range, or negative value

**Examples:**

```bash
# Single ledger
curl "http://localhost:3000/all?ledger=50000000"

# Ledger range
curl "http://localhost:3000/all?ledger=50000000-50000002"

# Recent ledgers
curl "http://localhost:3000/all?ledger=-5"
```

**Response:**
```json
{
  "start_sequence": 50000000,
  "end_sequence": 50000000,
  "ledgers_processed": 1,
  "ledgers": [...],
  "count": 1
}
```

##### `GET /contract`

Get transactions involving a specific smart contract.

**Parameters:**
- `ledger` (required): Ledger sequence number, range, or negative value
- `address` (required): Contract address (starts with 'C')

**Examples:**

```bash
# Search contract invocations in ledger range
curl "http://localhost:3000/contract?ledger=50000000-50000010&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"

# Search recent ledgers
curl "http://localhost:3000/contract?ledger=-100&address=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
```

**Response:**
```json
{
  "start_sequence": 50000000,
  "end_sequence": 50000010,
  "ledgers_processed": 11,
  "contract": "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
  "transactions": [...],
  "count": 15
}
```

##### `GET /function`

Get transactions calling a specific contract function by name.

**Parameters:**
- `ledger` (required): Ledger sequence number, range, or negative value
- `name` (required): Function name (e.g., 'transfer', 'approve', 'mint')

**Examples:**

```bash
# Search for transfer function calls
curl "http://localhost:3000/function?ledger=50000000-50000100&name=transfer"

# Search recent ledgers for approve calls
curl "http://localhost:3000/function?ledger=-1000&name=approve"
```

**Response:**
```json
{
  "start_sequence": 50000000,
  "end_sequence": 50000100,
  "ledgers_processed": 101,
  "function": "transfer",
  "transactions": [...],
  "count": 234
}
```

##### `GET /balance`

Get current token balance for a Stellar address using RPC.

**Parameters:**
- `address` (required): Stellar account address
- `token` (required): Token contract address or shortcut

**Token Shortcuts:**
- `xlm` - Native Stellar Lumens
- `usdc` - USD Coin
- `usdt` - Tether USD
- `aqua` - Aquarius token
- `btc` - Bitcoin (wrapped)

**Examples:**

```bash
# Get XLM balance
curl "http://localhost:3000/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=xlm"

# Get USDC balance
curl "http://localhost:3000/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=usdc"

# Get balance for specific token contract
curl "http://localhost:3000/balance?address=GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB&token=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
```

**Response:**
```json
{
  "address": "GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB",
  "token": "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA",
  "balance": "1121995790",
  "raw_balance": 1121995790
}
```

#### REST API Features

- **CORS Enabled**: The API has permissive CORS enabled for easy integration with web applications
- **Automatic Fallback**: Recent ledgers automatically fall back to RPC when not available in S3
- **Error Resilience**: Individual ledger failures in ranges don't stop processing
- **Interactive Documentation**: Visit `/help` endpoint in a browser for full interactive documentation

#### REST API vs CLI Mode

| Feature | CLI Mode | REST API Mode |
|---------|----------|---------------|
| **Use Case** | One-time queries, scripts, automation | Web applications, continuous access, integration |
| **Output** | stdout (console) | HTTP JSON responses |
| **Concurrency** | Single query per invocation | Multiple concurrent requests |
| **Setup** | Run directly | Start server once |
| **Documentation** | `--help` flag | Interactive `/help` endpoint |

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

## Publishing Notes
There's a bug in WSL that prevents metadata, use Windows console

```bash
cargo test
cargo login
cargo clean
cargo package
cargo publish --dry-run
cargo publish
```

## References

- [Stellar Public Data Documentation](https://github.com/stellar/stellar-public-data)
- [stellar-xdr Rust Crate](https://docs.rs/stellar-xdr/latest/stellar_xdr/)
- [AWS Public Blockchain Data](https://aws.amazon.com/public-datasets/blockchain/)

## License

MIT

## Contributions

Feel free to improve and put in a pull request ♥️
