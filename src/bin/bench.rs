//! Bench harness for signature schemes inside an NSSA-wrapped LEZ guest.
//!
//! Three modes:
//!
//! * **Local prove** (default): proves locally via
//!   `risc0_zkvm::default_prover()`. Records cycles, prove time, receipt size.
//! * **Public execution** (`--public`): runs the guest in
//!   `risc0_zkvm::default_executor()` without proving, which is what the
//!   sequencer does for a public transaction (`nssa::Program::execute` at
//!   v0.2.0-rc3), under the same 32M user-cycle cap. Records cycles and
//!   execution time.
//! * **End-to-end private TX** (`--account-id <id>`): submits a real
//!   privacy-preserving transaction via `wallet::WalletCore::send_privacy_preserving_tx`
//!   against a running localnet. Records wall-clock end-to-end time.
//!
//! `--all` runs the full (scheme × N) matrix plus the noop calibration
//! baseline. Output goes to `results/results.json` and
//! `results/README-snippet.md`.
//!
//! Requires `RISC0_DEV_MODE=0` for real numbers.

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use lez_signature_bench::{Scheme, VerifyInput, make_test_vector};
use lez_signature_bench_methods::{
    ECDSA_P256_ELF, ECDSA_SECP256K1_ELF, ED25519_ELF, LMS_ELF, NOOP_ELF, SCHNORR_SECP256K1_ELF,
    XMSS_ELF,
};
use nssa::{AccountId, program::Program};
use risc0_zkvm::{ExecutorEnv, default_executor, default_prover};
use serde::{Deserialize, Serialize};
use wallet::{PrivacyPreservingAccount, WalletCore};

#[derive(Parser, Debug)]
#[command(about = "Bench across signature schemes inside an NSSA-wrapped LEZ guest.")]
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
    /// proving locally. Requires a running localnet and `WalletCore::from_env()`.
    #[arg(long)]
    account_id: Option<String>,
    /// File suffix for output files in --all mode (e.g. "e2e" → results-e2e.json).
    #[arg(long)]
    label: Option<String>,
    /// Public execution: execute the guest without proving, as the
    /// sequencer does for a public transaction. Records cycles and
    /// execution time instead of prove time.
    #[arg(long, conflicts_with = "account_id")]
    public: bool,
}

/// `nssa::program::MAX_NUM_CYCLES_PUBLIC_EXECUTION` at v0.2.0-rc3: the
/// sequencer's session limit for one public program execution. The
/// executor counts it in user cycles.
const MAX_NUM_CYCLES_PUBLIC_EXECUTION: u64 = 1024 * 1024 * 32;

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
    /// E2E private-TX wall-clock (Some only in E2E mode).
    tx_e2e_seconds: Option<f64>,
}

fn elf_for(scheme: Scheme) -> &'static [u8] {
    match scheme {
        Scheme::EcdsaSecp256k1 => ECDSA_SECP256K1_ELF,
        Scheme::SchnorrSecp256k1 => SCHNORR_SECP256K1_ELF,
        Scheme::Ed25519 => ED25519_ELF,
        Scheme::EcdsaP256 => ECDSA_P256_ELF,
        // hbs-lms reads the LM-OTS / LMS type codes from the signature, so
        // the same guest verifies every LMS parameter set.
        Scheme::Lms | Scheme::LmsW1 => LMS_ELF,
        Scheme::Xmss => XMSS_ELF,
    }
}

fn build_local_env<'a>(input: &'a VerifyInput, session_limit: Option<u64>) -> ExecutorEnv<'a> {
    let self_program_id: [u32; 8] = [0; 8];
    let caller_program_id: Option<[u32; 8]> = None;
    let pre_states: Vec<nssa_core::account::AccountWithMetadata> = Vec::new();
    let instruction_data: Vec<u32> = risc0_zkvm::serde::to_vec(input).expect("encode VerifyInput");

    ExecutorEnv::builder()
        .write(&self_program_id)
        .unwrap()
        .write(&caller_program_id)
        .unwrap()
        .write(&pre_states)
        .unwrap()
        .write(&instruction_data)
        .unwrap()
        .session_limit(session_limit)
        .build()
        .unwrap()
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
        tx_e2e_seconds: None,
    }
}

fn run_public(label: &str, n: usize, elf: &'static [u8], input: &VerifyInput) -> Row {
    let env = build_local_env(input, Some(MAX_NUM_CYCLES_PUBLIC_EXECUTION));
    let t0 = Instant::now();
    let session = default_executor().execute(env, elf).expect("execute");
    let exec_seconds = t0.elapsed().as_secs_f64();
    Row {
        scheme: label.to_string(),
        n,
        // What a prover would pay: each segment padded to its po2.
        total_cycles: Some(session.segments.iter().map(|s| 1u64 << s.po2).sum()),
        user_cycles: Some(session.cycles()),
        // The executor's SessionInfo does not report paging cycles.
        paging_cycles: None,
        segments: Some(session.segments.len()),
        prove_seconds: None,
        receipt_bytes: None,
        exec_seconds: Some(exec_seconds),
        tx_e2e_seconds: None,
    }
}

async fn run_e2e(
    label: &str,
    n: usize,
    elf: &'static [u8],
    input: &VerifyInput,
    account_id: AccountId,
    wallet_core: &WalletCore,
) -> Row {
    let program = Program::new(elf.to_vec()).expect("parse program");
    let serialized = Program::serialize_instruction(input.clone()).expect("serialize VerifyInput");
    let accounts = vec![PrivacyPreservingAccount::PrivateOwned(account_id)];

    let t0 = Instant::now();
    wallet_core
        .send_privacy_preserving_tx(accounts, serialized, &program.into())
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
        tx_e2e_seconds: Some(elapsed),
    }
}

async fn run_row(
    mode: Mode,
    label: &str,
    n: usize,
    elf: &'static [u8],
    input: &VerifyInput,
    e2e: Option<(AccountId, &WalletCore)>,
) -> Row {
    if let Some((aid, wallet)) = e2e {
        return run_e2e(label, n, elf, input, aid, wallet).await;
    }
    match mode {
        Mode::Public => run_public(label, n, elf, input),
        _ => run_local(label, n, elf, input),
    }
}

fn print_row(r: &Row) {
    if let Some(s) = r.exec_seconds {
        let user = r.user_cycles.unwrap();
        println!(
            "{:<22} n={} cycles(user/padded)={}/{} segs={} exec={:.4}s cap={:.1}%",
            r.scheme,
            r.n,
            user,
            r.total_cycles.unwrap(),
            r.segments.unwrap(),
            s,
            100.0 * user as f64 / MAX_NUM_CYCLES_PUBLIC_EXECUTION as f64,
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
            "| scheme | N | user cycles | padded cycles | segments | exec time | share of 32M cap |\n",
        );
        md.push_str("|---|---:|---:|---:|---:|---:|---:|\n");
        for r in rows {
            let user = r.user_cycles.unwrap_or(0);
            md.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {:.1} ms | {:.1}% |\n",
                r.scheme,
                r.n,
                user,
                r.total_cycles.unwrap_or(0),
                r.segments.unwrap_or(0),
                1000.0 * r.exec_seconds.unwrap_or(0.0),
                100.0 * user as f64 / MAX_NUM_CYCLES_PUBLIC_EXECUTION as f64,
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

fn parse_account_id(s: &str) -> AccountId {
    s.strip_prefix("Private/")
        .or_else(|| s.strip_prefix("Public/"))
        .unwrap_or(s)
        .parse()
        .expect("parse account_id")
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
    let wallet_core = if mode == Mode::E2e {
        Some(WalletCore::from_env().expect("WalletCore from env (set NSSA_WALLET_HOME_DIR)"))
    } else {
        None
    };
    let account_id = cli.account_id.as_deref().map(parse_account_id);

    if cli.all {
        let mut rows: Vec<Row> = Vec::new();

        let baseline_input = make_test_vector(Scheme::EcdsaSecp256k1, 1);
        let row = run_row(
            mode,
            "noop",
            1,
            NOOP_ELF,
            &baseline_input,
            account_id.zip(wallet_core.as_ref()),
        )
        .await;
        print_row(&row);
        rows.push(row);

        for &scheme in Scheme::ALL {
            let elf = elf_for(scheme);
            for &n in &cli.ns {
                let input = make_test_vector(scheme, n);
                let row = run_row(
                    mode,
                    scheme.slug(),
                    n,
                    elf,
                    &input,
                    account_id.zip(wallet_core.as_ref()),
                )
                .await;
                print_row(&row);
                rows.push(row);
            }
        }

        write_outputs(&rows, &cli.out_dir, cli.label.as_deref(), mode);
    } else {
        let scheme_str = cli
            .scheme
            .as_deref()
            .expect("--scheme required without --all");
        let n = cli.n.expect("--n required without --all");

        let (label, elf, input) = if scheme_str == "noop" {
            (
                "noop".to_string(),
                NOOP_ELF,
                make_test_vector(Scheme::EcdsaSecp256k1, n),
            )
        } else {
            let scheme = Scheme::parse(scheme_str).expect("scheme");
            (
                scheme.slug().to_string(),
                elf_for(scheme),
                make_test_vector(scheme, n),
            )
        };

        let row = run_row(
            mode,
            &label,
            n,
            elf,
            &input,
            account_id.zip(wallet_core.as_ref()),
        )
        .await;
        print_row(&row);
    }
}
