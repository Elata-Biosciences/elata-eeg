# train_classifier.py
#
# This script trains a 3-class EEG classifier from a windowed NPZ dataset.
# It uses Welch's method to extract bandpower features and trains a 
# Logistic Regression model.
#
# To run:
# 1. Make sure you have the 'session_windowed.npz' file in the same directory.
# 2. Run from your terminal: python train_classifier.py

import numpy as np
import json
import matplotlib.pyplot as plt
import seaborn as sns
from scipy.signal import welch

from sklearn.preprocessing import StandardScaler
from sklearn.linear_model import LogisticRegression
from sklearn.pipeline import make_pipeline
from sklearn.metrics import accuracy_score, confusion_matrix, ConfusionMatrixDisplay

def main():
    """Main function to run the data loading, training, and evaluation."""
    
    # ---------------------------------
    # 1. Setup and Data Loading
    # ---------------------------------
    print("--- 1. Loading Data ---")
    z = np.load("data/session_cued.npz", allow_pickle=True)
    X = z["X"]
    y = z["y"]
    fs = float(z["fs"])
    ch_names = list(np.asarray(z["ch_names"]).tolist())
    label_map = json.loads(str(np.asarray(z["label_map_json"]).item()))
        
    print("Data loaded successfully from 'session_windowed.npz'!")


    print(f"X shape: {X.shape}")
    print(f"y shape: {y.shape}")
    print(f"Sampling frequency: {fs} Hz")
    print(f"Labels: {label_map}\n")

    # ---------------------------------
    # 2. Feature Extraction: Bandpower
    # ---------------------------------
    print("--- 2. Extracting Bandpower Features ---")
    X_features = extract_bandpower_features(X, fs)
    print(f"Shape of extracted features: {X_features.shape}\n")

    # ---------------------------------
    # 3. Data Splitting (Contiguous Blocks)
    # ---------------------------------
    print("--- 3. Splitting Data into Train/Validation Sets ---")
    # Find the indices where the label changes
    change_points = np.where(np.diff(y) != 0)[0] + 1
    # Identify the boundaries of the contiguous blocks
    block_boundaries = np.concatenate(([0], change_points, [len(y)]))
    
    # Determine the split point (approx. 80% of blocks for training)
    n_blocks = len(block_boundaries) - 1
    if n_blocks < 2:
        print("Not enough label blocks to perform a split. Using random 80/20 split.")
        from sklearn.model_selection import train_test_split
        X_train, X_val, y_train, y_val = train_test_split(
            X_features, y, test_size=0.2, random_state=42, stratify=y
        )
    else:
        split_block_idx = max(1, int(n_blocks * 0.8)) # Ensure at least one block for training
        split_window_idx = block_boundaries[split_block_idx]
        
        X_train, X_val = X_features[:split_window_idx], X_features[split_window_idx:]
        y_train, y_val = y[:split_window_idx], y[split_window_idx:]

    print(f"Splitting data based on contiguous blocks.")
    print(f"Total windows: {len(y)}")
    print(f"Training windows: {len(y_train)}")
    print(f"Validation windows: {len(y_val)}\n")
    
    # ---------------------------------
    # 4. Model Training & Evaluation
    # ---------------------------------
    print("--- 4. Training and Evaluating Model ---")
    model = make_pipeline(
        StandardScaler(),
        LogisticRegression(solver='liblinear', multi_class='ovr', random_state=42)
    )

    model.fit(X_train, y_train)
    y_pred = model.predict(X_val)

    accuracy = accuracy_score(y_val, y_pred)
    print(f"\nValidation Accuracy: {accuracy * 100:.2f}%\n")

    # ---------------------------------
    # 5. Results: Confusion Matrix
    # ---------------------------------
    print("--- 5. Displaying Results ---")
    class_names = list(label_map.keys())
    cm = confusion_matrix(y_val, y_pred, labels=list(label_map.values()))

    fig, ax = plt.subplots(figsize=(8, 6))
    disp = ConfusionMatrixDisplay(confusion_matrix=cm, display_labels=class_names)
    disp.plot(ax=ax, cmap='Blues', values_format='d')
    ax.set_title("Confusion Matrix - Logistic Regression on Bandpower")
    
    print("Showing confusion matrix plot...")
    plt.show()


def extract_bandpower_features(X, fs):
    """
    Extracts bandpower features from EEG windows using Welch's method.
    """
    bands = {'delta': (1, 4), 'theta': (4, 8), 'alpha': (8, 13), 'beta': (13, 30)}
    n_windows, n_channels, _ = X.shape
    n_bands = len(bands)
    features = np.zeros((n_windows, n_channels * n_bands))
    
    for i in range(n_windows):
        for j in range(n_channels):
            freqs, psd = welch(X[i, j, :], fs=fs, nperseg=int(fs))
            for k, (_, (low, high)) in enumerate(bands.items()):
                mask = (freqs >= low) & (freqs < high)
                if np.any(mask):
                    features[i, j * n_bands + k] = np.mean(psd[mask])
                else:
                    features[i, j * n_bands + k] = 0 # Handle cases with no power in band
    return features


if __name__ == '__main__':
    # Before running, make sure you have the required packages:
    # pip install numpy scipy scikit-learn matplotlib seaborn
    main()
