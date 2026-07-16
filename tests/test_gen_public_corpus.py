import importlib.util
import io
import pathlib
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "gen_public_corpus.py"
SPEC = importlib.util.spec_from_file_location("gen_public_corpus", SCRIPT)
gen_public_corpus = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(gen_public_corpus)


def manifest_paths(path: pathlib.Path) -> set[str]:
    return {
        line.split("\t", 1)[0]
        for line in path.read_text(encoding="utf-8").splitlines()
        if line and not line.startswith("#") and not line.startswith("path\t")
    }


class PublicCorpusGeneratorTests(unittest.TestCase):
    def run_generator(self, out_dir: pathlib.Path, *args: str) -> int:
        with mock.patch.object(gen_public_corpus, "OUT_DIR", str(out_dir)):
            with mock.patch.object(sys, "argv", [str(SCRIPT), *args]):
                with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                    return gen_public_corpus.main()

    def test_check_is_no_write_and_detects_missing_or_stale_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            out_dir = pathlib.Path(tmp) / "synthetic"

            self.assertEqual(self.run_generator(out_dir, "--check"), 1)
            self.assertFalse(out_dir.exists())

            self.assertEqual(self.run_generator(out_dir), 0)
            stale = out_dir / "comments.docx"
            stale.write_bytes(b"stale")
            self.assertEqual(self.run_generator(out_dir, "--check"), 1)
            self.assertEqual(stale.read_bytes(), b"stale")

    def test_every_public_docx_is_manifested_or_attributed(self):
        expected = {f"synthetic/{name}" for name in gen_public_corpus.CORPUS}
        self.assertEqual(manifest_paths(ROOT / "corpus/public/MANIFEST.tsv"), expected)
        self.assertEqual(
            manifest_paths(ROOT / "corpus/public/RENDER_MANIFEST.tsv"), expected
        )

        corpus_root = ROOT / "corpus/public"
        observed = {
            path.relative_to(corpus_root).as_posix()
            for path in corpus_root.rglob("*.docx")
        }
        attribution = (corpus_root / "ATTRIBUTION.md").read_text(encoding="utf-8")
        for relative in observed - expected:
            self.assertTrue(relative.startswith("vendored/"), relative)
            self.assertIn(f"`{pathlib.PurePosixPath(relative).name}`", attribution)


if __name__ == "__main__":
    unittest.main()
