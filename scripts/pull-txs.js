import StellarSdk from 'stellar-sdk';
import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import init, { decode, encode, guess, types, schema } from "@stellar/stellar-xdr-json";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const wasmPath = join(__dirname, 'node_modules/@stellar/stellar-xdr-json/stellar_xdr_json_bg.wasm');
const wasmBuffer = readFileSync(wasmPath);
await init(wasmBuffer);

const CONFIG = {
  rpcUrl: 'https://archive-rpc.lightsail.network/',
  networkPassphrase: StellarSdk.Networks.MAINNET,
};

async function rpcCall(method, params = {}) {
  const body = {
    jsonrpc: '2.0',
    id: Date.now(),
    method,
    params,
  };

  const res = await fetch(CONFIG.rpcUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

  const json = await res.json();
  if (json.error) throw new Error(JSON.stringify(json.error));
  return json.result;
}

function decodeTransactionXdr(xdrBase64) {
  try {
    const txEnvelope = StellarSdk.xdr.TransactionEnvelope.fromXDR(xdrBase64, 'base64');
    const tx = new StellarSdk.Transaction(txEnvelope, CONFIG.networkPassphrase);
    return {
      source: tx.source,
      fee: tx.fee,
      memo: tx.memo?.value?.toString() || null,
      operations: tx.operations.map((op) => ({
        type: op.type,
        from: op.source || tx.source,
        to: op.destination || null,
        asset: op.asset?.code || 'XLM',
        amount: op.amount || null,
      })),
    };
  } catch (e) {
    return { error: 'Failed to decode transaction XDR', details: e.message };
  }
}

async function getFirstTenLedgers() {
  console.log('Fetching first 10 ledgers from Soroban RPC...\n');
  const startLedger = 2;
  const ledgersResponse = await rpcCall('getLedgers', {
    startLedger,
    pagination: { limit: 10 },
  });

  const ledgers = ledgersResponse.ledgers || [];
  for (const ledger of ledgers) {
    console.log(`🧱 Ledger #${ledger.sequence}`);
    console.log(`${new Date(ledger.ledgerCloseTime * 1000).toISOString()}`);
    //console.log(`   Hash: ${ledger.hash}`);
    //console.log(`   Metadata XDR: ${ledger.metadataXdr}`);
    //console.log(decode("LedgerCloseMeta", ledger.metadataXdr));
    const decoded = decode("LedgerCloseMeta", ledger.metadataXdr);
    const txs = JSON.parse(decoded)?.v0?.tx_set?.txs || null;
    if (txs) {
      txs.forEach(tx => {
        console.log(tx.tx_v0?.tx);
      });
    } else {
      console.log('No Transactions')
    }
    console.log('---');
  }
}

getFirstTenLedgers().catch((err) => {
  console.error('Error:', err);
});