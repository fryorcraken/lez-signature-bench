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
runs inside a LEZ actor-model guest (`lee_core::actor_entrypoint!`, LEZ
`dev`) using RISC Zero's accelerated curve arithmetic, and is benchmarked
through the local prover and end-to-end as a private transaction, and
through the executor as a public transaction.

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
| Ed25519 | Curve25519 | none (sha512 internal) | `curve25519-4.1.3-risczero.0` (curve25519-dalek; ed25519-dalek 2.2.0 from crates.io) |
| ECDSA P-256 | NIST P-256 | sha256 | `p256/v0.13.2-risczero.1` |
| LMS (RFC 8554), W=8 | hash-based, post-quantum | sha256 (whole construction) | `hbs-lms 0.2.0-alpha.1` (no RISC0 fork; rides the patched `sha2`) |
| LMS (RFC 8554), W=1 | hash-based, post-quantum | sha256 (whole construction) | same crate and guest as W=8 |
| XMSS (RFC 8391) | hash-based, post-quantum | sha256 (whole construction) | `xmss 0.1.0-pre.0` (RustCrypto pre-release; no RISC0 fork; rides the patched `sha2`) |

The four EC schemes hit RISC Zero's `bigint2` / curve precompile path —
verified in the Phase 1 spike. Schnorr secp256k1 routes through the
same `ProjectivePoint::lincomb` accelerated path that ECDSA secp256k1
uses (no separate code path); Ed25519 uses curve25519-dalek's
`backend/serial/risc0` module. (Only curve25519-dalek is patched, as in
LEZ itself; LEZ `dev` needs `ed25519-dalek ^2.2`, so the risc0 fork's
2.1.1 copy is no longer used. Per-sig Ed25519 cycles moved +1.8%, so the
accelerated backend is still in play.) **LMS** has no RISC0 fork; its only
acceleration is the SHA-256 precompile reached transitively through
the patched `sha2` crate. Parameters used: `Sha256_256` / `LmsH5`
(single-tree HSS, 32 leaves) with `LmotsW8` (`lms`, smallest signature)
and `LmotsW1` (`lms-w1`, fewest hash-chain steps, largest signature).
hbs-lms reads the parameter set from the signature, so both rows run
the same guest ELF. **XMSS** uses `XmssSha2_10_256` (h=10, 1024 leaves,
WOTS+ w=16); like LMS, its only acceleration is the SHA-256 precompile.

## Results

Real-prove runs, three runs per cell; prove times are the median, with
the min–max range alongside. `RISC0_DEV_MODE=0`, one synthetic
same-message fixture per `(scheme, N)`.

**Machine:** AMD Ryzen 9 7940HS (16 threads, AMD Radeon 780M iGPU,
60 GiB RAM), Linux 7.3, CPU prover (no CUDA, no Bonsai), on AC with
the `performance` governor, inside a rootless podman container (no
cgroup CPU or memory limits, never throttled). While proving, the CPU
sits at its 100 °C limit (`k10temp` Tctl) and runs all cores at
~3.7 GHz (5.26 GHz max), so prove time depends on cooling. Other host
activity averaged 0.7–0.9 of 16 cores during the two monitored runs
(see [Run-to-run variance](#run-to-run-variance)).
**Stack:** LEZ `dev` @ [`dcbd318f0`](https://github.com/logos-blockchain/logos-execution-zone/commit/dcbd318f0f2ef601fc93e67e0dabc49895d811ef)
(the v0.4 actor model; there is no `master`, and `main` predates the
actor model), risc0-zkvm 3.1.0 with r0vm 3.1.0, risc0 guest toolchain
Rust 1.94.1, host Rust 1.98.1. Same tight `[profile.release]` as before
([Pass 3](#optimization-passes)).

| scheme | N | total cycles | user cycles | segments | prove time (median) | range | receipt size (B) |
|---|---:|---:|---:|---:|---:|---:|---:|
| `noop` | 1 | 32 768 | 7 201 | 1 | 4.3 s (~0:04) | 3.9–4.4 s | 210 058 |
| `ecdsa-secp256k1` | 1 | 524 288 | 311 082 | 1 | 137.5 s (~2:18) | 114.9–140.7 s | 491 815 |
| `ecdsa-secp256k1` | 3 | 1 048 576 | 905 444 | 1 | 207.6 s (~3:28) | 207.6–211.4 s | 505 483 |
| `schnorr-secp256k1` | 1 | 524 288 | 278 821 | 1 | 66.1 s (~1:06) | 64.9–68.4 s | 268 680 |
| `schnorr-secp256k1` | 3 | 1 048 576 | 822 978 | 1 | 147.5 s (~2:27) | 133.6–157.1 s | 282 344 |
| `ed25519` | 1 | 1 048 576 | 825 288 | 1 | 138.8 s (~2:19) | 127.5–139.4 s | 281 928 |
| `ed25519` | 3 | 3 145 728 | 2 462 279 | 3 | 390.4 s (~6:30) | 382.5–416.2 s | 845 076 |
| `ecdsa-p256` | 1 | 262 144 | 205 310 | 1 | 31.9 s (~0:32) | 31.7–33.1 s | 256 458 |
| `ecdsa-p256` | 3 | 1 048 576 | 592 783 | 1 | 130.8 s (~2:11) | 127.8–134.8 s | 282 350 |
| `lms` | 1 | 3 145 728 | 2 659 761 | 3 | 398.6 s (~6:39) | 381.7–420.6 s | 847 180 |
| `lms` | 3 | 8 519 680 | 8 130 131 | 9 | 1116.8 s (~18:37) | 1055.6–1242.1 s | 2 503 648 |

The `noop` row is the entrypoint-only calibration baseline: the actor
entrypoint reads the framed `ReceiveInput`, borsh-decodes the
`VerifyInput` message and commits the `Transition`, with no crypto
(7 201 user cycles). Subtract it from any other row for the per-scheme
verify cost in cycles. `ecdsa-secp256k1`'s larger receipt and longer
prove time are the keccak coprocessor (see
[Keccak coprocessor note](#keccak-coprocessor-note)). Cycle counts are
identical in every run; prove time spreads 2–19% (max − min over
median) between runs (see [Run-to-run variance](#run-to-run-variance)).

`lms-w1` and `xmss` have not been proven yet; their cycle counts are in
[Public execution](#public-execution-no-proof).

**Prove time follows the padded segment size, not the cycle count.**
Every row without keccak lands on the same curve: ~4.3 s for a 2^15
segment, ~32 s for 2^18, ~66 s for 2^19 and ~130–150 s per 2^20
segment (≈ 120–140 µs per padded cycle). Trimming cycles only pays when
it crosses a power-of-two boundary. LEZ's own `cycle_bench` reports the
same shape at ~73 µs per padded cycle on an Apple M2 Pro (see
[Comparison with LEZ's own benchmarks](#comparison-with-lezs-own-benchmarks)).

### LEZ v0.2.0-rc3 → `dev`: what changed

Same fixtures (the borsh wire-format test is derived from the old
bincode capture, so the signed bytes are identical), same machine, same
release profile:

| scheme | N | user cycles (rc3 → dev) | Δ | prove time (rc3 → dev median) | Δ |
|---|---:|---:|---:|---:|---:|
| `noop` | 1 | 38 623 → 7 201 | -81% | 9.0 s → 4.3 s (po2 16→15) | -52% |
| `ecdsa-secp256k1` | 1 | 342 000 → 311 082 | -9% | 146.8 s → 137.5 s | -6% |
| `ecdsa-secp256k1` | 3 | 988 083 → 905 444 | -8% | 260.1 s → 207.6 s (2→1 segs) | -20% |
| `schnorr-secp256k1` | 1 | 309 274 → 278 821 | -10% | 72.5 s → 66.1 s | -9% |
| `schnorr-secp256k1` | 3 | 903 957 → 822 978 | -9% | 146.7 s → 147.5 s | +1% |
| `ed25519` | 1 | 841 867 → 825 288 | -2% | 160.4 s → 138.8 s | -13% |
| `ed25519` | 3 | 2 501 609 → 2 462 279 | -2% | 460.6 s → 390.4 s | -15% |
| `ecdsa-p256` | 1 | 236 869 → 205 310 | -13% | 69.5 s → 31.9 s (po2 19→18) | -54% |
| `ecdsa-p256` | 3 | 677 267 → 592 783 | -12% | 142.3 s → 130.8 s | -8% |
| `lms` | 1 | 2 720 550 → 2 659 761 | -2% | 532.0 s → 398.6 s | -25% |
| `lms` | 3 | 8 281 737 → 8 130 131 | -2% | 1349.8 s → 1116.8 s | -17% |

- **The fixed entrypoint cost fell 81%** (38 623 → 7 201 user cycles).
  The v0.2 NSSA guest read `self_program_id`, `caller_program_id`, the
  pre-states and a risc0-serde instruction, then wrote every pre-state
  back as a post-state. The actor guest reads one borsh frame and
  returns `Response::keep_state()`. That ~31K-cycle saving is the
  entire delta for the ECDSA, Schnorr and P-256 rows.
- **Per-signature verify cost is unchanged** within ±2% (see below):
  the crypto crates and risc0 patches are the same. The outliers are
  Ed25519 (+1.8%/sig, from `ed25519-dalek` 2.1.1 → 2.2.0) and LMS
  (−1.1%/sig, not investigated; plausibly codegen from the newer guest
  toolchain).
- **Prove time fell where a row crossed a boundary.** `ecdsa-p256`
  N=1 now fits a 2^18 segment (**69.5 s → 31.9 s**), `ecdsa-secp256k1`
  N=3 now fits one segment instead of two, and `noop` dropped to 2^15.
  Rows with the same segment shape are mixed: `ed25519` N=1 (−2%
  cycles) is 13% faster, while `schnorr-secp256k1` N=3 (−9% cycles) is
  flat. The rc3 side is a single run and runs here spread up to 19%,
  so those residuals are within noise; the risc0 3.0.5 → 3.1.0 prover
  change was not isolated.

### Per-signature cycle deltas (subtracting noop)

| scheme | user cycles / sig (N=1) | user cycles / sig (≈ from N=3) | v0.2.0-rc3, N=1 |
|---|---:|---:|---:|
| ecdsa-secp256k1 | 303 881 | 299 414 | 303 377 |
| schnorr-secp256k1 | 271 620 | 271 926 | 270 651 |
| ecdsa-p256 | 198 109 | 195 194 | 198 246 |
| ed25519 | 818 087 | 818 359 | 803 244 |
| lms | 2 652 560 | 2 707 643 | 2 681 927 |

**Headline takes:**

- **P-256 ECDSA is the cheapest** verify on this stack — about
  **35% fewer cycles per sig than secp256k1 ECDSA** at N=1, and the only
  scheme whose N=1 proof fits a 2^18 segment (~32 s). The same RISC0
  `bigint2` curve precompile applies and the field is similar cost; what
  differs is keccak256 (k256 path) vs sha256 (p256 path) for the message
  digest.
- **Schnorr secp256k1 ≈ 11% cheaper than ECDSA secp256k1** per sig.
  The expected win from skipping the modular inversion is partially
  offset by Schnorr's `ProjectivePoint::lincomb(G, s, P, -e)` doing
  one combined mul vs ECDSA's separate mul + recovery. Same precompile
  path, smaller win than naive theory predicts.
- **Ed25519 is by far the most expensive** EC scheme — **2.7× the user
  cycles** of secp256k1 ECDSA. The RISC0 curve25519-dalek backend is
  available and active, but Edwards arithmetic plus the in-algorithm
  sha512 (no zkVM precompile) dominates.
- **Multi-sig scaling is linear** for all schemes — about **+300K user
  cycles per added secp256k1 ECDSA sig**, **+820K** per Ed25519 sig. No
  batch-verify shortcuts here (out of scope per [`SPEC.md`](./SPEC.md) §9).
- **LMS is ~8.7× the cycles and ~2.9× the prove time of secp256k1
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
- **Oracle notes:** see [Oracle signing: DIA vs RedStone](#oracle-signing-dia-vs-redstone).
  Both sign feeds with ECDSA secp256k1, so the `ecdsa-secp256k1` rows
  are the ones that apply to them today.

### Oracle signing: DIA vs RedStone

Read from the oracles' own contracts and SDKs
(diadata-org/Spectra-interoperability @ `b1b4fb9`,
redstone-finance/redstone-oracles-monorepo @ `251d6a4`,
redstone-finance/rust-sdk @ `05e3c9f`):

| | DIA (Lumina intents) | RedStone (pull payload) |
|---|---|---|
| Scheme | ECDSA secp256k1, 65-byte r‖s‖v, low-s enforced | ECDSA secp256k1, 65-byte r‖s‖v, low-s enforced |
| Digest | EIP-712: `keccak(0x1901 ‖ domainSeparator ‖ structHash)` over a 9-field struct (intentType, version, chainId, nonce, expiry, symbol, price, timestamp, source) ([`OracleIntentUtils.sol`](https://github.com/diadata-org/Spectra-interoperability/blob/main/contracts/contracts/libs/OracleIntentUtils.sol)) | plain `keccak256` of the raw data package (feed id ‖ value per data point, then timestamp ‖ value size ‖ count; 77 B for one feed), no `eth_sign` prefix |
| Signer check | `ecrecover` must equal the intent's `signer`, which must be on an owner-managed allowlist: **1-of-n** | `ecrecover` on every package, unique-signer bitmap, **3-of-5** hard-coded production signers |
| Other checks | replay by hash, newer timestamp wins; the registry checks expiry | all package timestamps equal, at most 3 min old / 1 min ahead; median of the values |
| Signatures per update | 1 per symbol (batches are separate signed intents) | 3 per feed, **each over a different message** (each node signs its own value) |
| Other schemes | none found | non-EVM connectors (Solana, Sui, Starknet, …) still use secp256k1 recovery; RedStone's EigenLayer AVS uses BLS aggregation (docs only) |

What a verifier on LEZ costs, from the rows above:

- **DIA ≈ `ecdsa-secp256k1` N=1** (~2:18 local prove, ~11:51 end to
  end, medians), plus ~5 small keccak calls for the EIP-712 hashes
  instead of our 1. The signer's address is in the struct, so the guest
  can verify against a known public key instead of recovering it.
- **RedStone ≈ `ecdsa-secp256k1` N=3** (~3:28 local, ~12:54 end to end,
  medians), with one difference from our fixtures: the 3 signers sign
  3 different packages, so it needs 3 package hashes rather than 1
  shared digest. The payload does not name its signers, so a guest
  either recovers each key (one more keccak each) or takes a signer
  hint from the host and verifies against allowlisted keys. Add payload
  parsing, the uniqueness and freshness checks and the median, all small
  in cycles next to the signature verifies.
- Neither oracle signs with Schnorr or P-256, so the cheaper rows would
  need the oracle to change its signing scheme.

### Decision note: budget → scheme on this laptop

Given the local prove times above on a CPU-only Ryzen 9 7940HS (the
inner proof only; see the end-to-end section for what a private TX
adds):

| TX prove budget | What fits |
|---|---|
| **15 s** | `noop` baseline (~0:04, no crypto) |
| **1 min** | `ecdsa-p256` n=1 (~0:32) |
| **1.5 min** | adds `schnorr-secp256k1` n=1 (~1:06) |
| **2.5 min** | adds `ecdsa-p256` n=3 (~2:11), `ecdsa-secp256k1` n=1 (~2:18), `ed25519` n=1 (~2:19), `schnorr-secp256k1` n=3 (~2:27) |
| **4 min** | adds `ecdsa-secp256k1` n=3 (~3:28) |
| **7 min** | adds `ed25519` n=3 (~6:30), `lms` n=1 (~6:39) |
| **20 min** | adds `lms` n=3 (~18:37) |

Times are medians. The slowest of the three runs is up to ~11% above
the median (`schnorr-secp256k1` n=3 reached 2:37, `lms` n=1 7:01, `lms`
n=3 20:42), so leave that much headroom.

For interactive RedStone-style oracle UX (3-of-N pulls, sub-30 s),
**no scheme fits on CPU** even before the private-TX wrapping. CUDA /
Bonsai would compress this dramatically; CPU alone is too heavy for
low-latency UX.

For batch / async workloads, **P-256 or secp256k1 Schnorr at N=3** is
the budget pick. If keys / addresses must stay secp256k1 (Ethereum
compat, DIA), Schnorr is the natural step up from ECDSA.

### End-to-end private-TX time (against a local LEZ `dev` sequencer)

The numbers above isolate the inner proving cost. The number a user
actually feels — "click submit on a privacy-preserving transaction →
confirmation back from the sequencer" — adds the privacy-preserving
circuit (which proves the actor transition and the in-proof fee
payment on top of our verifier), compression to a succinct receipt,
and the sequencer roundtrip. Measured against a fresh standalone LEZ
`dev` sequencer on the same machine (sequencer `RISC0_DEV_MODE=1`,
client `RISC0_DEV_MODE=0`), one funded private account reused across
rows, three runs per row. (Run 1 used one sequencer home and private
account; runs 2 and 3 a fresh pair, because the standalone sequencer's
in-memory mock Bedrock does not survive a restart.) Wrap overhead is the
median E2E minus the median local prove; the rc3 column is a single run.

| scheme | N | E2E private TX (median) | range | local prove (median) | wrap overhead | v0.2.0-rc3 E2E |
|---|---:|---:|---:|---:|---:|---:|
| `noop` | 1 | 610.1 s (~10:10) | 587.6–620.9 s | 4.3 s | +606 s | 103.5 s (~1:43) |
| `ecdsa-secp256k1` | 1 | 711.1 s (~11:51) | 702.0–778.5 s | 137.5 s | +574 s | 246.5 s (~4:06) |
| `ecdsa-secp256k1` | 3 | 774.5 s (~12:54) | 773.4–817.8 s | 207.6 s | +567 s | 446.1 s (~7:26) |
| `schnorr-secp256k1` | 1 | 646.4 s (~10:46) | 644.9–673.3 s | 66.1 s | +580 s | 153.7 s (~2:34) |
| `schnorr-secp256k1` | 3 | 739.0 s (~12:19) | 711.5–768.2 s | 147.5 s | +592 s | 323.0 s (~5:23) |
| `ed25519` | 1 | 740.6 s (~12:21) | 711.0–777.4 s | 138.8 s | +602 s | 283.0 s (~4:43) |
| `ed25519` | 3 | 1047.2 s (~17:27) | 1016.7–1243.6 s | 390.4 s | +657 s | 669.7 s (~11:10) |
| `ecdsa-p256` | 1 | 621.2 s (~10:21) | 614.0–653.1 s | 31.9 s | +589 s | 152.5 s (~2:33) |
| `ecdsa-p256` | 3 | 717.1 s (~11:57) | 711.0–743.6 s | 130.8 s | +586 s | 298.8 s (~4:59) |
| `lms` | 1 | 1014.0 s (~16:54) | 1007.3–1062.1 s | 398.6 s | +615 s | — |
| `lms` | 3 | 1811.2 s (~30:11) | 1792.5–1868.1 s | 1116.8 s | +694 s | — |

**The private-TX wrapper now dominates: ~570–610 s per TX** for any
kernel that fits one segment, up from ~75–105 s on v0.2.0-rc3. It grows
only a little with the kernel's segment count (+615 s and +657 s for the
3-segment `lms` N=1 and `ed25519` N=3, +694 s for the 9-segment `lms`
N=3), as each kernel segment is lifted and joined into the succinct
receipt. (Run 1 alone put `ed25519` N=3 at 1243.6 s, +861 s; runs 2 and
3 did not repeat it.)
Executing one `noop` private TX with `RISC0_INFO=1` shows where the
fixed part goes: the privacy-preserving circuit alone is **3 407 872
total / 2 818 870 user cycles in 4 segments** (1 711 SHA-256 calls,
192 keccak), i.e. about the size of an `lms` N=1 verify, proven and
compressed on every private TX. The rest is the kernel's own proof and
recursion plus up to one 15 s block interval (`block_create_timeout`).
The `dev` circuit was not profiled further (actor execution, ML-KEM
note encryption, nullifiers and the proven fee are all inside it).

#### Comparison with LEZ's own benchmarks

LEZ publishes prover numbers in
[`docs/benchmarks/cycle_bench.md`](https://github.com/logos-blockchain/logos-execution-zone/blob/dcbd318f0f2ef601fc93e67e0dabc49895d811ef/docs/benchmarks/cycle_bench.md),
but they were measured on 2026-10-06 (Apple M2 Pro, CPU only, risc0
3.0.5), before the actor model (#935, merged 2026-10-14) and the proven
fee (#1125). Running LEZ's own harness, unmodified, at `dcbd318f0` on
this laptop (`cargo run --release -p cycle_bench --features ppe -- --ppe`):

| `cycle_bench --ppe` case | this laptop, `dev` | LEZ docs (M2 Pro, pre-actor) |
|---|---:|---:|
| native Transfer in PPE, 0 dummy inputs | 242.5 s | 18.0 s |
| native Transfer in PPE, 6 dummy inputs (wallet padding) | **228.5 s** | 25.3 s (7 dummies) |
| scripted actor → native Transfer, depth 1 / 3 | 262.4 / 253.2 s | — |
| scripted actor → native Transfer, depth 5 / 9 | 295.9 / 317.2 s | — |
| proof size | 223 047 B | 223 047 B |

(The published depth sweep used a `chain_caller` program that `dev`
replaced with `scripted`, so those rows are not comparable. The
depth-5 case spanned a laptop suspend; Rust's `Instant` excludes
suspended time.)

- **Inner proving matches LEZ's docs** after the hardware difference:
  both show prove time following the padded segment size, at ~73 µs
  per padded cycle there vs ~120–140 µs here (~1.8×).
- **The privacy circuit does not.** A bare private native transfer
  costs ~230–260 s here, where the published numbers predict ~45–80 s
  on this laptop. So on `dev` the privacy circuit itself is roughly
  **5× costlier** than LEZ's docs show; this reproduces with LEZ's own
  harness, independent of our guests and wallet path.
- **Our E2E wrapper (~600 s) is still ~350 s above LEZ's harness.** Our
  path adds a call into a *deployed* program (its receipt is compressed
  and verified inside the circuit), the fee proven in-circuit, the
  wallet and sequencer roundtrip, and up to one 15 s block. That split
  was not profiled.

Consequences:

- **The scheme matters much less end-to-end.** `ecdsa-p256` N=1 is
  only ~11 s slower than `noop`; the full spread across EC schemes at
  N=1 is 10:21 → 12:21 (it was 2:33 → 4:43 on rc3).
- Ranking is unchanged: `ecdsa-p256` < `schnorr-secp256k1` <
  `ecdsa-secp256k1` < `ed25519` < `lms`.
- The rc3 E2E rows were captured with pre-optimization binaries (before
  [Pass 3](#optimization-passes)), so the rc3 → `dev` ratio slightly
  understates the slowdown.

Net for the headline RedStone shape — **3-of-N pull, end-to-end** —
and for a DIA-style single signed intent:

| scheme | N=3 E2E | N=1 E2E |
|---|---:|---:|
| `ecdsa-p256` | **11:57** (cheapest) | 10:21 |
| `schnorr-secp256k1` | 12:19 | 10:46 |
| `ecdsa-secp256k1` | 12:54 | 11:51 (DIA) |
| `ed25519` | 17:27 | 12:21 |
| `lms` | 30:11 | 16:54 |

For interactive UX (sub-30 s) **nothing fits on CPU**, and on `dev`
even a no-crypto private TX takes ~10 minutes here. CUDA / Bonsai, or a
cheaper privacy circuit, is the lever now, not the signature scheme.

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

Each local-prove row is a **real prove** through
`risc0_zkvm::default_prover()` (r0vm 3.1.0) with `RISC0_DEV_MODE=0`,
against the actor-model guest binary for that scheme. The guest's stdin
is exactly what the LEZ engine writes for a root call: a 4-byte
little-endian length followed by the borsh-encoded `ReceiveInput`
(`from: None`, not authorized, empty pre-state, the borsh `VerifyInput`
as `message`), built by `lez_signature_bench::guest_input_frame`.
Cycles and segment count come from `ProveInfo.stats`; prove time is
wall-clock around the `prove(...)` call; receipt size is
`bincode::serialize(&receipt).len()`.

Each E2E row submits a real privacy-preserving transaction via
`wallet::WalletCore::send_privacy_preserving_tx()` against a local
standalone LEZ `dev` sequencer (see [Run the bench](#run-the-bench)).
Before the timed rows, each guest is deployed once through the program
loader, paid by `--payer`; deploys are not timed. The wall clock wraps
`send_privacy_preserving_tx` only, so it includes serialization,
client-side proving (kernel + privacy-preserving circuit, compressed
to a succinct receipt), the sequencer roundtrip, and confirmation. On
`dev` every transaction pays a fee; with no public fee payer the wallet
pays it inside the proof from the funded private account.

Synthetic fixtures only — no captured oracle payloads. All signers
share the same message (`b"hello redstone"`) per row, signed with
deterministic seeds.

### Run-to-run variance

Each matrix (local and E2E) was run three times: run 1 on 2026-11-20
(not monitored), run 2 on 2026-11-21 (local in the afternoon, E2E
overnight) and run 3 on 2026-11-22 (overnight, back to back). Runs 2 and
3 logged every 10 s: host-wide CPU use (`/proc/stat`) against the
container's own (its cgroup `cpu.stat`), which gives the load from
everything else on the host; swap traffic (`/proc/vmstat`); and core
clocks with `k10temp` Tctl. Cycle counts and receipt sizes were
identical in all three runs.

| | run 1 local | run 1 E2E | run 2 local | run 2 E2E | run 3 local | run 3 E2E |
|---|---:|---:|---:|---:|---:|---:|
| other host load, mean (cores of 16) | — | — | 0.89 | 0.82 | 0.77 | 0.74 |
| other host load, max | — | — | 2.17 | 9.29 | 1.46 | 1.35 |
| 10 s samples above 1.5 cores | — | — | 14 | 26 | 0 | 0 |
| all-core clock, mean | — | — | 3.69 GHz | 3.71 GHz | 3.73 GHz | 3.72 GHz |
| Tctl, mean | — | — | 99.9 °C | 96.1 °C | 98.9 °C | 97.3 °C |
| matrix total (timed rows only) | 2 656 s | 9 658 s | 2 946 s | 9 744 s | 2 768 s | 9 309 s |

(Run 2's local clock covers only its second half, from when the clock
logger started.)

- **Host load does not explain the slow numbers.** Run 3 had a quiet
  host throughout (never above 1.5 other cores, no suspend) and landed
  between runs 1 and 2 on the local matrix and 4% below both on E2E.
  The ~10 min private TX reproduces on an idle machine.
- **The bursts that did happen line up with run 2's slow E2E rows:** a
  compile on the host (up to 9.3 cores, 23:25–23:32) during
  `ecdsa-secp256k1` N=1 (778.5 s vs 702.0 / 711.1 s), heavy swap-in
  during `ecdsa-secp256k1` N=3 (817.8 s vs 773.4 / 774.5 s), and ~2.3
  cores plus swap during `ecdsa-p256` N=1 (653.1 s vs 621.2 / 614.0 s).
  Medians drop these.
- **The CPU is thermally limited throughout.** Tctl averaged 96–100 °C
  over every monitored matrix (≥ 99 °C for 83% of run 3's samples), with
  all cores near 3.7 GHz. With the host quiet, how well the laptop sheds
  heat is the most likely source of the remaining spread; it was not
  tested.
- **Unexplained outliers:** run 1's `ed25519` N=3 E2E (1243.6 s vs
  1047.2 / 1016.7 s; not monitored), run 2's `lms` N=3 local (1242.1 s
  vs 1055.6 / 1116.8 s, quiet host) and run 3's `ecdsa-secp256k1` N=1
  local, which was *faster* (114.9 s vs 137.5 / 140.7 s).

Spread is max − min over the median: 2–19% (median 9%) for local
proves, 4–22% (median 6%) end to end.

| scheme | N | local run 1 | run 2 | run 3 | spread | E2E run 1 | run 2 | run 3 | spread |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `noop` | 1 | 4.4 s | 4.3 s | 3.9 s | 12% | 610.1 s | 620.9 s | 587.6 s | 5% |
| `ecdsa-secp256k1` | 1 | 137.5 s | 140.7 s | 114.9 s | 19% | 702.0 s | 778.5 s | 711.1 s | 11% |
| `ecdsa-secp256k1` | 3 | 207.6 s | 211.4 s | 207.6 s | 2% | 773.4 s | 817.8 s | 774.5 s | 6% |
| `schnorr-secp256k1` | 1 | 66.1 s | 68.4 s | 64.9 s | 5% | 644.9 s | 673.3 s | 646.4 s | 4% |
| `schnorr-secp256k1` | 3 | 133.6 s | 157.1 s | 147.5 s | 16% | 768.2 s | 739.0 s | 711.5 s | 8% |
| `ed25519` | 1 | 127.5 s | 139.4 s | 138.8 s | 9% | 777.4 s | 740.6 s | 711.0 s | 9% |
| `ed25519` | 3 | 382.5 s | 416.2 s | 390.4 s | 9% | 1243.6 s | 1047.2 s | 1016.7 s | 22% |
| `ecdsa-p256` | 1 | 31.7 s | 33.1 s | 31.9 s | 4% | 621.2 s | 653.1 s | 614.0 s | 6% |
| `ecdsa-p256` | 3 | 127.8 s | 134.8 s | 130.8 s | 5% | 717.1 s | 743.6 s | 711.0 s | 5% |
| `lms` | 1 | 381.7 s | 398.6 s | 420.6 s | 10% | 1007.3 s | 1062.1 s | 1014.0 s | 5% |
| `lms` | 3 | 1055.6 s | 1242.1 s | 1116.8 s | 17% | 1792.5 s | 1868.1 s | 1811.2 s | 4% |

### Optimization passes

*Measured on LEZ v0.2.0-rc3 / risc0 3.0.5, before the `dev` migration.
The profile still applies.*

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
  looped segments sequentially (`local_prover/prover_impl.rs` line 89:
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

### Keccak coprocessor note

`ecdsa-secp256k1` is the only scheme that hashes with keccak256, and
`tiny-keccak`'s RISC0 patch routes each permutation through the keccak
coprocessor. The coprocessor's permutations are proven in a **separate
keccak STARK**, attached to the receipt as an assumption, so neither its
size nor its prove time shows up in the main segment's cycle count:

| | `schnorr-secp256k1` | `ecdsa-secp256k1` | Δ |
|---|---:|---:|---:|
| N=1 (both one 2^19 segment): receipt | 268 680 B | 491 815 B | +223 KB |
| N=1: prove time, median (range) | 66.1 s (64.9–68.4) | 137.5 s (114.9–140.7) | **+71 s** (+50 to +72 per run) |
| N=3 (both one 2^20 segment): receipt | 282 344 B | 505 483 B | +223 KB |

At N=3 the `ecdsa-secp256k1` row (median 207.6 s) is ~60–75 s above
the other single-2^20-segment rows (130.8–147.5 s). The digest is
computed once per message, so one keccak proof covers all N signers.
That ~50–75 s and ~223 KB are a fixed cost of using keccak at
all; switching the prehash to sha256 would remove them but break
Ethereum compatibility, which DIA and RedStone both need. The extra
keccak work a real oracle verifier adds (EIP-712 struct hashing, or one
package hash per RedStone signer) is a few more permutations; whether
they still fit in one keccak proof was not measured. The privacy
circuit itself also calls keccak (192 calls per private TX).

## Build

```bash
cargo build --workspace --release
```

`risc0-build` cross-compiles seven guest ELFs (one per scheme + the
noop baseline; `lms` and `lms-w1` share one) for
`riscv32im-risc0-zkvm-elf` and embeds them as
`{ECDSA_SECP256K1,SCHNORR_SECP256K1,ED25519,ECDSA_P256,LMS,XMSS,NOOP}_ELF`.

The `e2e` feature (on by default) pulls in the LEZ wallet and its whole
dependency tree (rocksdb, keycard / `libpcsclite`, the Logos Blockchain
circuits). For the local-prove bench alone:

```bash
cargo build --release --no-default-features
```

## Run the bench

```bash
# One scheme + N
RISC0_DEV_MODE=0 cargo run --release --bin bench -- \
  --scheme ecdsa-secp256k1 --n 1

# Full matrix → results/results.json + results/README-snippet.md
RISC0_DEV_MODE=0 cargo run --release --bin bench -- --all

# Public-execution matrix: execute without proving, as the sequencer
# does for a public TX, under the 10M-cycle MAX_GAS_EXEC cap
cargo run --release --no-default-features --bin bench -- \
  --all --public --label public
# → results/results-public.json + results/README-snippet-public.md

# Noop guest on another scheme's fixture (input-handling cost alone)
cargo run --release --no-default-features --bin bench -- \
  --scheme noop --noop-fixture xmss --n 3 --public

# Generate just the JSON fixture for one (scheme, N) point
cargo run --release --bin gen_test_vectors -- \
  --scheme schnorr-secp256k1 --n 3
```

`RISC0_DEV_MODE=1` skips actual proving and prints fake numbers.
The bench logs a warning when this is set; never quote those numbers.

### End-to-end against a local LEZ `dev` sequencer

`lgs` (logos-scaffold) pins older LEZ releases and has no actor-model
support yet, so the E2E run uses a LEZ `dev` checkout at the same rev
directly:

```bash
# In logos-execution-zone @ dcbd318f0
cargo build --release -p sequencer_service --features sequencer_service/standalone -p wallet
./target/release/sequencer_service setup "$SEQ_HOME" \
  --config lez/sequencer/service/networks/local.json \
  --funding-public-key 2e03b2eff5a45478e7e79668d2a146cf2c5c7925bce927f2b1c67f2ab4fc0d26 \
  --create-channel
RISC0_DEV_MODE=1 ./target/release/sequencer_service start "$SEQ_HOME"

# Wallet (unencrypted storage), the two genesis-funded test accounts,
# and a funded private account. The test accounts' keys are in LEZ's
# Justfile (`wallet-import-test-accounts`); that recipe forces its own
# wallet home, so import them by hand:
export LEE_WALLET_HOME_DIR=$WALLET_HOME LEE_WALLET_PASSWORD=
cp lez/wallet/configs/debug/wallet_config.json "$WALLET_HOME"/
wallet check-health
wallet account import public --private-key <test key 1 from LEZ's Justfile>
wallet account import public --private-key <test key 2 from LEZ's Justfile>
wallet account list                      # note a Private/<id>
RISC0_DEV_MODE=1 wallet auth-transfer send \
  --from Public/<test account 1> --to Private/<id> --amount 1000000000000

# Back in this repo
RISC0_DEV_MODE=0 cargo run --release --bin bench -- \
  --all --account-id Private/<id> --payer Public/<test account 2> --label e2e
# → results/results-e2e.json + results/README-snippet-e2e.md
```

> **Standalone sequencer at `dcbd318f0` needs a one-line patch.** Its
> mock Bedrock reports a channel tip slot with no tip message, which the
> executor (since #955) reads as "channel configured but holds no
> blocks" and refuses to start on a fresh home (`ChannelWithoutBlocks`).
> For these runs the mock in `lez/sequencer/service/src/lib.rs` was
> changed to report no channel until `create_channel` has run. It only
> affects how the local sequencer boots, not what is measured.

The local matrix takes **~45–50 minutes** on the reference machine; the
end-to-end matrix takes **~2 h 45–55 min** (kernel + privacy circuit +
sequencer roundtrip per row, plus six untimed deploys).

## Lint and test

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --release --workspace
#   17 host tests: byte-stable borsh wire format (ecdsa-k1, lms),
#      positive + negative round-trip per scheme, LMS W=1 really is W=1
#    3 guest tests (tests/guest_exec.rs): every guest ELF run in the
#      RISC Zero executor with the engine's input frame — commits a
#      keep-state Transition, rejects a corrupted signature
```

CI (`.github/workflows/ci.yml`) runs the fmt + clippy + build set on
every push and PR, with `RISC0_SKIP_BUILD=1` (stub ELFs; the guest tests
skip themselves). Bench runs are local only.

## Prerequisites

- Rust toolchain 1.98.1 (auto-picked via `rust-toolchain.toml`; LEZ
  `dev`'s pin).
- The RISC Zero toolchain, at LEZ `dev`'s CI pins. Install via
  [`rzup`](https://dev.risczero.com/api/zkvm/install):
  ```bash
  curl -L https://risczero.com/install | bash
  rzup install rust 1.94.1
  rzup install cpp 2024.1.5
  rzup install r0vm 3.1.0
  rzup install cargo-risczero 3.1.0
  ```
- For the `e2e` feature: a C/C++ toolchain, `libclang` (rocksdb
  bindgen) and `libpcsclite` (keycard), e.g.
  `dnf install gcc-c++ clang-devel pcsc-lite-devel`.
- ~45 minutes of CPU for the local matrix; a few hours for E2E.

## Layout

```
.
├── methods/
│   ├── build.rs                          # risc0_build::embed_methods()
│   └── guest/
│       ├── src/lib.rs                    # shared VerifyInput + per-scheme verifier modules
│       └── src/bin/
│           ├── ecdsa_secp256k1.rs        # actor-model guest, one per scheme
│           ├── schnorr_secp256k1.rs
│           ├── ed25519.rs
│           ├── ecdsa_p256.rs
│           ├── lms.rs                    # hash-based / post-quantum rows (W=8 and W=1)
│           ├── xmss.rs                   # hash-based / post-quantum row
│           └── noop.rs                   # entrypoint-only calibration
├── src/
│   ├── lib.rs                            # Scheme enum, fixtures, engine input frame, verify
│   ├── verifier/                         # host-callable verify per scheme (mirrors guest)
│   └── bin/
│       ├── bench.rs                      # local-prove, public-execution + E2E bench (single + --all)
│       └── gen_test_vectors.rs           # write JSON fixture for one (scheme, N)
├── tests/guest_exec.rs                   # guest ELFs in the executor
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
- **Profile the LEZ `dev` privacy-preserving circuit** (~2.8M user
  cycles per private TX; LEZ's own `cycle_bench --ppe` puts a bare
  private transfer at ~230 s on this laptop vs ~25 s in LEZ's pre-actor
  docs) and split our ~600 s E2E wrapper into circuit, deployed-program
  composition, proven fee and roundtrip. Re-run E2E once LEZ tags v0.4.
- **RedStone payload parsing**, with per-signer messages (each RedStone
  node signs its own package; our fixtures share one message).
- **DIA EIP-712 verifier** on top of the `ecdsa-secp256k1` N=1 guest,
  to measure whether the extra keccak calls stay in one keccak proof.

## License

MIT or Apache-2.0 (per workspace `Cargo.toml`).
