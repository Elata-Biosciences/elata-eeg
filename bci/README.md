# BCI Model Training and Development

This directory contains all resources for training, evaluating, and managing Brain-Computer Interface (BCI) models.

## Directory Structure

- **/notebooks**: Jupyter notebooks for data exploration, visualization, and prototyping.
- **/scripts**: Python scripts for data preprocessing, model training, and evaluation.
- **/models**: Storage for trained model artifacts (e.g., `.h5`, `.pkl` files).
- **requirements.txt**: Python dependencies for the BCI project.

## Getting Started

1.  **Install Dependencies**:
    ```bash
    pip install -r requirements.txt
    ```

2.  **Explore Data**:
    - Check out the notebooks in the `/notebooks` directory to understand the data format and explore different features.

3.  **Train a Model**:
    - Use the scripts in the `/scripts` directory to preprocess data and train a new model.
    ```bash
    python scripts/train_model.py --data_path /path/to/your/data --output_path models/new_model.pkl
    ```
