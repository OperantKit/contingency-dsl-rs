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

- **Operant コア** — atomic、special（EXT / CRF）、second-order、compound
  （全 combinator、位置引数、スカラー kwargs COD / FRCO / BO、PUNISH の 3 形式、
  directional COD、Overlay target、Interpolate count / onset）、
  DR / PR / Pctl / Lag / Repeat modifier、LimitedHold 後置、
  TO / ResponseCost 後置、`let` バインディング、プログラム単位パラメータ宣言
- **嫌悪スケジュール** — Sidman、DiscrimAv、Escape と別名、必須パラメータ
  の検証（注釈依存ケースは未対応）
- **状態依存** — Adjusting、Interlocking、Percentile
- **試行ベース** — MTS、GoNoGo とデフォルト値
- **Respondent Tier A** — Pair.{ForwardDelay, ForwardTrace, Simultaneous,
  Backward}、Extinction、CSOnly、USOnly、Contingency（両引数形式）、
  TrulyRandom、ExplicitlyUnpaired、Compound、Serial、ITI、Differential
- **Annotation** — `@keyword(...)` をプログラム／スケジュールスコープで解析。
  位置文字列、kwarg、時間単位／パーセント付き数値、リスト／オブジェクトリテラル、
  負の数値、別名正規化（punisher → reinforcer など）に対応
- **Experiment** — `phase <Label>:` ブロックを FixedSessions、
  Stability（visual / cv / rate / iri / latency）、ExperimenterJudgment の
  基準で解釈

未対応：

- Phase-block で Conc/Mult を包む composed 手続き
  （conditioned-suppression、pit）
- Experiment 層の `shaping`、`progressive`、`experiment` ブロック（Paper）
- 意味解析器（binding 展開、LH 伝搬、予約語チェック、warning チャネル、
  `use <Label>` フェーズ参照の解決）
- `expected_error` fixture 用エラーコードチャネル
- declaration-mode MTS や probe_policy 等の複雑注釈に依存する一部 fixture

## 使い方

```rust
use contingency_dsl::{parse_top_level, program_to_json, json_to_string, TopLevel};

match parse_top_level("let baseline = VI60s\nbaseline").unwrap() {
    TopLevel::Program(p) => println!("{}", json_to_string(&program_to_json(&p))),
    TopLevel::PhaseSequence(_) | TopLevel::Paper(_) => unreachable!(),
}
```

単一スケジュール入力向けの旧 `parse(..)` → `Program` API も利用可能：

```rust
use contingency_dsl::{parse, program_to_json, json_to_string};

let program = parse("FR5").unwrap();
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
