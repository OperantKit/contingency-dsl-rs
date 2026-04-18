# contingency-dsl-rs

:gb: [English README](README.md)

[contingency-dsl](../contingency-dsl/) の Rust リファレンスパーサ。

外部ランタイム依存ゼロ。Python リファレンス実装 [`../contingency-dsl-py`](../contingency-dsl-py/) をミラーする。

## 状態

言語仕様の conformance スイート
[`../contingency-dsl/conformance/`](../contingency-dsl/conformance/)
上の全 non-error / non-warning fixture（474 / 474）が、
fixture の `pre_expansion` または `expected` ペイロードに対するサブセット
マッチを満たす。

対応カテゴリ：

- **Operant コア** — atomic、special（EXT / CRF）、second-order、compound
  （全 combinator、位置引数、スカラー kwargs COD / FRCO / BO、PUNISH の 3 形式、
  directional COD、Overlay target、Interpolate count / onset）、
  DR / PR / Pctl / Lag / Repeat modifier、LimitedHold 後置、
  TO / ResponseCost 後置、`let` バインディング、プログラム単位パラメータ宣言
- **嫌悪スケジュール** — Sidman、DiscrimAv、Escape と別名、必須パラメータ検証
- **状態依存** — Adjusting、Interlocking、Percentile
- **試行ベース** — MTS、GoNoGo（デフォルト値・correction-mode まで）
- **Respondent Tier A** — Pair.{ForwardDelay, ForwardTrace, Simultaneous,
  Backward}、Extinction、CSOnly、USOnly、Contingency（3 形式）、
  TrulyRandom（p 対応）、ExplicitlyUnpaired、Compound（mode 付）、Serial、
  ITI、Differential（full / short）
- **Annotation** — `@keyword(...)` をプログラム／スケジュールスコープで解析。
  位置文字列、kwarg、時間単位／パーセント付き数値、リスト／オブジェクトリテラル、
  負の数値、別名正規化（punisher → reinforcer など）に対応。
  `trial_mix`／declaration-mode MTS 上の `training`・`testing` は
  schedule-scope にルーティング
- **Experiment** — `phase <Label>:` ブロック（FixedSessions、Stability、
  ExperimenterJudgment）、`shaping <Label>:` ブロック（artful / percentile、
  stable()、sessions 基準対応）、`progressive <Label>:` ブロック
  （`steps v = [...]` テンプレート展開で N 個の `<Label>_1`..`<Label>_N`
  フェーズを生成）、`experiment <Label>:` paper ブロック（単一スケジュール／
  phase-sequence body、experiment 単位の注釈オーバーライド対応）、
  `use <Label>` フェーズ参照の解決、digit-form label
- **Composed** — autoshape、omission、two-process-theory、
  conditioned-suppression、pit
- **Analyzer** — `let` 束縛を schedule ツリー全体および `SecondOrder`
  overall / unit 位置に展開する binding 展開

未対応：

- `expected_error` fixture 用エラーコードチャネル
- `expected_warning` fixture 用ワーニングチャネル

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
