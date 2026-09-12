import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "hike_r0_reproduce.py"
spec = importlib.util.spec_from_file_location("hike_r0_reproduce", SCRIPT)
reproduce = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reproduce)


class HikeR0ReproductionTests(unittest.TestCase):
    def test_extract_llvm_definition_stops_at_matching_function_end(self):
        ir = """define internal i32 @strlen32(i8* %s) #0 {
entry:
  ret i32 0
}

define void @other() {
  ret void
}
"""
        self.assertEqual(
            reproduce.extract_llvm_definition(ir, "strlen32"),
            "define internal i32 @strlen32(i8* %s) #0 {\nentry:\n  ret i32 0\n}\n",
        )

    def test_extract_llvm_definition_rejects_missing_symbol(self):
        with self.assertRaisesRegex(RuntimeError, "LLVM definition not found"):
            reproduce.extract_llvm_definition("define void @other() {\n}\n", "strlen32")

    def test_extract_objdump_block_stops_at_next_heading(self):
        details = "Import[1]:\n - func[0] <- env.log\nFunction[1]:\n - func[1]\n"
        self.assertEqual(
            reproduce.extract_block(details, "Import"),
            ["Import[1]:", " - func[0] <- env.log"],
        )

    def test_normalize_output_removes_ephemeral_hike_ir_name(self):
        self.assertEqual(
            reproduce.normalize_output("/tmp/hike_build_1234.ll:9: error"),
            "<temp>.ll:9: error",
        )


if __name__ == "__main__":
    unittest.main()
