use crate::{SignerVerification, VerifyInput};
use hbs_lms::{
    HssParameter, LmotsAlgorithm, LmsAlgorithm, Seed, Sha256_256, keygen, signature::SignerMut,
    verify,
};

// Single-tree HSS, SHA-256, H=5 (32 leaves). HSS_LEVELS=1 keeps the row a
// "plain LMS" data point; keygen is fast enough that the bench can mint a
// fresh key per signer per run. LMS is stateful — never reuse a signing key
// across runs in production.
//
// Two Winternitz parameters are benched:
// - W=8 (`lms`): 34 hash chains of up to 255 steps, smallest signature.
// - W=1 (`lms-w1`): 265 chains of 1 step, ~6.5× larger signature.
//
// The verifier is parameter-agnostic: the LM-OTS / LMS type codes travel in
// the signature and public key, so one guest verifies both.
fn sign_with(lmots: LmotsAlgorithm, seed: &[u8; 32], message: &[u8]) -> SignerVerification {
    let params = [HssParameter::<Sha256_256>::new(lmots, LmsAlgorithm::LmsH5)];
    let lms_seed: Seed<Sha256_256> = (*seed).into();
    let (mut signing_key, verifying_key) =
        keygen::<Sha256_256>(&params, &lms_seed, None).expect("hbs-lms keygen");
    let signature = signing_key.try_sign(message).expect("hbs-lms sign");
    SignerVerification {
        pubkey: verifying_key.as_slice().to_vec(),
        signature: signature.as_ref().to_vec(),
    }
}

/// LM-OTS W=8.
pub fn sign(seed: &[u8; 32], message: &[u8]) -> SignerVerification {
    sign_with(LmotsAlgorithm::LmotsW8, seed, message)
}

/// LM-OTS W=1.
pub fn sign_w1(seed: &[u8; 32], message: &[u8]) -> SignerVerification {
    sign_with(LmotsAlgorithm::LmotsW1, seed, message)
}

pub fn verify_all(input: &VerifyInput) -> Result<(), String> {
    for s in &input.signers {
        verify::<Sha256_256>(&input.message, &s.signature, &s.pubkey)
            .map_err(|e| format!("verify: {e}"))?;
    }
    Ok(())
}
