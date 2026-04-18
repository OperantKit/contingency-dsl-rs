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

- **Operant core** — atomic, special (EXT/CRF), second-order, compound
  (all combinators; positional args, scalar kwargs COD/FRCO/BO,
  PUNISH in all three forms, directional COD, Overlay target,
  Interpolate count/onset), DR / PR / Pctl / Lag / Repeat modifiers,
  LimitedHold postfix, TO and ResponseCost postfix, let bindings,
  program-level parameter declarations
- **Aversive** — Sidman, DiscrimAv, Escape with canonical parameters
  and aliases (partial; annotation-dependent cases pending)
- **Stateful** — Adjusting, Interlocking, Percentile
- **Trial-based** — MTS, GoNoGo with defaults (partial for
  annotation-dependent declaration-mode cases)
- **Respondent Tier A** — Pair.{ForwardDelay, ForwardTrace,
  Simultaneous, Backward}, Extinction, CSOnly, USOnly, Contingency
  (both signature forms), TrulyRandom, ExplicitlyUnpaired, Compound,
  Serial, ITI, Differential
- **Annotations** — `@keyword(...)` in program and schedule scope with
  positional strings, keyword arguments, numeric values with
  time-unit / percent suffix, list and object literals, negative
  numbers, and keyword aliases (punisher → reinforcer, hw → hardware,
  subject → subjects)
- **Experiment** — `phase <Label>:` blocks with FixedSessions,
  Stability (visual / cv / rate / iri / latency methods), and
  ExperimenterJudgment criteria

Not yet exercised:

- Composed fixtures that wrap a Conc/Mult host in a phase block
  (conditioned-suppression, pit)
- Experiment layer `shaping <Label>:`, `progressive <Label>:`, and
  `experiment <Label>:` paper blocks
- Semantic analyzer (binding expansion, LH propagation, reserved-word
  checks, warning channel, `use <Label>` phase reference resolution)
- Error-code channel for `expected_error` fixtures
- A handful of annotation-dependent fixtures across aversive, respondent,
  trial-based, and procedure-trial-structure layers that require
  declaration-mode MTS with stimulus classes, structured probe-policy
  payloads, or correction-procedure metadata

## Usage

```rust
use contingency_dsl::{parse_top_level, program_to_json, json_to_string, TopLevel};

match parse_top_level("let baseline = VI60s\nbaseline").unwrap() {
    TopLevel::Program(p) => println!("{}", json_to_string(&program_to_json(&p))),
    TopLevel::PhaseSequence(_) | TopLevel::Paper(_) => unreachable!(),
}
```

For single-schedule inputs the legacy `parse(..)` → `Program` API is
still available:

```rust
use contingency_dsl::{parse, program_to_json, json_to_string};

let program = parse("FR5").unwrap();
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
