import unittest
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MODELS_DIR = HERE.parent / "models"
if str(MODELS_DIR) not in sys.path:
    sys.path.insert(0, str(MODELS_DIR))

from model_eog import select_channel_indices as eog_sel  # type: ignore
from model_mi_tongue import select_channel_indices as mi_sel  # type: ignore


class TestModesChannelSelection(unittest.TestCase):
    def test_eog_selection(self):
        idx = eog_sel(["Fp1","Fp2","C3","C4"])  # prefer Fp1/Fp2
        self.assertEqual(idx, [0,1])
        idx2 = eog_sel(["AF7","AF8","Fp1"])  # prefer AF7/AF8
        self.assertEqual(idx2, [0,1])
        idx3 = eog_sel(["F7","F8","T7","T8"])  # fallback F7/F8
        self.assertEqual(idx3, [0,1])

    def test_mi_tongue_selection(self):
        idx = mi_sel(["F7","F8","Fp1"])  # prefer F7/F8
        self.assertEqual(idx, [0,1])
        idx2 = mi_sel(["AF7","AF8","Fp1"])  # next AF7/AF8
        self.assertEqual(idx2, [0,1])
        idx3 = mi_sel(["Fp1","Fp2","C3"])  # fallback Fp1/Fp2
        self.assertEqual(idx3, [0,1])


if __name__ == "__main__":
    unittest.main()

