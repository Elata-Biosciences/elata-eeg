# train_csp_lda.py

# ... keep the data loading part from the previous script ...
# You will need to install one more package:
# pip install mne

from sklearn.pipeline import make_pipeline
from mne.decoding import CSP
from sklearn.discriminant_analysis import LinearDiscriminantAnalysis as LDA

# --- 1. Load Data (same as before) ---
# [Keep your existing data loading code here]
# NOTE: CSP works on the time-domain data (X), not the frequency features.

# --- 2. Data Splitting ---
# We split the original time-series data X, not X_features
# [Use the same contiguous block splitting code, but apply it to X and y]
# Example:
# split_window_idx = ...
# X_train, X_val = X[:split_window_idx], X[split_window_idx:]
# y_train, y_val = y[:split_window_idx], y[split_window_idx:]


# --- 3. Model Training & Evaluation with CSP + LDA ---
print("--- Training with CSP + LDA ---")

# Define the number of CSP components to use
n_components = 6 

# Create the model pipeline
# 1. CSP learns spatial filters from the training data
# 2. LDA classifies the resulting CSP features
model = make_pipeline(
    CSP(n_components=n_components, reg=None, log=True, norm_trace=False),
    LDA()
)

# Train the model
model.fit(X_train, y_train)

# Make predictions
y_pred = model.predict(X_val)

# Calculate accuracy
accuracy = accuracy_score(y_val, y_pred)
print(f"\nCSP + LDA Validation Accuracy: {accuracy * 100:.2f}%\n")

# --- 4. Displaying Results (same as before) ---
# [Keep your existing confusion matrix plotting code here]
