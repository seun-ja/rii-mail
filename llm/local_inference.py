import gc
import json
import logging
import sys
from typing import ClassVar, Protocol, Self, cast

import torch  # type: ignore
from transformers import (  # type: ignore
    AutoModelForSequenceClassification,
    AutoTokenizer,
)

logging.basicConfig(
    level=logging.INFO, format="[%(levelname)s] %(message)s", stream=sys.stderr
)
LOGGER = logging.getLogger(__name__)

MODEL_DIR = "./local-model"
LABEL_MAP = {0: "LABEL_0", 1: "LABEL_1", 2: "LABEL_2"}

DEBUG = False


class _Tokenizer(Protocol):
    model_max_length: int

    def __call__(
        self,
        text: str,
        *,
        return_tensors: str,
        truncation: bool,
        max_length: int,
    ) -> dict[str, torch.Tensor]: ...


class _ClassificationOutput(Protocol):
    logits: torch.Tensor


class _SequenceClassificationModel(Protocol):
    def to(self, device: torch.device) -> Self: ...

    def eval(self) -> Self: ...

    def __call__(self, **inputs: torch.Tensor) -> _ClassificationOutput: ...


def _load_tokenizer(model_dir: str) -> _Tokenizer:
    return cast(
        _Tokenizer,
        AutoTokenizer.from_pretrained(model_dir),  # pyright: ignore[reportUnknownMemberType]
    )


def _load_model(model_dir: str) -> _SequenceClassificationModel:
    return cast(
        _SequenceClassificationModel,
        AutoModelForSequenceClassification.from_pretrained(model_dir),  # pyright: ignore[reportUnknownMemberType]
    )


# Limit threads (important in containers)
torch.set_num_threads(1)
torch.set_num_interop_threads(1)


class ModelCache:
    _instance: ClassVar[Self | None] = None

    def __init__(self) -> None:
        if ModelCache._instance is not None:
            raise RuntimeError("ModelCache is a singleton!")

        LOGGER.info("Loading tokenizer ...")
        self.tokenizer: _Tokenizer = _load_tokenizer(MODEL_DIR)

        LOGGER.info("Loading model ...")
        self.model: _SequenceClassificationModel = _load_model(MODEL_DIR)

        # Device selection: prefer MPS (Apple Silicon), then CUDA, then CPU
        if (
            hasattr(torch.backends, "mps")
            and torch.backends.mps.is_available()
            and torch.backends.mps.is_built()
        ):
            self.device: torch.device = torch.device("mps")
            LOGGER.info("Running inference on: mps (Apple Silicon)")
        elif torch.cuda.is_available():
            self.device = torch.device("cuda")
            LOGGER.info("Running inference on: cuda (GPU)")
        else:
            self.device = torch.device("cpu")
            LOGGER.info("Running inference on: cpu")

        try:
            self.model = self.model.to(self.device)
        except RuntimeError as e:
            LOGGER.warning(
                "Failed to move model to %s, falling back to CPU: %s", self.device, e
            )
            self.device = torch.device("cpu")
            self.model = self.model.to(self.device)

        _ = self.model.eval()

        self.max_length: int = min(512, self.tokenizer.model_max_length)

        LOGGER.info("Using max_length=%s", self.max_length)
        self._warmup()
        ModelCache._instance = self

    def _warmup(self) -> None:
        try:
            dummy: dict[str, torch.Tensor] = self.tokenizer(
                "warmup",
                return_tensors="pt",
                truncation=True,
                max_length=self.max_length,
            )
            dummy = {k: v.to(self.device) for k, v in dummy.items()}

            with torch.inference_mode():
                _ = self.model(**dummy)

            LOGGER.info("Warmup complete")
        except (RuntimeError, TypeError, ValueError) as e:
            LOGGER.warning("Warmup failed: %s", e)

    @classmethod
    def get(cls) -> Self:
        if cls._instance is None:
            _ = cls()
        assert cls._instance is not None
        return cls._instance

    @classmethod
    def cleanup(cls) -> None:
        """Explicitly removes the model from memory and clears caches."""
        if cls._instance is not None:
            LOGGER.info("Cleaning up model cache to free memory...")
            del cls._instance.model
            del cls._instance.tokenizer
            cls._instance = None

            # Force garbage collection
            _ = gc.collect()

            # Clear hardware caches
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
            elif hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
                torch.mps.empty_cache()

            LOGGER.info("Cleanup complete.")


class ModelError(Exception):
    pass


def predict(text: str) -> dict[str, str | float]:
    cache = ModelCache.get()
    tokenizer = cache.tokenizer
    model = cache.model
    device = cache.device
    max_length = cache.max_length

    try:
        inputs: dict[str, torch.Tensor] = tokenizer(
            text, return_tensors="pt", truncation=True, max_length=max_length
        )

        # Move to device
        inputs = {k: v.to(device) for k, v in inputs.items()}

        LOGGER.info("Running model inference")

        with torch.inference_mode():
            outputs = model(**inputs)

            probs = torch.nn.functional.softmax(outputs.logits, dim=-1)
            pred_idx = int(probs.argmax(dim=-1).item())
            score = float(probs[0, pred_idx].item())

        label = LABEL_MAP.get(pred_idx, f"LABEL_{pred_idx}")

        LOGGER.info("Inference complete: label=%s, score=%s", label, score)

        return {"label": label, "score": score}

    except RuntimeError as e:
        msg = str(e).lower()

        if "out of memory" in msg:
            LOGGER.error("OOM during inference")

            # Minimal cleanup (fast)
            if torch.cuda.is_available():
                torch.cuda.empty_cache()
            elif hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
                torch.mps.empty_cache()

            raise ModelError(json.dumps({"message": "MODEL_OOM", "detail": str(e)}))

        LOGGER.exception("Runtime error")
        raise ModelError(json.dumps({"message": "MODEL_FATAL", "detail": str(e)}))

    except Exception as e:
        LOGGER.exception("Unhandled error")
        raise ModelError(json.dumps({"message": "MODEL_FATAL", "detail": str(e)}))
