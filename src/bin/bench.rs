//! Bench harness for signature schemes inside a LEZ actor-model guest.
//!
//! Targets LEZ `dev` (the v0.4 actor model): each guest is an
//! `actor_entrypoint!` handler that receives one borsh-encoded `VerifyInput`
//! message and returns `Response::keep_state()`.
//!
//! Three modes:
//!
//! * **Local prove** (default): proves locally via
//!   `risc0_zkvm::default_prover()`, feeding the guest the same borsh-framed
//!   `ReceiveInput` the LEZ engine writes. Records cycles, prove time,
//!   receipt size.
//! * **Public execution** (`--public`): runs the guest in
//!   `risc0_zkvm::default_executor()` without proving, which is what the
//!   sequencer does for a public transaction on LEZ `dev` (`lee`'s
//!   `Program::handle_message` → `execute_session`), with the same framed
//!   input. The session limit is the
//!   largest execution-gas limit a public transaction may declare
//!   (`MAX_GAS_EXEC`, 10M user cycles). Records cycles and execution time.
//! * **End-to-end private TX** (`--account-id <id> --payer <id>`, needs the
//!   `e2e` feature): deploys each guest through the program loader (fee paid
//!   by `--payer`), then submits a real privacy-preserving transaction via
//!   `wallet::WalletCore::send_privacy_preserving_tx` against a running
//!   sequencer. Records wall-clock end-to-end time of the transaction only
//!   (the deploy is not timed).
//!
//! `--all` runs the full (scheme × N) matrix plus the noop calibration
//! baseline. Output goes to `results/results.json` and
//! `results/README-snippet.md`.
//!
//! Requires `RISC0_DEV_MODE=0` for real numbers.

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use lez_signature_bench::{Scheme, VerifyInput, guest_input_frame, make_test_vector};
use lez_signature_bench_methods::{
    ECDSA_P256_ELF, ECDSA_SECP256K1_ELF, ED25519_ELF, LMS_ELF, NOOP_ELF, SCHNORR_SECP256K1_ELF,
    XMSS_ELF,
};
use risc0_zkvm::{ExecutorEnv, default_executor, default_prover};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(about = "Bench across signature schemes inside a LEZ actor-model guest.")]
struct Cli {
    /// Scheme slug (ecdsa-secp256k1 | schnorr-secp256k1 | ed25519 | ecdsa-p256 | lms | lms-w1 | xmss | noop). Ignored with --all.
    #[arg(long)]
    scheme: Option<String>,
    /// Number of signers in the synthetic fixture. Ignored with --all.
    #[arg(long)]
    n: Option<usize>,
    /// Run the full (scheme × N) matrix + noop baseline.
    #[arg(long)]
    all: bool,
    /// N values to sweep when --all is set.
    #[arg(long, value_delimiter = ',', default_values_t = [1usize, 3])]
    ns: Vec<usize>,
    /// Output directory for results.json + README snippet.
    #[arg(long, default_value = "results")]
    out_dir: PathBuf,
    /// PrivateOwned account id (e.g. `Private/...`) for end-to-end TX mode.
    /// When set, the bench submits real privacy-preserving TXs instead of
    /// proving locally. Requires a running sequencer and `WalletCore::from_env()`.
    #[arg(long)]
    account_id: Option<String>,
    /// Funded public account (e.g. `Public/...`) that pays the program-loader
    /// deploy fees in end-to-end mode.
    #[arg(long)]
    payer: Option<String>,
    /// File suffix for output files in --all mode (e.g. "e2e" → results-e2e.json).
    #[arg(long)]
    label: Option<String>,
    /// Public execution: execute the guest without proving, as the
    /// sequencer does for a public transaction. Records cycles and
    /// execution time instead of prove time.
    #[arg(long, conflicts_with = "account_id")]
    public: bool,
    /// With `--scheme noop`: the scheme whose fixture (keys and signatures)
    /// the noop guest receives. The noop guest decodes the message but
    /// skips verification, so this isolates the cost of reading that
    /// scheme's keys and signatures.
    #[arg(long, default_value = "ecdsa-secp256k1")]
    noop_fixture: String,
}

/// `fee_core::market::MAX_GAS_EXEC` on LEZ `dev`: the largest execution-gas
/// limit a public transaction may declare (validity rejects more), which is
/// also the per-block execution-gas cap. Execution gas is user cycles.
const MAX_GAS_EXEC: u64 = 10_000_000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Mode {
    Prove,
    Public,
    E2e,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Row {
    scheme: String,
    n: usize,
    /// Local-prove fields (None in E2E mode).
    total_cycles: Option<u64>,
    user_cycles: Option<u64>,
    paging_cycles: Option<u64>,
    segments: Option<usize>,
    prove_seconds: Option<f64>,
    receipt_bytes: Option<usize>,
    /// Execute-only wall-clock (Some only in public mode).
    #[serde(default)]
    exec_seconds: Option<f64>,
    /// Public mode: the execution ran past `MAX_GAS_EXEC`, so the sequencer
    /// would revert it out of gas. Cycles and time are then from a re-run
    /// without the cap.
    #[serde(default)]
    over_gas_cap: Option<bool>,
    /// E2E private-TX wall-clock (Some only in E2E mode).
    tx_e2e_seconds: Option<f64>,
}

fn elf_for(scheme: Scheme) -> &'static [u8] {
    match scheme {
        Scheme::EcdsaSecp256k1 => ECDSA_SECP256K1_ELF,
        Scheme::SchnorrSecp256k1 => SCHNORR_SECP256K1_ELF,
        Scheme::Ed25519 => ED25519_ELF,
        Scheme::EcdsaP256 => ECDSA_P256_ELF,
        // One guest for both W values: the LM-OTS type code is in the signature.
        Scheme::Lms | Scheme::LmsW1 => LMS_ELF,
        Scheme::Xmss => XMSS_ELF,
    }
}

fn build_local_env(input: &VerifyInput, session_limit: Option<u64>) -> ExecutorEnv<'static> {
    ExecutorEnv::builder()
        .write_slice(&guest_input_frame(input))
        .session_limit(session_limit)
        .build()
        .expect("build ExecutorEnv")
}

fn run_local(label: &str, n: usize, elf: &'static [u8], input: &VerifyInput) -> Row {
    let env = build_local_env(input, None);
    let t0 = Instant::now();
    let prove_info = default_prover().prove(env, elf).expect("prove");
    let prove_seconds = t0.elapsed().as_secs_f64();
    let receipt_bytes = bincode::serialize(&prove_info.receipt)
        .expect("receipt bincode")
        .len();
    let stats = &prove_info.stats;
    Row {
        scheme: label.to_string(),
        n,
        total_cycles: Some(stats.total_cycles),
        user_cycles: Some(stats.user_cycles),
        paging_cycles: Some(stats.paging_cycles),
        segments: Some(stats.segments),
        prove_seconds: Some(prove_seconds),
        receipt_bytes: Some(receipt_bytes),
        exec_seconds: None,
        over_gas_cap: None,
        tx_e2e_seconds: None,
    }
}

/// Public execution: the executor alone, under the public-transaction gas
/// cap. `SessionInfo::cycles()` is the user-cycle count, which is what the
/// sequencer charges as execution gas; the padded total is the po2-rounded
/// segment sum a prover would pay for. The executor reports no paging
/// split and there is no receipt.
///
/// A run that hits the cap is what the sequencer reverts as out of gas.
/// The row is then re-run without the cap, so it still reports what the
/// verification would need, and is flagged `over_gas_cap`.
fn run_public(label: &str, n: usize, elf: &'static [u8], input: &VerifyInput) -> Row {
    let execute = |limit| {
        let env = build_local_env(input, limit);
        let t0 = Instant::now();
        default_executor()
            .execute(env, elf)
            .map(|session| (session, t0.elapsed().as_secs_f64()))
    };
    let (session, exec_seconds, over_gas_cap) = match execute(Some(MAX_GAS_EXEC)) {
        Ok((session, secs)) => (session, secs, false),
        // Same test as LEZ's `Program::execute_session`: the root cause of
        // the executor's session-limit bail, not a guest panic message.
        Err(e)
            if e.root_cause()
                .to_string()
                .starts_with("Session limit exceeded") =>
        {
            let (session, secs) = execute(None).expect("execute without the gas cap");
            (session, secs, true)
        }
        Err(e) => panic!("execute: {e:#}"),
    };
    Row {
        scheme: label.to_string(),
        n,
        total_cycles: Some(session.segments.iter().map(|s| 1u64 << s.po2).sum()),
        user_cycles: Some(session.cycles()),
        paging_cycles: None,
        segments: Some(session.segments.len()),
        prove_seconds: None,
        receipt_bytes: None,
        exec_seconds: Some(exec_seconds),
        over_gas_cap: Some(over_gas_cap),
        tx_e2e_seconds: None,
    }
}

#[cfg(feature = "e2e")]
mod e2e {
    use std::time::Instant;

    use lee::{
        AccountId, privacy_preserving_transaction::circuit::ProgramCatalog, program::Program,
    };
    use lez_signature_bench::VerifyInput;
    use program_loader_core::MAX_SEGMENT_DATA_LEN;
    use wallet::{
        AccountIdentity, WalletCore, WalletSequencerClient,
        program_facades::program_loader::ProgramLoader,
    };

    use super::Row;

    pub struct Deployed {
        pub program_account_id: AccountId,
        pub programs: ProgramCatalog,
    }

    pub fn parse_account_id(s: &str) -> AccountId {
        s.strip_prefix("Private/")
            .or_else(|| s.strip_prefix("Public/"))
            .unwrap_or(s)
            .parse()
            .expect("parse account id")
    }

    /// Deploys `elf` through `program_loader` into fresh header + segment
    /// accounts, paid by `payer`. Mirrors LEZ's
    /// `examples/program_deployment/src/lib.rs::deploy_program`.
    pub async fn deploy<C: WalletSequencerClient>(
        wallet: &mut WalletCore<C>,
        elf: &'static [u8],
        payer: AccountId,
    ) -> Deployed {
        let bytecode = elf.to_vec();
        let segment_count = risc0_binfmt::ProgramBinary::decode(&bytecode)
            .expect("program binary")
            .user_elf
            .len()
            .div_ceil(MAX_SEGMENT_DATA_LEN);
        let header = wallet.create_new_account_public(None).0;
        let segments: Vec<AccountId> =
            std::iter::repeat_with(|| wallet.create_new_account_public(None).0)
                .take(segment_count)
                .collect();
        wallet.store_persistent_data().expect("store wallet");
        let program_account_id = ProgramLoader(&*wallet)
            .deploy(header, &segments, bytecode, true, Some(payer))
            .await
            .expect("program-loader deploy");
        let program = Program::new(elf.into()).expect("parse program");
        Deployed {
            program_account_id,
            programs: ProgramCatalog::from_iter([(program_account_id, program)]),
        }
    }

    pub async fn run<C: WalletSequencerClient>(
        label: &str,
        n: usize,
        deployed: &Deployed,
        input: &VerifyInput,
        account_id: AccountId,
        wallet: &WalletCore<C>,
    ) -> Row {
        let message = Program::serialize_message(input.clone()).expect("serialize VerifyInput");
        let account = AccountIdentity::PrivateOwned(account_id)
            .select_program_actor_state(deployed.program_account_id);

        let t0 = Instant::now();
        wallet
            .send_privacy_preserving_tx(vec![account], 0, message, &deployed.programs)
            .await
            .expect("send_privacy_preserving_tx");
        let elapsed = t0.elapsed().as_secs_f64();

        Row {
            scheme: label.to_string(),
            n,
            total_cycles: None,
            user_cycles: None,
            paging_cycles: None,
            segments: None,
            prove_seconds: None,
            receipt_bytes: None,
            exec_seconds: None,
            over_gas_cap: None,
            tx_e2e_seconds: Some(elapsed),
        }
    }
}

fn print_row(r: &Row) {
    if let Some(s) = r.exec_seconds {
        let user = r.user_cycles.unwrap();
        println!(
            "{:<22} n={} cycles(user/padded)={}/{} segs={} exec={:.4}s gas_cap={:.1}%{}",
            r.scheme,
            r.n,
            user,
            r.total_cycles.unwrap(),
            r.segments.unwrap(),
            s,
            100.0 * user as f64 / MAX_GAS_EXEC as f64,
            if r.over_gas_cap == Some(true) {
                " OVER CAP (out of gas on chain)"
            } else {
                ""
            },
        );
    } else if let Some(s) = r.tx_e2e_seconds {
        println!(
            "{:<22} n={} tx_e2e={:.2}s (~{}:{:02})",
            r.scheme,
            r.n,
            s,
            (s / 60.0) as u64,
            (s % 60.0) as u64
        );
    } else {
        println!(
            "{:<22} n={} cycles(total/user/paging)={}/{}/{} segs={} prove={:.2}s receipt={}B",
            r.scheme,
            r.n,
            r.total_cycles.unwrap(),
            r.user_cycles.unwrap(),
            r.paging_cycles.unwrap(),
            r.segments.unwrap(),
            r.prove_seconds.unwrap(),
            r.receipt_bytes.unwrap(),
        );
    }
}

fn write_outputs(rows: &[Row], out_dir: &PathBuf, label: Option<&str>, mode: Mode) {
    std::fs::create_dir_all(out_dir).expect("create out_dir");
    let suffix = label.map(|s| format!("-{}", s)).unwrap_or_default();

    let json = serde_json::to_string_pretty(rows).expect("serialize");
    std::fs::write(out_dir.join(format!("results{}.json", suffix)), json).expect("write json");

    let mut md = String::new();
    if mode == Mode::Public {
        md.push_str(
            "| scheme | N | user cycles (gas) | padded cycles | segments | exec time | share of 10M gas cap |\n",
        );
        md.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
        for r in rows {
            let user = r.user_cycles.unwrap_or(0);
            md.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {:.1} ms | {:.1}%{} |\n",
                r.scheme,
                r.n,
                user,
                r.total_cycles.unwrap_or(0),
                r.segments.unwrap_or(0),
                1000.0 * r.exec_seconds.unwrap_or(0.0),
                100.0 * user as f64 / MAX_GAS_EXEC as f64,
                if r.over_gas_cap == Some(true) {
                    " (over cap)"
                } else {
                    ""
                },
            ));
        }
    } else if mode == Mode::E2e {
        md.push_str("| scheme | N | end-to-end TX time |\n");
        md.push_str("|---|---:|---:|\n");
        for r in rows {
            let s = r.tx_e2e_seconds.unwrap_or(0.0);
            md.push_str(&format!(
                "| `{}` | {} | {:.2} s (~{}:{:02}) |\n",
                r.scheme,
                r.n,
                s,
                (s / 60.0) as u64,
                (s % 60.0) as u64,
            ));
        }
    } else {
        md.push_str("| scheme | N | total cycles | user cycles | segments | prove time | receipt size (B) |\n");
        md.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
        for r in rows {
            let s = r.prove_seconds.unwrap_or(0.0);
            md.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {:.2} s (~{}:{:02}) | {} |\n",
                r.scheme,
                r.n,
                r.total_cycles.unwrap_or(0),
                r.user_cycles.unwrap_or(0),
                r.segments.unwrap_or(0),
                s,
                (s / 60.0) as u64,
                (s % 60.0) as u64,
                r.receipt_bytes.unwrap_or(0),
            ));
        }
    }
    std::fs::write(out_dir.join(format!("README-snippet{}.md", suffix)), md)
        .expect("write snippet");
    println!(
        "\nwrote {}/results{}.json and README-snippet{}.md",
        out_dir.display(),
        suffix,
        suffix,
    );
}

/// The (label, n, ELF, fixture) points to run: one, or the full matrix.
fn cases(cli: &Cli) -> Vec<(String, usize, &'static [u8], VerifyInput)> {
    if cli.all {
        let mut cases = vec![(
            "noop".to_string(),
            1,
            NOOP_ELF,
            make_test_vector(Scheme::EcdsaSecp256k1, 1),
        )];
        for &scheme in Scheme::ALL {
            for &n in &cli.ns {
                cases.push((
                    scheme.slug().to_string(),
                    n,
                    elf_for(scheme),
                    make_test_vector(scheme, n),
                ));
            }
        }
        return cases;
    }
    let scheme_str = cli
        .scheme
        .as_deref()
        .expect("--scheme required without --all");
    let n = cli.n.expect("--n required without --all");
    if scheme_str == "noop" {
        let fixture = Scheme::parse(&cli.noop_fixture).expect("noop fixture");
        let label = if fixture == Scheme::EcdsaSecp256k1 {
            "noop".to_string()
        } else {
            format!("noop[{}]", fixture.slug())
        };
        vec![(label, n, NOOP_ELF, make_test_vector(fixture, n))]
    } else {
        let scheme = Scheme::parse(scheme_str).expect("scheme");
        vec![(
            scheme.slug().to_string(),
            n,
            elf_for(scheme),
            make_test_vector(scheme, n),
        )]
    }
}

#[cfg(feature = "e2e")]
async fn run_e2e_cases(
    cli: &Cli,
    cases: &[(String, usize, &'static [u8], VerifyInput)],
) -> Vec<Row> {
    let account_id = e2e::parse_account_id(cli.account_id.as_deref().expect("--account-id"));
    let payer = e2e::parse_account_id(
        cli.payer
            .as_deref()
            .expect("--payer <funded public account> required with --account-id"),
    );
    let mut wallet =
        wallet::WalletCore::from_env().expect("WalletCore from env (set LEE_WALLET_HOME_DIR)");

    // Deploy each distinct guest once, before any timed transaction.
    let mut deployed: Vec<(&'static [u8], e2e::Deployed)> = Vec::new();
    for (label, _, elf, _) in cases {
        if !deployed.iter().any(|(e, _)| std::ptr::eq(*e, *elf)) {
            eprintln!("deploying {label} …");
            deployed.push((elf, e2e::deploy(&mut wallet, elf, payer).await));
        }
    }

    let mut rows = Vec::new();
    for (label, n, elf, input) in cases {
        let (_, d) = deployed
            .iter()
            .find(|(e, _)| std::ptr::eq(*e, *elf))
            .expect("deployed");
        let row = e2e::run(label, *n, d, input, account_id, &wallet).await;
        print_row(&row);
        rows.push(row);
    }
    rows
}

#[cfg(not(feature = "e2e"))]
async fn run_e2e_cases(
    _cli: &Cli,
    _cases: &[(String, usize, &'static [u8], VerifyInput)],
) -> Vec<Row> {
    panic!("--account-id needs the `e2e` feature (build without --no-default-features)");
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if std::env::var("RISC0_DEV_MODE").as_deref() == Ok("1") {
        eprintln!("warning: RISC0_DEV_MODE=1 — numbers below are NOT real measurements.");
    }

    let mode = if cli.account_id.is_some() {
        Mode::E2e
    } else if cli.public {
        Mode::Public
    } else {
        Mode::Prove
    };
    let cases = cases(&cli);
    let rows = if mode == Mode::E2e {
        run_e2e_cases(&cli, &cases).await
    } else {
        cases
            .iter()
            .map(|(label, n, elf, input)| {
                let row = if mode == Mode::Public {
                    run_public(label, *n, elf, input)
                } else {
                    run_local(label, *n, elf, input)
                };
                print_row(&row);
                row
            })
            .collect()
    };

    if cli.all {
        write_outputs(&rows, &cli.out_dir, cli.label.as_deref(), mode);
    }
}
