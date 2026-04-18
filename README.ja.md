# contingency-dsl-rs

:gb: [English README](README.md)

[contingency-dsl](../contingency-dsl/) の Rust リファレンスパーサ。

外部ランタイム依存ゼロ。Python リファレンス実装 [`../contingency-dsl-py`](../contingency-dsl-py/) をミラーする。

## 状態

言語仕様の conformance スイート
[`../contingency-dsl/conformance/`](../contingency-dsl/conformance/)
に対する実装を進行中。`contingency-dsl-py` との全面パリティをタグ付けの目標とする。

現時点で conformance に対して形状一致を確認しているカテゴリ（各 fixture の
`pre_expansion` または `expected` ペイロードとのサブセットマッチ）：

- Operant の atomic、special（EXT / CRF）、second-order スケジュール
- Operant の compound スケジュール（位置引数とスカラー kwargs: COD / FRCO / BO）。
  PUNISH、directional COD、Overlay を除く
- DR modifier（DRL / DRH / DRO）と `Repeat(N, schedule)`
- `let` バインディングとプログラム単位パラメータ宣言
  （`LH`, `RD`, `COD`, `FRCO`, `BO`）
- Limited-hold の後置（`schedule LH value`）

未対応：

- PR / Pctl / Lag modifier、嫌悪スケジュール（Sidman / DiscrimAv / Escape）、
  状態依存スケジュール（Adjusting / Interlocking / Percentile）、
  試行ベース（MTS / GoNoGo）、Timeout / ResponseCost 後置、Interpolated
- Respondent 層（Pair / Extinction / CSOnly / USOnly / Contingency /
  TrulyRandom / ExplicitlyUnpaired / Compound / Serial / ITI / Differential）
- Composed procedure（phase-block サーフェス構文）
- Experiment 層（phase / shaping / progressive / experiment ブロック）
- Annotation（`@keyword(...)` と extension registry）
- 意味解析器（binding 展開、LH 伝搬、予約語チェック、warning チャネル）
- `expected_error` fixture 用エラーコードチャネル

## 使い方

```rust
use contingency_dsl::{parse, program_to_json, json_to_string};

let program = parse("let baseline = VI60s\nbaseline").unwrap();
println!("{}", json_to_string(&program_to_json(&program)));
```

Lexer 単体の利用も引き続き可能：

```rust
use contingency_dsl::lexer::tokenize;

let tokens = tokenize("FR 10").unwrap();
```

## テスト

```bash
cargo test
```

Conformance ハーネス（[`tests/conformance.rs`](tests/conformance.rs)）は
言語仕様の全 fixture を走査してファイル単位の通過率を出力する：

```bash
cargo test --test conformance -- --nocapture
```
