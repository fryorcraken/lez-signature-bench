//! Bench harness for signature schemes inside a LEZ actor-model guest.
//!
//! Targets LEZ `dev` (the v0.4 actor model): each guest is an
//! `actor_entrypoint!` handler that receives one borsh-encoded `VerifyInput`
//! message and returns `Response::keep_state()`.
//!
//! Two modes:
//!
//! * **Local prove** (default): proves locally via
//!   `risc0_zkvm::default_prover()`, feeding the guest the same borsh-framed
//!   `ReceiveInput` the LEZ engine writes. Records cycles, prove time,
//!   receipt size.
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
};
use risc0_zkvm::{ExecutorEnv, default_prover};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(about = "Bench across signature schemes inside a LEZ actor-model guest.")]
struct Cli {
    /// Scheme slug (ecdsa-secp256k1 | schnorr-secp256k1 | ed25519 | ecdsa-p256 | lms | noop). Ignored with --all.
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
    /// E2E private-TX wall-clock (Some only in E2E mode).
    tx_e2e_seconds: Option<f64>,
}

fn elf_for(scheme: Scheme) -> &'static [u8] {
    match scheme {
        Scheme::EcdsaSecp256k1 => ECDSA_SECP256K1_ELF,
        Scheme::SchnorrSecp256k1 => SCHNORR_SECP256K1_ELF,
        Scheme::Ed25519 => ED25519_ELF,
        Scheme::EcdsaP256 => ECDSA_P256_ELF,
        Scheme::Lms => LMS_ELF,
    }
}

fn build_local_env(input: &VerifyInput) -> ExecutorEnv<'static> {
    ExecutorEnv::builder()
        .write_slice(&guest_input_frame(input))
        .build()
        .expect("build ExecutorEnv")
}

fn run_local(label: &str, n: usize, elf: &'static [u8], input: &VerifyInput) -> Row {
    let env = build_local_env(input);
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
            tx_e2e_seconds: Some(elapsed),
        }
    }
}

fn print_row(r: &Row) {
    if let Some(s) = r.tx_e2e_seconds {
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

fn write_outputs(rows: &[Row], out_dir: &PathBuf, label: Option<&str>, e2e: bool) {
    std::fs::create_dir_all(out_dir).expect("create out_dir");
    let suffix = label.map(|s| format!("-{}", s)).unwrap_or_default();

    let json = serde_json::to_string_pretty(rows).expect("serialize");
    std::fs::write(out_dir.join(format!("results{}.json", suffix)), json).expect("write json");

    let mut md = String::new();
    if e2e {
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
        vec![(
            "noop".to_string(),
            n,
            NOOP_ELF,
            make_test_vector(Scheme::EcdsaSecp256k1, n),
        )]
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

    let e2e = cli.account_id.is_some();
    let cases = cases(&cli);
    let rows = if e2e {
        run_e2e_cases(&cli, &cases).await
    } else {
        cases
            .iter()
            .map(|(label, n, elf, input)| {
                let row = run_local(label, *n, elf, input);
                print_row(&row);
                row
            })
            .collect()
    };

    if cli.all {
        write_outputs(&rows, &cli.out_dir, cli.label.as_deref(), e2e);
    }
}
