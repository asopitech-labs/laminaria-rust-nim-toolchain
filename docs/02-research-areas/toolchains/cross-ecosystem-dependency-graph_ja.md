# Ecosystem横断依存関係グラフ研究

## 位置付け

[当面の研究プログラム](../../near-term-research-program_ja.md)は、ecosystem横断の依存解決を現在の中心研究に置く。本書はその問題を定義する。`cargo build`、`nimble build`、CMake/Meson、platform linkerを四つのopaque actionとして包む提案ではない。

Rust/Cargoが現在C/C++依存をbuildする実際の境界は、[Rust/CargoにおけるC/C++ native依存のbuild model](rust-c-cpp-native-build-model_ja.md)に整理する。

既存のbuild tool、package solver、incremental compiler、multi-level IRがどの層まで解き、どこにopaque境界を残すかは、[複数ecosystem依存とcompiler IRを結合して解く先行研究調査](cross-ecosystem-dependency-and-ir-resolution-landscape_ja.md)で比較する。Package Managers à la Carteがcross-ecosystem package resolutionの直接先行研究であることから、LAMINARIAの主張はpackage solver単独ではなく、source semantics、multi-level IR、native ABI、symbol、link closureまでの増分協調解決に置く。

## 代替不可能な問題

RustからNimへの単一callが成功しても、言語経路またはABI経路が一つ通ったことしか示さない。package ecosystemが注入するclosureは次を含み得る。

- Cargo package、feature、target predicate、build dependency、host側処理、native-link metadata
- Nimble package、module search path、compiler define、task、generated source、`importc`/`importcpp`要件
- C header、translation unit、compile definition、include path、archive、shared library、`pkg-config`相当情報、system library
- C++ template、inline/header-only code、明示的instantiation/adapter、言語／標準library ABI、exception、RTTI、constructor/destructor、link order

これらは同種のpackage edgeではない。各ecosystemのidentityを保持しながら、何を要求・生成・選択・拒否・linkするかを表す型付きgraphが必要である。

## Graph層

```text
要求されたnative executable
  -> package要求と選択version/feature
  -> source/module/header/generated-unit関係
  -> host/target toolchainとABI制約
  -> compile/adapter/archive成果物
  -> symbolとlink要件
  -> 最終native executable
```

要求された全artifactにproducerまたは受理済みprebuilt identityがあり、最終binaryの順序付きlink closureが完成して初めて解決完了とする。package managerの判断は入力・証拠であり、compile/linkを隠す許可ではない。

## Correctness不変条件

- package、source、semantic、artifact、action、physical placementのidentityを区別する。
- 同一packageが宣言してもhost workとtarget workを混同しない。
- feature、version、target predicate、toolchain capability、ABI、symbol、link orderを明示的制約として保持する。
- 未対応build script、generator、macro、native library探索は構造化されたgapとして拒否し、opaque fallbackを暗黙実行しない。
- 解決済みgraphは、要求したnative executableを生成・起動する直接テストで検証する。手書きYAML graphとfixture専用validatorを正本にしない。

## 効率仮説

package、version、feature、target、backend、toolchain、ABI、artifact kindの直積を先に生成しない。eager expansionと、demand-driven expansion、constraint propagation、canonicalization、memoization、equivalent-state merging、dominance pruning、SCC condensationを比較する。

枝刈りは候補stateだけでなく、native executableから到達不能なpackage、source/module、semantic item、IR、object/archive member、symbol/section、runtime artifactまで対象とする。後段DCEとlinker GCに加え、安全に確定できる不要workをcompile前に回避する。詳細は[cross-layer枝刈り](cross-layer-reachability-pruning_ja.md)に定義する。

graphの完了条件は到達closureを列挙するだけではない。package、source semantics、language/intermediate IR、ABI、symbol、linkの各依存義務が、成果物への変換によって`Discharged`、明示runtime contractとして`Externalized`、または理由付きで`Rejected`に到達しなければならない。これにより元のCargo/Nimble/C/C++ graphはprovenanceとして残る一方、artifact利用者が再resolutionすべきgraphではなくなる。詳細は[dependency-discharge artifact contract](dependency-resolved-artifact-closure_ja.md)に定義する。

correctnessを必須とし、正しいresolver間で次を比較する。

- 解決wall-clock
- peak resident memory
- 展開、枝刈り、merge、再計算したstate/node数
- no-op、leaf変更、feature変更、target条件変更後のinvalidation
- 解決前後に回避した外部compiler/linker action数

## 現在の境界

最初の実験はCargo crate、Nimble package、C library、C++ libraryを各一つ使い、native executableを要求rootとする。成功closure一つとcompile前拒否一つを扱い、全ecosystem compatibilityは主張しない。

WebAssemblyは将来、同じ解決済みgraphを消費する任意target variantとして評価できる。しかしこのgraphを規定せず、現在のmilestoneの完了条件でもない。
