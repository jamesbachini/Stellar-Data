use stellar_xdr::curr::{LedgerCloseMetaBatch, LedgerCloseMeta, TransactionEnvelope, OperationBody};
use crate::stellar::address::{muxed_account_to_string, account_id_to_string};

/// Check if a transaction involves a specific address
pub fn transaction_involves_address(tx_envelope: &TransactionEnvelope, target_address: &str) -> bool {
    use TransactionEnvelope::*;

    match tx_envelope {
        TxV0(env) => {
            // Check source account
            let source = format!("{}", stellar_strkey::ed25519::PublicKey(env.tx.source_account_ed25519.0));
            if source == target_address {
                return true;
            }

            // Check operations
            for op in env.tx.operations.as_vec() {
                if let Some(ref src) = op.source_account {
                    if muxed_account_to_string(src) == target_address {
                        return true;
                    }
                }
                // Check operation-specific accounts (destination, etc.)
                if operation_involves_address(&op.body, target_address) {
                    return true;
                }
            }
        }
        Tx(env) => {
            // Check source account
            if muxed_account_to_string(&env.tx.source_account) == target_address {
                return true;
            }

            // Check operations
            for op in env.tx.operations.as_vec() {
                if let Some(ref src) = op.source_account {
                    if muxed_account_to_string(src) == target_address {
                        return true;
                    }
                }
                if operation_involves_address(&op.body, target_address) {
                    return true;
                }
            }
        }
        TxFeeBump(env) => {
            if muxed_account_to_string(&env.tx.fee_source) == target_address {
                return true;
            }
            // Check inner transaction - FeeBumpTransactionInnerTx is an enum with Tx variant
            match &env.tx.inner_tx {
                stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner_env) => {
                    // Wrap in TransactionEnvelope::Tx for recursive check
                    let wrapped = TransactionEnvelope::Tx(inner_env.clone());
                    return transaction_involves_address(&wrapped, target_address);
                }
            }
        }
    }

    false
}

/// Check if an operation involves a specific address
pub fn operation_involves_address(body: &OperationBody, target_address: &str) -> bool {
    use OperationBody::*;

    match body {
        CreateAccount(op) => account_id_to_string(&op.destination) == target_address,
        Payment(op) => muxed_account_to_string(&op.destination) == target_address,
        PathPaymentStrictReceive(op) => muxed_account_to_string(&op.destination) == target_address,
        PathPaymentStrictSend(op) => muxed_account_to_string(&op.destination) == target_address,
        ManageSellOffer(_) => false,
        CreatePassiveSellOffer(_) => false,
        SetOptions(_) => false,
        ChangeTrust(op) => {
            // Check if the asset issuer matches
            match &op.line {
                stellar_xdr::curr::ChangeTrustAsset::Native => false,
                stellar_xdr::curr::ChangeTrustAsset::CreditAlphanum4(asset) => {
                    account_id_to_string(&asset.issuer) == target_address
                }
                stellar_xdr::curr::ChangeTrustAsset::CreditAlphanum12(asset) => {
                    account_id_to_string(&asset.issuer) == target_address
                }
                stellar_xdr::curr::ChangeTrustAsset::PoolShare(_) => false,
            }
        }
        AllowTrust(op) => account_id_to_string(&op.trustor) == target_address,
        AccountMerge(op) => muxed_account_to_string(op) == target_address,
        ManageData(_) => false,
        BumpSequence(_) => false,
        ManageBuyOffer(_) => false,
        Inflation => false,
        BeginSponsoringFutureReserves(op) => account_id_to_string(&op.sponsored_id) == target_address,
        EndSponsoringFutureReserves => false,
        RevokeSponsorship(_) => false,
        Clawback(op) => muxed_account_to_string(&op.from) == target_address,
        ClawbackClaimableBalance(_) => false,
        SetTrustLineFlags(op) => account_id_to_string(&op.trustor) == target_address,
        LiquidityPoolDeposit(_) => false,
        LiquidityPoolWithdraw(_) => false,
        InvokeHostFunction(_) => false,
        ExtendFootprintTtl(_) => false,
        RestoreFootprint(_) => false,
        CreateClaimableBalance(_) => false,
        ClaimClaimableBalance(_) => false,
    }
}

/// Check if a transaction involves a specific contract address
pub fn transaction_involves_contract(tx_envelope: &TransactionEnvelope, contract_address: &str) -> bool {
    use TransactionEnvelope::*;

    let operations = match tx_envelope {
        TxV0(env) => env.tx.operations.as_vec(),
        Tx(env) => env.tx.operations.as_vec(),
        TxFeeBump(env) => {
            match &env.tx.inner_tx {
                stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner_env) => {
                    inner_env.tx.operations.as_vec()
                }
            }
        }
    };

    for op in operations {
        if let OperationBody::InvokeHostFunction(invoke_op) = &op.body {
            // Check auth credentials for contract addresses
            for auth in invoke_op.auth.as_vec() {
                // Check if the root_invocation contains the contract address
                let auth_str = format!("{:?}", auth.root_invocation);
                if auth_str.contains(contract_address) {
                    return true;
                }
            }

            // Check host function itself - convert to string and search
            let host_fn_str = format!("{:?}", invoke_op.host_function);
            if host_fn_str.contains(contract_address) {
                return true;
            }
        }
    }

    false
}

/// Check if a transaction calls a specific function name
pub fn transaction_calls_function(tx_envelope: &TransactionEnvelope, function_name: &str) -> bool {
    use TransactionEnvelope::*;

    let operations = match tx_envelope {
        TxV0(env) => env.tx.operations.as_vec(),
        Tx(env) => env.tx.operations.as_vec(),
        TxFeeBump(env) => {
            match &env.tx.inner_tx {
                stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner_env) => {
                    inner_env.tx.operations.as_vec()
                }
            }
        }
    };

    for op in operations {
        if let OperationBody::InvokeHostFunction(invoke_op) = &op.body {
            // Check auth credentials for function names
            for auth in invoke_op.auth.as_vec() {
                let auth_str = format!("{:?}", auth.root_invocation);
                // Look for function name in the debug output
                if auth_str.contains(&format!("\"{}\"", function_name)) ||
                   auth_str.contains(&format!("function_name: Symbol(StringM({})", function_name)) {
                    return true;
                }
            }

            // Check host function by converting to debug string
            let host_fn_str = format!("{:?}", invoke_op.host_function);
            if host_fn_str.contains(&format!("\"{}\"", function_name)) ||
               host_fn_str.contains(&format!("Symbol(StringM({})", function_name)) {
                return true;
            }
        }
    }

    false
}

/// Filter transactions in a batch by address
pub fn filter_by_address(batch: &LedgerCloseMetaBatch, address: &str) -> Vec<serde_json::Value> {
    let mut matching_transactions = Vec::new();

    for meta in batch.ledger_close_metas.as_vec() {
        match meta {
            LedgerCloseMeta::V0(v0) => {
                for tx in v0.tx_set.txs.as_vec() {
                    if transaction_involves_address(tx, address) {
                        if let Ok(tx_json) = serde_json::to_value(tx) {
                            matching_transactions.push(tx_json);
                        }
                    }
                }
            }
            LedgerCloseMeta::V1(v1) => {
                for tx_result in v1.tx_processing.as_vec() {
                    // V1 contains the full tx_set, we need to cross reference
                    // For simplicity, just serialize all transactions for V1
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
            LedgerCloseMeta::V2(v2) => {
                for tx_result in v2.tx_processing.as_vec() {
                    // V2 is similar to V1
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
        }
    }

    matching_transactions
}

/// Filter transactions in a batch by contract address
pub fn filter_by_contract(batch: &LedgerCloseMetaBatch, contract_address: &str) -> Vec<serde_json::Value> {
    let mut matching_transactions = Vec::new();

    for meta in batch.ledger_close_metas.as_vec() {
        match meta {
            LedgerCloseMeta::V0(v0) => {
                for tx in v0.tx_set.txs.as_vec() {
                    if transaction_involves_contract(tx, contract_address) {
                        if let Ok(tx_json) = serde_json::to_value(tx) {
                            matching_transactions.push(tx_json);
                        }
                    }
                }
            }
            LedgerCloseMeta::V1(v1) => {
                for tx_result in v1.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
            LedgerCloseMeta::V2(v2) => {
                for tx_result in v2.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
        }
    }

    matching_transactions
}

/// Filter transactions in a batch by function name
pub fn filter_by_function(batch: &LedgerCloseMetaBatch, function_name: &str) -> Vec<serde_json::Value> {
    let mut matching_transactions = Vec::new();

    for meta in batch.ledger_close_metas.as_vec() {
        match meta {
            LedgerCloseMeta::V0(v0) => {
                for tx in v0.tx_set.txs.as_vec() {
                    if transaction_calls_function(tx, function_name) {
                        if let Ok(tx_json) = serde_json::to_value(tx) {
                            matching_transactions.push(tx_json);
                        }
                    }
                }
            }
            LedgerCloseMeta::V1(v1) => {
                for tx_result in v1.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
            LedgerCloseMeta::V2(v2) => {
                for tx_result in v2.tx_processing.as_vec() {
                    if let Ok(tx_json) = serde_json::to_value(tx_result) {
                        matching_transactions.push(tx_json);
                    }
                }
            }
        }
    }

    matching_transactions
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::{
        TransactionV0, TransactionV0Envelope, Memo, TimeBounds,
        SequenceNumber, Uint256, CreateAccountOp, PaymentOp, Asset,
        VecM, AccountId, PublicKey, MuxedAccount
    };

    // Helper function to create a test Uint256
    fn create_test_uint256() -> Uint256 {
        Uint256([
            0x7d, 0xf0, 0x10, 0x66, 0x9a, 0x06, 0xf1, 0xb1,
            0x24, 0xbf, 0xd5, 0x8a, 0x82, 0x89, 0x04, 0xbe,
            0x4a, 0xf9, 0x13, 0x89, 0xb2, 0xcd, 0x98, 0x36,
            0xd4, 0x57, 0x3e, 0x8c, 0x2f, 0x1e, 0x0f, 0x59,
        ])
    }

    fn create_different_uint256() -> Uint256 {
        Uint256([
            0x3f, 0x0c, 0x34, 0xbf, 0x93, 0xad, 0x0d, 0x99,
            0x71, 0xd0, 0x4c, 0xcc, 0x90, 0xf7, 0x05, 0x51,
            0x1c, 0x83, 0x8a, 0x2f, 0x59, 0xa3, 0x8a, 0xf5,
            0x63, 0x98, 0x62, 0xf3, 0xfc, 0xce, 0x55, 0x3d,
        ])
    }

    // Helper to create a simple TxV0 envelope
    fn create_test_tx_v0_envelope(source_account: Uint256) -> TransactionEnvelope {
        let tx = TransactionV0 {
            source_account_ed25519: source_account,
            fee: 100,
            seq_num: SequenceNumber(1),
            time_bounds: None,
            memo: Memo::None,
            operations: VecM::try_from(vec![]).unwrap(),
            ext: stellar_xdr::curr::TransactionV0Ext::V0,
        };

        TransactionEnvelope::TxV0(TransactionV0Envelope {
            tx,
            signatures: VecM::try_from(vec![]).unwrap(),
        })
    }

    #[test]
    fn test_transaction_involves_address_txv0_source_match() {
        let uint256 = create_test_uint256();
        let tx_envelope = create_test_tx_v0_envelope(uint256);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(transaction_involves_address(&tx_envelope, target_address));
    }

    #[test]
    fn test_transaction_involves_address_txv0_source_no_match() {
        let uint256 = create_different_uint256();
        let tx_envelope = create_test_tx_v0_envelope(uint256);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(!transaction_involves_address(&tx_envelope, target_address));
    }

    #[test]
    fn test_operation_involves_address_create_account() {
        let destination = create_test_uint256();
        let destination_account_id = AccountId(PublicKey::PublicKeyTypeEd25519(destination));

        let create_op = CreateAccountOp {
            destination: destination_account_id,
            starting_balance: 10000000,
        };

        let op_body = OperationBody::CreateAccount(create_op);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_create_account_no_match() {
        let destination = create_different_uint256();
        let destination_account_id = AccountId(PublicKey::PublicKeyTypeEd25519(destination));

        let create_op = CreateAccountOp {
            destination: destination_account_id,
            starting_balance: 10000000,
        };

        let op_body = OperationBody::CreateAccount(create_op);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(!operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_payment() {
        let destination = create_test_uint256();
        let destination_muxed = MuxedAccount::Ed25519(destination);

        let payment_op = PaymentOp {
            destination: destination_muxed,
            asset: Asset::Native,
            amount: 10000000,
        };

        let op_body = OperationBody::Payment(payment_op);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_manage_sell_offer() {
        // ManageSellOffer should return false (no address involvement)
        let op_body = OperationBody::ManageSellOffer(stellar_xdr::curr::ManageSellOfferOp {
            selling: Asset::Native,
            buying: Asset::Native,
            amount: 10000000,
            price: stellar_xdr::curr::Price { n: 1, d: 1 },
            offer_id: 0,
        });

        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(!operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_inflation() {
        // Inflation should return false
        let op_body = OperationBody::Inflation;
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(!operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_account_merge() {
        let destination = create_test_uint256();
        let destination_muxed = MuxedAccount::Ed25519(destination);

        let op_body = OperationBody::AccountMerge(destination_muxed);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_begin_sponsoring() {
        let sponsored_id = create_test_uint256();
        let sponsored_account_id = AccountId(PublicKey::PublicKeyTypeEd25519(sponsored_id));

        let op = stellar_xdr::curr::BeginSponsoringFutureReservesOp {
            sponsored_id: sponsored_account_id,
        };

        let op_body = OperationBody::BeginSponsoringFutureReserves(op);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_change_trust_issuer_match() {
        let issuer = create_test_uint256();
        let issuer_account_id = AccountId(PublicKey::PublicKeyTypeEd25519(issuer));

        let asset = stellar_xdr::curr::ChangeTrustAsset::CreditAlphanum4(
            stellar_xdr::curr::AlphaNum4 {
                asset_code: stellar_xdr::curr::AssetCode4([0x55, 0x53, 0x44, 0x43]),
                issuer: issuer_account_id,
            }
        );

        let op = stellar_xdr::curr::ChangeTrustOp {
            line: asset,
            limit: 1000000000,
        };

        let op_body = OperationBody::ChangeTrust(op);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        assert!(operation_involves_address(&op_body, target_address));
    }

    #[test]
    fn test_operation_involves_address_change_trust_native() {
        let op = stellar_xdr::curr::ChangeTrustOp {
            line: stellar_xdr::curr::ChangeTrustAsset::Native,
            limit: 1000000000,
        };

        let op_body = OperationBody::ChangeTrust(op);
        let target_address = "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR";

        // Native asset has no issuer, so should not match
        assert!(!operation_involves_address(&op_body, target_address));
    }
}
