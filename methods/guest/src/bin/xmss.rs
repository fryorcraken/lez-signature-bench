use lez_signature_bench_programs::{VerifyInput, verifier};
use nssa_core::program::{AccountPostState, ProgramInput, ProgramOutput, read_nssa_inputs};

fn main() {
    let (
        ProgramInput {
            self_program_id,
            caller_program_id,
            pre_states,
            instruction,
        },
        instruction_data,
    ) = read_nssa_inputs::<VerifyInput>();
    verifier::xmss::verify_all(&instruction).expect("verify failed");
    let post_states: Vec<AccountPostState> = pre_states
        .iter()
        .map(|awm| AccountPostState::new(awm.account.clone()))
        .collect();
    ProgramOutput::new(
        self_program_id,
        caller_program_id,
        instruction_data,
        pre_states,
        post_states,
    )
    .write();
}
