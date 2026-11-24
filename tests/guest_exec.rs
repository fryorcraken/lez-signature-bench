//! Runs each guest ELF in the RISC Zero executor (no proving) with the same
//! framed `ReceiveInput` the LEZ engine writes, and checks the committed
//! `Transition`. Skips when the ELFs are stubs (`RISC0_SKIP_BUILD`).

use lee_core::program::Transition;
use lez_signature_bench::{
    Scheme, VerifyInput, guest_input_frame, make_test_vector, receive_input,
};
use lez_signature_bench_methods::{
    ECDSA_P256_ELF, ECDSA_SECP256K1_ELF, ED25519_ELF, LMS_ELF, NOOP_ELF, SCHNORR_SECP256K1_ELF,
    XMSS_ELF,
};
use risc0_zkvm::{ExecutorEnv, default_executor};

fn elf_for(scheme: Scheme) -> &'static [u8] {
    match scheme {
        Scheme::EcdsaSecp256k1 => ECDSA_SECP256K1_ELF,
        Scheme::SchnorrSecp256k1 => SCHNORR_SECP256K1_ELF,
        Scheme::Ed25519 => ED25519_ELF,
        Scheme::EcdsaP256 => ECDSA_P256_ELF,
        Scheme::Lms | Scheme::LmsW1 => LMS_ELF,
        Scheme::Xmss => XMSS_ELF,
    }
}

fn execute(elf: &[u8], input: &VerifyInput) -> anyhow::Result<Vec<u8>> {
    let env = ExecutorEnv::builder()
        .write_slice(&guest_input_frame(input))
        .build()?;
    Ok(default_executor().execute(env, elf)?.journal.bytes)
}

fn assert_keeps_state(elf: &[u8], input: &VerifyInput) {
    let journal = execute(elf, input).expect("guest execution");
    let payload = lee_core::from_frame(&journal).expect("framed journal");
    let transition: Transition = borsh::from_slice(payload).expect("Transition");
    assert_eq!(transition.input, receive_input(input));
    assert!(
        transition.response.post_state.is_none(),
        "guest must keep state"
    );
    assert!(transition.response.sync_messages.is_empty());
    assert!(transition.response.async_messages.is_empty());
}

fn stubbed() -> bool {
    NOOP_ELF.is_empty()
}

#[test]
fn noop_guest_accepts_engine_frame() {
    if stubbed() {
        return;
    }
    assert_keeps_state(NOOP_ELF, &make_test_vector(Scheme::EcdsaSecp256k1, 1));
}

#[test]
fn every_scheme_guest_verifies_and_keeps_state() {
    if stubbed() {
        return;
    }
    for &scheme in Scheme::ALL {
        assert_keeps_state(elf_for(scheme), &make_test_vector(scheme, 1));
    }
}

#[test]
fn every_scheme_guest_rejects_a_bad_signature() {
    if stubbed() {
        return;
    }
    for &scheme in Scheme::ALL {
        let mut input = make_test_vector(scheme, 1);
        // Last byte: for LMS this lands in the C value, past the header the
        // parser checks (see the host-side `negative_lms` test).
        let last = input.signers[0].signature.len() - 1;
        input.signers[0].signature[last] ^= 0xff;
        assert!(
            execute(elf_for(scheme), &input).is_err(),
            "{scheme:?}: guest accepted a corrupted signature",
        );
    }
}
