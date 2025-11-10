use stellar_xdr::curr::{MuxedAccount, AccountId, PublicKey, Uint256};

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

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::MuxedAccountMed25519;

    // Helper function to create a test Uint256 from a known public key
    fn create_test_uint256() -> Uint256 {
        // This is a valid Stellar public key: GALPCCZN4YXA3YMJHKL6CVIECKPLJJCTVMSNYWBTKJW4K5HQLYLDMZTB
        // Decoded to bytes
        Uint256([
            0x7d, 0xf0, 0x10, 0x66, 0x9a, 0x06, 0xf1, 0xb1,
            0x24, 0xbf, 0xd5, 0x8a, 0x82, 0x89, 0x04, 0xbe,
            0x4a, 0xf9, 0x13, 0x89, 0xb2, 0xcd, 0x98, 0x36,
            0xd4, 0x57, 0x3e, 0x8c, 0x2f, 0x1e, 0x0f, 0x59,
        ])
    }

    #[test]
    fn test_muxed_account_ed25519_to_string() {
        let uint256 = create_test_uint256();
        let muxed = MuxedAccount::Ed25519(uint256);
        let result = muxed_account_to_string(&muxed);

        // Should start with 'G' for account addresses
        assert!(result.starts_with('G'));
        // Should be a valid length (56 characters)
        assert_eq!(result.len(), 56);
        // Should be the expected address
        assert_eq!(result, "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR");
    }

    #[test]
    fn test_muxed_account_muxed_ed25519_to_string() {
        let uint256 = create_test_uint256();
        let med = MuxedAccountMed25519 {
            id: 12345,
            ed25519: uint256,
        };
        let muxed = MuxedAccount::MuxedEd25519(med);
        let result = muxed_account_to_string(&muxed);

        // Should extract the underlying ed25519 key
        assert!(result.starts_with('G'));
        assert_eq!(result.len(), 56);
        assert_eq!(result, "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR");
    }

    #[test]
    fn test_account_id_to_string() {
        let uint256 = create_test_uint256();
        let public_key = PublicKey::PublicKeyTypeEd25519(uint256);
        let account_id = AccountId(public_key);
        let result = account_id_to_string(&account_id);

        assert!(result.starts_with('G'));
        assert_eq!(result.len(), 56);
        assert_eq!(result, "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR");
    }

    #[test]
    fn test_different_addresses() {
        // Test with a different known address
        let uint256_2 = Uint256([
            0x3f, 0x0c, 0x34, 0xbf, 0x93, 0xad, 0x0d, 0x99,
            0x71, 0xd0, 0x4c, 0xcc, 0x90, 0xf7, 0x05, 0x51,
            0x1c, 0x83, 0x8a, 0x2f, 0x59, 0xa3, 0x8a, 0xf5,
            0x63, 0x98, 0x62, 0xf3, 0xfc, 0xce, 0x55, 0x3d,
        ]);

        let muxed = MuxedAccount::Ed25519(uint256_2);
        let result = muxed_account_to_string(&muxed);

        assert!(result.starts_with('G'));
        assert_eq!(result.len(), 56);
        // This should be a different address from the test one
        assert_ne!(result, "GB67AEDGTIDPDMJEX7KYVAUJAS7EV6ITRGZM3GBW2RLT5DBPDYHVSVCR");
    }

    #[test]
    fn test_address_format_consistency() {
        // Ensure the same Uint256 always produces the same address
        let uint256 = create_test_uint256();

        let muxed1 = MuxedAccount::Ed25519(uint256);
        let result1 = muxed_account_to_string(&muxed1);

        let uint256_2 = create_test_uint256();
        let muxed2 = MuxedAccount::Ed25519(uint256_2);
        let result2 = muxed_account_to_string(&muxed2);

        assert_eq!(result1, result2);
    }

    #[test]
    fn test_muxed_and_account_id_consistency() {
        // Same Uint256 should produce same address whether from MuxedAccount or AccountId
        let uint256 = create_test_uint256();

        let muxed = MuxedAccount::Ed25519(uint256);
        let muxed_result = muxed_account_to_string(&muxed);

        let uint256_2 = create_test_uint256();
        let public_key = PublicKey::PublicKeyTypeEd25519(uint256_2);
        let account_id = AccountId(public_key);
        let account_result = account_id_to_string(&account_id);

        assert_eq!(muxed_result, account_result);
    }
}
