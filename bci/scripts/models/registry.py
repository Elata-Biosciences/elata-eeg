#!/usr/bin/env python3
"""
Model registry for EEG BCI models.

Contract:
- A model module must export get_inference_spec() -> Dict with:
    - "model_class": nn.Module subclass constructor (expects n_classes)
    - "offline_prepare_X4": fn(X2, fs) -> (N,4,T) preprocessed windows
    - "make_stream_transform": fn(fs) -> callable(fp1, fp2) -> (4,T) preprocessed
    - "select_fp_indices": fn(chan_names) -> (idx_fp1, idx_fp2)

Resolution order for a given --model <name>:
  1) bci.scripts.models.model_<name>
  2) DEFAULT_MODULES mapping (e.g., "gpt1" -> "bci.scripts.models.gpt1")
  3) Treat <name> as a fully-qualified module path (advanced)
"""

from __future__ import annotations

import importlib
from types import ModuleType
from typing import Any, Dict


DEFAULT_MODULES: Dict[str, str] = {
    # Back-compat: map short name to current gpt1 module
    "gpt1": "bci.scripts.models.gpt1",
    # Additional built-ins
    "gpt1_amp": "bci.scripts.models.model_gpt1_amp",
    "eog": "bci.scripts.models.model_eog",
    "eegnet_mi": "bci.scripts.models.eegnet",
}


def _import_module(path: str) -> ModuleType:
    return importlib.import_module(path)


def _validate_spec(spec: Dict[str, Any], name: str) -> Dict[str, Any]:
    required = ("model_class", "offline_prepare_X4", "make_stream_transform", "select_fp_indices")
    for k in required:
        if k not in spec:
            raise ValueError(f"Model '{name}' spec missing key: {k}")
    # Basic callable checks
    if not callable(spec["offline_prepare_X4"]):
        raise TypeError(f"Model '{name}' offline_prepare_X4 must be callable")
    if not callable(spec["make_stream_transform"]):
        raise TypeError(f"Model '{name}' make_stream_transform must be callable")
    if not callable(spec["select_fp_indices"]):
        raise TypeError(f"Model '{name}' select_fp_indices must be callable")
    return spec


def get_spec(name: str) -> Dict[str, Any]:
    """
    Resolve and import a model module by name and return its inference spec.
    """
    candidates = [
        f"bci.scripts.models.model_{name}",  # preferred convention
    ]
    if name in DEFAULT_MODULES:
        candidates.append(DEFAULT_MODULES[name])
    # allow fully-qualified module path input
    if "." in name and not name.startswith("bci.scripts.models.model_"):
        candidates.append(name)

    errors = []
    for modpath in candidates:
        try:
            mod = _import_module(modpath)
            if not hasattr(mod, "get_inference_spec"):
                raise AttributeError(f"Module '{modpath}' lacks get_inference_spec()")
            spec = mod.get_inference_spec()
            return _validate_spec(spec, name)
        except Exception as e:
            errors.append(f"{modpath}: {e}")

    tried = "\n - ".join(errors) if errors else "(no candidates tried)"
    raise ImportError(f"Unable to resolve model spec for '{name}'. Tried:\n - {tried}")


__all__ = ["get_spec"]