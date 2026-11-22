# SPEC — lez-signature-bench

Successor to the original `lez-ecdsa` PoC. The ECDSA verifier built in
that PoC is folded in as one row of a comparative benchmark across
multiple signature schemes inside a real LEZ private transaction.

## 1. Objective

Compare the cost of verifying common signature schemes inside a real
LEZ private transaction, so a developer choosing a verification
primitive for oracle / multi-sig / passkey workloads can predict
cycles, prove time, receipt size, and end-to-end TX latency on
consumer hardware.

**Schemes in scope:**

- secp256k1 ECDSA (current baseline, carried over from lez-ecdsa)
- secp256k1 Schnorr (BIP-340)
- Ed25519
- P-256 ECDSA
- LMS (RFC 8554, hash-based, post-quantum) — included as a
  "post-quantum cost on this stack" data point. Not RISC0-precompiled
  in the curve sense, but its inner SHA-256 calls dispatch to the SHA
  precompile via the patched `sha2` crate.

**Multiplicities:** N = 1 and N = 3 (3-of-5 RedStone-shaped, same
message across all signers).

**Deliverable:** a results table in README + a short "budget → scheme"
decision note. Not a production component; this is *measurement*.

**Target reader:** the project owner and any future contributor scoping
RedStone / multi-sig / passkey verification on LEZ. The bench output +
decision note are the primary artifacts.

## 2. Tech Stack

- LEZ `dev` @ `dcbd318f0` — the v0.4 actor model (`lee`, `lee_core`,
  `program_loader_core`, `wallet`). Originally written against
  v0.2.0-rc3 (`nssa`, `nssa_core`, `wallet`, `common`); see the README
  for the migration and the before/after numbers.
- `risc0-zkvm` 3.1.0 (was 3.0.5)
- Rust toolchain pinned to 1.98.1 via `rust-toolchain.toml` (was 1.92.0;
  LEZ `dev` pins 1.98.1)
- Patched crypto (all RISC0-accelerated; verified in Phase 1 spike):
  - `k256 v0.13.4-risczero.1` (`risc0/RustCrypto-elliptic-curves`) —
    secp256k1 ECDSA **and** Schnorr. Schnorr's `verify_raw` calls
    `ProjectivePoint::lincomb`, which on `target_os = "zkvm"` routes
    to `risc0_bigint2::ec` — the same accelerated path ECDSA uses.
  - `ed25519-dalek 2.1.1` from `risc0/curve25519-dalek` at tag
    `curve25519-4.1.3-risczero.0`. The fork ships an explicit
    `curve25519-dalek/src/backend/serial/risc0` backend; ed25519-dalek
    depends on curve25519-dalek via path inside the workspace, so
    verify benefits automatically.
  - `p256 v0.13.2-risczero.1` (`risc0/RustCrypto-elliptic-curves`) —
    NIST P-256 ECDSA, same fork as k256, same bigint2 acceleration.
  - `tiny-keccak v2.0.2-risczero.0`, `sha2 v0.10.9-risczero.0`,
    `crypto-bigint v0.5.5-risczero.0`
  - `hbs-lms 0.2.0-alpha.1` (Fraunhofer AISEC, pure-Rust, `no_std`).
    No RISC0 fork exists; acceleration comes solely from its `sha2`
    dependency hitting the SHA precompile via the workspace patch.
    Parameters used: `Sha256_256` / `LmotsW8` / `LmsH5` (single-tree HSS,
    32 leaves, smallest signature tradeoff).

## 3. Commands

| Command | Purpose |
|---|---|
| `cargo build --workspace --release` | Build host + all guest binaries. |
| `cargo run --release --bin gen_test_vectors -- --scheme <s> --n <n>` | Produce a deterministic synthetic fixture for one (scheme, N) point. |
| `cargo run --release --bin bench -- --scheme <s> --n <n> --account-id <id>` | Run a single bench point as a real private TX; print cycles, prove_time, receipt_size, tx_e2e. |
| `cargo run --release --bin bench -- --all --account-id <id>` | Run the full matrix; write `results/results.json` and a Markdown table snippet. |
| `cargo test --workspace` | Host-side fixture round-trip + executor-mode guest sanity per scheme. |
| `cargo fmt --all --check` | Format check (CI). |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lint (CI). |

All bench runs require `RISC0_DEV_MODE=0` and a reachable LEZ devnet
(via `WalletCore::from_env()` — same config the original `run_private`
used).

## 4. Project Structure

```
.
├── methods/guest/src/bin/
│   ├── ecdsa_secp256k1.rs       # one binary per scheme; N is data-driven
│   ├── schnorr_secp256k1.rs
│   ├── ed25519.rs
│   ├── ecdsa_p256.rs
│   └── noop.rs                  # NSSA wrap only — calibration baseline
├── methods/guest/src/verifier/  # shared per-scheme verify modules
│   └── {ecdsa_k256,schnorr_k256,ed25519,ecdsa_p256}.rs
├── src/lib.rs                   # Scheme enum, VerifyInput, fixtures builder
├── src/bin/
│   ├── gen_test_vectors.rs
│   └── bench.rs                 # generalized run_private; matrix runner
├── fixtures/                    # gitignored
├── results/                     # gitignored except results/README-snippet.md
├── SPEC.md                      # this file
├── PLAN.md                      # phase-2 task breakdown
└── README.md                    # results table + decision note
```

The original `run_private.rs` is folded into `bench.rs` and not
preserved. Per-binary isolation gives clean receipt-size attribution
per scheme; N=1 vs N=3 is data-driven (same ELF), differentiating
through cycles and (downstream) receipt size via segment count.

## 5. Code Style

Match the existing guest pattern. Each scheme binary is a thin shell:

```rust
// methods/guest/src/bin/schnorr_secp256k1.rs (LEZ `dev` actor model)
use lee_core::program::{ReceiveInput, Response};
use lez_signature_bench_programs::{VerifyInput, verifier};

lee_core::actor_entrypoint!(handle_message);

fn handle_message(_input: &ReceiveInput, message: VerifyInput) -> Response {
    verifier::schnorr_k256::verify_all(&message).expect("verify failed");
    Response::keep_state()
}
```

(On v0.2.0-rc3 the same shell used `read_nssa_inputs` / `ProgramOutput`
and echoed every pre-state back as a post-state.)

Per-scheme verifier modules expose a single
`verify_all(&VerifyInput) -> Result<(), Error>` so binaries stay 10–15
lines. No comments on what the code does; one line max for *why* (e.g.
why prehash variant chosen). No `#[allow]` without an inline reason.

## 6. Testing Strategy

1. **Host unit tests** (`cargo test`): for each scheme, the
   `gen_test_vectors` builder produces inputs that the host-callable
   `verify_all` accepts. Catches signing/encoding bugs without paying
   for a prove.
2. **Guest sanity** (executor mode, no STARK): one fixture per scheme
   runs the guest to completion and writes the expected `ProgramOutput`.
   Run as part of `cargo test --workspace`.
3. **Negative path** (host): per scheme, flip one byte of a signature
   and confirm `verify_all` returns `Err`. Proves the verifier isn't
   trivially passing.
4. **End-to-end bench** is the integration test: if a scheme can't
   complete a real private TX, that's the failure signal. Not gated in
   CI (needs devnet credentials).
5. **CI** runs `fmt + clippy + build` on every push and PR. Bench runs
   stay local.

No coverage target — this is a research bench, not a service.

## 7. Boundaries

### Always

- Keep the AI-generated / not-for-mainnet warning at the top of
  `README.md`. This codebase is a research bench, not an audited
  primitive; the warning must be the first thing a reader sees.
- Wrap every guest in the LEZ program entrypoint (`actor_entrypoint!`
  on `dev`; NSSA `read_nssa_inputs` on v0.2); bench numbers reflect
  real LEZ cost.
- Use RISC0 patched crypto for every scheme; flag any that fall back
  to soft crypto and exclude from the headline comparison.
- Measure cycles + prove time + receipt size + end-to-end TX wall time
  for every (scheme, N) point.
- Use deterministic fixtures (fixed seeds) so numbers reproduce.
- Same message across all N signers. (Real RedStone payloads differ: each
  node signs its own data package, so a RedStone verifier hashes N messages.
  The per-signature verify cost is the same; see README "Oracle signing".)
- When `RISC0_DEV_MODE=1` is used for a smoke run, output and any
  commit message referencing numbers must say so prominently. Dev-mode
  numbers are not real measurements.

### Ask first

- GitHub repo rename (`lez-ecdsa` → `lez-signature-bench`) — visible to
  others, breaks links.
- New crate dependencies beyond the four patched crypto crates.
- Toolchain bump (currently 1.98.1).
- Bumping `risc0-zkvm` major version.
- Changes to LEZ devnet config.
- Any push, deploy, or `gh repo *` command after the rename.

### Never

- Replace patched crypto with pure-Rust crates — distorts the
  comparison.
- Commit fixtures, results JSON, devnet credentials, mnemonic seeds,
  or `.env` files.
- Remove the no-op baseline — it's the calibration anchor for the LEZ
  entrypoint overhead.
- Use threshold cryptography or batch verification in the headline
  matrix (note as future work; if measured, add as separate rows).
- Bypass git hooks (`--no-verify`) or signing flags
  (`--no-gpg-sign`) without explicit user approval.
- Silently use `RISC0_DEV_MODE=1` for any output reported as a
  measurement.
- Refactor code outside the bench's surface area.

## 8. Success Criteria

The bench is complete when **all** of these are demonstrably true:

- [ ] **Patch availability validated** for all 4 schemes before any
      guest is written. Missing patches are flagged in the spec and the
      affected scheme is either dropped or marked "soft" in results.
- [ ] **All in-scope schemes complete a real private TX on LEZ** for
      both N=1 and N=3 (up to 8 measurement points + 1 noop baseline).
- [ ] **Host unit tests pass** for every in-scope scheme: positive
      round-trip + negative (one-byte flip → `Err`).
- [ ] **README results table** with columns: `scheme | N | cycles |
      prove_time | receipt_size | tx_e2e | notes`. Machine identified
      (CPU, RAM, OS, RISC0 version, LEZ version).
- [ ] **Decision note** in README answering: "given a 5s / 30s / 5min
      end-to-end TX budget on this machine, which (scheme, N) fits?"
- [ ] **Reproducibility**: a fresh checkout + `bench --all` reproduces
      the table within ±10% on the same machine.
- [ ] **CI** (`fmt + clippy + build`) green on the default branch.
- [ ] No secrets, fixtures, or results JSON committed.

## 9. Non-goals (deferred, explicit)

- **RedStone payload parsing.** Synthetic fixtures only.
- **Threshold cryptography** (Schnorr/Ed25519/BLS threshold sigs).
- **Batch verification.** Schnorr and Ed25519 support it; out of the
  headline matrix. Optional follow-up if 3-of-5 numbers warrant.
- **N > 3 or N-sweep.** Only the winning scheme might get a sweep, and
  only if the budget framing demands it.
- **BLS, RSA, Lamport, SPHINCS+.** Out of scope. (LMS is the one
  hash-based / post-quantum scheme included — see §1.)
- **Multi-machine numbers.** Single machine only; cycles generalize,
  prove-time doesn't.
- **Production deployment** of any scheme. Measurement only.
- **Cost target / pass-fail threshold.** Bench measures; acceptability
  is decided in a follow-up after the numbers exist.

## 10. Open Questions

**Resolved by Phase 1 spike:**

- ~~Does k256's `schnorr` submodule hit the RISC0 secp256k1 precompile?~~
  **Yes.** `schnorr::verifying::verify_raw` calls
  `ProjectivePoint::lincomb`, which on `target_os = "zkvm"` dispatches
  to `risc0_bigint2::ec` — the same accelerated path ECDSA uses.
  Source: `k256/src/arithmetic/mul.rs` lines 343–377 at tag
  `k256/v0.13.4-risczero.1`.
- ~~Latest stable RISC0 fork tag for `curve25519-dalek` and `p256`?~~
  **`curve25519-4.1.3-risczero.0`** (workspace ships ed25519-dalek
  2.1.1 with a `backend/serial/risc0` module) and
  **`p256/v0.13.2-risczero.1`** (same fork as k256).

**Still open:**

- Cycle counts: read from receipt metadata after prove, or do a
  separate executor pass? Prefer the former (one pass per measurement).
- Should the no-op baseline run with N=1 input, or also N=3 for parity?
  *(N=1 is the cleaner "pure NSSA cost" reference.)*

## 11. Reference URLs

- LEZ: https://github.com/logos-blockchain/logos-execution-zone
- RISC Zero precompiles: https://dev.risczero.com/api/zkvm/precompiles
- RISC Zero RustCrypto forks: https://github.com/risc0/RustCrypto-elliptic-curves
- BIP-340 Schnorr: https://github.com/bitcoin/bips/blob/master/bip-0340.mediawiki
- Ed25519 (RFC 8032): https://datatracker.ietf.org/doc/html/rfc8032
- Predecessor PoC numbers: see git history at `131d89d` (Pass 2 NSSA wrap).
