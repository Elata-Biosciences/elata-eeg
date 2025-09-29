import unittest
import numpy as np
import torch
import sys
from pathlib import Path

# Make bci/scripts/models importable
HERE = Path(__file__).resolve().parent
MODELS_DIR = HERE.parent / "models"
if str(MODELS_DIR) not in sys.path:
    sys.path.insert(0, str(MODELS_DIR))

from gpt1 import TinyBlinkNet, offline_prepare_X4, select_fp_indices  # type: ignore


class TestGpt1Pipeline(unittest.TestCase):
    def setUp(self):
        torch.manual_seed(1337)
        np.random.seed(1337)

    def test_select_fp_indices(self):
        idx = select_fp_indices(["Fp1", "Fp2", "C3"])
        self.assertEqual(idx, (0, 1))
        idx2 = select_fp_indices(["C3", "Cz", "Fp2", "Pz", "Fp1"])
        self.assertEqual(idx2, (4, 2))
        idx3 = select_fp_indices(None)
        # Current implementation falls back to (0,0) when channel names are unknown
        self.assertEqual(idx3, (0, 0))

    def test_offline_prepare_standardizes(self):
        N, C, T = 3, 2, 250
        X = np.random.randn(N, C, T).astype(np.float32) * 2 + 3
        Xp = offline_prepare_X4(X, fs=250.0)
        self.assertEqual(Xp.shape, (N, C, T))
        # Check z-score per channel roughly (mean ~ 0, std ~ 1)
        mu = Xp.mean(axis=-1)
        sd = Xp.std(axis=-1)
        self.assertTrue(np.all(np.abs(mu) < 0.2), f"means too far from 0: {mu}")
        self.assertTrue(np.all(np.abs(sd - 1.0) < 0.2), f"stds not near 1: {sd}")

    def test_tinyblinknet_fits_synthetic(self):
        # Two classes with different frequencies (6 Hz vs 10 Hz) so features survive z-scoring
        fs = 250.0
        T = 250
        t = np.arange(T) / fs
        N_per = 40
        C = 2
        X0 = (np.sin(2 * np.pi * 6 * t))[None, None, :].repeat(N_per, axis=0).repeat(C, axis=1)
        X1 = (np.sin(2 * np.pi * 10 * t))[None, None, :].repeat(N_per, axis=0).repeat(C, axis=1)
        X = np.concatenate([X0, X1], axis=0).astype(np.float32)
        y = np.array([0] * N_per + [1] * N_per, dtype=np.int64)
        # Add slight noise
        X += 0.05 * np.random.randn(*X.shape).astype(np.float32)
        # Preprocess
        Xp = offline_prepare_X4(X, fs)
        # Shuffle/split
        idx = np.random.permutation(len(y))
        Xp, y = Xp[idx], y[idx]
        n_train = int(0.8 * len(y))
        Xtr, Xva = Xp[:n_train], Xp[n_train:]
        ytr, yva = y[:n_train], y[n_train:]

        # Torch loaders
        tr = torch.utils.data.TensorDataset(torch.from_numpy(Xtr), torch.from_numpy(ytr))
        va = torch.utils.data.TensorDataset(torch.from_numpy(Xva), torch.from_numpy(yva))
        tr_ld = torch.utils.data.DataLoader(tr, batch_size=16, shuffle=True)
        va_ld = torch.utils.data.DataLoader(va, batch_size=16)

        model = TinyBlinkNet(n_classes=2, n_in_ch=C)
        device = torch.device("cpu")
        model.to(device)
        opt = torch.optim.AdamW(model.parameters(), lr=3e-3)
        crit = torch.nn.CrossEntropyLoss()

        # Train briefly
        for _ in range(10):
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
        self.assertGreater(acc, 0.85, f"Synthetic accuracy too low: {acc}")


if __name__ == "__main__":
    unittest.main()

