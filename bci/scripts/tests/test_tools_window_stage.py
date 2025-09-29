import unittest
import numpy as np
import sys
from pathlib import Path

# Make bci/scripts importable
HERE = Path(__file__).resolve().parent
SCRIPTS_DIR = HERE.parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from models.tools import window_stage  # type: ignore


class TestWindowStage(unittest.TestCase):
    def test_window_labels_majority(self):
        fs = 10.0
        # 1 channel, 10 seconds, labels switch at 5s
        T = int(10 * fs)
        data = np.zeros((1, T), dtype=np.float32)
        labels = np.zeros((T,), dtype=np.int64)
        labels[int(5*fs):] = 1
        X, y = window_stage(data, labels, fs, window_s=2.0, hop_s=2.0, label_mode="majority")
        # Windows at [0-2), [2-4), [4-6), [6-8), [8-10)
        # With majority=0.70, the [4-6) window is dropped (50/50 mix)
        self.assertEqual(X.shape, (4, 1, int(2*fs)))
        self.assertEqual(y.shape, (4,))
        self.assertListEqual(list(y), [0, 0, 1, 1])


if __name__ == "__main__":
    unittest.main()

