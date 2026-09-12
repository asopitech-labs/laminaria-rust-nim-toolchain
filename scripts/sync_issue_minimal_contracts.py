#!/usr/bin/env python3
"""Apply the canonical minimal-hypothesis contract to every GitHub issue.

The existing issue body is preserved.  This script only inserts or replaces the
bounded block at its beginning, so research history and detailed future evidence
lists remain available without being mistaken for finished-product requirements.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from dataclasses import dataclass


START = "<!-- minimal-hypothesis-contract:start -->"
END = "<!-- minimal-hypothesis-contract:end -->"
POLICY = "https://github.com/asopitech-labs/laminaria-rust-nim-toolchain/blob/main/docs/research-prioritization-policy_ja.md"


@dataclass(frozen=True)
class Contract:
    priority: str
    hypothesis: str
    experiment: str
    stop: str


CONTRACTS: dict[int, Contract] = {
    2: Contract("P0 umbrella", "Rust/Nim sourceから独自IR・変換・計画・生成を通る経路を段階的にLAMINARIA自身へ拡張できる。", "子Issueで得た最小owned sliceを一つずつ統合し、各段階で既存compiler fallbackなしのproducer lineageを確認する。", "現在選んだ子仮説の採否と次のcoverage gapが決まった時点。Epic全体を一度に完成させない。"),
    3: Contract("P0", "Rust/Nimの対応source constructから、意味とprovenanceを保持したowned IRを導ける。", "同一契約のRust/Nim各1 sourceをlowerし、対応factと一つの拒否入力を比較する。", "保持できたfact、失ったfact、次にIRへ追加すべきfactを判断できた時点。"),
    4: Contract("P1", "早期C ABI固定なしでも、限定したRust/Nim値・呼出契約を明示できる。", "一つのdirect native call shapeと一つの非互換shapeを再現し、必要なABI/runtime obligationを列挙する。", "direct、adapter-required、unsupportedの境界を一つ決定できた時点。"),
    5: Contract("P0/P1", "owned IRから既存backendへ委譲せず、一つのtarget routeを選択・生成できる。", "現在のValidatedProgramをowned WebAssemblyへ生成し、同じ入力値を実行確認する。", "routeの成立または不足IR/runtime obligationが判定できた時点。route matrix完成は求めない。"),
    6: Contract("P1", "LAMINARIA固有のcompiler-work依存をNim計画とRust実行へ分離しても意味結果を保てる。", "独立2 actionと依存2 actionを一つの予算下で実行し、順序・結果・一つの資源待ちを観測する。", "必要なscheduler stateと意味依存境界が判定できた時点。汎用scheduler完成は求めない。"),
    7: Contract("P1", "semantic inputとoperationから作るidentityで、変更影響を必要なartifactだけへ限定できる。", "一つのno-op、一つの意味変更、一つの非意味変更でreuse/invalidation集合を比較する。", "identityに含めるfactと含めないfactを一つ決定できた時点。CAS完成は求めない。"),
    8: Contract("P1", "demandと制約を使えば候補直積を全展開せず必要planを選べる。", "小さなvariant graphで要求1件と既知不適合1件を与え、探索・prune数を比較する。", "少なくとも一つのpruning/merge ruleの有効性または失敗が判定できた時点。"),
    9: Contract("P0/P1", "Rust/Nim由来のowned artifactsを一つのWasm target contractで統合できる。", "最小の一方向依存を持つ2 unitについて一つの統合方式と一つの拒否方式を比較する。", "統合に必要なsemantic/ABI factと採用topologyを一つ判断できた時点。"),
    10: Contract("P1", "plan/変換/再利用の判断を、使用factと理由へ追跡可能にできる。", "一つの採用判断と一つの拒否判断をmachine-readable explanationで出力する。", "人とagentが入力factから結論を再構成できると確認した時点。説明UI完成は求めない。"),
    11: Contract("P1", "核心仮説の比較に必要な測定をobserver costと分離して再現できる。", "一つのowned workloadを固定環境で反復し、raw runとobserver-on/off差を保存する。", "次のarchitecture判断に足る信頼区間または測定不能理由が得られた時点。benchmark製品完成は求めない。"),
    12: Contract("P1", "semantic identityとdemandにより、不要compiler workを正しく除外できる。", "cold、true no-op、leaf変更を一例ずつ実行し、executed/skipped集合と結果を比較する。", "一つの安全な省略規則と一つの省略禁止条件を判断できた時点。"),
    13: Contract("P0/P1", "backend内部のどの境界をcheckpoint/execution nodeにするかをcostから選べる。", "一つのtarget pipelineをopaque/fine-grainedの2分割で表し、materializationと再計算costを比較する。", "一つの境界を採用、棄却、保留できた時点。全backend graph完成は求めない。"),
    14: Contract("P1 baseline", "LLVMの一つの境界が必要になった理由をworkloadから再導出できる。", "一つのpass/analysisについて有無で壊れるcaseとLAMINARIA候補表現を比較する。", "LLVM境界を確認、再表現、または不要と判断できた時点。LLVM全体のwhite-box化は求めない。"),
    15: Contract("P1 baseline", "ThinLTO/DTLTOのsummary/backend-job境界がLAMINARIAにも必要か判定できる。", "同一workloadをmodule partitionとsemantic-fact partitionの各1案で比較する。", "一つのpartition boundaryを採用、変更、または拒否できた時点。"),
    16: Contract("P1 baseline", "Wasm生成後のlink/opt/component境界のうち、owned routeに必要なものを識別できる。", "一つのCore Wasm artifactをopaque routeと一段展開routeで比較する。", "一つの必須境界と一つの任意境界を判断できた時点。pipeline完成は求めない。"),
    17: Contract("P2 baseline", "Rust/Nim由来LLVM artifactの収束可能性と、収束しても失われるsemantic factを区別できる。", "一つの同等workloadをlink/LTOし、成功可否と上流fact lossを記録する。", "LLVM収束がLAMINARIA substrateの代替になるか否かを判断できた時点。"),
    18: Contract("P2 enabler", "比較runに必要なtoolchain/environment identityを再現可能に固定できる。", "Rust/Nim各1環境を解決し、同一・差異fingerprintを検証する。", "現在の一実験を再現・拒否できるidentityが得られた時点。環境matrix完成は求めない。"),
    19: Contract("P2 enabler", "一つの実験runのproducer、process、resource結果を失敗時も保存できる。", "成功1件と失敗1件をversioned Runとしてround-tripする。", "現在の仮説証拠を再構成できた時点。汎用tracer完成は求めない。"),
    20: Contract("P2 enabler", "architecture比較に必要なartifact差分とcompiler-native telemetryを同じRunへ関連付けられる。", "一つの編集前後でartifact deltaと一種類のnative telemetryを保存する。", "変更原因を一つ帰属できた時点。全adapter実装は求めない。"),
    21: Contract("P2 enabler", "一つの性能差をnoiseと区別し、再計算可能な判定として保存できる。", "固定workloadで反復し、既知の同一ケースと差異ケースを比較する。", "次の一比較を採用または測定不能と判定できた時点。統計基盤完成は求めない。"),
    22: Contract("P2 baseline", "toolchain version差をartifact互換性の明示制約として扱える。", "RustまたはNimの2 versionで一つのartifact reuse可否を比較する。", "一つの互換／非互換規則を証拠付きで決めた時点。全version matrixは求めない。"),
    23: Contract("P2 UX", "検証済みbundleを曖昧な最新選択と分離して提示できる。", "一つのqualified profileと一つのoverrideによる資格喪失を表示する。", "qualification継承規則を一つ判断できた時点。profile製品完成は求めない。"),
    24: Contract("P2 UX", "既知制約とnegative knowledgeでagentの試行錯誤を一つ削減できる。", "同じ要求を無制約／bounded planningで実行し、外部試行数と説明を比較する。", "一つの探索削減ruleの効果または無効性が判定できた時点。UX完成は求めない。"),
    25: Contract("P0", "Rust/Nim semantic factsから、LLVM境界を前提にしない共通substrateを一つ導出できる。", "一つのpaired source workloadでfactを保持し、候補表現、owned変換、owned target生成まで通してLLVM由来routeと比較する。", "少なくとも一つのLLVM概念を確認・再表現・拒否し、次のsubstrate判断を行えた時点。LLVM再発見全体の完遂は求めない。"),
    26: Contract("P0 delivery evidence", "Rust-only、Nim-only、mixedの最小入力が同じowned substrateを利用できる。", "各入力class一例を同じ公開entryから実行し、不要compilerとhidden fallbackがないことを確認する。", "三分類で経路同一性または不足能力が判定できた時点。一般project support完成は求めない。"),
    27: Contract("P1 milestone", "source-derived owned compiler workをproduction Nim plannerとRust executorで意味を保って実行できる。", "宣言済みsubsetのRust/Nim chainを計画・検証・実行し、一つの失敗境界を確認する。", "bridge仮説の採否と後続gapが決まった時点。resource managerやcompiler完成は求めない。"),
    28: Contract("P0/P1", "実プロジェクト形状でも、需要駆動のowned compiler chainが静的一括経路と異なる価値を示せる。", "確定済みcaseから単一言語1件とmixed1件だけを選び、static/pullのwork開始・実行集合・結果を比較する。", "pull方式固有の一つの利点、欠点、または不成立理由を判断できた時点。全M1〜M10や高速化完成は求めない。"),
    29: Contract("P0 umbrella", "低水準化前のRust/Nim semantic factsが既存backend任せでは得にくい最適化を一つ可能にする。", "子実験一つでowned変換と拒否をsource-to-targetで比較する。", "一つのsemantic factの有用性を採用・棄却できた時点。全最適化群は一括完了しない。"),
    30: Contract("P1 prior art", "既存上流最適化が必要とするsemantic factをRust/Nim source producerまで逆追跡できる。", "一つのprior-art変換と失敗caseを固定revisionで再現し、必要factを記録する。", "LAMINARIA子実験へ渡すfactとlegality conditionが一つ確定した時点。prior art全調査は求めない。"),
    31: Contract("P0", "Rust/Nim境界を跨いで保持したsemantic factsにより、separate compilationでは困難なowned変換を成立させられる。", "固定したRust callerとNim calleeをsource-derived owned IRで合成し、cross-language inlineの適用前後と不整合signature一件を比較する。", "cross-language合成と変換の合法性を一つ採用または棄却できた時点。incremental reuseはこの判定後の別実験とする。"),
    32: Contract("P0", "shape/alias/effect factがあれば、map/filter/reduceの一つを合法に融合し、不明aliasでは拒否できる。", "一つのfusion positiveと一つのalias-negativeをsource-derived IRからtargetまで比較する。", "融合に必要な最小fact集合を判断できた時点。一般collection optimizer完成は求めない。"),
    33: Contract("P0", "affine factで一つのstencil変換を合法化し、irregular accessを明示拒否できる。", "小さなaffine stencilと一つの非affine counterexampleを比較する。", "必要factと拒否境界を判断できた時点。polyhedral optimizer完成は求めない。"),
    34: Contract("P0", "shape/context factと実測により一つのspecialization/layout候補を選択できる。", "固定候補2つをtraining/holdoutで比較し、一つの変更時invalidationを確認する。", "static選択かempirical選択かを一例で判断できた時点。autotuner完成は求めない。"),
    35: Contract("P1 completed design evidence", "#28の最初の実験対象を実装者へ選定させず固定できる。", "LAMINARIA自身と補完fixtureのcaseを有限に定義し、期待値を独立検証する。", "D1へ渡せる確定revisionが得られた時点。全モノレポ完成を意味しない。"),
    36: Contract("P1 completed experiment", "動的依存発見中でも、確定した別branchのowned workを進行できる。", "一つのsessionでdependency discoveryと独立workを増分契約により処理し、結果と失敗境界を確認する。", "増分協調の成立と後続gapが判断できた時点。汎用incremental build system完成を意味しない。"),
    37: Contract("P0/P1", "source recursion、analysis SCC、無効artifact cycleを同じcycleとして誤処理せず分類できる。", "各分類一つの最小graphを作り、許可／fixpoint／拒否を比較する。", "三分類のsemantic contractを判断できた時点。一般SCC engine完成は求めない。"),
    38: Contract("P0/P1", "IRの保持・spill・再計算の選択をartifact semanticsとcostから一例で決められる。", "一つのIR artifactをkeep/spill/recomputeし、同一結果と時間・bytesを比較する。", "一つの条件で選択方針を採用または保留できた時点。memory manager完成は求めない。"),
    39: Contract("P2 enabler", "compiler artifactを不完全なままconsumerへ公開せず、crash後に判別できる。", "commit直前／直後のfault各1件でconsumer visibilityとrecoveryを確認する。", "一つのpublication protocolの成立または不足が判定できた時点。durable store完成は求めない。"),
    40: Contract("P0/P1", "LAMINARIA semantic workのpartitionはgeneric remote Action境界と異なる配置判断を生む。", "同一workloadをlocal、coarse remote、semantic partitionの3案で一度比較する。", "一つのpartitionを採用・棄却し、必要な転送factを判断できた時点。distributed platform完成は求めない。"),
    41: Contract("P1", "execute-onとproduces-forを分離すれば、異種nodeで安全に一つのcross-compile actionを配置できる。", "異なる2 host/target契約と一つの不適合caseでplacementを判定する。", "一つのportable actionと一つのtarget-bound actionを分類できた時点。node platform完成は求めない。"),
    42: Contract("P0", "source semanticsからruntime obligationを導けば、未使用supportを生成せず必要supportだけをowned artifactへ含められる。", "一つのcapability使用caseと未使用caseを生成し、symbol/bytes差と拒否を確認する。", "一つのcapability derivation ruleを採用または棄却できた時点。runtime完成は求めない。"),
    43: Contract("P0/P1", "owned Rust/Nim sourceから、必要byteとhost obligationを説明できる極小Wasm経路を作れる。", "既存R0をbaselineに、Rust/Nim各一つのsource-derived moduleでcapability delta一件を生成・実行する。", "artifact差をsemantic/runtime obligationへ帰属できた時点。V0〜V5全完遂や最小サイズ記録更新は求めない。"),
    44: Contract("P0/P1", "Nim target unitを既存compilerへ委譲せず、宣言されたC/C++依存を独立artifactとして利用できる。", "一つのimportcまたはimportcpp呼出をowned Nim IRからforeign artifactへ接続し、一つの非互換ABIを拒否する。", "foreign dependencyとhidden fallbackの境界を一つ判断できた時点。C/C++統合完成は求めない。"),
}


def run(*args: str) -> str:
    return subprocess.check_output(args, text=True)


def block(number: int, contract: Contract) -> str:
    return f"""{START}
## 現段階の最小仮説検証契約（2026-09-13）

[研究Issueの優先順位・最小仮説検証ポリシー]({POLICY})をこのIssueの既存本文・チェックリストより優先する。

- **優先度**: {contract.priority}
- **最小仮説**: {contract.hypothesis}
- **最小実験**: {contract.experiment}
- **停止条件**: {contract.stop}
- **非ゴール**: 完成品、全機能、全target、全OS、全failure mode、最適性能、production運用をこのIssue単独で完成させること。

以下の既存Acceptance criteria、拡張項目、将来要件は研究backlog／候補証拠として保持する。現段階で全項目を一括達成する条件ではない。次に実施する項目は、上記の最小実験を判定する範囲だけ明示して着手する。
{END}
"""


def updated_body(number: int, body: str) -> str:
    prefix = block(number, CONTRACTS[number])
    if START not in body:
        return prefix + "\n" + body
    before, rest = body.split(START, 1)
    _old, after = rest.split(END, 1)
    return before + prefix + "\n" + after.lstrip("\n")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    issues = json.loads(run("gh", "issue", "list", "--state", "all", "--limit", "100", "--json", "number,body"))
    actual = {item["number"] for item in issues}
    expected = set(CONTRACTS)
    if actual != expected:
        raise SystemExit(f"issue coverage mismatch: missing={sorted(actual - expected)}, stale={sorted(expected - actual)}")

    changed: list[int] = []
    for issue in sorted(issues, key=lambda item: item["number"]):
        number = issue["number"]
        body = issue["body"]
        new_body = updated_body(number, body)
        if new_body == body:
            continue
        changed.append(number)
        if args.apply:
            with tempfile.NamedTemporaryFile("w", encoding="utf-8") as temp:
                temp.write(new_body)
                temp.flush()
                subprocess.run(
                    ["gh", "issue", "edit", str(number), "--body-file", temp.name],
                    check=True,
                )

    mode = "updated" if args.apply else "would update"
    print(f"{mode} {len(changed)} issue(s): {changed}")


if __name__ == "__main__":
    main()
