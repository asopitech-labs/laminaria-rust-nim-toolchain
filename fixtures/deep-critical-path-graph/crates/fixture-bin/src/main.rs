const SEED: u64 = 20260909;
const EXPECTED_RESULT: u64 = 357142201;

fn main() {
    let result = stage_12::run(SEED);
    println!("deep-critical-path-graph seed={SEED} result={result}");
    assert_eq!(
        result, EXPECTED_RESULT,
        "chained result drifted from the committed reference value"
    );
}
