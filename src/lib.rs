use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

pub mod verifier;

/// Signature schemes the bench compares.
///
/// Wire format: every fixture serializes to `VerifyInput`, whose `signers`
/// field is interpreted by the per-scheme guest binary picked at submission
/// time. The `Scheme` enum is host-side only — it tells the host which
/// guest ELF to embed and which signing path to use when building fixtures.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Scheme {
    EcdsaSecp256k1,
    SchnorrSecp256k1,
    Ed25519,
    EcdsaP256,
    Lms,
    LmsW1,
    Xmss,
}

impl Scheme {
    pub const ALL: &'static [Scheme] = &[
        Scheme::EcdsaSecp256k1,
        Scheme::SchnorrSecp256k1,
        Scheme::Ed25519,
        Scheme::EcdsaP256,
        Scheme::Lms,
        Scheme::LmsW1,
        Scheme::Xmss,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Scheme::EcdsaSecp256k1 => "ecdsa-secp256k1",
            Scheme::SchnorrSecp256k1 => "schnorr-secp256k1",
            Scheme::Ed25519 => "ed25519",
            Scheme::EcdsaP256 => "ecdsa-p256",
            Scheme::Lms => "lms",
            Scheme::LmsW1 => "lms-w1",
            Scheme::Xmss => "xmss",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        for scheme in Self::ALL {
            if scheme.slug() == s {
                return Ok(*scheme);
            }
        }
        Err(format!(
            "unknown scheme `{s}`; expected one of {:?}",
            Self::ALL.iter().map(|s| s.slug()).collect::<Vec<_>>()
        ))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignerVerification {
    /// Scheme-specific pubkey encoding.
    pub pubkey: Vec<u8>,
    /// Scheme-specific signature encoding.
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VerifyInput {
    pub message: Vec<u8>,
    pub signers: Vec<SignerVerification>,
}

pub const SEEDS: &[[u8; 32]] = &[
    [
        0x4c, 0x0a, 0xc8, 0x6f, 0x12, 0x4d, 0xa0, 0x91, 0xc7, 0x3e, 0xb8, 0x55, 0x29, 0x6e, 0xfb,
        0x10, 0x00, 0xc4, 0x4d, 0x68, 0xa9, 0xa3, 0x6e, 0x2d, 0x83, 0xb1, 0x55, 0x77, 0x91, 0x6e,
        0xab, 0xcd,
    ],
    [
        0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        0x77, 0x01,
    ],
    [
        0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f,
        0x90, 0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e,
        0x8f, 0x02,
    ],
];

pub const MESSAGE: &[u8] = b"hello redstone";

pub fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak::v256();
    let mut out = [0u8; 32];
    h.update(bytes);
    h.finalize(&mut out);
    out
}

/// Build a deterministic synthetic test vector for `scheme` with
/// `num_signers` signers, all signing `MESSAGE`.
pub fn make_test_vector(scheme: Scheme, num_signers: usize) -> VerifyInput {
    assert!(
        num_signers <= SEEDS.len(),
        "only {} seeds available",
        SEEDS.len()
    );

    let signers: Vec<SignerVerification> = SEEDS
        .iter()
        .take(num_signers)
        .map(|seed| match scheme {
            Scheme::EcdsaSecp256k1 => verifier::ecdsa_k256::sign(seed, MESSAGE),
            Scheme::SchnorrSecp256k1 => verifier::schnorr_k256::sign(seed, MESSAGE),
            Scheme::Ed25519 => verifier::ed25519::sign(seed, MESSAGE),
            Scheme::EcdsaP256 => verifier::ecdsa_p256::sign(seed, MESSAGE),
            Scheme::Lms => verifier::lms::sign(seed, MESSAGE),
            Scheme::LmsW1 => verifier::lms::sign_w1(seed, MESSAGE),
            Scheme::Xmss => verifier::xmss::sign(seed, MESSAGE),
        })
        .collect();

    VerifyInput {
        message: MESSAGE.to_vec(),
        signers,
    }
}

/// Host-callable verify, mirroring the per-scheme guest verifier.
pub fn verify(scheme: Scheme, input: &VerifyInput) -> Result<(), String> {
    match scheme {
        Scheme::EcdsaSecp256k1 => verifier::ecdsa_k256::verify_all(input),
        Scheme::SchnorrSecp256k1 => verifier::schnorr_k256::verify_all(input),
        Scheme::Ed25519 => verifier::ed25519::verify_all(input),
        Scheme::EcdsaP256 => verifier::ecdsa_p256::verify_all(input),
        Scheme::Lms => verifier::lms::verify_all(input),
        Scheme::LmsW1 => verifier::lms::verify_all(input),
        Scheme::Xmss => verifier::xmss::verify_all(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured wire-format (bincode) hex of `make_test_vector(EcdsaSecp256k1, 1)`
    /// at the commit that introduced the Scheme enum. Locks the wire format so a
    /// future refactor of the builder cannot silently change what the
    /// guest sees on the wire.
    const ECDSA_SECP256K1_N1_BINCODE_HEX: &str = "0e0000000000000068656c6c6f2072656473746f6e65010000000000000021000000000000000202e3c3d6475bd9820f1b6966a40950b4475dbf7c39bcfc38e7bf1eae4194c4c840000000000000003ad850b0ac28fda79d349cfa96204eb72161ebd102b89c6c523b60ecb6b5e96e0d87b364eab41d3f7a3aee55f5cefab80b0e36fee6502e9053b5dee8d5f99c07";

    /// LMS is deterministic (HSS keygen + signing are derived from the seed),
    /// so the wire format is locked too. Parameters: Sha256_256 / LmotsW8 / LmsH5.
    const LMS_N1_BINCODE_HEX: &str = "0e0000000000000068656c6c6f2072656473746f6e6501000000000000003c0000000000000000000001000000050000000464fc3062a44336c0424777e1ec759825dcc7011b4aa871fc80efcadc3ded530ca76d788e14d6827df7a87a7d3d2695251005000000000000000000000000000000000004e33b70653ff6b7c174eac09797186038be7eb830ff069bfc3d2666c5c6adb2b95f2fa2e5fcb74b0b717fa8fa1ecd23b8c9b50d5bdc02f6f4eac11d367af523c06b6cad1fd053855cbbf5bde76c9ee7618b412b97c291101fc72d6d3a43414b0de4a702b9dd19ed251c5dafc5de18485f78a33b632edd81f3082ef260e2925121f1197890f30a7e32cd8d11be26ecaa662d181b91c0cc8602ea173a0158ee736b42a9eb7a0bead46d60cd6493ce167c2dcbf6c140d145f9eaba1afb31a2d025af11c4ede76af5564c8c25e6b6ad2307004ebdca193ef49c4b5cd6e9f3537bf39dcda3b79c015f9b6b88cc7fca94aac9e0fc2622053c136a5810e7a69887fdca9b0a8fba3fe270c107452e996db5c21a740bf9148e32805a079fde5b8bdc6fa237c63e6e2e2557fd93e473b9f9315b371bb6a95bef9e907dfab359fe065193bce38b0c93a93fe8a07bf9e01c9db7ef6f773205e929fbe52eda2c76baf082a2d2a09c9351e72091c11a79cdca61118ce9cd7e6d5a6e5f640f0847c9ea7ccbad994fefc1483deb5a005a9e4334e2dec0fc7789dccc260609b702d0d760f8dd748902c5ff160520370fd7cda7cf299a3c25b38f19393fac93b55ac411a520f251602987d8a77c8ea9096b0f86875aa57928a17206642b2ba551d4fcbfb374f35cf63b004968ef3620591f415ac587cd29bb96fcd224a5def7cd31a8948bffaad341b763e60af2576f08ddeef21ff4689e5dcf401c2fdafce7146b9807ca3a068a9f4f3447e092e63140cb19852420a528052457be4f7b5f7dd84e0776274c540658d7e46e61f45c1c5da3f79241d38bbd50a2ac82f0814a9d7eae8dee6c86fe8e5594ad6cc6209138ddfc742bdbfc74ccf00e1518b439bd88aca58ded9032404c3b1454e69dd3f3da8ea46e738bf936d673e5e0732cd04836498b3135aa9075c39417520d867eabb034672a038a0b6131abd823fda734ffd4c9308b26e84b3cb891949cc110e99354195db29df1662aae28e93b26872d6f95379f95a7acf926152761647eb798514d36fe57f40dcb1a2c41be7c527e4b6ccb180c83064e0470355673857fb792c69db6cf9137f6843763fe7d0a979b2c602b2256bb1729798cd6597e0d4402ab44d0e192bc68378bf331da6127ec7c7da839c02199495743dad3a7eff44b840bce32fb32c7fd1bfb983b844ca4c3c87833b4a39ce2fd925b70d547427f134425d9b8f5c7b1555732f181a99a579725c2f4d53ee4e8317d85f45b12246b98f02aa4cf6eec88ca52e2af269a5a64ca5762a1917bb82778233261d4b322f2f511bc97094b17c94ff645810f33467df60b9bfa84ae5f3e22864d9c66374f48ead83607d6acb3d84fa8d5ae0d90aa53cd1d2746a2cf6b59ba6a56c1ec98b0a9a7ce66df5142729ff30e543c03f93a6f7b096c505d7a15623860016d77e44e2a74bd26c983234cd3d95a22ac0e0ab8055a3bb688378b03d0bceaba1adc512cf45adc15574ff3d9e4ff5e0fb08bb96e6f8861fc5a220c64baa1c7f6c24dc918a0d621d5d44cc9f1b1963995aa01433d81eb82beda0f1dbcad63bed8eb4f9ad40000000531fc2c0b383a54536eabc1f53dfb429f80825b1ffbbd74c8d895fd90dcf1ccf2a8dff7e1df3d364ec2cb23d4644f6edc9b9ed1a2a35c509adc05df7162cd5e144e8bc43c0d92380fe0251e1948d30e8925196890bcd187902f566baef910ed4c9dbe17ae1476f49224a9a76a2e1fcd2c5cc26f5684cb911ebef331b576a0758068a2eaacc7ec8f1cf2556ed9bc13896a76ff817448d88e17af41326d30a326a9";

    #[test]
    fn make_test_vector_ecdsa_secp256k1_n1_is_byte_stable() {
        let v = make_test_vector(Scheme::EcdsaSecp256k1, 1);
        let actual = hex::encode(bincode::serialize(&v).expect("bincode serialize"));
        assert_eq!(
            actual, ECDSA_SECP256K1_N1_BINCODE_HEX,
            "wire format for (ECDSA secp256k1, n=1) changed",
        );
    }

    #[test]
    fn make_test_vector_lms_n1_is_byte_stable() {
        let v = make_test_vector(Scheme::Lms, 1);
        let actual = hex::encode(bincode::serialize(&v).expect("bincode serialize"));
        assert_eq!(
            actual, LMS_N1_BINCODE_HEX,
            "wire format for (LMS, n=1) changed",
        );
    }

    fn round_trip(scheme: Scheme, n: usize) {
        let v = make_test_vector(scheme, n);
        verify(scheme, &v).unwrap_or_else(|e| panic!("{scheme:?} n={n} verify: {e}"));
    }

    fn flip_one_byte_fails(scheme: Scheme) {
        let mut v = make_test_vector(scheme, 1);
        v.signers[0].signature[0] ^= 0xff;
        assert!(
            verify(scheme, &v).is_err(),
            "{scheme:?}: one-byte sig flip should fail",
        );
    }

    #[test]
    fn round_trip_ecdsa_secp256k1() {
        round_trip(Scheme::EcdsaSecp256k1, 1);
        round_trip(Scheme::EcdsaSecp256k1, 3);
    }

    #[test]
    fn round_trip_schnorr_secp256k1() {
        round_trip(Scheme::SchnorrSecp256k1, 1);
        round_trip(Scheme::SchnorrSecp256k1, 3);
    }

    #[test]
    fn round_trip_ed25519() {
        round_trip(Scheme::Ed25519, 1);
        round_trip(Scheme::Ed25519, 3);
    }

    #[test]
    fn round_trip_ecdsa_p256() {
        round_trip(Scheme::EcdsaP256, 1);
        round_trip(Scheme::EcdsaP256, 3);
    }

    #[test]
    fn round_trip_lms() {
        round_trip(Scheme::Lms, 1);
        round_trip(Scheme::Lms, 3);
    }

    #[test]
    fn negative_ecdsa_secp256k1() {
        flip_one_byte_fails(Scheme::EcdsaSecp256k1);
    }

    #[test]
    fn negative_schnorr_secp256k1() {
        flip_one_byte_fails(Scheme::SchnorrSecp256k1);
    }

    #[test]
    fn negative_ed25519() {
        flip_one_byte_fails(Scheme::Ed25519);
    }

    #[test]
    fn negative_ecdsa_p256() {
        flip_one_byte_fails(Scheme::EcdsaP256);
    }

    #[test]
    fn round_trip_lms_w1() {
        round_trip(Scheme::LmsW1, 1);
        round_trip(Scheme::LmsW1, 3);
    }

    #[test]
    fn round_trip_xmss() {
        round_trip(Scheme::Xmss, 1);
        round_trip(Scheme::Xmss, 3);
    }

    /// Flip the last signature byte rather than byte 0: hbs-lms parses the
    /// signature header (HSS levels + LM-OTS/LMS type codes) before checking
    /// content and panics on a malformed type code, and byte 0 of an XMSS
    /// signature is the leaf index. The last byte is authentication-path
    /// data, so this exercises verification, not parsing.
    fn flip_last_byte_fails(scheme: Scheme) {
        let mut v = make_test_vector(scheme, 1);
        let last = v.signers[0].signature.len() - 1;
        v.signers[0].signature[last] ^= 0xff;
        assert!(
            verify(scheme, &v).is_err(),
            "{scheme:?}: one-byte sig flip should fail",
        );
    }

    #[test]
    fn negative_lms() {
        flip_last_byte_fails(Scheme::Lms);
    }

    #[test]
    fn negative_lms_w1() {
        flip_last_byte_fails(Scheme::LmsW1);
    }

    #[test]
    fn negative_xmss() {
        flip_last_byte_fails(Scheme::Xmss);
    }

    /// W=1 must really produce W=1 signatures: a W=8 fixture would silently
    /// make the `lms-w1` row a duplicate of `lms`.
    #[test]
    fn lms_w1_signature_is_larger_than_w8() {
        let w8 = make_test_vector(Scheme::Lms, 1).signers[0].signature.len();
        let w1 = make_test_vector(Scheme::LmsW1, 1).signers[0]
            .signature
            .len();
        assert!(w1 > 5 * w8, "W=1 sig {w1} B vs W=8 sig {w8} B");
    }
}
