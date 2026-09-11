# NimにおけるC/C++ library再利用とforeign-native build統合

## 目的

LAMINARIAは、Nimの実用上の中心的な利点を保存する。すなわち、Nimで記述するコードは、同等の機能を再実装するのではなくC/C++ ecosystemを再利用できなければならない。この要求はLAMINARIA自身の実装と、LAMINARIAがcompileするNim projectの両方に適用する。

本書は[compiler ownership contract](compiler-ownership-contract_ja.md)に従い、正当なforeign libraryのcompile/linkと、Nimのgenerated C/C++へのtarget compilation委譲を区別する。

追跡issue: [#44](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/44)。

## upstreamの根拠

Nim公式backend文書は、`nim c`、`nim cpp`、`importc`、`importcpp`、`compile`、`passL`、`dynlib`、header wrappingを通常のnative統合表面として説明している。

- <https://nim-lang.org/docs/backends.html>
- <https://nim-lang.org/docs/manual.html>
- <https://github.com/nim-lang/c2nim>
- <https://github.com/nim-lang/Nim/blob/devel/compiler/extccomp.nim>

LAMINARIAの独自compilerはNimのgenerated-C実装方式を再現する必要はないが、適切なC/C++ libraryを利用できる外部的な能力を保存する。

## 必須の2つの再利用表面

### LAMINARIA自身の実装

必要な機能をカバーする成熟したC/C++ libraryがある場合、正しさ、platform coverage、license、maintenance、security、binary/runtime cost、build再現性が要件を満たすなら、新規のRust/Nim実装より再利用を優先する。採用・不採用の理由を記録する。言語の純粋性だけを、適切な実装の重複の理由にしない。

### LAMINARIAがcompileするproject

対応範囲のNim projectはforeign function、type、libraryを宣言できる。独自Nim frontendとsemantic IRはこれらを保持し、target loweringはforeign実装本体をLAMINARIA IRに要求せずexternal referenceを生成する。build graphはforeign native成果物を生成または解決し、LAMINARIA生成のtarget成果物とlinkする。

## ownership boundary

```text
Nim source
  -> LAMINARIA parsing / semantic analysis
  -> foreign declaration/callを持つ独自IR
  -> LAMINARIA target lowering
  -> LAMINARIA生成object

宣言済みC/C++依存
  -> identityを持つ既存成果物を解決
     OR identityを持つC/C++ sourceをcompile
     OR 明示的なC++ adapter/instantiation unitを生成・compile
  -> foreign object/archive/shared library

LAMINARIA object + foreign native成果物 + runtime obligation
  -> 明示的なlinker Action
  -> final target artifact
```

外部C/C++ compilerは宣言済みforeign dependency分岐で使用できる。独自経路のRust/Nim target unitの実装として生成されたC/C++をcompileするために使用してはならない。全compile、archive、link Actionは宣言済み入力、出力、producer identity、lineageを持つ。

## 必要なsemantic model

source/semantic layerは少なくとも次を表現する。

- `ForeignDecl`: symbolまたはC++ expression identity、source language、linkage
- `ForeignType`: scalar、pointer/reference、function pointer、opaque handle、enum、union、record layout
- calling convention、variadic、visibility、symbol decoration
- 宣言から得られるmutability、aliasing、pointer provenance
- ownership、allocation/freeの対応、borrow lifetime、callback lifetime
- exception/unwindとfailure boundary
- header/moduleの由来とconditional compilation要件
- 必要libraryとtarget/runtime compatibility
- 直接のexternal symbolが存在するか、adapter/instantiationが必要か

構文の対応とは、target codeとlink生成まで意味を保持することである。pragmaを受理して捨てるだけでは対応としない。

## 必要なProgram/Action Graph model

graphは次を区別する。

- header setとgenerated binding入力
- C/C++ source unit
- generated C++ adapter/template-instantiation unit
- compile definition、include path、language standard、feature flag
- object、static archive、import library、shared library artifact
- archive生成とfinal link Action
- library search path、link order、whole-archive/dead-strip、runtime search path
- target triple、object format、ABI、sysroot/SDK、compiler、linker、C++ standard library
- runtime file、dynamic-library deployment、target execution要件

package manager、CMake、`pkg-config`は解決済みfactを供給できるが、opaqueなnested buildにtarget compilationを隠してはならない。未対応のgenerated-build behaviorはstructured diagnosticで停止する。

## CとC++は別のcapability levelとする

初期C経路は、安定した外部function/data symbolを直接loweringし、object、archive、shared libraryからlinkできる。

C++にはoverload resolution、name mangling、constructor/destructor、class layout、virtual dispatch、template、inline/header-only API、exception、compiler/standard-library ABI結合が加わる。link可能なsymbolがない場合、LAMINARIAは次のいずれかを行う。

1. 観測可能なC++ adapter/instantiation unitを生成し、foreign Actionとしてcompileする。
2. 別途保守される明示的wrapperを使う。
3. structured reasonで該当構文を拒否する。

mangled nameの推測や、C++構文のCとしての暗黙処理は認めない。

## identity、cache、evidence

foreign-native identityには、振る舞いまたはbyte列を変え得るすべての意味入力を含める。

- header、source、generated binding、adapterの内容
- 解決済みlibrary byte/versionと推移的native依存
- compiler/linker/archiver identity
- C/C++ language standard、standard library、ABI mode
- target、CPU feature、sysroot/SDK、compile/link flag
- macro definition、include search order、package-resolution result
- static/shared選択とruntime deployment契約

変更は、依存するforeign declaration、compile unit、link Action、final artifactだけをinvalidateする。evidenceは各Actionが実行、再利用、拒否された理由を示す。

## 初期vertical slice

### C slice

実在する保守中のC libraryをNim宣言から使用し、次を検証する。

- opaque handle
- scalarとfixed-layout recordのcall
- allocation/free ownership
- lifetimeを明示したcallback
- 別経路のforeign compilationまたは既存archive
- LAMINARIA自身のNim loweringとtarget object生成
- 明示的なfinal linkとexecution test
- `nim c`とgenerated-C fallbackを無効化したnegative test

### C++ slice

実在する保守中のC++ libraryを使用し、次を検証する。

- method、constructor、destructorの利用
- instantiationが必要なtemplateまたはheader-only operation
- 必要な場合の明示的adapter/instantiation生成
- compilerとstandard-library ABI identity
- 未対応のexception、layout、ABI caseのstructured rejection
- LAMINARIA生成objectとのfinal link

## 完了条件

- LAMINARIA自身の実装inventoryで、適切なC/C++ libraryを再利用すべき機能を特定し、採用・不採用の根拠を記録する。
- Nim frontend/IRが宣言済みC依存を、実行可能なowned target artifactまで保存する。
- graphがforeign compilation、adapter生成、archive/shared-library入力、final linkを別の仕事とartifactとして示す。
- Nim target unitのgenerated C/C++実装をforeign dependencyに偽装できない。
- C/C++ vertical sliceが正確なtoolchain/artifact lineageとnegative caseと共に合格する。
- static/dynamic、ownership、callback、ABI obligationに明示的な対応または拒否結果がある。
- Windows target生成はプロジェクト所定の`wslc` container workflowで再現可能である。

## 現状

外部委譲の`NimBuild` baselineは実Nim compilerからこれらの能力をopaqueに引き継げる。既存Nim wrapperはnative compiler/linker invocationを観測する。しかし現行のowned Nim subsetはpragmaを拒否し、foreign declarationやnative dependency/link Actionをまだモデル化していない。したがって本書は必要作業を定めるものであり、実装完了を示さない。
