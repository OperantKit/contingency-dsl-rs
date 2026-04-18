# contingency-dsl-rs

:jp: [日本語版 README](README.ja.md)

Rust reference parser for [contingency-dsl](../contingency-dsl/).

Zero external runtime dependencies. Mirrors the Python reference
implementation in [`../contingency-dsl-py`](../contingency-dsl-py/).

## Status

In-progress implementation tracked against the language-independent
conformance suite at [`../contingency-dsl/conformance/`](../contingency-dsl/conformance/).
Full scope parity with `contingency-dsl-py` is the tagging target.

Currently exercised fixture categories (subset match against each
fixture's `pre_expansion` or `expected` payload):

- Operant atomic, special (EXT/CRF), and second-order schedules
- Operant compound schedules with positional args and scalar keyword
  args (COD / FRCO / BO) — excluding PUNISH, directional COD, Overlay
- DR modifiers (DRL / DRH / DRO) and `Repeat(N, schedule)`
- `let` bindings and program-level parameter declarations
  (`LH`, `RD`, `COD`, `FRCO`, `BO`)
- Limited-hold postfix (`schedule LH value`)

Not yet exercised:

- PR / Pctl / Lag modifiers, aversive schedules (Sidman / DiscrimAv /
  Escape), stateful (Adjusting / Interlocking / Percentile), trial-based
  (MTS / GoNoGo), Timeout / ResponseCost postfix, interpolated schedules
- Respondent layer (Pair / Extinction / CSOnly / USOnly / Contingency /
  TrulyRandom / ExplicitlyUnpaired / Compound / Serial / ITI / Differential)
- Composed procedures (phase-block surface syntax)
- Experiment layer (phase / shaping / progressive / experiment blocks)
- Annotations (`@keyword(...)` and extension registries)
- Semantic analyzer (binding expansion, LH propagation, reserved-word
  checks, warning channel)
- Error-code channel for `expected_error` fixtures

## Usage

```rust
use contingency_dsl::{parse, program_to_json, json_to_string};

let program = parse("let baseline = VI60s\nbaseline").unwrap();
println!("{}", json_to_string(&program_to_json(&program)));
```

Lexer-only use is still available:

```rust
use contingency_dsl::lexer::tokenize;

let tokens = tokenize("FR 10").unwrap();
```

## Testing

```bash
cargo test
```

The conformance harness at [`tests/conformance.rs`](tests/conformance.rs)
walks every fixture in the language spec and prints a per-file pass
rate. Run it with output visible:

```bash
cargo test --test conformance -- --nocapture
```
