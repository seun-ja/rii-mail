import importlib.util
import json
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

_MISSING = object()
_TARGET = Path(__file__).resolve().parents[1] / "local_inference.py"


class FakeTensor:
    def __init__(self):
        self.to_calls = []

    def to(self, device):
        self.to_calls.append(device)
        return self


class FakeScalar:
    def __init__(self, value):
        self.value = value

    def item(self):
        return self.value


class FakeProbabilities:
    def __init__(self, predicted_index=1, score=0.75):
        self.predicted_index = predicted_index
        self.score = score
        self.argmax_calls = []
        self.index_calls = []

    def argmax(self, dim):
        self.argmax_calls.append(dim)
        return FakeScalar(self.predicted_index)

    def __getitem__(self, key):
        self.index_calls.append(key)
        return FakeScalar(self.score)


class FakeTokenizer:
    def __init__(self, model_max_length=_MISSING):
        if model_max_length is not _MISSING:
            self.model_max_length = model_max_length
        self.calls = []
        self.side_effect = None
        self.return_value = None

    def __call__(self, text, **kwargs):
        self.calls.append((text, kwargs))
        if self.side_effect is not None:
            if callable(self.side_effect):
                return self.side_effect(text, **kwargs)
            raise self.side_effect
        if self.return_value is not None:
            return self.return_value
        return {"input_ids": FakeTensor(), "attention_mask": FakeTensor()}


class FakeModel:
    def __init__(self, inference_state):
        self.inference_state = inference_state
        self.to_calls = []
        self.fail_to_devices = set()
        self.eval_calls = 0
        self.calls = []
        self.side_effect = None
        self.output = types.SimpleNamespace(logits=object())

    def to(self, device):
        self.to_calls.append(device)
        if device in self.fail_to_devices:
            raise RuntimeError("cannot move model to " + str(device))
        return self

    def eval(self):
        self.eval_calls += 1
        return self

    def __call__(self, **kwargs):
        self.calls.append((kwargs, self.inference_state.active))
        if self.side_effect is not None:
            raise self.side_effect
        return self.output


class FakeInferenceMode:
    def __init__(self, state):
        self.state = state

    def __enter__(self):
        self.state.enters += 1
        self.state.active += 1
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self.state.active -= 1
        self.state.exits += 1
        return False


class FakeEnvironment:
    def __init__(
        self,
        *,
        mps_available=False,
        mps_built=True,
        cuda_available=False,
        tokenizer_max_length=_MISSING,
        predicted_index=1,
        score=0.75,
    ):
        self.inference_state = types.SimpleNamespace(active=0, enters=0, exits=0)
        self.tokenizer = FakeTokenizer(tokenizer_max_length)
        self.model = FakeModel(self.inference_state)
        self.probabilities = FakeProbabilities(predicted_index, score)

        self.torch = types.ModuleType("torch")
        self.torch.set_num_threads = Mock()
        self.torch.set_num_interop_threads = Mock()
        self.torch.device = Mock(side_effect=lambda name: name)
        self.torch.inference_mode = Mock(
            side_effect=lambda: FakeInferenceMode(self.inference_state)
        )

        self.mps_backend = types.SimpleNamespace(
            is_available=Mock(return_value=mps_available),
            is_built=Mock(return_value=mps_built),
        )
        self.torch.backends = types.SimpleNamespace(mps=self.mps_backend)
        self.torch.cuda = types.SimpleNamespace(
            is_available=Mock(return_value=cuda_available),
            empty_cache=Mock(),
        )
        self.torch.mps = types.SimpleNamespace(empty_cache=Mock())
        self.softmax = Mock(return_value=self.probabilities)
        self.torch.nn = types.SimpleNamespace(
            functional=types.SimpleNamespace(softmax=self.softmax)
        )

        self.transformers = types.ModuleType("transformers")
        self.tokenizer_from_pretrained = Mock(return_value=self.tokenizer)
        self.model_from_pretrained = Mock(return_value=self.model)
        self.transformers.AutoTokenizer = types.SimpleNamespace(
            from_pretrained=self.tokenizer_from_pretrained
        )
        self.transformers.AutoModelForSequenceClassification = types.SimpleNamespace(
            from_pretrained=self.model_from_pretrained
        )

    def reset_inference_records(self):
        self.tokenizer.calls.clear()
        self.model.calls.clear()
        self.inference_state.active = 0
        self.inference_state.enters = 0
        self.inference_state.exits = 0
        self.torch.inference_mode.reset_mock()


class LocalInferenceTests(unittest.TestCase):
    module_number = 0

    def load_target(self, environment):
        type(self).module_number += 1
        module_name = "_local_inference_under_test_" + str(type(self).module_number)
        spec = importlib.util.spec_from_file_location(module_name, _TARGET)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)

        injected_modules = {
            "torch": environment.torch,
            "transformers": environment.transformers,
            module_name: module,
        }
        with patch.dict(sys.modules, injected_modules):
            spec.loader.exec_module(module)

        setattr(module, "MODEL_DIR", str(_TARGET.parents[1] / "local-model"))
        self.assertIsNone(module.ModelCache._instance)
        environment.torch.set_num_threads.assert_called_once_with(1)
        environment.torch.set_num_interop_threads.assert_called_once_with(1)
        return module

    def assert_error_payload(self, exception, message, detail):
        payload = json.loads(str(exception))
        self.assertEqual({"message": message, "detail": detail}, payload)

    def test_device_precedence_is_mps_then_cuda_then_cpu(self):
        cases = (
            (True, True, True, "mps"),
            (True, False, True, "cuda"),
            (False, True, True, "cuda"),
            (False, True, False, "cpu"),
        )
        for mps_available, mps_built, cuda_available, expected in cases:
            with self.subTest(
                mps_available=mps_available,
                mps_built=mps_built,
                cuda_available=cuda_available,
            ):
                environment = FakeEnvironment(
                    mps_available=mps_available,
                    mps_built=mps_built,
                    cuda_available=cuda_available,
                )
                target = self.load_target(environment)
                cache = target.ModelCache.get()

                self.assertEqual(expected, cache.device)
                self.assertEqual(expected, environment.model.to_calls[0])
                if expected == "mps":
                    environment.torch.cuda.is_available.assert_not_called()

    def test_model_move_failure_falls_back_to_cpu(self):
        environment = FakeEnvironment(mps_available=True, cuda_available=True)
        environment.model.fail_to_devices.add("mps")
        target = self.load_target(environment)

        cache = target.ModelCache.get()

        self.assertEqual("cpu", cache.device)
        self.assertEqual(["mps", "cpu"], environment.model.to_calls)
        self.assertEqual(1, environment.model.eval_calls)

    def test_max_length_is_capped_defaults_and_preserves_smaller_value(self):
        cases = ((_MISSING, 512), (4096, 512), (128, 128))
        for configured, expected in cases:
            with self.subTest(configured=configured, expected=expected):
                environment = FakeEnvironment(tokenizer_max_length=configured)
                target = self.load_target(environment)

                cache = target.ModelCache.get()

                self.assertEqual(expected, cache.max_length)
                warmup_text, warmup_kwargs = environment.tokenizer.calls[0]
                self.assertEqual("warmup", warmup_text)
                self.assertEqual(expected, warmup_kwargs["max_length"])

    def test_get_is_singleton_and_direct_second_construction_is_rejected(self):
        environment = FakeEnvironment()
        target = self.load_target(environment)

        first = target.ModelCache.get()
        second = target.ModelCache.get()

        self.assertIs(first, second)
        self.assertEqual(1, environment.model_from_pretrained.call_count)
        environment.tokenizer_from_pretrained.assert_called_once_with(
            target.MODEL_DIR, local_files_only=True
        )
        environment.model_from_pretrained.assert_called_once_with(
            target.MODEL_DIR, local_files_only=True
        )
        with self.assertRaisesRegex(RuntimeError, "singleton"):
            target.ModelCache()

    def test_warmup_failure_is_tolerated_and_instance_is_cached(self):
        environment = FakeEnvironment()
        environment.tokenizer.side_effect = ValueError("warmup unavailable")
        target = self.load_target(environment)

        cache = target.ModelCache.get()

        self.assertIs(cache, target.ModelCache._instance)
        self.assertEqual("cpu", cache.device)
        self.assertEqual(1, environment.model.eval_calls)

    def test_warmup_uses_expected_tokenizer_args_device_and_inference_mode(self):
        environment = FakeEnvironment(cuda_available=True, tokenizer_max_length=300)
        warmup_tensors = {
            "input_ids": FakeTensor(),
            "attention_mask": FakeTensor(),
        }
        environment.tokenizer.return_value = warmup_tensors
        target = self.load_target(environment)

        target.ModelCache.get()

        self.assertEqual(
            [
                (
                    "warmup",
                    {"return_tensors": "pt", "truncation": True, "max_length": 300},
                )
            ],
            environment.tokenizer.calls,
        )
        for tensor in warmup_tensors.values():
            self.assertEqual(["cuda"], tensor.to_calls)
        self.assertEqual(1, environment.torch.inference_mode.call_count)
        self.assertEqual(
            (1, 1, 0),
            (
                environment.inference_state.enters,
                environment.inference_state.exits,
                environment.inference_state.active,
            ),
        )
        self.assertEqual(1, environment.model.calls[0][1])

    def test_predict_maps_known_label_and_score(self):
        environment = FakeEnvironment(predicted_index=2, score=0.875)
        target = self.load_target(environment)
        target.ModelCache.get()
        environment.reset_inference_records()

        result = target.predict("A useful email")

        self.assertEqual({"label": "LABEL_2", "score": 0.875}, result)
        environment.softmax.assert_called_once_with(
            environment.model.output.logits, dim=-1
        )
        self.assertEqual([-1], environment.probabilities.argmax_calls)
        self.assertEqual([(0, 2)], environment.probabilities.index_calls)

    def test_predict_uses_fallback_label_for_unknown_index(self):
        environment = FakeEnvironment(predicted_index=7, score=0.42)
        target = self.load_target(environment)
        target.ModelCache.get()

        result = target.predict("An unusual label")

        self.assertEqual({"label": "LABEL_7", "score": 0.42}, result)

    def test_predict_uses_tokenizer_contract_moves_inputs_and_inference_mode(self):
        environment = FakeEnvironment(cuda_available=True, tokenizer_max_length=256)
        target = self.load_target(environment)
        target.ModelCache.get()
        environment.reset_inference_records()
        prediction_tensors = {
            "input_ids": FakeTensor(),
            "attention_mask": FakeTensor(),
        }
        environment.tokenizer.return_value = prediction_tensors

        target.predict("classify me")

        self.assertEqual(
            [
                (
                    "classify me",
                    {
                        "return_tensors": "pt",
                        "truncation": True,
                        "max_length": 256,
                    },
                )
            ],
            environment.tokenizer.calls,
        )
        for tensor in prediction_tensors.values():
            self.assertEqual(["cuda"], tensor.to_calls)
        self.assertEqual(1, environment.torch.inference_mode.call_count)
        self.assertEqual(
            (1, 1, 0),
            (
                environment.inference_state.enters,
                environment.inference_state.exits,
                environment.inference_state.active,
            ),
        )
        model_kwargs, active_during_call = environment.model.calls[0]
        self.assertEqual(prediction_tensors, model_kwargs)
        self.assertEqual(1, active_during_call)

    def test_cuda_oom_returns_json_payload_and_clears_cuda_cache(self):
        environment = FakeEnvironment(cuda_available=True)
        target = self.load_target(environment)
        target.ModelCache.get()
        environment.model.side_effect = RuntimeError(
            "CUDA out of memory: allocation failed"
        )

        with self.assertRaises(target.ModelError) as raised:
            target.predict("large input")

        self.assert_error_payload(
            raised.exception,
            "MODEL_OOM",
            "CUDA out of memory: allocation failed",
        )
        environment.torch.cuda.empty_cache.assert_called_once_with()
        environment.torch.mps.empty_cache.assert_not_called()

    def test_mps_oom_returns_json_payload_and_clears_mps_cache(self):
        environment = FakeEnvironment(mps_available=True, cuda_available=False)
        target = self.load_target(environment)
        target.ModelCache.get()
        environment.model.side_effect = RuntimeError("MPS backend out of memory")

        with self.assertRaises(target.ModelError) as raised:
            target.predict("large input")

        self.assert_error_payload(
            raised.exception,
            "MODEL_OOM",
            "MPS backend out of memory",
        )
        environment.torch.mps.empty_cache.assert_called_once_with()
        environment.torch.cuda.empty_cache.assert_not_called()

    def test_non_oom_runtime_error_is_fatal_and_does_not_clear_cache(self):
        environment = FakeEnvironment(cuda_available=True)
        target = self.load_target(environment)
        target.ModelCache.get()
        environment.model.side_effect = RuntimeError("kernel launch failed")

        with self.assertRaises(target.ModelError) as raised:
            target.predict("input")

        self.assert_error_payload(
            raised.exception,
            "MODEL_FATAL",
            "kernel launch failed",
        )
        environment.torch.cuda.empty_cache.assert_not_called()
        environment.torch.mps.empty_cache.assert_not_called()

    def test_non_runtime_error_is_fatal_json(self):
        environment = FakeEnvironment()
        target = self.load_target(environment)
        target.ModelCache.get()
        environment.tokenizer.side_effect = ValueError("bad tokenization")

        with self.assertRaises(target.ModelError) as raised:
            target.predict("input")

        self.assert_error_payload(
            raised.exception,
            "MODEL_FATAL",
            "bad tokenization",
        )

    def test_cleanup_deletes_resources_clears_cuda_and_allows_reinitialization(self):
        environment = FakeEnvironment(cuda_available=True, mps_available=True)
        target = self.load_target(environment)
        first = target.ModelCache.get()

        with patch.object(target.gc, "collect") as collect:
            target.ModelCache.cleanup()

        self.assertIsNone(target.ModelCache._instance)
        self.assertFalse(hasattr(first, "model"))
        self.assertFalse(hasattr(first, "tokenizer"))
        collect.assert_called_once_with()
        environment.torch.cuda.empty_cache.assert_called_once_with()
        environment.torch.mps.empty_cache.assert_not_called()

        second = target.ModelCache.get()
        self.assertIsNot(first, second)
        self.assertIs(second, target.ModelCache._instance)
        self.assertEqual(2, environment.model_from_pretrained.call_count)

    def test_cleanup_clears_mps_cache_when_cuda_is_unavailable(self):
        environment = FakeEnvironment(mps_available=True, cuda_available=False)
        target = self.load_target(environment)
        target.ModelCache.get()

        target.ModelCache.cleanup()

        environment.torch.mps.empty_cache.assert_called_once_with()
        environment.torch.cuda.empty_cache.assert_not_called()
        self.assertIsNone(target.ModelCache._instance)

    def test_cleanup_with_no_instance_is_a_noop(self):
        environment = FakeEnvironment(cuda_available=True, mps_available=True)
        target = self.load_target(environment)

        with patch.object(target.gc, "collect") as collect:
            target.ModelCache.cleanup()

        collect.assert_not_called()
        environment.torch.cuda.empty_cache.assert_not_called()
        environment.torch.mps.empty_cache.assert_not_called()


if __name__ == "__main__":
    unittest.main()
