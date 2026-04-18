# contingency-dsl-rs

:jp: [日本語版 README](README.ja.md)

Rust reference parser for [contingency-dsl](../contingency-dsl/).

Zero external runtime dependencies. Mirrors the Python reference
implementation in [`../contingency-dsl-py`](../contingency-dsl-py/).

## Status

Conformance against the language-independent suite at
[`../contingency-dsl/conformance/`](../contingency-dsl/conformance/)
covers three channels:

- **shape** — fixtures with `expected` / `pre_expansion` produce the
  matching JSON shape: **542 / 542**
- **warning** — fixtures with `warnings` emit each listed code via
  `collect_warnings()`: **69 / 69**
- **error** — fixtures with `error` / `expected_error` produce a
  `DslError` whose kind and code match: **283 / 383** (73.9%)
- **total**: 894 / 994 (89.9%)

Remaining error-channel fixtures are fine-grained annotation-extension
validators (SESSION_*, TRAINING_*, TESTING_*, DEPENDENT_MEASURE_*,
LOGGING_*, etc.) whose exact thresholds and enum vocabularies vary
from the fixture prose; the analyzer framework is in place and can
accept more rules as they are specified.

Exercised fixture categories:

- **Operant core** — atomic, special (EXT/CRF), second-order, compound
  (all combinators; positional args, scalar kwargs COD/FRCO/BO,
  PUNISH in all three forms, directional COD, Overlay target,
  Interpolate count/onset), DR / PR / Pctl / Lag / Repeat modifiers,
  LimitedHold postfix, TO and ResponseCost postfix, let bindings,
  program-level parameter declarations
- **Aversive** — Sidman, DiscrimAv, Escape with canonical parameters
  and aliases
- **Stateful** — Adjusting, Interlocking, Percentile
- **Trial-based** — MTS, GoNoGo with defaults and correction-mode
- **Respondent Tier A** — Pair.{ForwardDelay, ForwardTrace,
  Simultaneous, Backward}, Extinction, CSOnly, USOnly, Contingency
  (all three signature forms), TrulyRandom (with optional p),
  ExplicitlyUnpaired, Compound (with mode), Serial, ITI, Differential
  (both full and short form)
- **Annotations** — `@keyword(...)` in program and schedule scope with
  positional strings, keyword arguments, numeric values with time-unit
  or percent suffix, list and object literals, negative numbers, and
  keyword aliases (punisher → reinforcer, hw → hardware, subject →
  subjects). Schedule-scoped routing for `trial_mix` and for
  `training` / `testing` on declaration-mode MTS
- **Experiment** — `phase <Label>:` blocks with FixedSessions,
  Stability (visual / cv / rate / iri / latency methods), and
  ExperimenterJudgment criteria; `shaping <Label>:` blocks with
  artful / percentile methods, stable() and sessions criteria;
  `progressive <Label>:` blocks with `steps v = [...]` template
  expansion into N labelled phases (`<Label>_1`..`<Label>_N`);
  `experiment <Label>:` paper blocks (single-schedule or phase-
  sequence body, with per-experiment annotation overrides); `use
  <Label>` phase-reference resolution; digit-form phase and experiment
  labels
- **Composed** — autoshape, omission, two-process-theory,
  conditioned-suppression, pit (phase-wrapped respondent / compound
  procedures)
- **Analyzer** — binding-expansion pass that substitutes `let`-bound
  IdentifierRef nodes with their values throughout the schedule tree,
  including inside `SecondOrder` overall and unit slots

Not yet exercised:

- Fine-grained annotation validators beyond the core semantic checks
  (SESSION_*, TRAINING_*, TESTING_*, DEPENDENT_MEASURE_* and related
  extension-registry-shaped codes). The analyzer framework is in
  place and extensible.

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
