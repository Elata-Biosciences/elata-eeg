# gems_model.py
#
# A complete script to train an EEGNet model for 3-class BCI classification.
# Assumes you have a file named `windowed_loader.py` in the same directory
# with the `load_windowed` function.
#
# To run:
# 1. Make sure `windowed_loader.py`, `gems_model.py`, and your NPZ data file are in the same folder.
# 2. Run from your terminal: python gems_model.py

import torch
import torch.nn as nn
from torch.utils.data import TensorDataset, DataLoader
import tools

import numpy as np
from sklearn.model_selection import train_test_split
from sklearn.metrics import accuracy_score, confusion_matrix, ConfusionMatrixDisplay
import matplotlib.pyplot as plt
import sys

# --- Import your custom data loader ---
try:
    from tools import load_windowed
except ImportError:
    print("❌ FATAL ERROR: Could not find `windowed_loader.py`.")
    print("Please ensure that file is in the same directory as this script.")
    sys.exit(1)


# --- 1. EEGNet Model Definition in PyTorch ---
class EEGNet(nn.Module):
    """
    A PyTorch implementation of the EEGNet architecture.
    """
    def __init__(self, num_classes, chans, samples, dropout_rate=0.5):
        super(EEGNet, self).__init__()
        
        # Block 1: Temporal and Spatial Convolution
        self.block1 = nn.Sequential(
            nn.Conv2d(1, 8, kernel_size=(1, 64), padding=(0, 32), bias=False),
            nn.BatchNorm2d(8),
            nn.Conv2d(8, 16, kernel_size=(chans, 1), groups=8, bias=False),  # Depthwise Conv
            nn.BatchNorm2d(16),
            nn.ELU(),
            nn.AvgPool2d(kernel_size=(1, 4)),
            nn.Dropout(dropout_rate)
        )
        
        # Block 2: Depthwise Separable Convolution
        self.block2 = nn.Sequential(
            # Depthwise
            nn.Conv2d(16, 16, kernel_size=(1, 16), padding=(0, 8), groups=16, bias=False),
            # Pointwise
            nn.Conv2d(16, 16, kernel_size=(1, 1), bias=False),
            nn.BatchNorm2d(16),
            nn.ELU(),
            nn.AvgPool2d(kernel_size=(1, 8)),
            nn.Dropout(dropout_rate)
        )
        
        # Classifier
        # Calculate the flattened size after convolutions and pooling
        # Initial samples: 512. After pool1 (x4): 128. After pool2 (x8): 16.
        # So, the final flattened feature size is F2 * (Samples // 32)
        final_feature_size = 16 * (samples // 32)
        self.classifier = nn.Linear(final_feature_size, num_classes)

    def forward(self, x):
        x = self.block1(x)
        x = self.block2(x)
        x = x.view(x.size(0), -1)  # Flatten
        x = self.classifier(x)
        return x

def main():
    """Main function to run the training and evaluation pipeline."""
    
    # --- Configuration ---
    NPZ_PATH = "data/session.npz" # Change this to the path of your raw (not windowed) NPZ file
    WINDOW_S = 1.25
    HOP_S = 0.25
    EPOCHS = 50
    BATCH_SIZE = 32
    LEARNING_RATE = 0.001
    
    # --- 2. Load and Prepare Data using your function ---
    print(f"🧠 Loading data from '{NPZ_PATH}'...")
    try:
        X, y, fs, ch_names = load_windowed(NPZ_PATH, window_s=WINDOW_S, hop_s=HOP_S)
        print("✅ Data loaded successfully!")
        print(f"   X shape: {X.shape} | y shape: {y.shape}")
        print(f"   Sampling Rate: {fs} Hz | Channels: {len(ch_names)}")
    except FileNotFoundError:
        print(f"❌ FATAL ERROR: Data file not found at '{NPZ_PATH}'.")
        sys.exit(1)
    except Exception as e:
        print(f"❌ An error occurred during data loading: {e}")
        sys.exit(1)

    # Reshape for PyTorch (Batch, Channels, Height, Width) -> (Batch, 1, Chans, Samples)
    X = X[:, np.newaxis, :, :].astype(np.float32)

    # Split data into training and validation sets
    X_train, X_val, y_train, y_val = train_test_split(X, y, test_size=0.2, stratify=y, random_state=42)
    
    # Create PyTorch DataLoaders
    train_dataset = TensorDataset(torch.from_numpy(X_train), torch.from_numpy(y_train).long())
    val_dataset = TensorDataset(torch.from_numpy(X_val), torch.from_numpy(y_val).long())
    train_loader = DataLoader(train_dataset, batch_size=BATCH_SIZE, shuffle=True)
    val_loader = DataLoader(val_dataset, batch_size=BATCH_SIZE, shuffle=False)

    # --- 3. Initialize Model and Optimizer ---
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    print(f"🚀 Using device: {device}\n")

    num_classes = len(np.unique(y))
    _, _, num_chans, num_samples = X_train.shape
    
    model = EEGNet(num_classes=num_classes, chans=num_chans, samples=num_samples).to(device)
    criterion = nn.CrossEntropyLoss()
    optimizer = torch.optim.Adam(model.parameters(), lr=LEARNING_RATE)

    # --- 4. Training Loop ---
    for epoch in range(EPOCHS):
        model.train()
        train_loss_sum = 0
        for batch_X, batch_y in train_loader:
            batch_X, batch_y = batch_X.to(device), batch_y.to(device)
            
            optimizer.zero_grad()
            outputs = model(batch_X)
            loss = criterion(outputs, batch_y)
            loss.backward()
            optimizer.step()
            train_loss_sum += loss.item()
        
        # Validation after each epoch
        model.eval()
        val_loss_sum = 0
        correct_preds = 0
        total_preds = 0
        with torch.no_grad():
            for batch_X, batch_y in val_loader:
                batch_X, batch_y = batch_X.to(device), batch_y.to(device)
                outputs = model(batch_X)
                loss = criterion(outputs, batch_y)
                val_loss_sum += loss.item()
                
                _, predicted = torch.max(outputs.data, 1)
                total_preds += batch_y.size(0)
                correct_preds += (predicted == batch_y).sum().item()

        avg_train_loss = train_loss_sum / len(train_loader)
        avg_val_loss = val_loss_sum / len(val_loader)
        val_accuracy = (correct_preds / total_preds) * 100
        
        print(f'Epoch [{epoch+1:02d}/{EPOCHS}] | Train Loss: {avg_train_loss:.4f} | Val Loss: {avg_val_loss:.4f} | Val Acc: {val_accuracy:.2f}%')

    # --- 5. Final Evaluation and Reporting ---
    print("\n--- Training Finished ---")
    model.eval()
    all_preds, all_labels = [], []
    with torch.no_grad():
        for batch_X, batch_y in val_loader:
            batch_X = batch_X.to(device)
            outputs = model(batch_X)
            _, predicted = torch.max(outputs.data, 1)
            all_preds.extend(predicted.cpu().numpy())
            all_labels.extend(batch_y.numpy())

    final_accuracy = accuracy_score(all_labels, all_preds)
    print(f"\nFinal Validation Accuracy: {final_accuracy * 100:.2f}%")

    # Plotting Confusion Matrix
    class_names = [str(i) for i in range(num_classes)] # Use generic names if not available
    cm = confusion_matrix(all_labels, all_preds)
    disp = ConfusionMatrixDisplay(confusion_matrix=cm, display_labels=class_names)
    disp.plot(cmap='Blues')
    plt.title("Confusion Matrix - EEGNet")
    print("📈 Displaying confusion matrix...")
    plt.show()

if __name__ == '__main__':
    main()
