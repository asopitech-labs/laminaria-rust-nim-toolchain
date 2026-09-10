# 研究意図・文書・Issueの整合性監査（2026-09-10）

## 結論

**誤解を誘発する表現だけでなく、ご意図と異なる実装を正当化する指示があった。** 「既存compilerを細かく観測・操作する」「既存compilerによるself-buildを先に完成させ、独自基盤は後段」「単一言語なら不要な方のcompilerを要求しない」という具体的指示が、独自IR・独自コンパイラ・独自schedulerを開発する目的から逸れていた。

一方、「単なるwrapperではない」「LLVMを再発見する」「functional correctnessとexecution correctnessは別」という警告も既に存在した。文書だけが原因ではない。エージェントが警告と利用者の訂正より、外部ツール連携で通る実装・テストを優先して解釈した責任がある。利用者の説明不足として扱わない。

訂正の基準は[独自コンパイラの責務契約](compiler-ownership-contract_ja.md)（[English](compiler-ownership-contract.md)）。独自compiler/IRの形やpass構成をここで既定にはせず、**誰が意味解析・変換・生成・計算scheduleを担うか**を明確にした。

## 対象と方法

- 基準コミット: [`1bc0286`](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/commit/1bc028630a3cc7d5079ebeaa3ddc44cc17770fca)。
- repository管理下の既存Markdown **41ファイル**: `docs/` 30、README 1、fixture/実装の補助記録10。隠しファイルも含めた再検索で研究Markdownの取りこぼしを確認した。
- GitHub **全25 Issue（#2–#26、取得時すべてOPEN）**の本文と**51コメント**。更新前に本文・titleを再取得し、他作業との競合がないことを確認した。
- 全件を取得し、目的、非目標、主経路、段階的実装、fallback、toolchain選択、self-build、単一言語、依存順序、受け入れ条件を横断検索し、該当節・英日対応・Issue指示を照合した。
- 現行CLIの `project_build.rs` / `self_build.rs` のAction種別・実行loopと、semantic-substrate fixtureの契約・制約を照合した。
- 第三者ソース、license本文、生成済み測定出力、過去コミットは書き換えない。外部参考サイトの全技術記述の再検証や過去benchmarkの再実行をしたという主張ではない。

## 具体的な不整合

以下の引用は**訂正前の記録**であり、現行方針ではない。文書は上記コミットで追跡できる。Issue本文は今回更新したため、旧方針と訂正理由をここに残す。

| 対象 | 訂正前の表現・指示 | 誘導された誤り | 訂正 |
| --- | --- | --- | --- |
| research-foundations §12/§15（英日） | “Existing ecosystem tools remain the execution engines.” / “reuse mature resolvers and compilers” | 既存compilerを本ビルドengineとして正当化 | resolverとcompilerを分離。外部compilerは比較/bootstrapのみ |
| research-foundations §10、research-program Track A（英日） | “Fine-grained integration is an optimization, not a prerequisite for a valid build.” | 独自compilerがなくても外部fallbackで本機能を達成扱い | 独自compiler内のgroupingと外部委譲を区別。未対応は診断 |
| project-proposal §4/§6（英日） | 各compilerからsemantic modelへの写像、Rust CGU→LLVM/Cranelift、Nim C→clangを研究ゴールの図として記述 | compiler出力を統合するbuild graphが独自IR開発の代替になる | 独自source処理→IR→変換→schedule→target生成へ図と責務を変更 |
| llvm-rediscovery、#5/#25 | “future LAMINARIA-native route”、既存backendへのprojectionを完了条件化 | 独自compilerが任意の将来backendになった | 独自生成を本経路にし、LLVM等への投影は比較実験へ |
| Epic #2、#6/#8ほかの先頭delivery指示 | “self-hosting of the build tool”、既存compilerを初期に許容し#25を後段に配置 | 外部compilerで自身をbuildすることが主要目標になった | 独自compilerで自身と依存をstage1/stage2へコンパイルする条件を明示 |
| #26、project-build（英日） | Rust-onlyではNim不要、Nim-onlyではrustc不要という対称条件 | 必要な側の既存compilerへ戻る実装でも達成になる | 両方の単一言語と混成で同じ独自compilerを使う条件へ |
| #18/#22/#23/#24、version/profile/UX文書 | rustc/Nimの組合せ選択を通常profileとして規定 | package互換性とcompiler実装の選択が混ざる | 独自compiler/IR/language-contract profileと比較/bootstrap行列を分離 |
| native-linking「非目標」（英日） | “replacing either compiler frontend” | linking Issueの局所的非目標がプロジェクト全体の非目標に見える | frontendは#3/#25の責務と明記 |
| native-linkingの値渡し評価（英日） | 手書きFFIで一般的でないため低優先度とする説明 | ソース意味を実装するcompilerの要求をFFI作法で除外 | 既存routeの観測を残し、対応言語意味から優先度・adapter義務を導出 |
| measurement-foundation（英日） | runtime/CI/lockがまだ存在しないという現在形 | 実績と着手順序の認識が古いままになる | 現在の基盤と外部委譲baselineの存在を反映 |

さらに[#18の過去コメント](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/18#issuecomment-5594863244)には “LAMINARIA isn't redeveloping LLVM/rustc” とあり、環境計測の説明に研究目的を狭める断定を混ぜていた。LLVM/rustcのAPIをそのまま再実装する必要がないことと、独自compilerを開発しないことは別である。コメントは履歴として残し、現在のIssue本文で訂正した。

## 実装実績の再分類（コード変更なし）

- 現行project-build/self-buildは `CargoBuild` / `NimBuild` をproduction Nim plannerとRust runtimeで処理するが、実行loopは逐次であり、コンパイルは既存compilerへ委譲している。プロセス連携・入力検査・成果物・失敗伝播の改善は有用だが、独自compilerの証拠ではない。
- semantic-substrate fixtureには独自の小さな表現・evaluator・変換がある。ただし表現は手書きで、LLVM projectionは外部 `llc` 等を使う限定実験である。ソースfrontend、一般的な言語意味の適合性、独自target生成を達成したとはしない。
- stage0→stage1という名前や世代数ではcompiler self-hostingを判定しない。外部compilerによる再構築をstage2まで繰り返してもbaselineである。
- 測定結果、失敗実験、既存checkmark、修正履歴は消さない。今回の文書変更でcode/CLI/schemaが対応済みになったとはしない。

## 維持する研究対象

垂直統合と水平分割の両立、many-core/cache/NUMA・memory容量/帯域に基づくschedule、disk/networkの実測比較、メモリ内保持・永続化・replica・再計算、Windows/macOS/ラズパイのhost/target分離と並行cross compileは研究対象のまま維持する。

ただし分割するのはLAMINARIA自身のcompiler計算である。既存compilerをremoteへ送る実験は比較用であり、その成功を独自IRの分割原理の証明にしない。Rust-onlyの速度改善をプロジェクト成立の前提にはしない。

## 文書別の監査・処置一覧

| ファイル | 処置 |
| --- | --- |
| [README.md](../README.md) | 冒頭のプロジェクト定義と現状・未達を訂正 |
| [crates/laminaria-run/NOTES.md](../crates/laminaria-run/NOTES.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [docs/agent-oriented-toolchain-ux.md](agent-oriented-toolchain-ux.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/agent-oriented-toolchain-ux_ja.md](agent-oriented-toolchain-ux_ja.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/backend-pipeline-whiteboxing.md](backend-pipeline-whiteboxing.md) | 既存backend/link実験と独自compiler責務を分離 |
| [docs/backend-pipeline-whiteboxing_ja.md](backend-pipeline-whiteboxing_ja.md) | 既存backend/link実験と独自compiler責務を分離 |
| [docs/horizontal-distribution-research.md](horizontal-distribution-research.md) | 垂直統合・水平分散・永続化・異種nodeの研究を維持し、独自compiler計算を明示 |
| [docs/horizontal-distribution-research_ja.md](horizontal-distribution-research_ja.md) | 垂直統合・水平分散・永続化・異種nodeの研究を維持し、独自compiler計算を明示 |
| [docs/issue-plan.md](issue-plan.md) | #25/#3/#6/#8を中核に依存順序を訂正 |
| [docs/llvm-rediscovery-research.md](llvm-rediscovery-research.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/llvm-rediscovery-research_ja.md](llvm-rediscovery-research_ja.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/measurement-foundation.md](measurement-foundation.md) | 独自in-process計算の証拠と外部process計測を区別 |
| [docs/measurement-foundation_ja.md](measurement-foundation_ja.md) | 独自in-process計算の証拠と外部process計測を区別 |
| [docs/metrics-policy.md](metrics-policy.md) | 独自in-process計算の証拠と外部process計測を区別 |
| [docs/multi-version-toolchains.md](multi-version-toolchains.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/multi-version-toolchains_ja.md](multi-version-toolchains_ja.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/project-build.md](project-build.md) | 外部委譲baselineの実装記録へ訂正。コマンド挙動は変更しない |
| [docs/project-build_ja.md](project-build_ja.md) | 外部委譲baselineの実装記録へ訂正。コマンド挙動は変更しない |
| [docs/project-proposal.md](project-proposal.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/project-proposal_ja.md](project-proposal_ja.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/research-foundations.md](research-foundations.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/research-foundations_ja.md](research-foundations_ja.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/research-program.md](research-program.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/research-program_ja.md](research-program_ja.md) | 本経路の責務を明示。目的・主経路・完了条件を訂正 |
| [docs/rust-nim-native-linking.md](rust-nim-native-linking.md) | 既存backend/link実験と独自compiler責務を分離 |
| [docs/rust-nim-native-linking_ja.md](rust-nim-native-linking_ja.md) | 既存backend/link実験と独自compiler責務を分離 |
| [docs/self-build.md](self-build.md) | 外部委譲baselineの実装記録へ訂正。コマンド挙動は変更しない |
| [docs/self-build_ja.md](self-build_ja.md) | 外部委譲baselineの実装記録へ訂正。コマンド挙動は変更しない |
| [docs/toolchain-ux-research-contract.md](toolchain-ux-research-contract.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/toolchain-ux-research-contract_ja.md](toolchain-ux-research-contract_ja.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/validated-toolchain-profiles.md](validated-toolchain-profiles.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [docs/validated-toolchain-profiles_ja.md](validated-toolchain-profiles_ja.md) | 独自compiler profileの条件を追加。既存tool行列は比較/bootstrapへ限定 |
| [fixtures/README.md](../fixtures/README.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/STATE-CONTRACTS.md](../fixtures/STATE-CONTRACTS.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/direct-native-link/NOTES.md](../fixtures/direct-native-link/NOTES.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/incremental-semantic-edit/EDIT.md](../fixtures/incremental-semantic-edit/EDIT.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/laminaria-semantic-substrate-prototype/CONTRACT.md](../fixtures/laminaria-semantic-substrate-prototype/CONTRACT.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/laminaria-semantic-substrate-prototype/NOTES.md](../fixtures/laminaria-semantic-substrate-prototype/NOTES.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/llvm-rediscovery-semantic-workload/CONTRACT.md](../fixtures/llvm-rediscovery-semantic-workload/CONTRACT.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/llvm-rediscovery-semantic-workload/NOTES.md](../fixtures/llvm-rediscovery-semantic-workload/NOTES.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |
| [fixtures/rust-nim-llvm-lto-compatibility/NOTES.md](../fixtures/rust-nim-llvm-lto-compatibility/NOTES.md) | 履歴・fixtureの証拠範囲を明示。測定値と過去の修正記録は保持 |

追加文書は本監査と責務契約の英日版。旧文書の具体的な矛盾箇所は本文で修正し、比較研究・実装履歴には証拠範囲を明示した。過去の観測値そのものは改変していない。

## Issue別の監査・処置一覧

全25件の本文を更新し、改訂したtitle/bodyとOPEN状態をGitHubから再取得して一致を確認した。単なる文書訂正で研究Issueをcloseしたり、新たな実装要件を達成済みにしたりしていない。既存のreference fixture checkmarkはその範囲で残し、新しい独自経路の条件は未完了で追加した。

| Issue | 現在の責務・訂正の要点 |
| --- | --- |
| [#2](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/2) | Develop and self-host LAMINARIA's own Rust/Nim compiler, IR and scheduler. The delegated build driver is only a bootstrap/reference milestone. |
| [#3](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/3) | Implement source-to-owned-IR semantic processing for a declared Rust/Nim subset with #25. Upstream stage maps are reference/provenance evidence, not the production frontend. |
| [#4](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/4) | Derive cross-language runtime/layout/call contracts for the owned compiler. Preserve existing native-link and planner-ABI experiments as reference/bootstrap evidence; their link success is not compiler completion. |
| [#5](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/5) | Select and exercise LAMINARIA-owned target lowering/code generation. Existing compiler backend families remain role-separated reference experiments, not alternate production compilers. |
| [#6](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/6) | Schedule LAMINARIA-owned compiler computations over semantic/analysis dependencies, not only whole Cargo/Nim processes. Co-design work units, grouping and resource control with #25/#3/#8. |
| [#7](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/7) | Apply identity, invalidation and reuse to owned source/IR/analysis/target artifacts. Existing rustc/Nim cache experiments remain comparison evidence. |
| [#8](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/8) | Make the production Nim planner consume LAMINARIA semantic/IR dependencies and plan owned compiler work jointly with #25/#3/#6. Topological ordering of external compiler calls does not meet this requirement. |
| [#9](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/9) | Evaluate mixed-language Wasm topology with the independent compiler's source/IR path. Existing-compiler-produced modules are comparison baselines, not proof of owned Wasm compilation. |
| [#10](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/10) | Explain compiler ownership, source/IR provenance, legality, analysis invalidation and actual scheduler decisions. A visible external fallback still fails independent-compilation acceptance. |
| [#11](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/11) | Maintain shared measurement infrastructure with separate independent-compilation, reference and bootstrap evidence. Build minimum instrumentation alongside the compiler slice rather than a serial all-measurement gate. |
| [#12](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/12) | Prove work elimination and no-op behavior in LAMINARIA's own compiler; an existing compiler's incremental cache is baseline behavior, not delegated proof of that goal. |
| [#13](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/13) | Execute and evaluate owned compiler pipeline boundaries from #25/#3. Existing backend pipelines are reference cases; merely admitting a future native variant is insufficient. |
| [#14](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/14) | Study LLVM as prior art and compare mechanisms against owned semantic/IR experiments. This issue may produce reference evidence without claiming to deliver the independent compiler. |
| [#15](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/15) | Keep LLVM ThinLTO/DTLTO integration as a distributed-backend baseline. Derive and compare an owned compiler partition with #25/#6/#7 rather than adopting LLVM jobs as the production substrate. |
| [#16](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/16) | Classify the wasm-ld/Binaryen/WIT pipeline as prior-art/target-boundary research, and connect its lessons to the owned compiler path. Existing-backend compilation cannot qualify owned target generation. |
| [#17](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/17) | Retain LLVM convergence as a reference experiment only. No success, local or distributed, may promote merged LLVM IR or an existing compiler to the target-production path. |
| [#18](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/18) | Separate bootstrap/reference tool fingerprints from LAMINARIA compiler/IR/transform/runtime and host/target identities. Already checked fixture criteria below remain historical baseline evidence only. |
| [#19](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/19) | Add owned in-process compiler-work and scheduler events to the common Run, separate from external process traces; process observation is not proof of compiler scheduling ownership. |
| [#20](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/20) | Record source-derived IR, transform/analysis dependencies and target artifacts with actual producer lineage, not only external compiler telemetry/files. |
| [#21](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/21) | Compare owned compiler scenarios against explicitly separate reference/bootstrap runs, with semantic equivalence, path, state and resource evidence. Repeated external self-build is not compiler self-hosting. |
| [#22](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/22) | Model LAMINARIA implementation/IR revisions and supported language contracts separately from exact external compiler versions used in reference/bootstrap matrices. |
| [#23](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/23) | Qualify owned-compiler profiles for Rust-only, Nim-only and mixed inputs. Existing rustc/Nim bundle qualification must not qualify the product compiler profile. |
| [#24](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/24) | Bound planning over owned compiler/language/IR/target/resource capabilities. Missing support returns a diagnostic or an explicitly separate research option, never an automatic existing-compiler fallback. |
| [#25](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/25) | Own the central independent IR/compiler research and implement it with #3/#6/#8 now. It is not an optional later backend or a fixture-only track deferred behind delegated self-build. |
| [#26](https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/issues/26) | All supported Rust-only, Nim-only and mixed projects use the same LAMINARIA-owned compiler/IR/scheduler. Neither rustc nor Nim's compiler becomes the default for its single-language case. |

## 次の実装判断

最初のまとまりは **#25 + #3 + #6 + #8**。小さなRust/Nimソースの対応範囲を決め、source-derived IR、意味を保持する変換、独自compiler計算の実行と資源制約、狭いtarget生成を端から端まで実装・検証する。手書きIRの改善だけ、command wrapperの改善だけをこのまとまりの完成としない。

必要な計測・説明は並行し、#7/#12で同じ独自経路の無効化・再利用を研究する。#26はその共通基盤の単一言語／混成入口である。#4のplanner linkや全profile行列の完成をcompiler研究に先行する一律gateにしない。自身のRust + Nimソースと依存をコンパイルするself-hostingは最終目標として保持する。

## 検証範囲

今回の差分はMarkdownのみ。追加分を含む44ファイルの文書リンク・fence/見出し構造・責務契約への参照、diffの空白、訂正前の危険な方針句、英日対応を検査した。全25 Issueは更新後に本文・title・OPEN状態を再取得して一致を確認した。コード機能・性能の再検証や修正はこの監査では実施していない。

## English summary

The audit found actual contradictions, not merely a lack of emphasis: existing compilers were prescribed as execution engines, owned compiler work was deferred behind delegated self-build, and single-language support only excluded the unused upstream compiler. Anti-wrapper and LLVM-rediscovery statements already existed; the agent also failed to honor them.

The corrected contract requires LAMINARIA-owned source processing, IR, transformations, target generation and compiler-work scheduling for Rust-only, Nim-only and mixed inputs, ultimately including its own Rust + Nim implementation. Package resolution, reference experiments and external bootstrap remain separate roles. Historical evidence is preserved at its demonstrated scope. All 41 existing Markdown files and 25 issues/51 comments were inventoried and checked for objective/acceptance alignment; all 25 issue bodies were updated and read back. No compiler implementation or CLI behavior is claimed changed.
