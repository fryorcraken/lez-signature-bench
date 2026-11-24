use crate::{SignerVerification, VerifyInput};
use sha2::{Digest, Sha256};
use xmss::{DetachedSignature, KeyPair, VerifyingKey, XmssSha2_10_256};

fn expand_seed(seed: &[u8; 32]) -> [u8; 96] {
    let mut out = [0u8; 96];
    out[..32].copy_from_slice(seed);
    let h1: [u8; 32] = Sha256::digest(seed).into();
    out[32..64].copy_from_slice(&h1);
    let h2: [u8; 32] = Sha256::digest(h1).into();
    out[64..96].copy_from_slice(&h2);
    out
}

pub fn sign(seed: &[u8; 32], message: &[u8]) -> SignerVerification {
    let seed96 = expand_seed(seed);
    let mut kp = KeyPair::<XmssSha2_10_256>::from_seed(&seed96).expect("xmss keygen");
    let sig = kp.signing_key().sign_detached(message).expect("xmss sign");
    SignerVerification {
        pubkey: kp.verifying_key().as_ref().to_vec(),
        signature: sig.as_ref().to_vec(),
    }
}

pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
    for s in &input.signers {
        let vk = VerifyingKey::<XmssSha2_10_256>::try_from(s.pubkey.as_slice())
            .map_err(|e| format!("xmss pubkey parse: {e}"))?;
        let sig = DetachedSignature::<XmssSha2_10_256>::try_from(s.signature.as_slice())
            .map_err(|e| format!("xmss sig parse: {e}"))?;
        vk.verify_detached(&sig, &input.message)
            .map_err(|e| format!("xmss verify: {e}"))?;
    }
    Ok(())
}
