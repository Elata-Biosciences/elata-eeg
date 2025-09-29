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

from model_gpt1_amp import offline_prepare_X4  # type: ignore
from gpt1 import TinyBlinkNet  # type: ignore


class TestGpt1Amp(unittest.TestCase):
    def setUp(self):
        torch.manual_seed(1337)
        np.random.seed(1337)

    def test_amp_preserves_amplitude_signal(self):
        # Two classes differ by amplitude at same frequency; should be learnable with amp features
        fs = 250.0
        T = 250
        t = np.arange(T) / fs
        N_per = 40
        C = 2
        X0 = (0.5 * np.sin(2 * np.pi * 8 * t))[None, None, :].repeat(N_per, axis=0).repeat(C, axis=1)
        X1 = (2.0 * np.sin(2 * np.pi * 8 * t))[None, None, :].repeat(N_per, axis=0).repeat(C, axis=1)
        X = np.concatenate([X0, X1], axis=0).astype(np.float32)
        y = np.array([0] * N_per + [1] * N_per, dtype=np.int64)
        X += 0.03 * np.random.randn(*X.shape).astype(np.float32)

        Xp = offline_prepare_X4(X, fs)  # (N, 2C, T)
        idx = np.random.permutation(len(y))
        Xp, y = Xp[idx], y[idx]
        n_train = int(0.8 * len(y))
        Xtr, Xva = Xp[:n_train], Xp[n_train:]
        ytr, yva = y[:n_train], y[n_train:]

        tr = torch.utils.data.TensorDataset(torch.from_numpy(Xtr), torch.from_numpy(ytr))
        va = torch.utils.data.TensorDataset(torch.from_numpy(Xva), torch.from_numpy(yva))
        tr_ld = torch.utils.data.DataLoader(tr, batch_size=16, shuffle=True)
        va_ld = torch.utils.data.DataLoader(va, batch_size=16)

        model = TinyBlinkNet(n_classes=2, n_in_ch=2*C)
        device = torch.device("cpu")
        model.to(device)
        opt = torch.optim.AdamW(model.parameters(), lr=3e-3)
        crit = torch.nn.CrossEntropyLoss()

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

        model.eval()
        correct, total = 0, 0
        with torch.no_grad():
            for xb, yb in va_ld:
                preds = model(xb.to(device)).argmax(dim=1).cpu().numpy()
                correct += int((preds == yb.numpy()).sum())
                total += int(yb.shape[0])
        acc = correct / max(1, total)
        self.assertGreater(acc, 0.85, f"Amplitude-based classification too low: {acc}")


if __name__ == "__main__":
    unittest.main()

