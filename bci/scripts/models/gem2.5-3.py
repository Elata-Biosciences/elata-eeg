# train_eegnet_pytorch.py
#
# You will need to install PyTorch:
# pip install torch torchvision

import numpy as np
import json
import matplotlib.pyplot as plt
from sklearn.model_selection import train_test_split
from sklearn.metrics import accuracy_score, confusion_matrix, ConfusionMatrixDisplay

import torch
import torch.nn as nn
from torch.utils.data import TensorDataset, DataLoader

# --- 1. EEGNet Model Definition in PyTorch ---
class EEGNet(nn.Module):
    def __init__(self, num_classes=3, chans=8, samples=512, dropout_rate=0.5):
        super(EEGNet, self).__init__()
        # Block 1: Temporal and Spatial Filtering
        self.block1 = nn.Sequential(
            nn.Conv2d(1, 8, kernel_size=(1, 64), padding=(0, 32), bias=False),
            nn.BatchNorm2d(8),
            nn.Conv2d(8, 16, kernel_size=(chans, 1), groups=8, bias=False), # Depthwise
            nn.BatchNorm2d(16),
            nn.ELU(),
            nn.AvgPool2d(kernel_size=(1, 4)),
            nn.Dropout(dropout_rate)
        )
        # Block 2: Depthwise Separable Convolution
        self.block2 = nn.Sequential(
            nn.Conv2d(16, 16, kernel_size=(1, 16), padding=(0, 8), groups=16, bias=False), # Depthwise
            nn.Conv2d(16, 16, kernel_size=(1, 1), bias=False), # Pointwise
            nn.BatchNorm2d(16),
            nn.ELU(),
            nn.AvgPool2d(kernel_size=(1, 8)),
            nn.Dropout(dropout_rate)
        )
        # Classifier
        self.classifier = nn.Linear(16 * (samples // 32), num_classes)

    def forward(self, x):
        x = self.block1(x)
        x = self.block2(x)
        x = x.view(x.size(0), -1) # Flatten
        x = self.classifier(x)
        return x

# --- 2. Load and Prepare Data ---
try:
    z = np.load("session_cued.npz", allow_pickle=True)
    X, y, label_map = z["X"], z["y"], json.loads(str(np.asarray(z["label_map_json"]).item()))

# Reshape for PyTorch (Batch, Channels, Height, Width) -> (Batch, 1, Chans, Samples)
X = X[:, np.newaxis, :, :].astype(np.float32)

# Split data
X_train, X_val, y_train, y_val = train_test_split(X, y, test_size=0.2, stratify=y, random_state=42)

# Create PyTorch DataLoaders
train_dataset = TensorDataset(torch.from_numpy(X_train), torch.from_numpy(y_train).long())
val_dataset = TensorDataset(torch.from_numpy(X_val), torch.from_numpy(y_val).long())
train_loader = DataLoader(train_dataset, batch_size=16, shuffle=True)
val_loader = DataLoader(val_dataset, batch_size=16, shuffle=False)

# --- 3. Training and Evaluation ---
device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
print(f"Using device: {device}")

n_classes = len(label_map)
_, _, n_chans, n_samples = X_train.shape
model = EEGNet(num_classes=n_classes, chans=n_chans, samples=n_samples).to(device)

criterion = nn.CrossEntropyLoss()
optimizer = torch.optim.Adam(model.parameters(), lr=0.001)
epochs = 50

for epoch in range(epochs):
    model.train()
    for batch_X, batch_y in train_loader:
        batch_X, batch_y = batch_X.to(device), batch_y.to(device)
        optimizer.zero_grad()
        outputs = model(batch_X)
        loss = criterion(outputs, batch_y)
        loss.backward()
        optimizer.step()
    if (epoch + 1) % 10 == 0:
      print(f'Epoch [{epoch+1}/{epochs}], Loss: {loss.item():.4f}')

# --- 4. Final Evaluation ---
model.eval()
all_preds, all_labels = [], []
with torch.no_grad():
    for batch_X, batch_y in val_loader:
        batch_X = batch_X.to(device)
        outputs = model(batch_X)
        _, predicted = torch.max(outputs.data, 1)
        all_preds.extend(predicted.cpu().numpy())
        all_labels.extend(batch_y.numpy())

accuracy = accuracy_score(all_labels, all_preds)
print(f"\nEEGNet (PyTorch) Validation Accuracy: {accuracy * 100:.2f}%")

# Plotting Confusion Matrix
# [Your existing confusion matrix plotting code here]
