# #5 T0 — 独自target生成契約の確定（設計提案）

対象issue: #5（「LAMINARIA-owned target lowering/code generation」の選定・実証、
`docs/research-intent-audit-2026-09-10.md:109`）。T0はそのうち「最初に採用する
target route」を確定する設計タスクであり、実装（T1以降）ではない。本書はゴール
設定担当（Claude）が候補比較・推奨案・契約を提出し、指示者(Codex)が確定する。

基準commit: `5f1832f2b6ff0a823c276ee4cb4f4340191b45ef`(#28 D1-a最終補正)。
本書のすべての事実主張は、下記の各行番号のファイルを直接読んで確認したもの
であり、issue #5自体の原文はこのリポジトリ内に存在しないため、想像で補って
いない(§0参照)。

---

## §0. issue #5のスコープについて — ローカルに存在する記録の限界

このリポジトリには issue #5 の本文そのものは存在しない(`docs/issue-5*.md`等
は存在しない)。ローカルで確認できるのは以下の間接的な言及のみ:

- `docs/research-intent-audit-2026-09-10.md:109`: 「#5 | Select and exercise
  LAMINARIA-owned target lowering/code generation. Existing compiler backend
  families remain role-separated reference experiments, not alternate
  production compilers.」
- `docs/issue-plan.md:113`: 「#5 answers **which backend route is valid and
  selected**. #13 answers how the selected backend expands into internal
  computation...」
- `docs/issue-plan.md:51`: 「4. Backend route selection and capability
  constraints — #5」
- `docs/compiler-ownership-contract.md:9-21`(必須パイプライン、後述§1で全文
  引用)の最終段「LAMINARIA target lowering and code generation → target
  artifacts with explicit runtime / assembly / link contracts」が#5の実体。

「T0」というラベル自体はリポジトリ内のどのファイルにも出現しない(全文grep
で確認済み)。本書では、この会話で与えられた定義「最初のtarget生成方式を
選ぶ設計タスク」をそのまま採用し、それ以上の解釈を加えていない。

---

## §1. 前提となる制約(`docs/compiler-ownership-contract.md`から直接引用)

- 5行目: 「LAMINARIA researches and develops its own compiler, intermediate
  representations, and scheduler for Rust and Nim. It is not a toolchain
  orchestrator whose compilation engine is Cargo/rustc, the Nim compiler, or
  an existing compiler backend.」
- 9-21行目(必須パイプライン、全文):
  ```
  Rust source / Nim source / both + resolved dependency sources
    → LAMINARIA language processing and semantic analysis
    → LAMINARIA-owned IR(s), semantic facts and provenance
    → LAMINARIA analysis / transformations / partition decisions
    → LAMINARIA planning and resource-aware compiler-work scheduling
    → LAMINARIA target lowering and code generation
    → target artifacts with explicit runtime / assembly / link contracts
  ```
- 38行目: 「Dependency acquisition is not permission to invoke `cargo build`,
  `rustc`, `nim c`, `nim cpp`, nlvm, Nimony, or C/LLVM compilation on the
  target-production path.」
- 61行目: 「LLVM/Cranelift/GCC projection may be studied as a comparison
  experiment. Making LLVM optional or invoking it as a library does not by
  itself establish compiler ownership.」

**この契約が定めているのは「target artifactを生成する経路(=コンパイル)を
LAMINARIA自身が持つこと」であり、「生成済みartifactを実行する主体まで
LAMINARIA自身が持つこと」ではない。** 実行主体(CPU、OS、WASM runtime等)を
外部に置くことは、x86-64をtargetにした場合にCPU自体を自作しないのと同じ
意味で、この契約の対象外である。§6でこの区別を契約文言に基づいて明示する。

---

## §2. 現行owned IRで生成可能な候補と制約

現行 `crates/laminaria-ir` (issue #25, commit `ac3b904`以降) が表現できる
文法は以下に限定される(`crates/laminaria-ir/src/types.rs`を直接読んで確認):

- `IntWidth`(types.rs:70-73): `I32` の1バリアントのみ。整数はこれ以外
  存在しない(浮動小数点、64bit整数、符号なし整数は一切ない)。
- `Expr`(types.rs:91-120): `IntLit(i64, IntWidth, Provenance)` /
  `Param(usize, _)` / `Local(LocalId, _)` /
  `WrappingAdd/Sub/Mul(Box<Expr>, Box<Expr>, _)` /
  `NotEqZero(Box<Expr>, _)` / `Call(FnId, Vec<Expr>, _)` /
  `Let { local, value, body, _ }`。
- `Stmt`(types.rs:148-163): `Let { local, value: Expr, body: Box<Stmt>, _ }`
  / `If { cond: Expr, then: Box<Stmt>, els: Box<Stmt>, _ }`(elsは必須、
  Option型ではない) / `Return(Expr, _)`。
- `FnFact`(types.rs:184-190): `name, params: Vec<(String, IntWidth)>,
  return_width: IntWidth, body: Stmt, provenance`。関数は必ず1つの
  `IntWidth`値を返す(void関数は存在しない)。
- `Program`(types.rs:193-195): `functions: BTreeMap<String, FnFact>`
  (キー順=名前のソート順で決定的にイテレートできる)。

**制約として確定していること(存在しないものの一覧)**:
ループ構文なし(`While`/`Loop`相当のバリアントはStmtに存在しない)、配列・
文字列・ポインタなし、浮動小数点なし、グローバル可変状態なし、
`WrappingAdd/Sub/Mul`はすべて2の補数・オーバーフロートラップなしの意味論
(`types.rs:98-99`のコメントで明記、rustcの`wrapping_add`/Nimの`+%`と対応)。
再帰呼び出しが`validate_program`/`interpreter::eval_function`で許容される
かどうかは未確認(このIRの現行テストでは確認していない) — T1で明示的に
検証すべき項目として`subset_scope`未確定事項に記録する(推測で断定しない)。

**target候補の比較**(この制約下で実装コストが現実的な候補のみ):

| 候補 | 長所 | 短所(このIRの現行subsetに対して) |
|---|---|---|
| x86-64 (System V AMD64 ABI) | 実ハードウェアで直接実行可能、最も「コンパイラらしい」成果物 | 可変長命令エンコーディング、レジスタ割付が必須(I32のみの単純な式でも)、OS毎にobject形式が異なる(ELF/Mach-O/PE)ため成果物契約が3通りに分岐する |
| AArch64 (AAPCS64) | 固定32bit命令幅でエンコーダが単純、開発機(Apple Silicon)とホストISAが一致 | x86-64と同様にレジスタ割付が必須、OS毎のobject形式分岐も同様に残る |
| WebAssembly 1.0 (MVP, i32のみ) | スタックマシンでレジスタ割付が不要、単一のportable binary形式(OS分岐なし)、呼出規約が型付きexport/callのみで極めて単純、`i32`演算はWASM仕様上すでに2の補数ラップ(このIRの`WrappingAdd/Sub/Mul`と完全一致)、既存runtime(wasmtime等)で実行のみを検証でき自前runtime実装が不要 | 「実ハードウェア機械語」ではない(ただし§1の通りcontractはハードウェアターゲットを要求していない) |
| 独自bytecode + 自作VM | 実装の自由度が最大 | runtime契約を含め全てゼロから自作する必要があり、既存の外部検証手段(標準仕様に基づくruntime)を使えない。`docs/design/issue-35-d0-spec.md`のM10が要求するWASM経路を一切前進させない |

---

## §3. 推奨する最初のtarget: **WebAssembly 1.0 (MVP)、i32 core subsetのみ**

理由(比較表からの結論、新規の主張は含まない):
1. 現行IRの制約(I32のみ、ループなし、配列/ポインタなし)と、WASM MVPの
   `i32`命令・構造化制御フロー(`if/else/end`)・`local`変数モデルが
   ほぼ1対1で対応し、レジスタ割付や複雑な命令選択が一切不要(§4で詳細)。
2. `docs/design/issue-35-d0-spec.md:63`(M10行)と`:284-291`(§3.3)が、
   Nim→WASM経路の実現可否を明示的に「#5 T0の確定に依存する」としている
   ため、T0でWASMを選ぶことがM10の保留状態を前進させる直接的な効果を持つ。
3. 成果物(`.wasm`バイナリ)はOSに依存しない単一形式であり、x86-64/AArch64
   のようなELF/Mach-O/PEの3分岐が発生しない。

---

## §4. execute-on / produces-for、ISA、成果物形式

- **execute-on(コンパイルを実行するホスト)**: LAMINARIA自身のRust実装
  (T0が生成するコードは純粋なRust関数、後述)が動作する任意のホスト。
  ホスト固有の分岐(条件付きコンパイル、FFI、アセンブリ)を一切持たない
  ため、`cargo test`が現在通っている全ホスト(CI: ubuntu-latest,
  macos-latest, windows)で同一に動作する。
- **produces-for**: WebAssembly Core Specification 1.0(2019年12月版、
  いわゆるMVP)の`i32`数値型のみを用いた実行ターゲット。post-MVP拡張
  (multi-value, reference-types, threads, SIMD, 64bit memory等)は
  一切対象外。`memory`/`table`/`global`セクションは使用しない
  (現行IRに配列・ポインタ・グローバル変数がないため)。
- **ISA(使用命令の全集合、これ以外は生成しない)**:
  `i32.const`, `i32.add`, `i32.sub`, `i32.mul`, `i32.eqz`,
  `local.get`, `local.set`, `call`, `if`(blocktype=empty)/`else`/`end`,
  `return`, `end`(function末尾)。
- **成果物形式**: WebAssembly Core Spec 1.0のバイナリ形式そのもの
  (magic `\0asm` + version `\x01\x00\x00\x00`、続けてType section /
  Function section / Export section / Code sectionのみ)。LAMINARIA自身の
  純粋なRust関数`fn generate_wasm_module(program: &Program) -> Result<Vec<u8>, CodegenError>`
  がバイト列を直接構築する。`wat2wasm`・`wasm-ld`・`binaryen`(`wasm-opt`)
  ・`nim -d:emscripten`等、外部WASMツールチェインは生成経路のどこにも
  呼び出さない(§6で検証方法を確定)。

---

## §5. 呼出規約、runtime/link境界

- 1つの`FnFact`は1つのWASM関数になる。シグネチャは
  `(params.len() 個の i32) -> (1 個の i32)`
  (`FnFact.return_width`は常に`IntWidth::I32`なので、結果型は常に単一の
  `i32`、void関数は存在しない)。
- 引数はソース順(`params`のVec順)のままWASM関数の引数順にマップする
  (`Expr::Call`の`Vec<Expr>`もソース上の左から右の順で評価してスタックに
  積む — この評価順はWASM仕様のオペランド評価順と一致する)。
- `Program::functions`(`BTreeMap<String, FnFact>`)を名前のソート順で
  イテレートし、その順にWASM関数インデックス0, 1, 2, ...を割り当てる
  (`BTreeMap`のキー順は決定的なので、この割り当ても決定的)。
- すべての関数はソース上の名前でexportする(`export`セクション)。
  `start`セクションは持たない(暗黙の自動実行はしない) — ちょうど
  `laminaria_ir::interpreter::eval_function`が関数名を明示的に指定して
  呼び出すのと同じ形。
- **runtime/link境界**: 埋め込み側(T1のテストハーネスや、将来M10の
  WASM実行検証)は、生成された`.wasm`モジュールをロードし、関数名で
  exportを引いて、i32引数を渡し、i32戻り値を読む — これだけがこの
  境界の全体である。メモリ共有・ポインタ受け渡し・構造体レイアウトは
  一切発生しない(現行IRにそれらが存在しないため)。
- ローカル変数のWASMインデックス割当: 関数本体を1回走査し、出現する
  すべての`LocalId`を収集して`LocalId`の`u32`値の昇順でソートし、
  パラメータの後(インデックス`params.len()`以降)に順番に割り当てる
  (走査順ではなく数値順にソートすることで、木の辿り方に依存しない
  決定的な割当にする)。

---

## §6. 「外部compiler/backendへ委譲していないことの検証方法」

区別すべき2つの行為(§1の結論の具体化):

| 行為 | 誰が行うか | 契約上の扱い |
|---|---|---|
| ソース→WASMバイト列の生成(コンパイル) | LAMINARIA自身の純粋Rust関数のみ | **契約の対象。外部ツール禁止** |
| 生成済み`.wasm`バイト列の実行(検証目的) | 既存のWASM runtime(下記) | 契約の対象外(x86-64ターゲットでCPUを自作しないのと同じ) |

検証方法(4点、いずれもD1-aで確立した「ソース自体をgrepで確認する」
パターンの踏襲):

1. **生成関数の純粋性**: `generate_wasm_module`はファイルシステム・
   ネットワーク・サブプロセスに一切触れない、`&Program -> Vec<u8>`の
   純粋関数として実装する(シグネチャ自体が`std::process::Command`を
   受け取れないことで構造的に保証される)。
2. **ソース内の禁止文字列テスト**: T0モジュールの実ソーステキストに
   `Command::new`が一切出現しないことをテストで確認する
   (`compiler_work_executor.rs`冒頭コメント「No external compiler
   fallback anywhere in this module」と同じ主張を、テストとして固定する)。
3. **バイト列の直接検査**: 生成された`.wasm`の先頭8バイトが
   `\0asm\x01\x00\x00\x00`(WASMマジック+version)と一致することを
   テストで確認する — `wat2wasm`等の外部ツールの出力を右から左に
   受け流していないことの直接証拠。
4. **実行検証は明確に分離された別ステップとして行う**: `.wasm`の実行
   (=検証目的のみ)には`wasmtime`crateを使う(Bytecode Allianceが
   保守する、Rustで最も広く使われるWASM runtime。現時点でこのワーク
   スペースに依存関係なし、`grep`で確認済み)。**T1で追加するときは
   コンパイルを行うクレートの`[dependencies]`ではなく、検証テストのみが
   参照する`[dev-dependencies]`として追加する** — 本番のコンパイル経路
   から`wasmtime`への依存が絶対に生じないようにする。

---

## §7. 対応する演算と未対応診断

現行IRの`Expr`/`Stmt`/`IntWidth`は全てRustのenumであり(types.rs:70-73,
91-120, 148-163)、T0の生成関数はこれらを**ワイルドカードアーム
(`_ => ...`)を使わない網羅的match**として実装する。これにより:

- **現時点で「未対応の演算」は存在しない**(`Expr`/`Stmt`の全バリアントが
  §4のISA集合にマップ済み — §4参照)。ワイルドカードなしの網羅的matchは
  Rustのコンパイラ自身が「新しいバリアントが追加されたのに対応するmatch
  アームがない」状態をビルドエラーとして強制するため、これが唯一かつ
  最も確実な「将来の未対応検出」の仕組みになる。
- `IntWidth`は現在`I32`の1バリアントのみだが、生成関数は
  `IntWidth`についても網羅的matchを使い、将来`I64`等が追加された際に
  コンパイルが失敗するようにする(未対応幅を黙って`i32`として誤生成
  することを構造的に防ぐ)。
- `NotEqZero`は`i32.eqz`を2回連続で適用する(`NOT(x == 0)`を
  `i32.eqz(i32.eqz(x))`として計算する)ことで、追加のWASM命令を
  必要とせずに正確な意味論を得る(§4のISA集合が`i32.eqz`のみで足りる
  理由)。

---

## §8. 固定source fixtureと期待実行値

**新しい数値を作らない** — D0全体を通じた方針(#35 D0スペック§2.2/2.3の
先例)に従い、すでにD0で独立検証済みの値をそのまま再利用する:

- fixture: `fixtures/laminaria-semantic-substrate-prototype/rust-src/add_or_double.rs`
  (実ファイルを直接読んで確認済み、`double`/`add_or_double`の2関数、
  `TEST_INPUTS`をファイル自身が19-24行目で宣言) / 対応する
  `nim-src/add_or_double.nim`。
- 入力: 上記fixture自身が宣言する`TEST_INPUTS`:
  `(3,4,0)`, `(3,4,1)`, `(i32::MAX,1,0)`, `(-5,10,1)`。
- 期待値(既存テストで確定済み、再導出しない): `(3,4,0)->7`,
  `(3,4,1)->6`, `(i32::MAX,1,0)->i32::MIN`, `(-5,10,1)->-10`。
  一次証拠は`crates/laminaria-ir/src/lib.rs:99-150`の
  `fixture_parity_tests::rust_frontend_output_matches_a_real_rustc_compiled_binary`
  — この**fixtureファイル自体をディスクから読み、実際に`rustc -O`で
  コンパイルした実バイナリの標準出力**と、`lower_rust_source`+
  `eval_function`によるtree-walk結果を直接diffして一致を確認している
  (推測や別テストからの流用ではない、ファイル名`add_or_double.rs`を
  直接joinして読んでいる107行目で確認済み)。Nim側は同様の
  `nim_frontend_output_matches_a_real_nim_compiled_binary`
  (`lib.rs:152-`)が`nim-src/add_or_double.nim`を同じ方式で検証する。
- T1が追加するのは **3方向一致**の検証: (a) 上記の既存
  `laminaria_ir::interpreter::eval_function`によるtree-walk結果、
  (b) 新設のWASM生成→`wasmtime`による実行結果、(c) 上記の実rustc/実nim
  コンパイル済みバイナリの実測出力 — 3つ全てが一致することを1つの
  テストで確認する。これはD0で確立済みの2方向一致(interpreter vs
  実コンパイラ)より強い証拠になる。

---

## §9. T1の具体的入力・出力・停止条件

**入力**: §8の`add_or_double`/`double`の`Program`(既存の
`rust_frontend::lower_rust_source`/`nim_frontend::lower_nim_source`が
`fixtures/laminaria-semantic-substrate-prototype/{rust-src,nim-src}/add_or_double.{rs,nim}`
から生成する、すでに存在するIR — 新しいfixtureは作らない)。

**出力**(4点、いずれも境界を超えない):
1. `crates/laminaria-ir/src/wasm_target.rs`(新規モジュール。
   `interpreter.rs`/`validate.rs`と同じ階層に置く — target生成は
   `Program`のみに依存し、`laminaria-plan`/`laminaria-run`側の型には
   一切依存しないため、既存の「laminaria-irは他クレートに触れない」
   境界(`lib.rs:30-46`)を維持できる。issue境界上の理由で別クレートに
   分離すべきという判断があれば、それは指示者の確定事項として本書の
   この一点のみ上書き可能)。
2. `fn generate_wasm_module(program: &Program) -> Result<Vec<u8>, CodegenError>`
   本体(§4-§7の契約通り)。
3. テスト4件: (a) マジックバイト一致、(b) §8の3方向一致、(c) §6-2の
   禁止文字列テスト、(d) `IntWidth`網羅的match(ワイルドカードなし)を
   静的に強制していることのコードレビュー可能な構造(実行時テストでは
   なく、コード自体がその証拠になる)。
4. `Cargo.toml`(laminaria-irまたは新規クレート側)の`[dev-dependencies]`
   に`wasmtime`を1件追加(§6-4)。

**明示的な停止条件**(このプロジェクトで確立した「範囲を増やさない」
パターンの踏襲):
- `laminaria-plan`の`ActionKind`/`CompilerWorkDescriptor`に新しい
  target生成用バリアント(例: `GenerateTarget`)を追加しない — それは
  planner/executor配線の別タスク(D2以降、または#13)。
- WASMの`memory`/`table`/`global`セクション、配列・ポインタ相当の
  IR拡張は行わない(現行IRに存在しないため対象外)。
- `fixtures/laminaria-semantic-substrate-prototype`の2関数を超える
  fixtureを追加しない。
- `docs/design/issue-35-d0-spec.md`のM10(実際のWASM feasibility spike
  配線)には進まない — それは同スペック§3.3が明示的に「D1完了後、
  指示者が別途判断」としている独立タスクのまま維持する。
- 速度・性能に関する主張は一切行わない。
- 再帰呼び出しの挙動(§2で未確認と記録した項目)について、T1は
  「現行`validate_program`/`interpreter::eval_function`が再帰をどう
  扱うか」をまず確認し、もしIR側が未対応/未定義なら、その事実を
  記録するに留め、IR側の拡張はこのタスクのスコープ外として扱う。
