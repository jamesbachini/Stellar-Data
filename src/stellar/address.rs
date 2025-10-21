use stellar_xdr::curr::{MuxedAccount, AccountId, PublicKey};

/// Extract account ID as string from MuxedAccount
pub fn muxed_account_to_string(muxed: &MuxedAccount) -> String {
    match muxed {
        MuxedAccount::Ed25519(uint256) => {
            format!("{}", stellar_strkey::ed25519::PublicKey(uint256.0))
        }
        MuxedAccount::MuxedEd25519(med) => {
            format!("{}", stellar_strkey::ed25519::PublicKey(med.ed25519.0))
        }
    }
}

/// Extract account ID as string from AccountId
pub fn account_id_to_string(account: &AccountId) -> String {
    match &account.0 {
        PublicKey::PublicKeyTypeEd25519(uint256) => {
            format!("{}", stellar_strkey::ed25519::PublicKey(uint256.0))
        }
    }
}
