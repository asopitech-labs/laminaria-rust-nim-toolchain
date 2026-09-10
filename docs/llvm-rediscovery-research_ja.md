# LAMINARIA — LLVM再発見研究

## 責務の訂正（2026-09-10）

[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)を研究目的・完了判定の基準とする。

独自コンパイラ・IR・スケジューラが本経路であり、任意の後続統合ではない。Cargo/Nim ecosystemは依存解決に利用できるが、以下に登場する既存コンパイル経路は比較・観測または外部bootstrapのbaselineであり、本ビルドの選択肢ではない。Action Graphは言語IRの代わりにならない。

## 位置付け

LAMINARIAはLLVMを前提基盤として採用する研究ではない。

LLVMは、長年のcompiler engineeringによって得られた一つの巨大な解であり、LAMINARIAにとっては重要な比較対象・観測対象・分解対象である。しかしLAMINARIAの目的は、RustとNimをLLVMへ接続することでも、LLVMの既存境界をそのままAction Graphへ写経することでもない。

LAMINARIAは、RustとNimを一つの計算システムとして扱うという別の出発点から、compiler/backend architectureに必要な概念を再導出する。

> LLVMが持つ概念を「正しい前提」として採用するのではなく、なぜその概念が必要になったのかを実験から再発見し、LAMINARIAに必要な形へ再構成する。

結果としてLLVMと似た構造へ到達してもよい。異なる構造へ到達してもよい。重要なのは、LLVMとの一致ではなく、LAMINARIA自身の研究仮説と実験からその構造を導出できることである。

## 研究上の出発点

異なるcompiler frontendは、同じ意味の処理や同じC ABI surfaceを与えられても、同じIR、同じbitcode、同じmachine code、同じoptimization opportunityを生成するとは仮定しない。

Rust/rustc、Nim 2、Nimony/Nim 3、Clang等は、それぞれ異なるlanguage semantics、internal representation、lowering policy、runtime obligation、attribute/metadata policyを持つ。

したがって次のような素朴な収束モデルを前提にしない。

```text
Rust ─┐
      ├→ LLVM IR → 共通最適化 → machine code
Nim  ─┘
```

実際の研究対象は次である。

```text
Rust semantic facts ─┐
                     ├→ transformation / information loss / normalization
Nim semantic facts  ─┤
                     ├→ candidate common computational representation
Other compiler facts ┘
                     ↓
             backend planning / optimization
                     ↓
                target artifacts
```

## 中心研究質問

1. RustとNimの異なるsemantic modelから、cross-language planningに本当に必要な共通情報は何か。
2. その情報は既存compiler pipelineのどの段階で保持、変形、消失するか。
3. LLVM IRが表現しているもののうち、本質的に必要なものとLLVM固有の設計判断は何か。
4. SSA、CFG、typed/partially typed IR、memory model、alias information、attributes、metadata、calling convention、data layout等はLAMINARIAでも同じ形で必要か。それとも異なる表現が適切か。
5. optimization passという構造はLAMINARIAでも妥当か。どの分析・変換・invalidationsが本当に独立概念として必要か。
6. analysis preservation/invalidation、pass ordering、fixpoint、interprocedural optimization、LTOが必要になる条件を、既存LLVM APIからではなくworkloadから再導出できるか。
7. target loweringとmachine-independent optimizationの境界はどこに置くべきか。
8. object/link boundary、LTO、ThinLTO型の分割が必要になる理由を、artifact economicsとdistributed/scheduling要求から再導出できるか。
9. RustとNimが同一のbackendへ入る場合、既存frontendが生成したIRをmergeするのではなく、上流semantic factsからより適切な共通表現を生成できるか。
10. LLVMと異なる答えを選ぶ場合、その差をcorrectness、compile latency、memory、I/O、generated-code quality、incrementality、explainabilityで説明できるか。

## LLVMの扱い

LLVMは三つの役割で利用する。

### 1. Prior art

LLVMが解決している問題と設計理由をsource/documentation/実験から調べる。

例:

- SSA/IR設計
- DataLayout
- attributes / metadata
- alias analysis
- pass manager
- analysis preservation/invalidation
- inlining
- IPO
- target-independent / target-dependent separation
- SelectionDAG / GlobalISel等のlowering
- codegen pipeline
- LTO / ThinLTO
- optimization remarks / instrumentation

### 2. Experimental oracleではなく比較対象

LLVMの判断を「正解」として採用しない。

LLVM optimization remarksやpass instrumentationは、LLVMが何をしたかを知る証拠である。LAMINARIAが同じ判断をすべきことの証明ではない。

### 3. 独自backendと比較用projection

単にLLVMを取り外せるだけでは足りない。本経路は独自IR・変換・target lowering・コード生成を実装する。LLVM/Cranelift/GCCへの投影は比較実験として保持し、独自backendを「将来の任意候補」にしない。

## 再発見の研究方法

### A. Semantic workloadから始める

既存LLVM IRを入力として研究を始めない。

RustとNimで意味的に対応する小さなworkloadを定義し、各compilerが何を知っており、どこで何へ変換するかを比較する。

対象例:

- integer arithmetic / overflow
- branch / loop
- aggregate
- enum
- pointer / aliasing
- ownership / lifetime
- allocation
- callback
- exception / panic / unwind
- generic / specialization
- vectorizable loop
- interprocedural constant propagation
- dead code / reachability
- target feature dependent operation

### B. 差異を上流へ逆追跡する

最終IRやmachine codeが異なること自体は発見ではない。

```text
observed difference
→ producer stage
→ originating semantic fact / compiler policy
→ transformation
→ information preserved / transformed / lost
```

まで追跡する。

### C. LLVM概念を一つずつ再検証する

例えば「function attributesが必要」という結論をLLVMが持っているから採用しない。

1. attributeなしで必要なoptimization/correctnessが失われるworkloadを作る。
2. どのsemantic factが必要だったかを特定する。
3. そのfactをLAMINARIAの共通表現でどう保持するかを設計する。
4. LLVM attributeへ投影する場合と、別backendへ投影する場合を比較する。

同様にpass manager、analysis cache、LTO summary、target lowering等も再検証する。

## Candidate LAMINARIA Semantic/Optimization Substrate

現時点で名称や構造を固定しない。

以下は研究対象であり、採用済みarchitectureではない。

```text
Language-specific semantic facts
  ↓
Preserved semantic facts + provenance
  ↓
Cross-language computational relations
  ↓
Optimization requirements / legality facts
  ↓
Backend-specific projection
  ↓
LAMINARIA-owned target lowering / code generation
(comparison only: LLVM | Cranelift | GCC)
```

LAMINARIAは既存language IRを一つの万能IRへ強制的に変換する必要があるとは仮定しない。複数IR、typed facts、graph relations、analysis databasesの組合せの方が適切な可能性も研究対象とする。

## 「LLVM再実装」との違い

LLVM APIやpassを同じ形で作り直すことを目的としない。

再発見とは、LLVMが解いている問題をLAMINARIA自身の制約下で再び問題として立てることである。

例えばThinLTOを研究するとき、最初の問いは「DTLTO JSONをどう取り込むか」だけではない。

- なぜwhole-program optimizationにglobal summaryが必要なのか。
- どの情報をmodule間で共有すれば十分か。
- そのsummaryはRust/Nim共通で同じ形でよいか。
- dynamic backend jobsという分割はLAMINARIAのschedulerに最適か。
- LAMINARIAがsemantic factsをより上流で持つなら、別のpartitioningが可能か。

を問う。

## 研究成果の判定

次は研究成果として不十分である。

- LLVM IRを生成できた。
- bitcodeをlinkできた。
- LTO flagを有効にできた。
- LLVM passを列挙できた。
- optimization remarksを取得できた。
- RustとNimのIR差を観測できた。
- LLVMの既存境界をAction Graphへ写像できた。

これらは観測・実験基盤である。

研究成果には少なくとも一つが必要である。

- 既存compiler/LLVMの設計判断が必要になる条件を再導出した。
- LLVMと異なるcandidate architectureを作り、比較した。
- semantic informationの保持方法を変更することで既存pipelineでは不可能/困難なcross-language optimizationを成立させた。
- 既存LLVM境界より適切なLAMINARIA固有のartifact/execution boundaryを示した。
- あるLLVM概念がLAMINARIAでは不要、または別表現で十分であることを証拠付きで示した。
- LLVMの設計がLAMINARIAでも必要であることを独立した実験から支持した。

## 既存研究トラックへの影響

### Compiler Pipeline Decomposition

既存compilerのstage map作成だけでは完了しない。stageがなぜ存在するか、何のsemantic informationを受け渡すか、LAMINARIAが同じ境界を必要とするかを検証する。

### Native Linking

link可能性の確認は基礎実験である。異なるlanguage semanticsをどの段階で共通契約へ投影すべきかを研究する。

### Backend Graph

LAMINARIA独自の最適化・target生成を本経路として実行し、既存backendの比較経路と分類を分ける。将来の任意candidateにしない。

### LLVM White-boxing

LLVM内部を観測する目的は、LLVMをより細かく操作することだけではない。LLVMが必要としているanalysis、transformation、summary、partitioning、target contractを再発見するためのprior-art studyとする。

### Cross-language LLVM/LTO

Rust/Nim-origin bitcodeの収束は一つのbaselineに格下げする。最終目的は「既存frontendが別々に生成したLLVM IRを何とかmergeする」ことではない。

## 必須の比較軸

LLVMとの比較は少なくとも次を含む。

- semantic information retained/lost
- optimization legality information
- compile latency
- CPU / memory / I/O
- intermediate representation size
- analysis recomputation
- invalidation precision
- cross-language optimization opportunity
- target portability
- generated-code size/runtime
- scheduling/distribution suitability
- explainability

## 研究原則

> LLVMを理解して使うのではなく、LLVMが必要になった理由をもう一度発見する。

> LLVMと同じ答えになった場合も、それはLLVMを信じたからではなく、LAMINARIAの実験が同じ必要条件を示したからでなければならない。
