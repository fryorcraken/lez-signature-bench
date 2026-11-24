//! Actor-model guest: verifies every signer in the message, keeps its state.
use lee_core::program::{ReceiveInput, Response};
use lez_signature_bench_programs::{VerifyInput, verifier};

lee_core::actor_entrypoint!(handle_message);

fn handle_message(_input: &ReceiveInput, message: VerifyInput) -> Response {
    verifier::xmss::verify_all(&message).expect("verify failed");
    Response::keep_state()
}
