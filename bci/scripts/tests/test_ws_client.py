import json
import struct
import unittest
import sys
from pathlib import Path

# Make bci/scripts importable when running from repo root
HERE = Path(__file__).resolve().parent
SCRIPTS_DIR = HERE.parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from ws_client import _ws_data_received  # type: ignore


class TestWsClientParsing(unittest.TestCase):
    def test_meta_and_binary_samples_parsing(self):
        metadata = {}
        # 1) Send a meta_update text frame
        meta_msg = {
            "message_type": "meta_update",
            "topic": "eeg_voltage",
            "meta": {"fs": 250.0, "num_channels": 2},
        }
        kind, payload = _ws_data_received(json.dumps(meta_msg), metadata)
        self.assertEqual(kind, "meta")
        self.assertIn("eeg_voltage", metadata)
        self.assertEqual(metadata["eeg_voltage"]["fs"], 250.0)

        # 2) Send a binary data frame: [u32_be json_len][json][raw payload]
        header = {
            "topic": "eeg_voltage",
            "packet_type": "RawI32",
            "ts_ns": 0,
            "batch_size": 4,
            "num_channels": 2,
            "meta_rev": 1,
        }
        header_bytes = json.dumps(header).encode("utf-8")
        json_len = struct.pack(">I", len(header_bytes))
        # Payload: 4x2 = 8 int32 values 0..7
        import numpy as np
        payload = np.arange(8, dtype=np.int32).tobytes()
        frame = json_len + header_bytes + payload

        kind, payload = _ws_data_received(frame, metadata)
        self.assertEqual(kind, "samples")
        hdr, samples, meta = payload
        self.assertEqual(hdr["topic"], "eeg_voltage")
        self.assertEqual(samples.shape, (4, 2))
        self.assertTrue((samples.flatten() == list(range(8))).all())
        self.assertEqual(meta["fs"], 250.0)


if __name__ == "__main__":
    unittest.main()

