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
arithmetic, and is benchmarked end-to-end through the local prover
(private transactions) and the executor (public transactions).

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
| LMS (RFC 8554), W=8 | hash-based, post-quantum | sha256 (whole construction) | `hbs-lms 0.2.0-alpha.1` (no RISC0 fork; rides the patched `sha2`) |
| LMS (RFC 8554), W=1 | hash-based, post-quantum | sha256 (whole construction) | same crate and guest as W=8 |
| XMSS (RFC 8391) | hash-based, post-quantum | sha256 (whole construction) | `xmss 0.1.0-pre.0` (RustCrypto pre-release; no RISC0 fork; rides the patched `sha2`) |

The four EC schemes hit RISC Zero's `bigint2` / curve precompile path —
verified in the Phase 1 spike. Schnorr secp256k1 routes through the
same `ProjectivePoint::lincomb` accelerated path that ECDSA secp256k1
uses (no separate code path); Ed25519 uses curve25519-dalek's
`backend/serial/risc0` module. **LMS** has no RISC0 fork; its only
acceleration is the SHA-256 precompile reached transitively through
the patched `sha2` crate. Parameters used: `Sha256_256` / `LmsH5`
(single-tree HSS, 32 leaves) with `LmotsW8` (`lms`, smallest signature)
and `LmotsW1` (`lms-w1`, fewest hash-chain steps, largest signature).
hbs-lms reads the parameter set from the signature, so both rows run
the same guest ELF. **XMSS** uses `XmssSha2_10_256` (h=10, 1024 leaves,
WOTS+ w=16); like LMS, its only acceleration is the SHA-256 precompile.

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

`lms-w1` and `xmss` have not been proven yet; their cycle counts are in
[Public execution](#public-execution-no-proof).

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

### Public execution (no proof)

A public LEZ transaction is not proven. At v0.2.0-rc3 the sequencer
runs the program in the RISC Zero executor (`nssa::Program::execute` →
`default_executor().execute`, through `r0vm` as the sequencer image
ships it), with a hard cap of 32M user cycles per execution
(`MAX_NUM_CYCLES_PUBLIC_EXECUTION`; rc3 has no fees). `bench --public`
makes the same call with the same cap and the same NSSA input framing.
Cycles are deterministic; exec time is the median of 5 runs. Run on
2026-11-24 on the same laptop with risc0-zkvm 3.0.5 and r0vm 3.0.5. The
host was busy (load average ~7 of 16 threads, from outside the bench),
so read exec times as indicative.

| scheme | N | user cycles | noop guest, same fixture | segments | exec time | range (5 runs) | share of 32M cap |
|---|---:|---:|---:|---:|---:|---:|---:|
| `noop` | 1 | 38 636 | — | 1 | 23 ms | 23–30 ms | 0.1% |
| `ecdsa-secp256k1` | 1 | 342 537 | 38 636 | 1 | 37 ms | 36–38 ms | 1.0% |
| `ecdsa-secp256k1` | 3 | 989 668 | 93 163 | 2 | 57 ms | 53–57 ms | 2.9% |
| `schnorr-secp256k1` | 1 | 310 053 | 38 372 | 1 | 33 ms | 33–34 ms | 0.9% |
| `schnorr-secp256k1` | 3 | 906 312 | 92 384 | 1 | 49 ms | 48–50 ms | 2.7% |
| `ed25519` | 1 | 841 370 | 38 372 | 1 | 46 ms | 44–48 ms | 2.5% |
| `ed25519` | 3 | 2 500 221 | 92 384 | 3 | 74 ms | 72–78 ms | 7.5% |
| `ecdsa-p256` | 1 | 236 769 | 38 636 | 1 | 33 ms | 33–34 ms | 0.7% |
| `ecdsa-p256` | 3 | 677 017 | 93 163 | 1 | 47 ms | 46–49 ms | 2.0% |
| `lms` | 1 | 3 029 047 | 383 567 | 4 | 71 ms | 69–72 ms | 9.0% |
| `lms` | 3 | 9 227 435 | 1 127 815 | 10 | 163 ms | 162–165 ms | 27.5% |
| `lms-w1` | 1 | 2 686 482 | 2 408 513 | 3 | 75 ms | 74–75 ms | 8.0% |
| `lms-w1` | 3 | 8 042 960 | 7 202 653 | 9 | 196 ms | 187–198 ms | 24.0% |
| `xmss` | 1 | 4 650 652 | 715 453 | 5 | 113 ms | 111–118 ms | 13.9% |
| `xmss` | 3 | 14 023 941 | 2 123 978 | 15 | 256 ms | 247–267 ms | 41.8% |

The "noop guest, same fixture" column runs the verify-free `noop` guest
on that row's keys and signatures
(`bench --scheme noop --noop-fixture <scheme> --n <N> --public`), so it
is the NSSA wrapper's cost for that input. The rest is verification.

| scheme | signature | verify / sig (N=1) | verify / sig (N=3 − N=1, ÷2) | input handling / sig | ≈ max N under the cap |
|---|---:|---:|---:|---:|---:|
| `ecdsa-p256` | 64 B | 198 133 | 192 860 | 27 264 | 152 |
| `schnorr-secp256k1` | 64 B | 271 681 | 271 124 | 27 006 | 112 |
| `ecdsa-secp256k1` | 64 B | 303 901 | 296 302 | 27 264 | 103 |
| `ed25519` | 64 B | 802 998 | 802 420 | 27 006 | 40 |
| `lms-w1` | 8 688 B | 277 969 | 281 169 | 2 397 070 | 12 |
| `lms` | 1 296 B | 2 645 480 | 2 727 070 | 372 124 | 10 |
| `xmss` | 2 500 B | 3 935 199 | 3 982 382 | 704 262 | 7 |

"Max N" extrapolates linearly from N=1 and N=3; the fixtures only go
to N=3.

- **Public execution is cheap at every N measured: 23–260 ms per
  call**, against minutes for a private TX. The `noop` floor (~23 ms)
  is mostly `r0vm` start-up and the IPC round trip. The heaviest row,
  `xmss` N=3, uses 42% of the cap.
- **The NSSA wrapper costs ~274 user cycles per input byte.** rc3
  encodes the instruction with RISC Zero serde (one u32 word per byte);
  the guest decodes it and echoes the raw instruction data into
  `ProgramOutput`. For 64-byte curve signatures that is noise (~27K
  cycles per signer); for hash-based signatures it is not.
- **LMS W=1 verifies as cheaply as ECDSA, but its signature costs
  more than the verify saves.** W=1 has 265 hash chains of one step
  where W=8 has 34 chains of up to 255 steps, so verification alone
  drops ~10× to ~280K cycles per signature, about what ECDSA secp256k1
  costs. Its 8 688-byte signature adds ~2.4M cycles
  of input handling, so `lms-w1` lands within 11% of `lms` (W=8,
  1 296 B). On this wrapper, the smaller hash-based signature wins.
- **XMSS (h=10) costs ~1.5× LMS (h=5, W=8) per signature**: ~3.96M
  cycles of verification plus ~0.70M of input handling for a 2 500-byte
  signature. In exchange, each key signs 1 024 messages instead of 32.
- **LMS cycles differ from the prove-mode table above**: 3 029 047 at
  N=1 here vs 2 720 550 there (+11%). A dev-mode prove of this build
  reports the same 3 029 047, so execute and prove count alike, and
  the LMS fixture is byte-identical (a test pins it). The gap is
  between this guest build and the one behind that table; the curve
  rows reproduce within 0.3%.

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

`risc0-build` cross-compiles seven guest ELFs (one per scheme + the
noop baseline; `lms` and `lms-w1` share one) for
`riscv32im-risc0-zkvm-elf` and embeds them as
`{ECDSA_SECP256K1,SCHNORR_SECP256K1,ED25519,ECDSA_P256,LMS,XMSS,NOOP}_ELF`.

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
│           ├── lms.rs                    # hash-based / post-quantum rows (W=8 and W=1)
│           ├── xmss.rs                   # hash-based / post-quantum row
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
