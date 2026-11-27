//! Calibration baseline: same actor entrypoint and borsh decode of the
//! `VerifyInput` message as the scheme guests, but no crypto.
use lee_core::program::{ReceiveInput, Response};
use lez_signature_bench_programs::VerifyInput;

lee_core::actor_entrypoint!(handle_message);

fn handle_message(_input: &ReceiveInput, message: VerifyInput) -> Response {
    core::hint::black_box(&message);
    Response::keep_state()
}
