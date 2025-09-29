import unittest
import numpy as np
import torch
import sys
from pathlib import Path

# Make bci/scripts/models importable
HERE = Path(__file__).resolve().parent
SCRIPTS_DIR = HERE.parent
MODELS_DIR = SCRIPTS_DIR / "models"
if str(MODELS_DIR) not in sys.path:
    sys.path.insert(0, str(MODELS_DIR))

from model_mi_tongue import offline_prepare_X4  # type: ignore
from tools import window_stage  # type: ignore
from gpt1 import TinyBlinkNet  # type: ignore


class TestMiTonguePipelineSynthetic(unittest.TestCase):
    def setUp(self):
        torch.manual_seed(1337)
        np.random.seed(1337)

    def _gen_two_class_stream(self, seconds=40.0, fs=250.0):
        """
        Generate 2-channel continuous data with alternating 2s blocks of LEFT (12Hz on ch0) and RIGHT (20Hz on ch1).
        Returns (data CxN, labels N)
        """
        C = 2
        N = int(seconds * fs)
        t = np.arange(N, dtype=np.float32) / fs
        labels = np.zeros(N, dtype=np.int64)
        block = int(2.0 * fs)
        cur = 0
        i = 0
        rng = np.random.default_rng(1337)
        while i < N:
            L = min(block, N - i)
            labels[i:i+L] = cur
            seg = slice(i, i+L)
            if cur == 0:  # left
                ch0 = 1.5 * np.sin(2 * np.pi * 12.0 * t[seg]) + 0.15 * rng.standard_normal(L)
                ch1 = 0.3 * np.sin(2 * np.pi * 12.0 * t[seg] + np.pi/4) + 0.15 * rng.standard_normal(L)
            else:  # right
                ch0 = 0.3 * np.sin(2 * np.pi * 20.0 * t[seg] + np.pi/5) + 0.15 * rng.standard_normal(L)
                ch1 = 1.5 * np.sin(2 * np.pi * 20.0 * t[seg]) + 0.15 * rng.standard_normal(L)
            if i == 0:
                data = np.zeros((C, N), dtype=np.float32)
            data[0, seg] = ch0.astype(np.float32)
            data[1, seg] = ch1.astype(np.float32)
            cur = 1 - cur
            i += L
        return data.astype(np.float32), labels

    def test_pipeline_trains_and_beats_chance(self):
        fs = 250.0
        data, labels = self._gen_two_class_stream(seconds=40.0, fs=fs)
        # Window into (N,C,T)
        X, y = window_stage(data, labels, fs, window_s=1.0, hop_s=0.25, majority=0.70)
        # Preprocess for mi_tongue
        Xp = offline_prepare_X4(X, fs)
        # Shuffle/split
        idx = np.random.permutation(len(y))
        Xp, y = Xp[idx], y[idx]
        n_train = int(0.8 * len(y))
        Xtr, Xva = Xp[:n_train], Xp[n_train:]
        ytr, yva = y[:n_train], y[n_train:]

        tr = torch.utils.data.TensorDataset(torch.from_numpy(Xtr), torch.from_numpy(ytr))
        va = torch.utils.data.TensorDataset(torch.from_numpy(Xva), torch.from_numpy(yva))
        tr_ld = torch.utils.data.DataLoader(tr, batch_size=32, shuffle=True)
        va_ld = torch.utils.data.DataLoader(va, batch_size=32)

        model = TinyBlinkNet(n_classes=2, n_in_ch=Xp.shape[1])
        device = torch.device("cpu")
        model.to(device)
        opt = torch.optim.AdamW(model.parameters(), lr=3e-3)
        crit = torch.nn.CrossEntropyLoss()

        # Train briefly
        for _ in range(8):
            model.train()
            for xb, yb in tr_ld:
                xb = xb.to(device)
                yb = yb.to(device)
                opt.zero_grad()
                logits = model(xb)
                loss = crit(logits, yb)
                loss.backward()
                opt.step()

        # Evaluate
        model.eval()
        correct = 0
        total = 0
        with torch.no_grad():
            for xb, yb in va_ld:
                preds = model(xb.to(device)).argmax(dim=1).cpu().numpy()
                correct += int((preds == yb.numpy()).sum())
                total += int(yb.shape[0])
        acc = correct / max(1, total)
        self.assertGreater(acc, 0.85, f"Synthetic mi_tongue accuracy too low: {acc}")


if __name__ == "__main__":
    unittest.main()

