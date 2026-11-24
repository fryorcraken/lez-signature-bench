# lez-signature-bench

> ⚠️ **AI-generated codebase — research bench only.** This repository
> was produced with AI assistance to get a rough indication of
> signature-verification cost on RISC Zero / LEZ. It has not been
> audited. **It MUST NOT be used in any mainnet program** or any
> production system. Use it for measurement, comparison, and
> intuition-building — nothing else.

A comparative benchmark of common signature-verification schemes
inside the [Logos Execution Zone (LEZ)](https://github.com/logos-blockchain/logos-execution-zone)
guest model on the [RISC Zero zkVM](https://risczero.com/). Each scheme
runs inside an NSSA-wrapped guest using RISC Zero's accelerated curve
arithmetic, and is benchmarked end-to-end through the local prover.

The goal is *data*, not deployment: cycles, prove time, and receipt
size for each scheme so a developer choosing a verification primitive
for oracle / multi-sig / passkey workloads on LEZ can predict cost
on consumer hardware.

See [`SPEC.md`](./SPEC.md) for acceptance criteria and
[`PLAN.md`](./PLAN.md) for the task breakdown.

## Schemes in scope

| Scheme | Curve / family | Prehash | RISC0 patch source |
|---|---|---|---|
| ECDSA secp256k1 | secp256k1 | keccak256 | `k256/v0.13.4-risczero.1` |
| Schnorr secp256k1 (BIP-340) | secp256k1 | sha256 | (same fork as ECDSA) |
| Ed25519 | Curve25519 | none (sha512 internal) | `curve25519-4.1.3-risczero.0` |
| ECDSA P-256 | NIST P-256 | sha256 | `p256/v0.13.2-risczero.1` |
| LMS (RFC 8554) | hash-based, post-quantum | sha256 (whole construction) | `hbs-lms 0.2.0-alpha.1` (no RISC0 fork; rides the patched `sha2`) |

The four EC schemes hit RISC Zero's `bigint2` / curve precompile path —
verified in the Phase 1 spike. Schnorr secp256k1 routes through the
same `ProjectivePoint::lincomb` accelerated path that ECDSA secp256k1
uses (no separate code path); Ed25519 uses curve25519-dalek's
`backend/serial/risc0` module. **LMS** has no RISC0 fork; its only
acceleration is the SHA-256 precompile reached transitively through
the patched `sha2` crate. Parameters used: `Sha256_256` / `LmotsW8` /
`LmsH5` (single-tree HSS, 32 leaves, smallest-signature tradeoff).

## Results

Real-prove run, single-pass per cell on an idle machine. NSSA-wrapped
guests, `RISC0_DEV_MODE=0`, one synthetic same-message fixture per
`(scheme, N)`.

**Machine:** AMD Ryzen 9 7940HS (16 threads, AMD Radeon 780M iGPU,
60 GiB RAM), Linux 6.19, CPU prover (no CUDA, no Bonsai).
**Stack:** risc0-zkvm 3.0.5, LEZ v0.2.0-rc3, Rust 1.92.0.

After applying the [Pass 3 optimizations](#optimization-passes) (workspace-wide
`[profile.release]` with `lto="fat"`, `codegen-units=1`, `panic="abort"`,
`strip=true`):

| scheme | N | total cycles | user cycles | segments | prove time | receipt size (B) |
|---|---:|---:|---:|---:|---:|---:|
| `noop` | 1 | 65 536 | 38 623 | 1 | 9.00 s (~0:09) | 222 266 |
| `ecdsa-secp256k1` | 1 | 524 288 | 342 000 | 1 | 146.80 s (~2:26) | 492 375 |
| `ecdsa-secp256k1` | 3 | 1 081 344 | 988 083 | 2 | 260.06 s (~4:20) | 716 649 |
| `schnorr-secp256k1` | 1 | 524 288 | 309 274 | 1 | 72.51 s (~1:12) | 269 234 |
| `schnorr-secp256k1` | 3 | 1 048 576 | 903 957 | 1 | 146.69 s (~2:26) | 284 050 |
| `ed25519` | 1 | 1 048 576 | 841 867 | 1 | 160.43 s (~2:40) | 282 482 |
| `ed25519` | 3 | 3 145 728 | 2 501 609 | 3 | 460.65 s (~7:40) | 846 678 |
| `ecdsa-p256` | 1 | 524 288 | 236 869 | 1 | 69.52 s (~1:09) | 269 242 |
| `ecdsa-p256` | 3 | 1 048 576 | 677 267 | 1 | 142.29 s (~2:22) | 284 074 |
| `lms` | 1 | 3 145 728 | 2 720 550 | 3 | 531.95 s (~8:51) | 855 190 |
| `lms` | 3 | 8 912 896 | 8 281 737 | 9 | 1 349.76 s (~22:29) | 2 551 554 |

The `noop` row is the NSSA-wrap-only calibration baseline (52 705 user
cycles with empty pre-states, no crypto). Subtract it from any other
row for the per-scheme verify cost in cycles. Note `ecdsa-secp256k1`
shows a much larger receipt than other schemes at N=1; this is driven
by the segment-padding power-of-two and may also reflect the ELF that
keccak256 + k256 path pulls in. Re-runs may vary ±10% on prove time.

### Per-signature cycle deltas (subtracting noop)

| scheme | user cycles / sig (N=1) | user cycles / sig (≈ from N=3) |
|---|---:|---:|
| ecdsa-secp256k1 | 303 377 | 316 487 |
| schnorr-secp256k1 | 270 651 | 288 445 |
| ecdsa-p256       | 198 246 | 212 881 |
| ed25519          | 803 244 | 820 995 |
| lms              | 2 681 927 | 2 747 705 |

**Headline takes:**

- **P-256 ECDSA is the cheapest** verify on this stack — about
  **32% fewer cycles per sig than secp256k1 ECDSA** at N=1. The same
  RISC0 `bigint2` curve precompile applies and the field is similar
  cost; what differs is keccak256 (k256 path) vs sha256 (p256 path)
  for the message digest.
- **Schnorr secp256k1 ≈ 9% cheaper than ECDSA secp256k1** per sig.
  The expected win from skipping the modular inversion is partially
  offset by Schnorr's `ProjectivePoint::lincomb(G, s, P, -e)` doing
  one combined mul vs ECDSA's separate mul + recovery. Same precompile
  path, smaller win than naive theory predicts.
- **Ed25519 is by far the most expensive** here — **2.7× the user
  cycles** of secp256k1 ECDSA. The RISC0 curve25519-dalek backend is
  available and active, but Edwards arithmetic plus the in-algorithm
  sha512 (no zkVM precompile) dominates.
- **Multi-sig scaling is roughly linear** for all schemes — about
  **+320–340K user cycles per added secp256k1 sig**, **+870K** per
  Ed25519 sig. No batch-verify shortcuts here (out of scope per
  [`SPEC.md`](./SPEC.md) §9).
- **LMS is ~8× the cycles and ~3.5× the prove time of secp256k1
  ECDSA at N=1**, with a receipt 1.7× as large. Per-sig delta scales
  cleanly (~2.7M user cycles/sig). The SHA precompile carries the
  inner work, but signature parsing in `tinyvec` arrays + LM-OTS hash
  chains + the per-call SHA dispatch overhead add up. **LMS does not
  beat any RISC0-precompiled EC scheme on this stack** — its value
  here is as a "what does post-quantum cost on RISC0 today" data point,
  not a recommendation. Two production caveats inherent to LMS:
  signing is *stateful* (the signer must persist a leaf counter; reuse
  breaks security), and the keypair is one-shot exhausted after 2^h
  signatures (h=5 → 32 sigs in this configuration).

### Decision note: budget → scheme on this laptop

Given the prove times above on a CPU-only Ryzen 9 7940HS:

| TX prove budget | What fits |
|---|---|
| **15 s** | `noop` baseline (~0:09, no crypto) |
| **1 min** | nothing in scope |
| **1.5 min** | `ecdsa-p256` n=1 (~1:09); `schnorr-secp256k1` n=1 (~1:12) |
| **3 min** | adds `ecdsa-p256` n=3 (~2:22), `schnorr-secp256k1` n=3 (~2:26), `ecdsa-secp256k1` n=1 (~2:26), `ed25519` n=1 (~2:40) |
| **5 min** | adds `ecdsa-secp256k1` n=3 (~4:20) |
| **8 min** | adds `ed25519` n=3 (~7:40) |
| **10 min** | adds `lms` n=1 (~8:51) |
| **25 min** | adds `lms` n=3 (~22:29) |

For interactive RedStone-style oracle UX (3-of-N pulls, sub-30 s),
**no scheme fits on CPU**. CUDA / Bonsai would compress this
dramatically; CPU alone is too heavy for low-latency UX.

For batch / async workloads (sub-5 min acceptable), **secp256k1 Schnorr
or P-256 at N=3** is the budget pick. If keys / addresses must stay
secp256k1 (Ethereum compat), Schnorr is the natural step up from ECDSA.

### End-to-end private-TX time (against `lgs localnet`)

The numbers above isolate the inner proving cost. The number a user
actually feels — "click submit on a privacy-preserving transaction →
confirmation back from the sequencer" — adds NSSA framing, the
privacy-preserving circuit (which proves the account-state transition
on top of our verifier), and the sequencer roundtrip. Measured against
a fresh `lgs localnet` and a fresh `PrivateOwned` account, same
machine, same `RISC0_DEV_MODE=0`. **These rows were captured against
the *pre-optimization* binaries**, so they're a slight overestimate
relative to the optimized local-prove table above (re-running E2E
against the optimized binaries is left as future work):

| scheme | N | E2E private TX | local prove (baseline) | wrap overhead |
|---|---:|---:|---:|---:|
| `noop`              | 1 | 103.46 s (~1:43) | 23.15 s (~0:23) | +80.3 s (+347%) |
| `ecdsa-secp256k1`   | 1 | 246.46 s (~4:06) | 141.47 s (~2:21) | +105.0 s (+74%) |
| `ecdsa-secp256k1`   | 3 | 446.12 s (~7:26) | 260.13 s (~4:20) | +186.0 s (+72%) |
| `schnorr-secp256k1` | 1 | 153.67 s (~2:33) | 77.82 s (~1:18) | +75.9 s (+97%) |
| `schnorr-secp256k1` | 3 | 322.95 s (~5:22) | 166.46 s (~2:46) | +156.5 s (+94%) |
| `ed25519`           | 1 | 282.95 s (~4:42) | 153.97 s (~2:34) | +129.0 s (+84%) |
| `ed25519`           | 3 | 669.66 s (~11:09) | 451.82 s (~7:32) | +217.8 s (+48%) |
| `ecdsa-p256`        | 1 | 152.50 s (~2:32) | 71.44 s (~1:11) | +81.1 s (+113%) |
| `ecdsa-p256`        | 3 | 298.82 s (~4:58) | 141.36 s (~2:21) | +157.5 s (+111%) |

The privacy-preserving wrapping adds **roughly 75–220 s** per TX. It's
not a fixed overhead: there's a constant component (~80 s, visible on
the `noop` row) plus a variable component that scales with the inner
kernel's segment count. Larger kernels (e.g. `ed25519` N=3) see a lower
*percentage* overhead because the fixed component is amortized.

Scheme ranking carries over from local-prove to E2E unchanged:
`ecdsa-p256` ≈ `schnorr-secp256k1` < `ecdsa-secp256k1` < `ed25519`.
The wrap overhead compresses the spread (Ed25519 is ~1.9× ECDSA-k1 in
E2E vs ~2.7× in user cycles), so for end-user latency the gap is real
but smaller than the cycle deltas suggest.

Net for the headline RedStone shape — **3-of-N pull, end-to-end**:

| scheme | N=3 E2E |
|---|---:|
| `ecdsa-p256` | **4:58** (cheapest) |
| `schnorr-secp256k1` | 5:22 |
| `ecdsa-secp256k1` | 7:26 |
| `ed25519` | 11:09 |

For interactive UX (sub-30 s) **no scheme fits on CPU**. CUDA / Bonsai
would compress this; CPU alone is too heavy.

## Methodology

Each local-prove row is one **real prove pass** through
`risc0_zkvm::default_prover()` with `RISC0_DEV_MODE=0`, against the
NSSA-wrapped guest binary for that scheme. Inputs are written in the
exact shape `nssa_core::read_nssa_inputs` expects (`self_program_id`,
`caller_program_id`, `pre_states`, NSSA-encoded instruction). Cycles
and segment count come from `ProveInfo.stats`; prove time is wall-clock
around the `prove(...)` call; receipt size is
`bincode::serialize(&receipt).len()`.

Each E2E row submits a real privacy-preserving transaction via
`wallet::WalletCore::send_privacy_preserving_tx()` against a running
`lgs localnet` (sequencer in `risc0_dev_mode=true`, client in
`RISC0_DEV_MODE=0`). The wall clock wraps `send_privacy_preserving_tx`,
so it includes serialization, client-side proving (kernel +
privacy-preserving wrapping circuit), the sequencer roundtrip, and
confirmation.

Synthetic fixtures only — no captured oracle payloads. All signers
share the same message (`b"hello redstone"`) per row, signed with
deterministic seeds.

### Optimization passes

The numbers above include a workspace-level tight `[profile.release]`
applied to both host and guest (`lto = "fat"`, `codegen-units = 1`,
`panic = "abort"`, `strip = true`). Effect on guest ELFs:

| scheme | ELF before | ELF after | Δ |
|---|---:|---:|---:|
| `noop` | 371 KB | 184 KB | -50% |
| `ecdsa-secp256k1` | 489 KB | 257 KB | -47% |
| `schnorr-secp256k1` | 426 KB | 221 KB | -48% |
| `ed25519` | 497 KB | 281 KB | -43% |
| `ecdsa-p256` | 456 KB | 237 KB | -48% |

**Cycle / prove-time effect** vs the unoptimized profile:

- `noop` user cycles **-27%** (52 705 → 38 623), prove time **-61%**
  (23 s → 9 s) — small ELF benefits hugely from less paging.
- Per-signature user cycles **-8% to -12%** uniformly across schemes
  — LTO pays a few % through the curve arithmetic.
- `schnorr-secp256k1` N=3 dropped from 2 segments to 1 (the smaller
  ELF + LTO-tightened code lands the work just under po2=20),
  shaving ~12% off prove time (166 s → 147 s).
- A few rows ticked up slightly within run-to-run variance (±10%);
  the cycle reductions are the more reliable signal.

**Things tried that didn't ship:**

- *Parallel segment proving.* `risc0_zkvm::default_prover()` in 3.0.5
  loops segments sequentially (`local_prover/prover_impl.rs` line 89:
  `for segment ... segments.push(prove_segment)`). Manual rayon
  parallelism would help N=3 cases but is a larger change than this
  pass; left as future work.
- *Switching ECDSA k1 prehash from keccak256 to sha256.* Would save
  ~250 KB of receipt and ~100 K user cycles, but breaks
  Ethereum-compatible signing. Don't.

**Things deliberately not tried:**

- *Batch verify for Schnorr / Ed25519.* Would change the algorithm
  shape; out of headline matrix per [`SPEC.md`](./SPEC.md) §9.
- *Receipt compression to Succinct.* Reduces receipt to ~25 KB but
  adds significant CPU. Useful for downstream consumers, not for
  per-row bench numbers.
- *GPU / Bonsai prove.* Out of scope per the user's review request.

### Receipt-size note

`ecdsa-secp256k1`'s 492 KB local-prove receipt is roughly 1.8× any
other scheme's receipt at the same segment count. This is the keccak
coprocessor: `tiny-keccak`'s RISC0 patch routes each permutation
through `risc0_keccak_update`, and the coprocessor's STARK proof is
attached as a receipt assumption (~247 KB per keccak call vs ~24 KB
for sha256-precompile use). The cost is mostly inherent to using
keccak — switching to sha256 prehash would save the bytes but break
Ethereum compatibility.

## Build

```bash
cargo build --workspace --release
```

`risc0-build` cross-compiles six guest ELFs (one per scheme + the
noop baseline) for `riscv32im-risc0-zkvm-elf` and embeds them as
`{ECDSA_SECP256K1,SCHNORR_SECP256K1,ED25519,ECDSA_P256,LMS,NOOP}_ELF`.

## Run the bench

```bash
# One scheme + N
RISC0_DEV_MODE=0 cargo run --release --bin bench -- \
  --scheme ecdsa-secp256k1 --n 1

# Full matrix → results/results.json + results/README-snippet.md
RISC0_DEV_MODE=0 cargo run --release --bin bench -- --all

# Public-execution matrix: execute without proving, as the sequencer
# does for a public TX (needs an r0vm matching risc0-zkvm 3.0, e.g.
# RISC0_SERVER_PATH=/path/to/r0vm-3.0.5)
cargo run --release --bin bench -- --all --public --label public
# → results/results-public.json + results/README-snippet-public.md

# End-to-end matrix against a running lgs localnet
lgs localnet start
lgs wallet -- account new private    # note the printed Private/<id>
NSSA_WALLET_HOME_DIR="$PWD/.scaffold/wallet" RISC0_DEV_MODE=0 \
  cargo run --release --bin bench -- \
  --all --account-id Private/<your-id> --label e2e
# → results/results-e2e.json + results/README-snippet-e2e.md

# Generate just the JSON fixture for one (scheme, N) point
cargo run --release --bin gen_test_vectors -- \
  --scheme schnorr-secp256k1 --n 3
```

`RISC0_DEV_MODE=1` skips actual proving and prints fake numbers.
The bench logs a warning when this is set; never quote those numbers.

The local matrix takes **~28 minutes** on the reference machine; the
end-to-end matrix takes **~46 minutes** end-to-end (kernel + wrapping
+ sequencer roundtrip per row).

## Lint and test

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace      # 12 host tests: byte-stable wire format
                            # (ecdsa-k1, lms), positive + negative
                            # round-trip per scheme
```

CI (`.github/workflows/ci.yml`) runs the fmt + clippy + build set on
every push and PR. Bench runs are local only.

## Prerequisites

- Rust toolchain 1.92.0 (auto-picked via `rust-toolchain.toml`).
- The RISC Zero RISC-V guest toolchain. Install via
  [`rzup`](https://dev.risczero.com/api/zkvm/install):
  ```bash
  curl -L https://risczero.com/install | bash
  rzup install
  ```
- ~30 minutes of idle CPU time for a full matrix run.

## Layout

```
.
├── methods/
│   ├── build.rs                          # risc0_build::embed_methods()
│   └── guest/
│       ├── src/lib.rs                    # shared VerifyInput + per-scheme verifier modules
│       └── src/bin/
│           ├── ecdsa_secp256k1.rs        # NSSA-wrapped guest, one per scheme
│           ├── schnorr_secp256k1.rs
│           ├── ed25519.rs
│           ├── ecdsa_p256.rs
│           ├── lms.rs                    # hash-based / post-quantum row
│           └── noop.rs                   # NSSA-wrap-only calibration
├── src/
│   ├── lib.rs                            # Scheme enum + host-side fixtures + verify_all
│   ├── verifier/                         # host-callable verify per scheme (mirrors guest)
│   └── bin/
│       ├── bench.rs                      # local-prove bench (single + --all)
│       └── gen_test_vectors.rs           # write JSON fixture for one (scheme, N)
├── Cargo.toml                            # workspace + [patch.crates-io] for risc0 crypto forks
├── results/                              # gitignored — matrix output lives here
├── fixtures/                             # gitignored
├── SPEC.md                               # acceptance criteria + boundaries
├── PLAN.md                               # 5-phase task breakdown
└── README.md                             # this file
```

## Roadmap

Deliberately deferred (see [`SPEC.md`](./SPEC.md) §9):

- **GPU / Bonsai prove.** All numbers above are CPU-only on the
  reference Ryzen 9. CUDA acceleration or Bonsai would collapse prove
  time substantially; left as future work.
- **Threshold cryptography** (Schnorr / Ed25519 / BLS threshold sigs).
- **Batch verification** for Schnorr and Ed25519 — could collapse
  3-of-N cost meaningfully; out of the headline matrix.
- **N-sweep** beyond {1, 3} for the winning scheme.
- **Multi-machine numbers.** Cycles generalize; prove time doesn't.
- **RedStone payload parsing.**

## License

MIT or Apache-2.0 (per workspace `Cargo.toml`).
