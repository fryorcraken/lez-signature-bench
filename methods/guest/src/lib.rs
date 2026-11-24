//! Shared types + per-scheme verifier modules for guest binaries.
//!
//! Each scheme has a `verify_all(&VerifyInput) -> Result<(), &'static str>`
//! function. Guest binaries are thin shells that call the matching one.
//!
//! Wire format mirrors the host crate's `lez_signature_bench::VerifyInput`.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SignerVerification {
    pub pubkey: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VerifyInput {
    pub message: Vec<u8>,
    pub signers: Vec<SignerVerification>,
}

pub mod verifier {
    pub mod ecdsa_k256 {
        use super::super::VerifyInput;
        use alloc::string::{String, ToString};
        use k256::ecdsa::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier};
        use tiny_keccak::{Hasher, Keccak};

        fn keccak256(bytes: &[u8]) -> [u8; 32] {
            let mut h = Keccak::v256();
            let mut out = [0u8; 32];
            h.update(bytes);
            h.finalize(&mut out);
            out
        }

        pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
            let digest = keccak256(&input.message);
            for s in &input.signers {
                let vk =
                    VerifyingKey::from_sec1_bytes(&s.pubkey).map_err(|_| "pubkey".to_string())?;
                let sig = Signature::from_slice(&s.signature).map_err(|_| "sig".to_string())?;
                vk.verify_prehash(&digest, &sig)
                    .map_err(|_| "verify".to_string())?;
            }
            Ok(())
        }
    }

    pub mod schnorr_k256 {
        use super::super::VerifyInput;
        use alloc::string::{String, ToString};
        use k256::schnorr::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier};
        use sha2::{Digest, Sha256};

        pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
            let digest: [u8; 32] = Sha256::digest(&input.message).into();
            for s in &input.signers {
                let vk = VerifyingKey::from_bytes(&s.pubkey).map_err(|_| "pubkey".to_string())?;
                let sig =
                    Signature::try_from(s.signature.as_slice()).map_err(|_| "sig".to_string())?;
                vk.verify_prehash(&digest, &sig)
                    .map_err(|_| "verify".to_string())?;
            }
            Ok(())
        }
    }

    pub mod ed25519 {
        use super::super::VerifyInput;
        use alloc::string::{String, ToString};
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
            for s in &input.signers {
                let pk_arr: [u8; 32] = s
                    .pubkey
                    .as_slice()
                    .try_into()
                    .map_err(|_| "pubkey-len".to_string())?;
                let vk = VerifyingKey::from_bytes(&pk_arr).map_err(|_| "pubkey".to_string())?;
                let sig = Signature::from_slice(&s.signature).map_err(|_| "sig".to_string())?;
                vk.verify(&input.message, &sig)
                    .map_err(|_| "verify".to_string())?;
            }
            Ok(())
        }
    }

    pub mod ecdsa_p256 {
        use super::super::VerifyInput;
        use alloc::string::{String, ToString};
        use p256::ecdsa::{Signature, VerifyingKey, signature::hazmat::PrehashVerifier};
        use sha2::{Digest, Sha256};

        pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
            let digest: [u8; 32] = Sha256::digest(&input.message).into();
            for s in &input.signers {
                let vk =
                    VerifyingKey::from_sec1_bytes(&s.pubkey).map_err(|_| "pubkey".to_string())?;
                let sig = Signature::from_slice(&s.signature).map_err(|_| "sig".to_string())?;
                vk.verify_prehash(&digest, &sig)
                    .map_err(|_| "verify".to_string())?;
            }
            Ok(())
        }
    }

    pub mod lms {
        use super::super::VerifyInput;
        use alloc::string::{String, ToString};
        use hbs_lms::{Sha256_256, verify};

        pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
            for s in &input.signers {
                verify::<Sha256_256>(&input.message, &s.signature, &s.pubkey)
                    .map_err(|_| "verify".to_string())?;
            }
            Ok(())
        }
    }

    /// XMSS (RFC 8391), XMSS-SHA2_10_256: h=10 (1024 leaves), WOTS+ w=16.
    pub mod xmss {
        use super::super::VerifyInput;
        use alloc::string::{String, ToString};
        use xmss::{DetachedSignature, VerifyingKey, XmssSha2_10_256};

        pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
            for s in &input.signers {
                let vk = VerifyingKey::<XmssSha2_10_256>::try_from(s.pubkey.as_slice())
                    .map_err(|e| ToString::to_string(&e))?;
                let sig = DetachedSignature::<XmssSha2_10_256>::try_from(s.signature.as_slice())
                    .map_err(|e| ToString::to_string(&e))?;
                vk.verify_detached(&sig, &input.message)
                    .map_err(|e| ToString::to_string(&e))?;
            }
            Ok(())
        }
    }
}
