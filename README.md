# contingency-dsl-rs

:jp: [日本語版 README](README.ja.md)

Rust reference parser for [contingency-dsl](../contingency-dsl/).

Zero external runtime dependencies. Mirrors the Python reference
implementation in [`../contingency-dsl-py`](../contingency-dsl-py/).

## Status

Early skeleton. Currently implemented:

- Lexer (core subset: atomic schedules, numbers, time units, punctuation)

Not yet implemented:

- Parser, AST, extensions, annotations, experiment layer, respondent parser,
  serializer, conformance runner

## Usage

```rust
use contingency_dsl::lexer::tokenize;

let tokens = tokenize("FR 10").unwrap();
```

## Testing

```bash
cargo test
```
