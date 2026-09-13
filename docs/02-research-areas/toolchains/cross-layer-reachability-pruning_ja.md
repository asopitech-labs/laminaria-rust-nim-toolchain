# Native executableをrootとするcross-layer枝刈り

## 結論

不要なcodeは最終binaryから除去するだけでなく、可能な限りcompile前に不要と確定し、package取得、source解析、IR生成、monomorphization、codegen、archive materialization、link input化そのものを回避すべきである。

ただし枝刈りはLAMINARIAの全体価値ではない。中心は、異なるecosystem、source semantics、language/intermediate IR、ABI、symbol、linkにまたがる依存義務をbuild時に解決・変換してnative artifactへdischargeすることである。枝刈りは、そのうち成果物に影響しない義務を`ProvenIrrelevant`と証明し、処理せずに済ませる最適化である。成果物契約は[異種依存義務をbuild時にdischargeするartifact contract](dependency-resolved-artifact-closure_ja.md)に定義する。

ただし「不要」はsource fileに書かれているが呼ばれていない、という一種類の判定ではない。要求されたnative executableから、package、feature、module、semantic item、IR operation、symbol、object section、runtime artifactへ至る**cross-layer reachability**で定義する。削除できるのは、到達不能であり、observable side effectや外部公開契約がないことを、対象層に応じた保守的規則で示せるものだけである。

## 遅い枝刈りだけでは不十分

既存compiler／linkerにも枝刈りはある。

- Rust compilerはMIR levelで到達可能なmonomorphized itemを収集してcodegen unitへpartitionする。`-C link-dead-code`のdefaultはdead codeを保持しない。[^rust-monomorph] [^rust-link-dead-code]
- Nim compilerの新しいC backend入口は、packed module graph全体に対するDCE prepassを行う。[^nim-dce]
- LLVMのDCE／ADCE／GlobalDCEは副作用がなく未使用のinstructionや到達不能なinternal globalを除去する。MLIRにもtrivial DCE、symbol DCE、dead-value removalがある。[^llvm-dce] [^mlir-dce]
- GNU ldの`--gc-sections`はentry symbol等をrootとしてrelocationを再帰追跡し、未使用input sectionを除去する。`KEEP()`、export、dynamic objectからの参照等は保持規則になる。[^ld-gc]
- LTO／ThinLTOは複数moduleのLLVM IR／summaryをlink時に見られるため、通常のobject単位linkより広い範囲でinterprocedural eliminationできる。[^llvm-lto]

しかしlinker GCでfunction sectionを捨てても、そのfunctionをparse、typecheck、monomorphize、optimize、object emitした費用は既に支払っている。LLVM DCEでも、不要packageをdownloadしてbuildした費用は戻らない。LAMINARIAの研究対象は、既存の後段DCEを置き換えることではなく、同じnative-executable demandを上流まで伝播させて不要workを早く止めることである。

## 枝刈りの層

| 層 | 枝刈り対象 | 保持root／反例 | 期待効果 |
|---|---|---|---|
| package | 未選択version、disabled optional dependency、不要provider | build tool、runtime、system requirement | solve state、download、build削減 |
| source/module | 到達しないmodule、translation unit、generated unit | macro、generator、registration unit | parse／semantic work削減 |
| semantic item | 未使用function/type/impl、不要generic instantiation | public export、FFI、trait/vtable、reflection | typecheck／monomorphization削減 |
| high/mid IR | 到達不能operation、block、region、dialect fragment | side effect、volatile、exception edge | lowering／optimization／memory削減 |
| artifact | 不要object、archive member、shared library | constructor、whole-archive、linker script | compiler／I/O／link入力削減 |
| symbol/section | 未参照symbol、function/data section | entry、export、`KEEP`、dynamic lookup | binary size／load time削減 |
| runtime | 到達しないruntime component、plugin | dynamic loading contract、platform startup | distribution size／startup削減 |

同じnodeが層により異なる判定を持つ。例えばpackageが必要でも、そのpackage内の全functionが必要とは限らない。逆にsource call graphから見えなくても、C ABI export、static constructor、plugin registration、linker scriptによる保持があればliveである。

## Root setと到達関係

### 明示root

- demanded `NativeExecutable`のentry point
- manifest／public contractで要求したexport
- Rust/Nim/C/C++間のFFI entryとcallback
- platform startup／unwind／runtime support
- testをbuildする場合のtest entry。ただしrelease binaryのrootと混ぜない

### 条件付きroot

- dynamic symbol lookup、reflection、serialization registration
- C/C++ static constructor／destructor、Objective-C category等
- linker scriptの`KEEP`、`--undefined`、whole-archive指定
- weak symbol、plugin ABI、externally loaded shared-library export
- procedural macro、build-time generator、compile-time evaluationのentry

静的に精密なtarget setが得られない場合は、対象集合をover-approximateして保持する。誤って保持するfalse positiveは性能損失だが、必要codeを消すfalse negativeはmiscompileである。unsafeな枝刈りで数値を良くしない。

## LAMINARIAでの表現

各graph nodeに単純な`used: bool`を置かない。少なくとも次を保持する。

```text
LivenessState = Unknown | Live | ProvenDead | RetainedConservatively
LivenessReason = Entry | Dependency | SemanticUse | SideEffect | Export
               | Ffi | Constructor | DynamicLookup | LinkerDirective
               | RuntimeContract | UnsupportedAnalysis
```

`Live` edgeは層型を持つ。

```text
NativeExecutable -> EntrySymbol
EntrySymbol -> SemanticItem
SemanticItem -> Type/Impl/GenericInstance
SemanticItem -> IRUnit
IRUnit -> RequiredSymbol
RequiredSymbol -> ArchiveMember/Object/SharedLibrary
Artifact -> Package/Toolchain producer
```

producer edgeを逆向きに要求伝播し、semantic use／symbol relocation等を順向きに到達探索するため、通常の単一方向DAGよりtyped hypergraphまたは双方向indexが適する。各保持判断にはroot、path、analysis version、toolchain identityをprovenanceとして付ける。

## 解決と枝刈りの協調

```text
1. NativeExecutableと外部公開contractをroot setにする
2. package候補をlazyに展開し、不要version/providerをconstraint pruning
3. live candidateに必要なsource/moduleだけをsemantic query
4. semantic解析から新しいcall/FFI/export/side-effect edgeを追加
5. live itemだけをlowering／monomorphization候補にする
6. IRからrequired symbol／runtime edgeを抽出
7. 必要object/archive memberだけをmaterialize可能なら選択
8. linker GC／LTOへ同じroot/retention contractを渡す
9. 新しいedgeで上流choiceが変われば影響sliceだけ再解決
10. final binaryの保持／削除reportを実際のartifactと照合する
```

non-monotonicなpackage選択と、到達事実の追加を分離する。あるcandidate epoch内では`Unknown -> Live`を基本に単調に進め、choice撤回時はそのepoch由来のliveness provenanceだけをinvalidateする。`ProvenDead`はroot set、feature、target、ABI、dynamic-retention policyを含むkeyの下でのみcacheする。

## 正しさの条件

- 枝刈り後binaryのobservable behaviorが枝刈りなしのreferenceと一致する。
- demanded symbolとruntime artifactには全てproducerがある。
- retained nodeにはrootからのpathまたは保守的保持理由がある。
- pruned nodeには適用したanalysisと、その前提となるfeature／target／visibility／ABIがある。
- opaque dynamic lookupやunsupported constructor semanticsをdeadと仮定しない。
- package／sourceを早期にpruneした結果が、後段linkerだけを使ったreference buildと一致する。

## 直接的な実行可能テスト

最初のmixed Cargo/Nimble/C/C++ workloadには、意図的に次を含める。

- 依存graphには現れるが選択されないpackage／provider候補
- importされないNim/Rust module
- 呼ばれないRust/Nim functionとgeneric instantiation候補
- 未参照C translation unitまたはarchive member
- 未参照C++ functionと、必ず保持すべきconstructor／adapter
- 未参照LLVM/MLIR operationまたはsymbol

production resolver/planner/compiler/linker経路を直接実行し、次を検証する。

1. native executableが期待結果を出す。
2. early-pruned nodeに対応するparse／lowering／compiler actionが起動していない。
3. final link map、symbol table、またはsection-GC reportに不要symbol／sectionがない。
4. FFI export、constructor、runtime supportは残る。
5. featureまたはdynamic-retention policy変更後、必要sliceだけが復活・再計算される。

手書きの期待YAMLとfixture専用validatorを正本にせず、production graph event、実action、生成binaryを検査する。

## 測定

枝刈りの成果をbinary sizeだけで評価しない。

- package candidate／selected package／download／build数
- parsed source、semantic query、monomorphized item数
- generated／retained／pruned IR operationとpeak IR bytes
- compiler、archiver、linker action数
- object、archive member、symbol、sectionのinput／retained／pruned数とbytes
- native binary size、cold start、runtime dependency数
- wall-clock、CPU time、peak RSS
- no-op／leaf／feature／root-set変更後のrecomputed node数
- conservative retention数と理由別内訳

比較対象は、枝刈りなし、linker GCのみ、compiler DCE＋linker GC、cross-layer early pruning＋DCE＋linker GCとする。全構成でobservable behaviorが一致した場合だけ性能を比較する。

## Sources

[^rust-monomorph]: Rust Project, “[Monomorphization — Rust Compiler Development Guide](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html).” Accessed 2026-09-13.
[^rust-link-dead-code]: Rust Project, “[Codegen Options — `link-dead-code`](https://doc.rust-lang.org/rustc/codegen-options/index.html#link-dead-code).” Accessed 2026-09-13.
[^nim-dce]: Nim Project, “[Nim compiler C backend](https://nim-lang.org/docs/compiler/ic/cbackend.html).” Accessed 2026-09-13.
[^llvm-dce]: LLVM Project, “[LLVM’s Analysis and Transform Passes](https://llvm.org/docs/Passes.html).” Accessed 2026-09-13.
[^mlir-dce]: MLIR Project, “[Passes](https://mlir.llvm.org/docs/Passes/).” Accessed 2026-09-13.
[^ld-gc]: GNU Project, “[GNU ld `--gc-sections`](https://sourceware.org/binutils/docs/ld/Options.html#index-_002d_002dgc_002dsections).” Accessed 2026-09-13.
[^llvm-lto]: LLVM Project, “[LLVM Link Time Optimization: Design and Implementation](https://llvm.org/docs/LinkTimeOptimization.html).” Accessed 2026-09-13.
