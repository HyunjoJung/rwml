"""Descriptor readers must preserve binary bytes on Windows as well as POSIX."""

import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import render_oracle_contract as contract  # noqa: E402
import table_oracle_topology as topology  # noqa: E402
import word_oracle_capture as word  # noqa: E402


READERS = (
    contract._read_bounded_regular_file,
    topology._read_bounded_regular_file,
    word._read_regular_file,
)


class OracleBinaryReadTests(unittest.TestCase):
    def test_readers_preserve_crlf_and_ctrl_z(self):
        payload = b"PK\x03\x04\r\n\x1a\x00\xfftrailing bytes\r\n"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "input.bin"
            path.write_bytes(payload)
            for read in READERS:
                with self.subTest(reader=read.__module__):
                    self.assertEqual(read(path, len(payload)), payload)

    def test_readers_request_binary_mode_when_platform_provides_it(self):
        real_open = os.open
        native_binary = getattr(os, "O_BINARY", 0)
        binary = native_binary or 1 << 29

        def open_binary(path, flags):
            self.assertTrue(flags & binary, "descriptor did not request O_BINARY")
            return real_open(path, flags & ~binary | native_binary)

        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "input.bin"
            path.write_bytes(b"binary")
            for read in READERS:
                with (
                    self.subTest(reader=read.__module__),
                    mock.patch.object(os, "O_BINARY", binary, create=True),
                    mock.patch.object(os, "open", side_effect=open_binary),
                ):
                    self.assertEqual(read(path, 6), b"binary")


if __name__ == "__main__":
    unittest.main()
