# contingency-dsl-rs

:gb: [English README](README.md)

[contingency-dsl](../contingency-dsl/) の Rust リファレンスパーサ。

外部ランタイム依存ゼロ。Python リファレンス実装 [`../contingency-dsl-py`](../contingency-dsl-py/) をミラーする。

## 状態

初期スケルトン。現時点で実装済み：

- Lexer（コアサブセット：原子スケジュール、数値、時間単位、句読点）

未実装：

- Parser, AST, extensions, annotations, experiment layer, respondent parser,
  serializer, conformance runner

## 使い方

```rust
use contingency_dsl::lexer::tokenize;

let tokens = tokenize("FR 10").unwrap();
```

## テスト

```bash
cargo test
```
