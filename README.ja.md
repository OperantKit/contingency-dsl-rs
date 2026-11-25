# contingency-dsl-rs

:gb: [English README](README.md)

[contingency-dsl](https://github.com/OperantKit/contingency-dsl) の Rust のリファレンスパーサ。文字列を読み、構文木を返す。Python 版 ([contingency-dsl-py](https://github.com/OperantKit/contingency-dsl-py)) と同じ文法を読む、もう 1 つのリファレンスパーサである。どちらも contingency-dsl の適合テスト (conformance tests) に合格することで同じ言語とみなす。

実行時に依存する外部の crate は無い。中核のアノテーション拡張 6 種を同梱する。

## アノテーション拡張

6 種のコア annotator (`stimulus`, `temporal`, `subjects`, `apparatus`,
`composed`, `measurement`) は `contingency_dsl::annotations` 配下に
同梱されている。contingency-dsl-py と同じ 6 種である:

```rust
use contingency_dsl::annotations::{StimulusExtension, MeasurementExtension};
use contingency_dsl::extensions::ExtensionRegistry;

let registry = ExtensionRegistry::build(vec![
    Box::new(StimulusExtension::default()),
    Box::new(MeasurementExtension::default()),
])?;
```

`AnnotationExtension` trait と `ExtensionRegistry` は
[`src/extensions.rs`](src/extensions.rs) に定義されている
(Kahn 法による `requires()` トポロジカルソート、キーワード
disjointness 検査)。サードパーティの crate は本 trait を実装して
ビルトインと並列に登録できる。

## 適合テスト

テストは、言語に依存しない [contingency-dsl の `conformance/`](https://github.com/OperantKit/contingency-dsl/tree/main/conformance) のケースを読み、5 つの面を調べる:

- **shape** — `expected` / `pre_expansion` のあるケースが、対応する JSON の形になる
- **warning** — `warnings` のあるケースで、挙げられた各コードが `collect_warnings()` から出る
- **error** — `error` / `expected_error` のあるケースが、kind と code の一致する `DslError` になるか、そのコードの診断が出る。`@clock`・`@warmup`・`@algorithm`・`@deprivation`・`@n` の値はパーサが受理し、注釈の検査器が報告する。これらのコードは検査器から出る
- **diagnostics** — 注釈のケースで `expected_diagnostics` のあるものは構文解析が通り、挙げられた各コードを拡張の registry が報告する (この crate でも Python の検査器でも出ない 4 件は [`tests/conformance_annotations.rs`](tests/conformance_annotations.rs) に並べてある)
- **algebra** — `lhs` / `rhs` / `relation` のあるケースで両辺を簡約し、`equivalent` なら Conc / Alt / Conj の成分の順序を除いて同じ結果になる (`not_equivalent` なら異なる)

まだ読んでいない物:

- `representations/` (T-τ スケジュール変換)

対応カテゴリ：

- **Operant コア** — atomic、special (EXT / CRF)、second-order、compound
  (全 combinator、位置引数、スカラー kwargs COD / FRCO / BO、PUNISH の 3 形式、
  directional COD、Overlay target、Interpolate count / onset)、
  DR / PR / Pctl / Lag / Repeat modifier、LimitedHold 後置、
  TO / ResponseCost 後置、`let` バインディング、プログラム単位パラメータ宣言
- **嫌悪スケジュール** — Sidman、DiscrimAv、Escape と別名、必須パラメータ検証
- **状態依存** — Adjusting、Interlocking、Percentile
- **試行ベース** — MTS、GoNoGo (デフォルト値・correction-mode まで)
- **Respondent Tier A** — Pair.{ForwardDelay, ForwardTrace, Simultaneous,
  Backward}、Extinction、CSOnly、USOnly、Contingency (3 形式)、
  TrulyRandom (p 対応)、ExplicitlyUnpaired、Compound (mode 付)、Serial、
  ITI、Differential (full / short)
- **Annotation** — `@keyword(...)` をプログラム／スケジュールスコープで解析。
  位置文字列、kwarg、時間単位／パーセント付き数値、リスト／オブジェクトリテラル、
  負の数値、別名正規化 (punisher → reinforcer など) に対応。
  `trial_mix`／declaration-mode MTS 上の `training`・`testing` は
  schedule-scope にルーティング
- **Experiment** — `phase <Label>:` ブロック (FixedSessions、Stability、
  ExperimenterJudgment)、`shaping <Label>:` ブロック (artful / percentile、
  stable()、sessions 基準対応)、`progressive <Label>:` ブロック
  (`steps v = [...]` テンプレート展開で N 個の `<Label>_1`..`<Label>_N`
  フェーズを生成)、`experiment <Label>:` paper ブロック (単一スケジュール／
  phase-sequence body、experiment 単位の注釈オーバーライド対応)、
  `use <Label>` フェーズ参照の解決、digit-form label
- **Composed** — autoshape、omission、two-process-theory、
  conditioned-suppression、pit
- **Analyzer** — `let` 束縛を schedule ツリー全体および `SecondOrder`
  overall / unit 位置に展開する binding 展開。呼んだときだけ働く
  `simplify_algebra` は、オペラントの理論の書き換え規則 (単位元・零元、
  入れ子の複合スケジュールの平坦化、`Repeat` の展開) を当てる。構文解析は
  簡約しない。キーワード引数のある複合スケジュールは平坦化も縮約もせず、
  Chain の CRF は先頭のものだけを落とす。contingency-dsl-py の `analyze` は
  これらも書き換えるので、そこでは結果が異なることがある

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

適合テストは、このリポジトリの隣に置いた contingency-dsl の clone を読む:

```bash
git clone https://github.com/OperantKit/contingency-dsl
git clone https://github.com/OperantKit/contingency-dsl-rs
cd contingency-dsl-rs
cargo test
```

[`tests/conformance.rs`](tests/conformance.rs) の検証環境 (harness) は、ファイル単位の通過率を出力する。これは報告であり、一致しないケースがあってもテストは失敗しない。出力を表示して走らせるには:

```bash
cargo test --test conformance -- --nocapture
```
