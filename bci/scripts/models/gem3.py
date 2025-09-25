# gems_model.py

#

# Trains an EEGNet model using the provided tools for data handling.

#

# File Structure:

# - gems_model.py (this script)

# - tools.py (contains your load_file and window_stage functions)

# - session.npz (your raw data file)

#

# To Run:

# python gems_model.py


import torch

import torch.nn as nn

from torch.utils.data import TensorDataset, DataLoader


import numpy as np

from sklearn.model_selection import train_test_split

from sklearn.metrics import accuracy_score, confusion_matrix, ConfusionMatrixDisplay

import matplotlib.pyplot as plt

from scipy.signal import butter, lfilter

import sys


# --- Import your custom tool functions ---

try:

    from tools import load_file, window_stage

except ImportError:

    print("❌ FATAL ERROR: Could not find `tools.py`.")

    print("Please ensure that file is in the same directory as this script.")

    sys.exit(1)



# --- 1. EEGNet Model Definition (same as before) ---

class EEGNet(nn.Module):

    """A PyTorch implementation of the EEGNet architecture."""

    def __init__(self, num_classes, chans, samples, dropout_rate=0.5):

        super(EEGNet, self).__init__()

        self.block1 = nn.Sequential(

            nn.Conv2d(1, 16, kernel_size=(1, 64), padding=(0, 32), bias=False),

            nn.BatchNorm2d(16),

            nn.Conv2d(16, 32, kernel_size=(chans, 1), groups=8, bias=False),

            nn.BatchNorm2d(32),

            nn.ELU(),

            nn.AvgPool2d(kernel_size=(1, 4)),

            nn.Dropout(dropout_rate)

        )

        self.block2 = nn.Sequential(

            nn.Conv2d(32, 32, kernel_size=(1, 16), padding=(0, 8), groups=16, bias=False),

            nn.Conv2d(32, 32, kernel_size=(1, 1), bias=False),

            nn.BatchNorm2d(32),

            nn.ELU(),

            nn.AvgPool2d(kernel_size=(1, 8)),

            nn.Dropout(dropout_rate)

        )

        final_feature_size = 32 * (samples // 32)

        self.classifier = nn.Linear(final_feature_size, num_classes)


    def forward(self, x):

        x = self.block1(x)

        x = self.block2(x)

        x = x.view(x.size(0), -1)

        x = self.classifier(x)

        return x


# --- Utility Function for Filtering ---

def bandpass_filter(data, lowcut, highcut, fs, order=5):

    """Applies a bandpass filter to the data."""

    nyq = 0.5 * fs

    low = lowcut / nyq

    high = highcut / nyq

    b, a = butter(order, [low, high], btype='band')

    y = lfilter(b, a, data, axis=-1)

    return y


def main():

    """Main function to run the training and evaluation pipeline."""

    

    # --- Configuration ---

    NPZ_PATH = "data/session2.npz"

    WINDOW_S = 1.25

    HOP_S = 0.25

    EPOCHS = 75

    BATCH_SIZE = 32

    LEARNING_RATE = 0.0001

    

    # --- 2. Data Processing Pipeline ---

    try:

        # Step 1: Load raw, continuous data

        print(f"🧠 Step 1/3: Loading raw data from '{NPZ_PATH}'...")

        d = load_file(NPZ_PATH)

        print(f"   ✅ Loaded {d.data.shape[1] / d.fs:.2f} seconds of data.")

        

        # Step 2: Filter the continuous data

        print("🔧 Step 2/3: Applying bandpass filter (1-40 Hz)...")

        filtered_data = bandpass_filter(d.data, 1.0, 40.0, d.fs).astype(np.float32)

        print("   ✅ Filtering complete.")

        

        # Step 3: Window the filtered data

        print("🔪 Step 3/3: Slicing data into windows...")

        X, y = window_stage(

            filtered_data, d.labels, d.fs,

            window_s=WINDOW_S, hop_s=HOP_S

        )

        print(f"   ✅ Created {X.shape[0]} windows.")

        print(f"   X shape: {X.shape} | y shape: {y.shape}\n")

        

    except FileNotFoundError:

        print(f"❌ FATAL ERROR: Data file not found at '{NPZ_PATH}'.")

        sys.exit(1)

    except Exception as e:

        print(f"❌ An error occurred during data processing: {e}")

        sys.exit(1)


    # --- 3. Prepare Data for PyTorch ---

    X = X[:, np.newaxis, :, :].astype(np.float32)

    X_train, X_val, y_train, y_val = train_test_split(X, y, test_size=0.2, stratify=y, random_state=42)

    

    train_dataset = TensorDataset(torch.from_numpy(X_train), torch.from_numpy(y_train).long())

    val_dataset = TensorDataset(torch.from_numpy(X_val), torch.from_numpy(y_val).long())

    train_loader = DataLoader(train_dataset, batch_size=BATCH_SIZE, shuffle=True)

    val_loader = DataLoader(val_dataset, batch_size=BATCH_SIZE, shuffle=False)


    # --- 4. Initialize and Train Model ---

    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    print(f"🚀 Using device: {device}\n")


    num_classes = len(np.unique(y))

    _, _, num_chans, num_samples = X_train.shape

    

    model = EEGNet(num_classes=num_classes, chans=num_chans, samples=num_samples).to(device)

    criterion = nn.CrossEntropyLoss()

    optimizer = torch.optim.Adam(model.parameters(), lr=LEARNING_RATE)


    # --- Training Loop ---

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

        

        # Validation

        model.eval()

        val_loss_sum = 0

        correct_preds, total_preds = 0, 0

        with torch.no_grad():

            for batch_X, batch_y in val_loader:

                batch_X, batch_y = batch_X.to(device), batch_y.to(device)

                outputs = model(batch_X)

                val_loss_sum += criterion(outputs, batch_y).item()

                _, predicted = torch.max(outputs.data, 1)

                total_preds += batch_y.size(0)

                correct_preds += (predicted == batch_y).sum().item()

        

        avg_train_loss = train_loss_sum / len(train_loader)

        avg_val_loss = val_loss_sum / len(val_loader)

        val_accuracy = (correct_preds / total_preds) * 100

        

        print(f'Epoch [{epoch+1:02d}/{EPOCHS}] | Train Loss: {avg_train_loss:.4f} | Val Loss: {avg_val_loss:.4f} | Val Acc: {val_accuracy:.2f}%')


    # --- 5. Final Evaluation ---

    print("\n--- Training Finished ---")

    # (Final evaluation logic can be added here if needed, but per-epoch validation is usually sufficient)

    final_accuracy = val_accuracy

    print(f"\nFinal Validation Accuracy: {final_accuracy:.2f}%")


if __name__ == '__main__':

    main()
