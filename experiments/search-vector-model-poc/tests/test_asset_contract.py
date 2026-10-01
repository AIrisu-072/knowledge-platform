"""P2-01 checks immutable, offline asset and run inputs only."""

from copy import deepcopy
from datetime import datetime, timedelta, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
import shutil
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from asset_contract import (  # noqa: E402
    ContractError,
    freeze_run,
    load_json,
    model_id,
    seal_run,
    validate_baseline_receipt,
    validate_paired_receipt,
    validate_assets,
    validate_public_slice,
    verify_frozen_run,
    verify_assets,
    _verify_source_files,
    _verify_complete_source_closure,
    derive_source_inventory,
    capture_source_pins,
    capture_capacity_receipt,
    _verify_capacity_admission,
    _verify_data_artifacts,
    begin_arm,
)


class AssetContractTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.assets = load_json(ROOT / "assets-manifest.json")
        cls.run_manifest = load_json(ROOT / "run-manifest.json")
        cls.public = load_json(ROOT / "public-ja-slice.json")

    @classmethod
    def ready_run(cls):
        run = deepcopy(cls.run_manifest)
        run["state"] = "PRE_RUN_READY_UNRUN"
        run["execution"]["selected_candidate_id"] = "e5-candle-f32"
        run["execution"]["route_order_state"] = "BASELINE_L_LG_PLANNER_VERIFIED_DENSE_UNRUN"
        run["execution_guard"]["current_capacity_verdict"] = "ADMITTED"
        inputs = run["pre_run_inputs"]
        inputs["state"] = "EXACT_INPUTS_CAPTURED"
        for name in inputs["code"]:
            category = name.removesuffix("_sha256")
            relative = f"experiments/search-vector-model-poc/test/{category}.bin"
            inputs["source_files"][category] = {relative: "a" * 64}
            inputs["code"][name] = sha256(f"{relative}\0{'a' * 64}\n".encode()).hexdigest()
        for name in inputs["data"]:
            inputs["data"][name] = "b" * 64
            inputs["data_artifacts"][name] = {
                "local_path": f"test/{name}.json", "sha256": "b" * 64, "bytes": 1,
            }
        inputs["host_hardware"]["fingerprint_sha256"] = "c" * 64
        inputs["host_hardware"]["cpu_model"] = "test CPU"
        inputs["host_hardware"]["free_memory_bytes_at_admission"] = 8_000_000_000
        inputs["planner_output_sha256"] = "d" * 64
        inputs["data_artifacts"]["planner_output"] = {
            "local_path": "test/planner.json", "sha256": "d" * 64, "bytes": 1,
        }
        inputs["build_identity"] = {"schema": "p2-build-identity-v1"}
        inputs["capacity_admission"] = {"schema": "p2-capacity-admission-v1"}
        inputs["sample_plan"] = {"query_ids": ["q0"], "repetitions_per_query": 1}
        model = next(item for item in cls.assets["models"] if item["id"] == "e5")
        embedding = model["embedding"]
        inputs["embedding_policy"] = {
            "tokenizer_sha256": model["artifacts"]["tokenizer_json"]["sha256"],
            "pooling": embedding["pooling"],
            "mask": "attention_mask",
            "normalization": embedding["retrieval_normalization"],
            "truncation": embedding["truncation"],
            "metric": embedding["metric"],
            "precision": embedding["precision"],
        }
        inputs["deadline_utc"] = "2026-10-02T00:00:00+00:00"
        inputs["fanout_by_arm"] = {"L": 1, "LG": 2, "D": 1, "LD": 2, "LDG": 3}
        inputs["resource_scenario_id"] = "e5_candle_only"
        run["resource_ceiling"]["current_free_bytes"] = 6_000_000_000
        run["execution"]["observed_free_disk_kib"] = 6_000_000_000 // 1024
        return run

    @classmethod
    def ready_pin(cls):
        run = cls.ready_run()
        with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
            pin = freeze_run(run, cls.assets, cls.public, ROOT)
        return run, pin

    @classmethod
    def actual_data_fixture(cls, root):
        run = cls.ready_run()
        baseline = root / "experiments/search-vector-poc"
        model_root = root / "experiments/search-vector-model-poc"
        baseline.mkdir(parents=True)
        model_root.mkdir(parents=True)
        baseline_files = {
            "corpus_manifest.json": json.dumps({"schema": "temporary-protocol-fixture", "seed":
                run["execution"]["seed"], "generation": run["synthetic_fixture"]["source_generation"],
                "scales": [1]}, sort_keys=True) + "\n",
            "queries.jsonl": '{"query_id":"q0","text":"監査"}\n',
            "qrels.jsonl": '{"query_id":"q0","resource_index":0,"grade":1}\n',
        }
        for name, content in baseline_files.items():
            (baseline / name).write_text(content, encoding="utf-8")
        for field, name in (("corpus_manifest_sha256", "corpus_manifest.json"),
                            ("queries_sha256", "queries.jsonl"), ("qrels_sha256", "qrels.jsonl")):
            run["synthetic_fixture"][field] = sha256((baseline / name).read_bytes()).hexdigest()
        shutil.copy2(ROOT / "public-ja-slice.json", model_root / "public-ja-slice.json")
        row = {"docid": "d#0", "text": "監査証跡", "source_id": "s", "version_id": "v",
               "part_id": "p", "unit_id": "u", "locator": "paragraph:0",
               "source_generation": run["synthetic_fixture"]["source_generation"], "resource_index": 0,
               "source_snapshot": "fixture-snapshot", "logical_path": "primary",
               "profile": "fixture-profile", "parser_build_id": "fixture-parser",
               "representation_ref": "fixture-representation", "raw_sha256": sha256("監査証跡".encode()).hexdigest(),
               "raw_size_bytes": len("監査証跡".encode()), "text_sha256": sha256("監査証跡".encode()).hexdigest(),
               "part_ordinal": 0, "unit_ordinal": 0}
        values = {
            "corpus_text_sha256": {"schema": "p2-corpus-rows-v1", "rows": [row]},
            "query_text_sha256": {"schema": "p2-query-rows-v1", "queries": [
                {"query_id": "q0", "text": "監査", "split": "development", "family": "audit"}]},
            "label_split_sha256": {"schema": "p2-label-split-v1", "development": ["q0"],
                                     "untouched_holdout": []},
            "source_snapshot_sha256": {"schema": "p2-source-snapshot-v1",
                                       "source_generation": row["source_generation"],
                                       "parents": [{"source_id": "s", "version_id": "v", "current": True}]},
            "current_read_sha256": {"schema": "p2-current-read-v1", "actor": run["execution"]["actor"],
                                    "allowed": [{"source_id": "s", "version_id": "v"}],
                                    "denied": [], "unknown": []},
            "filter_sha256": {"schema": "p2-filter-v1", "included_unit_ids": ["u"]},
            "eligible_parent_set_sha256": {"schema": "p2-eligible-qrels-v1", "queries": [
                {"query_id": "q0", "parents": [{"source_id": "s", "version_id": "v", "grade": 1}]}]},
            "unit_binding_sha256": {"schema": "p2-unit-bindings-v1", "bindings": [
                {key: value for key, value in row.items() if key not in ("text", "resource_index")}]},
            "judgments_sha256": {"schema": "p2-judgments-v1", "judgments": [
                {"query_id": "q0", "docid": "d#0", "grade": 1}]},
            "planner_output": {"schema": "p2-planner-output-v1", "window": 20,
                               "seed": run["execution"]["seed"], "baseline_plans": [
                                   {"arm": "L", "query_id": "q0", "retriever_kinds": ["Lexical"],
                                    "retriever_ids": ["fixture:lexical"],
                                    "s1_retriever_order": ["fixture:lexical"]},
                                   {"arm": "LG", "query_id": "q0", "retriever_kinds": ["Lexical"],
                                    "retriever_ids": ["fixture:lexical"],
                                    "s1_retriever_order": ["fixture:lexical"]}]},
        }
        for name, value in values.items():
            relative = f"experiments/search-vector-model-poc/test-data/{name}.json"
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n").encode()
            path.write_bytes(encoded)
            digest = sha256(encoded).hexdigest()
            run["pre_run_inputs"]["data_artifacts"][name] = {
                "local_path": relative, "sha256": digest, "bytes": len(encoded)}
            if name == "planner_output":
                run["pre_run_inputs"]["planner_output_sha256"] = digest
            else:
                run["pre_run_inputs"]["data"][name] = digest
        exe = root / "bin/test-runner"
        exe.parent.mkdir()
        exe.write_bytes(b"fixture executable bytes")
        exe_entry = {"local_path": "bin/test-runner", "sha256": sha256(exe.read_bytes()).hexdigest(),
                     "bytes": exe.stat().st_size}
        run["pre_run_inputs"]["build_identity"]["arm_executables"] = {
            arm: exe_entry for arm in ("L", "LG", "D", "LD", "LDG")}
        exported = {"schema": "p2-synthetic-executed-input-v1", "seed": run["execution"]["seed"],
                    "scale": 1, "window": run["execution"]["window"],
                    "actor": run["execution"]["actor"], "artifacts": values}
        exporter = root / "bin/baseline-export"
        exporter.write_text("#!/usr/bin/env python3\nimport sys\nsys.stdout.write(" +
                            repr(json.dumps(exported, ensure_ascii=False)) + ")\n", encoding="utf-8")
        os.chmod(exporter, 0o755)
        run["pre_run_inputs"]["build_identity"]["baseline_export_executable"] = {
            "local_path": "bin/baseline-export", "sha256": sha256(exporter.read_bytes()).hexdigest(),
            "bytes": exporter.stat().st_size}
        for arm in ("L", "LG"):
            run["pre_run_inputs"]["build_identity"]["arm_executables"][arm] = (
                run["pre_run_inputs"]["build_identity"]["baseline_export_executable"])
        return run, model_root

    @staticmethod
    def repin_data_artifact(run, workspace, name, value):
        entry = run["pre_run_inputs"]["data_artifacts"][name]
        encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n").encode()
        (workspace / entry["local_path"]).write_bytes(encoded)
        entry["sha256"] = sha256(encoded).hexdigest()
        entry["bytes"] = len(encoded)
        if name == "planner_output":
            run["pre_run_inputs"]["planner_output_sha256"] = entry["sha256"]
        else:
            run["pre_run_inputs"]["data"][name] = entry["sha256"]

    @staticmethod
    def arm_receipt(pin, arm, start, end, root):
        route = pin["inputs"]["execution"]["route_order"][arm]
        common = {"run_pin_sha256": pin["sha256"], "arm": arm, "route_order": route}
        scored = [{"rank": 1, "route": route[0], "score": 1.0, "unit_id": "u",
                   "source_id": "s", "version_id": "v", "part_id": "p", "locator": "paragraph:0"}]
        sample = {"sample_id": "q0#0", "query_id": "q0", "route_order": route,
                  "planner_output_sha256": pin["inputs"]["pre_run_inputs"]["planner_output_sha256"],
                  "latency_ms": 1.0, "scored_results": scored}
        artifacts = {}
        for kind, content in (
            ("preflight", {"checked_at_utc": (datetime.fromisoformat(start) - timedelta(seconds=1)).isoformat(),
                            "executable_sha256": pin["inputs"]["pre_run_inputs"]["build_identity"]["arm_executables"][arm]["sha256"]}),
            ("measurements", {"sample_count": 1, "samples": [sample],
                              "metrics": {"parent_recall_at_20": 1.0, "visible_false_positive": 0,
                                          "unjudged_count": 0}}),
            ("trace", {"events": [{"sample_id": "q0#0", "query_id": "q0", "route_order": route,
                                    "scored_results": scored}]}),
        ):
            path = root / arm / f"{kind}.json"
            path.parent.mkdir(parents=True, exist_ok=True)
            value = {"schema": f"p2-arm-{kind}-v1", **common, **content}
            data = (json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n").encode()
            path.write_bytes(data)
            artifacts[kind] = {"local_path": f"{arm}/{kind}.json", "bytes": len(data), "sha256": sha256(data).hexdigest()}
        return {
            "arm": arm,
            "run_pin_sha256": pin["sha256"],
            "route_order": route,
            "status": "MEASURED",
            "started_at_utc": start,
            "finished_at_utc": end,
            **artifacts,
            "sample_count": 1,
        }

    @classmethod
    def baseline_receipt(cls, pin, root, seal_sha256):
        now = datetime.now(timezone.utc)
        at = lambda minutes: (now - timedelta(minutes=minutes)).isoformat()
        return {
            "schema": "p2-postrun-baseline-v1",
            "run_pin_sha256": pin["sha256"],
            "seal_receipt_sha256": seal_sha256,
            "arms": {
                "L": cls.arm_receipt(pin, "L", at(6), at(5), root),
                "LG": cls.arm_receipt(pin, "LG", at(4), at(3), root),
            },
        }

    @staticmethod
    def synthetic_past_seal(pin, path):
        value = {"schema": "p2-run-seal-v1", "run_pin_sha256": pin["sha256"],
                 "sealed_at_utc": (datetime.now(timezone.utc) - timedelta(minutes=10)).isoformat(),
                 "pin": pin}
        encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
        if path.exists():
            path.chmod(0o644)
        path.write_bytes(encoded)
        return sha256(encoded).hexdigest()

    def test_real_pins_and_small_metadata_validate_without_network(self):
        with patch("socket.create_connection", side_effect=AssertionError("network")), patch(
            "socket.socket.connect", side_effect=AssertionError("network")
        ):
            validate_assets(self.assets)
            verify_assets(self.assets, ROOT, full=False)

    def test_mutable_alias_or_missing_digest_or_license_fails_closed(self):
        for change in ("alias", "digest", "license"):
            with self.subTest(change=change):
                item = deepcopy(self.assets)
                model = item["models"][0]
                if change == "alias":
                    model["revision"] = "main"
                elif change == "digest":
                    model["artifacts"]["safetensors"]["sha256"] = ""
                else:
                    model["license"] = ""
                with self.assertRaises(ContractError):
                    validate_assets(item)

    def test_weight_tokenizer_and_runtime_change_model_identity(self):
        original = model_id(self.assets, "e5-candle-f32")
        for change in ("weight", "tokenizer", "runtime"):
            with self.subTest(change=change):
                item = deepcopy(self.assets)
                if change == "weight":
                    item["models"][0]["artifacts"]["safetensors"]["sha256"] = "f" * 64
                elif change == "tokenizer":
                    item["models"][0]["artifacts"]["tokenizer_json"]["sha256"] = "f" * 64
                else:
                    item["runtimes"]["candle_cpu"]["crates"]["candle-core"]["version"] = "0.11.1"
                validate_assets(item)
                self.assertNotEqual(original, model_id(item, "e5-candle-f32"))

    def test_full_verification_does_not_treat_metadata_as_weights(self):
        with self.assertRaisesRegex(ContractError, "missing asset"):
            verify_assets(self.assets, ROOT, full=True)

    def test_candidate_name_cannot_point_to_other_model(self):
        item = deepcopy(self.assets)
        item["candidates"][0]["model_id"] = "minilm"
        with self.assertRaises(ContractError):
            validate_assets(item)

    def test_changed_metadata_bytes_fail_sha_before_inference(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copytree(ROOT / "metadata", root / "metadata")
            config = root / "metadata/e5/config.json"
            config.write_bytes(config.read_bytes().replace(b'"BertModel"', b'"BertModeX"'))
            with self.assertRaises(ContractError):
                verify_assets(self.assets, root, full=False)

    def test_tokenizer_semantics_cannot_be_redeclared(self):
        item = deepcopy(self.assets)
        item["models"][0]["embedding"]["tokenizer_class"] = "BertTokenizer"
        with self.assertRaises(ContractError):
            verify_assets(item, ROOT, full=False)

    def test_public_split_is_disjoint_and_qrels_are_closed(self):
        validate_public_slice(self.public)
        item = deepcopy(self.public)
        item["queries"][0]["judgments"].append({"docid": "not-judged#0", "grade": 0})
        with self.assertRaises(ContractError):
            validate_public_slice(item)

    def test_2019_wikipedia_dump_is_not_relicensed_as_2023_terms(self):
        item = deepcopy(self.public)
        item["corpus"]["underlying_wikipedia_text_license"] = "CC BY-SA 4.0"
        with self.assertRaises(ContractError):
            validate_public_slice(item)

    def test_paired_run_stays_blocked_until_new_baseline_snapshot(self):
        with self.assertRaisesRegex(ContractError, "pre-run inputs"):
            freeze_run(self.run_manifest, self.assets, self.public)

    def test_future_run_cannot_reuse_stale_asset_manifest_digest(self):
        run = self.ready_run()
        public = deepcopy(self.public)
        assets = deepcopy(self.assets)
        assets["models"][0]["artifacts"]["safetensors"]["sha256"] = "f" * 64
        with self.assertRaisesRegex(ContractError, "asset manifest digest"):
            freeze_run(run, assets, public)

    def test_synthetic_pairing_is_not_blocked_by_unfetched_public_rows(self):
        run = self.ready_run()
        with self.assertRaisesRegex(ContractError, "missing asset"):
            freeze_run(run, self.assets, self.public, ROOT)

    def test_public_lane_and_missing_exact_inputs_remain_blocked(self):
        run = self.ready_run()
        with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
            with self.assertRaisesRegex(ContractError, "public corpus rows not frozen"):
                freeze_run(run, self.assets, self.public, ROOT, evaluation_lane="public_ja")
            run["pre_run_inputs"]["data"]["current_read_sha256"] = None
            with self.assertRaisesRegex(ContractError, "current_read_sha256"):
                freeze_run(run, self.assets, self.public, ROOT)

    def test_prerun_pin_does_not_need_future_d_lg_ldg_results(self):
        run, pin = self.ready_pin()
        self.assertEqual("p2-run-pin-v2", pin["schema"])
        self.assertNotIn("paired_baseline", run)
        self.assertNotIn("results", pin["inputs"])
        with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
            verify_frozen_run(pin, run, self.assets, self.public, ROOT)

    def test_independent_canonical_runpin_golden(self):
        run, pin = self.ready_pin()
        excluded = {"historical_baseline", "observed_at"}
        independent_inputs = {key: value for key, value in run.items() if key not in excluded}
        independent_payload = {"evaluation_lane": "synthetic", "inputs": independent_inputs}
        encoded = json.dumps(independent_payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
        independent_sha = sha256(b"p2-run-pin-v2\0" + encoded).hexdigest()
        self.assertEqual("daf2d060d34cea527bdad8c13bd915072d4d3b3c4932134ead7bf5f7fd044a0b", independent_sha)
        self.assertEqual(independent_sha, pin["sha256"])

    def test_all_semantic_input_mutations_change_pin_and_fail_frozen_check(self):
        run, pin = self.ready_pin()
        changes = {
            "corpus": ("pre_run_inputs", "data", "corpus_text_sha256"),
            "query": ("pre_run_inputs", "data", "query_text_sha256"),
            "qrels": ("synthetic_fixture", "qrels_sha256"),
            "split": ("pre_run_inputs", "data", "label_split_sha256"),
            "source": ("synthetic_fixture", "source_generation"),
            "read": ("pre_run_inputs", "data", "current_read_sha256"),
            "filter": ("pre_run_inputs", "data", "filter_sha256"),
            "baseline_code": ("pre_run_inputs", "code", "baseline_harness_sha256"),
            "path_code": ("pre_run_inputs", "code", "path_crates_sha256"),
            "planner_code": ("pre_run_inputs", "code", "planner_executor_sha256"),
            "model_code": ("pre_run_inputs", "code", "model_adapter_sha256"),
            "runtime_code": ("pre_run_inputs", "code", "runtime_build_sha256"),
            "index_code": ("pre_run_inputs", "code", "index_engine_sha256"),
            "source_file": ("pre_run_inputs", "source_files", "path_crates",
                            "experiments/search-vector-model-poc/test/path_crates.bin"),
            "hardware": ("pre_run_inputs", "host_hardware", "fingerprint_sha256"),
            "disk": ("resource_ceiling", "current_free_bytes"),
            "rss": ("resource_ceiling", "per_process_rss_max_bytes"),
            "reserve": ("resource_ceiling", "reserve_free_bytes"),
            "wall": ("resource_ceiling", "paired_run_wall_seconds"),
            "window": ("execution", "window"),
            "seed": ("execution", "seed"),
            "s1": ("execution", "fusion"),
            "route": ("execution", "route_order", "LDG"),
            "pooling": ("pre_run_inputs", "embedding_policy", "pooling"),
            "mask": ("pre_run_inputs", "embedding_policy", "mask"),
            "normalization": ("pre_run_inputs", "embedding_policy", "normalization"),
            "truncation": ("pre_run_inputs", "embedding_policy", "truncation"),
            "metric": ("pre_run_inputs", "embedding_policy", "metric"),
            "precision": ("pre_run_inputs", "embedding_policy", "precision"),
            "tokenizer": ("pre_run_inputs", "embedding_policy", "tokenizer_sha256"),
            "deadline": ("pre_run_inputs", "deadline_utc"),
        }
        for name, path in changes.items():
            with self.subTest(name=name):
                changed = deepcopy(run)
                node = changed
                for key in path[:-1]:
                    node = node[key]
                key = path[-1]
                if isinstance(node[key], str):
                    node[key] = "z" * 64
                elif isinstance(node[key], list):
                    node[key] = list(reversed(node[key]))
                else:
                    node[key] += 1
                with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
                    try:
                        altered = freeze_run(changed, self.assets, self.public, ROOT)
                    except ContractError:
                        pass  # Unsupported semantic changes must fail closed.
                    else:
                        self.assertNotEqual(pin["sha256"], altered["sha256"])
                    with self.assertRaises(ContractError):
                        verify_frozen_run(pin, changed, self.assets, self.public, ROOT)

    def test_valid_input_changes_have_distinct_runpins(self):
        run, pin = self.ready_pin()
        for name in ("seed", "read", "disk", "rss", "deadline", "source_file"):
            with self.subTest(name=name):
                changed = deepcopy(run)
                if name == "seed":
                    changed["execution"]["seed"] += 1
                elif name == "read":
                    changed["pre_run_inputs"]["data"]["current_read_sha256"] = "c" * 64
                    changed["pre_run_inputs"]["data_artifacts"]["current_read_sha256"]["sha256"] = "c" * 64
                elif name == "disk":
                    changed["execution"]["observed_free_disk_kib"] += 1
                    changed["resource_ceiling"]["current_free_bytes"] += 1024
                elif name == "rss":
                    changed["resource_ceiling"]["per_process_rss_max_bytes"] += 1024
                elif name == "deadline":
                    changed["pre_run_inputs"]["deadline_utc"] = "2026-10-03T00:00:00+00:00"
                else:
                    category = "path_crates"
                    relative = "experiments/search-vector-model-poc/test/path_crates.bin"
                    changed["pre_run_inputs"]["source_files"][category][relative] = "b" * 64
                    changed["pre_run_inputs"]["code"][category + "_sha256"] = sha256(
                        f"{relative}\0{'b' * 64}\n".encode()
                    ).hexdigest()
                with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
                    altered = freeze_run(changed, self.assets, self.public, ROOT)
                self.assertNotEqual(pin["sha256"], altered["sha256"])

        changed_assets = deepcopy(self.assets)
        changed_assets["models"][0]["artifacts"]["safetensors"]["sha256"] = "b" * 64
        changed_run = deepcopy(run)
        changed_run["assets_manifest_sha256"] = sha256(
            (json.dumps(changed_assets, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
        ).hexdigest()
        with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
            altered = freeze_run(changed_run, changed_assets, self.public, ROOT)
        self.assertNotEqual(pin["sha256"], altered["sha256"])

    def test_unsupported_policy_priority_raw_add_and_results_smuggling_rejected(self):
        run = self.ready_run()
        mutations = (
            lambda r: r["execution"].__setitem__("fusion", "RRF"),
            lambda r: r["execution"].__setitem__("score_policy", "add raw scores"),
            lambda r: r["execution"]["route_order"].__setitem__("LDG", ["Vector", "HyperGraph", "Lexical"]),
            lambda r: r["pre_run_inputs"].__setitem__("results", {"D": "PASS"}),
            lambda r: r["measurement"].__setitem__("actual_scores", {"D": 0.9}),
        )
        for mutate in mutations:
            changed = deepcopy(run)
            mutate(changed)
            with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
                with self.assertRaises(ContractError):
                    freeze_run(changed, self.assets, self.public, ROOT)

    def test_declared_code_file_bytes_are_rehashed_before_seal(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "src" / "run.rs"
            path.parent.mkdir()
            path.write_bytes(b"fixed source")
            expected = sha256(path.read_bytes()).hexdigest()
            _verify_source_files({"baseline_harness": {"src/run.rs": expected}}, root)
            path.write_bytes(b"changed source")
            with self.assertRaisesRegex(ContractError, "code/build bytes changed"):
                _verify_source_files({"baseline_harness": {"src/run.rs": expected}}, root)

    def test_missing_real_baseline_cargo_source_rejected(self):
        run = self.ready_run()
        with TemporaryDirectory() as directory:
            root = Path(directory)
            model_root = root / "experiments/search-vector-model-poc"
            model_root.mkdir(parents=True)
            (root / "experiments/search-vector-poc/src").mkdir(parents=True)
            (root / "experiments/search-vector-poc/Cargo.toml").write_text(
                '[package]\nname="baseline"\nversion="0.1.0"\n', encoding="utf-8"
            )
            (root / "experiments/search-vector-poc/src/run.rs").write_text("fn changed() {}", encoding="utf-8")
            for category, files in run["pre_run_inputs"]["source_files"].items():
                relative = next(iter(files))
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(category, encoding="utf-8")
                files[relative] = sha256(path.read_bytes()).hexdigest()
                run["pre_run_inputs"]["code"][category + "_sha256"] = sha256(
                    f"{relative}\0{files[relative]}\n".encode()
                ).hexdigest()
            with patch("asset_contract.verify_assets"):
                with self.assertRaises(ContractError):
                    freeze_run(run, self.assets, self.public, model_root)

    def test_real_cargo_dependency_and_patch_tree_closure(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            def put(relative, content):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
            put("experiments/search-vector-model-poc/Cargo.toml",
                '[package]\nname="model"\nversion="0.1.0"\n[workspace]\n')
            put("Cargo.toml", '[workspace]\n[workspace.dependencies]\nshared={path="crates/shared"}\n')
            put("Cargo.lock", "version = 4\n")
            put("experiments/search-vector-model-poc/Cargo.lock", "version = 4\n")
            put("experiments/search-vector-model-poc/src/lib.rs", "pub fn model() {}\n")
            put("experiments/search-vector-poc/Cargo.toml",
                '[package]\nname="baseline"\nversion="0.1.0"\n[workspace]\n'
                '[dependencies]\nlocal = { path = "../../crates/local" }\n'
                '[patch.crates-io]\npatched = { path = "../../third_party/patched" }\n')
            put("experiments/search-vector-poc/Cargo.lock", "version = 4\n")
            put("experiments/search-vector-poc/src/run.rs", "pub fn run() {}\n")
            put("crates/local/Cargo.toml", '[package]\nname="local"\nversion="0.1.0"\n'
                '[dependencies]\nshared={workspace=true}\n')
            put("crates/shared/Cargo.toml", '[package]\nname="shared"\nversion="0.1.0"\n')
            put("crates/shared/src/lib.rs", "pub fn shared() {}\n")
            put("crates/local/build.rs", "fn main() {}\n")
            put("crates/local/src/lib.rs", 'const X: &str = include_str!("../../../shared-data.txt");\n')
            put("crates/local/data.txt", "included data\n")
            put("shared-data.txt", "external included data\n")
            put("third_party/patched/Cargo.toml", '[package]\nname="patched"\nversion="0.1.0"\n')
            put("third_party/patched/src/lib.rs", "pub fn patched() {}\n")
            inventory = derive_source_inventory(root)
            self.assertIn("experiments/search-vector-poc/src/run.rs", inventory)
            self.assertIn("crates/local/build.rs", inventory)
            self.assertIn("crates/local/data.txt", inventory)
            self.assertIn("shared-data.txt", inventory)
            self.assertIn("crates/shared/src/lib.rs", inventory)
            self.assertIn("Cargo.lock", inventory)
            self.assertIn("third_party/patched/src/lib.rs", inventory)
            categories, code_pins = capture_source_pins(root)
            self.assertEqual(set(code_pins), set(self.ready_run()["pre_run_inputs"]["code"]))
            _verify_complete_source_closure(categories, root)
            for change in ("missing", "extra", "mutated"):
                with self.subTest(change=change):
                    declared = deepcopy(categories)
                    if change == "missing":
                        del declared["baseline_harness"]["experiments/search-vector-poc/src/run.rs"]
                    elif change == "extra":
                        declared["baseline_harness"]["not-reachable.rs"] = "0" * 64
                    else:
                        (root / "experiments/search-vector-poc/src/run.rs").write_text("pub fn changed() {}\n")
                    with self.assertRaises(ContractError):
                        _verify_complete_source_closure(declared, root)
                    if change == "mutated":
                        put("experiments/search-vector-poc/src/run.rs", "pub fn run() {}\n")
            put("crates/local/src/new.rs", "pub fn newly_reachable() {}\n")
            with self.assertRaisesRegex(ContractError, "missing, extra or changed"):
                _verify_complete_source_closure(categories, root)
            (root / "crates/local/src/new.rs").unlink()
            (root / "crates/local/link.rs").symlink_to(root / "crates/local/src/lib.rs")
            with self.assertRaisesRegex(ContractError, "symlink"):
                derive_source_inventory(root)

    def test_public_fake_mapping_without_row_bytes_rejected(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, local_root = self.actual_data_fixture(workspace)
            public = deepcopy(self.public)
            corpus_entry = run["pre_run_inputs"]["data_artifacts"]["corpus_text_sha256"]
            corpus_path = workspace / corpus_entry["local_path"]
            corpus_value = json.loads(corpus_path.read_bytes())
            public_keys = ("docid", "text", "source_id", "version_id", "part_id", "unit_id",
                           "locator", "source_generation")
            corpus_value["rows"][0] = {key: corpus_value["rows"][0][key] for key in public_keys}
            corpus_bytes = (json.dumps(corpus_value, ensure_ascii=False, sort_keys=True) + "\n").encode()
            corpus_path.write_bytes(corpus_bytes)
            corpus_entry["sha256"] = sha256(corpus_bytes).hexdigest()
            corpus_entry["bytes"] = len(corpus_bytes)
            run["pre_run_inputs"]["data"]["corpus_text_sha256"] = corpus_entry["sha256"]
            binding = {"schema": "p2-unit-bindings-v1", "bindings": [{key: corpus_value["rows"][0][key]
                       for key in ("docid", "source_id", "version_id", "part_id", "unit_id", "locator")}]}
            self.repin_data_artifact(run, workspace, "unit_binding_sha256", binding)
            planner = {"schema": "p2-planner-output-v1", "route_order": run["execution"]["route_order"],
                       "fanout_by_arm": run["pre_run_inputs"]["fanout_by_arm"],
                       "window": run["execution"]["window"], "seed": run["execution"]["seed"]}
            self.repin_data_artifact(run, workspace, "planner_output", planner)
            public["state"] = "ROWS_FROZEN"
            run["public_japanese"]["corpus_rows_ready"] = True
            selected_path = workspace / "selected-rows.jsonl"
            selected_path.write_text('{"docid":"d#0","text":"監査証跡"}\n', encoding="utf-8")
            selected = selected_path.read_bytes()
            selected_entry = {"local_path": "selected-rows.jsonl", "bytes": len(selected),
                              "sha256": sha256(selected).hexdigest()}
            run["public_japanese"]["selected_rows_artifact"] = selected_entry
            public["corpus"]["selected_rows_sha256"] = selected_entry["sha256"]
            public["corpus"]["selected_row_to_source_version_part_unit_map"] = {
                "fake-row": "fake-binding"}
            run["public_slice_manifest_sha256"] = sha256(
                (json.dumps(public, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
            ).hexdigest()
            with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_capacity_admission"):
                with self.assertRaisesRegex(ContractError, "public row to Source"):
                    freeze_run(run, self.assets, public, local_root, evaluation_lane="public_ja")

    def test_one_byte_model_capacity_component_rejected(self):
        run = self.ready_run()
        scenario = run["resource_scenarios"]["e5_candle_only"]
        scenario["components_bytes"]["model_artifact"] = 1
        scenario["owned_peak_additional_bytes"] = sum(scenario["components_bytes"].values())
        scenario["required_free_with_reserve_bytes"] = (
            scenario["owned_peak_additional_bytes"] + run["resource_ceiling"]["reserve_free_bytes"]
        )
        with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
            with self.assertRaises(ContractError):
                freeze_run(run, self.assets, self.public, ROOT)

    def test_actual_statvfs_and_available_ram_receipt_fail_closed_when_insufficient(self):
        run = self.ready_run()
        observed = capture_capacity_receipt(run, ROOT)
        self.assertGreater(observed["free_disk_bytes"], 0)
        self.assertGreater(observed["available_memory_bytes"], 0)
        self.assertEqual("p2-capacity-admission-v1", observed["schema"])
        impossible = deepcopy(run)
        impossible["resource_scenarios"]["e5_candle_only"]["required_free_with_reserve_bytes"] = (
            observed["free_disk_bytes"] + 1
        )
        with self.assertRaises(ContractError):
            _verify_capacity_admission(impossible, ROOT, observed, initial=False)
        future = deepcopy(observed)
        future["captured_at_utc"] = (datetime.now(timezone.utc) + timedelta(minutes=1)).isoformat()
        with self.assertRaisesRegex(ContractError, "future"):
            _verify_capacity_admission(run, ROOT, future, initial=False)

    def test_real_data_artifact_mutation_and_ineligible_qrels_rejected(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, _ = self.actual_data_fixture(workspace)
            original = self._fixture_export(workspace)["artifacts"]
            _verify_data_artifacts(run, self.public, workspace, "synthetic")
            entry = run["pre_run_inputs"]["data_artifacts"]["current_read_sha256"]
            path = workspace / entry["local_path"]
            value = json.loads(path.read_bytes())
            value["allowed"] = []
            value["denied"] = [{"source_id": "s", "version_id": "v"}]
            encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n").encode()
            path.write_bytes(encoded)
            with self.assertRaisesRegex(ContractError, "asset SHA-256 changed"):
                _verify_data_artifacts(run, self.public, workspace, "synthetic")
            entry["sha256"] = sha256(encoded).hexdigest()
            entry["bytes"] = len(encoded)
            run["pre_run_inputs"]["data"]["current_read_sha256"] = entry["sha256"]
            with self.assertRaises(ContractError):
                _verify_data_artifacts(run, self.public, workspace, "synthetic")

    def test_synthetic_corpus_text_and_part_binding_match_executed_baseline(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, _ = self.actual_data_fixture(workspace)
            original = self._fixture_export(workspace)["artifacts"]
            _verify_data_artifacts(run, self.public, workspace, "synthetic")
            for mutation in ("text", "part"):
                with self.subTest(mutation=mutation):
                    changed = deepcopy(run)
                    entry = changed["pre_run_inputs"]["data_artifacts"]["corpus_text_sha256"]
                    corpus = json.loads((workspace / entry["local_path"]).read_bytes())
                    binding_entry = changed["pre_run_inputs"]["data_artifacts"]["unit_binding_sha256"]
                    bindings = json.loads((workspace / binding_entry["local_path"]).read_bytes())
                    if mutation == "text":
                        corpus["rows"][0]["text"] = "改ざんされた別本文"
                        corpus["rows"][0]["text_sha256"] = sha256("改ざんされた別本文".encode()).hexdigest()
                        bindings["bindings"][0]["text_sha256"] = corpus["rows"][0]["text_sha256"]
                    else:
                        corpus["rows"][0]["part_id"] = "forged-part"
                        bindings["bindings"][0]["part_id"] = "forged-part"
                    self.repin_data_artifact(changed, workspace, "corpus_text_sha256", corpus)
                    self.repin_data_artifact(changed, workspace, "unit_binding_sha256", bindings)
                    try:
                        with self.assertRaisesRegex(ContractError, "executed synthetic input"):
                            _verify_data_artifacts(changed, self.public, workspace, "synthetic")
                    finally:
                        self.repin_data_artifact(run, workspace, "corpus_text_sha256",
                                                 original["corpus_text_sha256"])
                        self.repin_data_artifact(run, workspace, "unit_binding_sha256",
                                                 original["unit_binding_sha256"])

    @staticmethod
    def _fixture_export(workspace):
        import subprocess
        return json.loads(subprocess.check_output([str(workspace / "bin/baseline-export"),
                                                  "--export-synthetic-input", "1", "20"], text=True))

    def test_synthetic_read_filter_and_planner_forgery_rejected_after_repin(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, _ = self.actual_data_fixture(workspace)
            for name in ("current_read_sha256", "filter_sha256", "planner_output"):
                with self.subTest(artifact=name):
                    changed = deepcopy(run)
                    original = self._fixture_export(workspace)["artifacts"][name]
                    value = deepcopy(original)
                    if name == "current_read_sha256":
                        value["allowed"] = []
                        value["denied"] = [{"source_id": "s", "version_id": "v"}]
                        eligible = {"schema": "p2-eligible-qrels-v1", "queries": [
                            {"query_id": "q0", "parents": []}]}
                        self.repin_data_artifact(changed, workspace, "eligible_parent_set_sha256", eligible)
                    elif name == "filter_sha256":
                        value["included_unit_ids"] = []
                        eligible = {"schema": "p2-eligible-qrels-v1", "queries": [
                            {"query_id": "q0", "parents": []}]}
                        self.repin_data_artifact(changed, workspace, "eligible_parent_set_sha256", eligible)
                    else:
                        value["baseline_plans"][0]["retriever_ids"] = ["forged:lexical"]
                        value["baseline_plans"][0]["s1_retriever_order"] = ["forged:lexical"]
                    self.repin_data_artifact(changed, workspace, name, value)
                    try:
                        with self.assertRaisesRegex(ContractError, "executed synthetic input"):
                            _verify_data_artifacts(changed, self.public, workspace, "synthetic")
                    finally:
                        self.repin_data_artifact(run, workspace, name, original)
                        if name != "planner_output":
                            self.repin_data_artifact(run, workspace, "eligible_parent_set_sha256",
                                                     self._fixture_export(workspace)["artifacts"]["eligible_parent_set_sha256"])

    def test_compiled_baseline_export_binds_real_corpus_and_planner(self):
        binary = os.environ.get("P2_BASELINE_BIN")
        if not binary:
            self.skipTest("set P2_BASELINE_BIN to the compiled isolated baseline executable")
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            baseline = workspace / "experiments/search-vector-poc"
            baseline.mkdir(parents=True)
            run = self.ready_run()
            for field, filename in (("corpus_manifest_sha256", "corpus_manifest.json"),
                                    ("queries_sha256", "queries.jsonl"),
                                    ("qrels_sha256", "qrels.jsonl")):
                shutil.copy2(ROOT.parent / "search-vector-poc" / filename, baseline / filename)
                run["synthetic_fixture"][field] = sha256((baseline / filename).read_bytes()).hexdigest()
            executable = workspace / "bin/search-vector-poc"
            executable.parent.mkdir()
            shutil.copy2(binary, executable)
            exported = json.loads(__import__("subprocess").check_output(
                [str(executable), "--export-synthetic-input", "32", "20"], text=True))
            run["execution"]["actor"] = exported["actor"]
            run["execution"]["seed"] = exported["seed"]
            run["execution"]["window"] = exported["window"]
            run["synthetic_fixture"]["source_generation"] = exported["artifacts"]["source_snapshot_sha256"]["source_generation"]
            run["pre_run_inputs"]["sample_plan"]["query_ids"] = [
                item["query_id"] for item in exported["artifacts"]["query_text_sha256"]["queries"]]
            executable_entry = {"local_path": "bin/search-vector-poc",
                                "sha256": sha256(executable.read_bytes()).hexdigest(),
                                "bytes": executable.stat().st_size}
            identity = run["pre_run_inputs"]["build_identity"]
            identity["baseline_export_executable"] = executable_entry
            identity["arm_executables"] = {arm: executable_entry for arm in ("L", "LG")}
            for name, value in exported["artifacts"].items():
                relative = f"inputs/{name}.json"
                path = workspace / relative
                path.parent.mkdir(exist_ok=True)
                encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n").encode()
                path.write_bytes(encoded)
                entry = {"local_path": relative, "sha256": sha256(encoded).hexdigest(),
                         "bytes": len(encoded)}
                run["pre_run_inputs"]["data_artifacts"][name] = entry
                if name == "planner_output":
                    run["pre_run_inputs"]["planner_output_sha256"] = entry["sha256"]
                else:
                    run["pre_run_inputs"]["data"][name] = entry["sha256"]
            _verify_data_artifacts(run, self.public, workspace, "synthetic")
            corpus = deepcopy(exported["artifacts"]["corpus_text_sha256"])
            corpus["rows"][0]["text"] = "別の本文"
            corpus["rows"][0]["text_sha256"] = sha256("別の本文".encode()).hexdigest()
            bindings = deepcopy(exported["artifacts"]["unit_binding_sha256"])
            bindings["bindings"][0]["text_sha256"] = corpus["rows"][0]["text_sha256"]
            self.repin_data_artifact(run, workspace, "corpus_text_sha256", corpus)
            self.repin_data_artifact(run, workspace, "unit_binding_sha256", bindings)
            with self.assertRaisesRegex(ContractError, "executed synthetic input"):
                _verify_data_artifacts(run, self.public, workspace, "synthetic")

    def test_future_measured_arm_rejected(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, local_root = self.actual_data_fixture(workspace)
            with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_capacity_admission"):
                pin = freeze_run(run, self.assets, self.public, local_root)
            root = workspace / "evidence"
            root.mkdir()
            seal_path = root / "seal.json"
            seal_sha = self.synthetic_past_seal(pin, seal_path)
            future = (datetime.now(timezone.utc) + timedelta(minutes=5)).isoformat()
            later = (datetime.now(timezone.utc) + timedelta(minutes=6)).isoformat()
            baseline = {
                "schema": "p2-postrun-baseline-v1", "run_pin_sha256": pin["sha256"],
                "seal_receipt_sha256": seal_sha,
                "arms": {arm: self.arm_receipt(pin, arm, future, later, root) for arm in ("L", "LG")},
            }
            with self.assertRaisesRegex(ContractError, "future"):
                validate_baseline_receipt(pin, baseline, root, seal_path, workspace)

    def test_begin_arm_reverifies_real_data_and_writes_exclusive_preflight(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, local_root = self.actual_data_fixture(workspace)
            evidence = workspace / "evidence"
            evidence.mkdir()
            with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_capacity_admission"):
                pin = freeze_run(run, self.assets, self.public, local_root)
                entry = begin_arm(pin, "L", run, self.assets, self.public, local_root, evidence)
                self.assertEqual("p2-arm-preflight-v1", json.loads(
                    (evidence / entry["local_path"]).read_text())["schema"])
                with self.assertRaisesRegex(ContractError, "preflight already exists"):
                    begin_arm(pin, "L", run, self.assets, self.public, local_root, evidence)

    def test_baseline_before_dense_and_same_pin_receipts(self):
        with TemporaryDirectory() as directory:
            workspace = Path(directory)
            run, local_root = self.actual_data_fixture(workspace)
            with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_capacity_admission"):
                pin = freeze_run(run, self.assets, self.public, local_root)
            root = workspace / "evidence"
            root.mkdir()
            seal_path = root / "run-pin-seal.json"
            with patch("asset_contract.verify_assets"), patch("asset_contract._verify_source_files"), patch("asset_contract._verify_complete_source_closure"), patch("asset_contract._verify_build_identity"), patch("asset_contract._verify_data_artifacts"), patch("asset_contract._verify_capacity_admission"):
                seal_run(pin, seal_path, run, self.assets, self.public, local_root)
                with self.assertRaisesRegex(ContractError, "seal already exists"):
                    seal_run(pin, seal_path, run, self.assets, self.public, local_root)
            seal_sha256 = self.synthetic_past_seal(pin, seal_path)
            baseline = self.baseline_receipt(pin, root, seal_sha256)
            mutated_pin = deepcopy(pin)
            mutated_pin["inputs"]["execution"]["seed"] += 1
            with self.assertRaisesRegex(ContractError, "RunPin mutated"):
                validate_baseline_receipt(mutated_pin, baseline, root, seal_path, workspace)
            digest = validate_baseline_receipt(pin, baseline, root, seal_path, workspace)
            now = datetime.now(timezone.utc)
            dense_start = (now - timedelta(minutes=2)).isoformat()
            dense_end = (now - timedelta(minutes=1)).isoformat()
            dense = {
                "schema": "p2-postrun-dense-v1",
                "run_pin_sha256": pin["sha256"],
                "baseline_receipt_sha256": digest,
                "arms": {
                    arm: self.arm_receipt(pin, arm, dense_start, dense_end, root)
                    for arm in ("D", "LD", "LDG")
                },
            }
            validate_paired_receipt(pin, baseline, dense, root, seal_path, workspace)
            measurements_path = root / "L/measurements.json"
            original_measurements = measurements_path.read_bytes()
            bad_measurements = json.loads(original_measurements)
            bad_measurements["metrics"]["parent_recall_at_20"] = 0.0
            encoded = (json.dumps(bad_measurements, ensure_ascii=False, sort_keys=True) + "\n").encode()
            measurements_path.write_bytes(encoded)
            bad_baseline = deepcopy(baseline)
            bad_baseline["arms"]["L"]["measurements"] = {
                "local_path": "L/measurements.json", "sha256": sha256(encoded).hexdigest(), "bytes": len(encoded)}
            with self.assertRaisesRegex(ContractError, "numeric metrics differ"):
                validate_baseline_receipt(pin, bad_baseline, root, seal_path, workspace)
            measurements_path.write_bytes(original_measurements)
            trace_path = root / "L/trace.json"
            original_trace = trace_path.read_bytes()
            ineligible = deepcopy(baseline)
            for kind, path, original in (("measurements", measurements_path, original_measurements),
                                         ("trace", trace_path, original_trace)):
                value = json.loads(original)
                results = value["samples"][0]["scored_results"] if kind == "measurements" else value["events"][0]["scored_results"]
                results[0]["unit_id"] = "not-an-eligible-unit"
                encoded = (json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n").encode()
                path.write_bytes(encoded)
                ineligible["arms"]["L"][kind] = {
                    "local_path": f"L/{kind}.json", "sha256": sha256(encoded).hexdigest(), "bytes": len(encoded)}
            with self.assertRaisesRegex(ContractError, "scored result route/eligibility"):
                validate_baseline_receipt(pin, ineligible, root, seal_path, workspace)
            measurements_path.write_bytes(original_measurements)
            trace_path.write_bytes(original_trace)
            missing_preflight = deepcopy(baseline)
            del missing_preflight["arms"]["L"]["preflight"]
            with self.assertRaises(ContractError):
                validate_baseline_receipt(pin, missing_preflight, root, seal_path, workspace)
            for tamper in ("wrong_pin", "wrong_order", "missing_arm", "early_dense", "unmeasured", "wrong_baseline"):
                with self.subTest(tamper=tamper):
                    item = deepcopy(dense)
                    if tamper == "wrong_pin":
                        item["arms"]["D"]["run_pin_sha256"] = "0" * 64
                    elif tamper == "wrong_order":
                        item["arms"]["LDG"]["route_order"] = ["Vector", "Lexical", "HyperGraph"]
                    elif tamper == "missing_arm":
                        del item["arms"]["LD"]
                    elif tamper == "early_dense":
                        item["arms"]["D"]["started_at_utc"] = baseline["arms"]["L"]["started_at_utc"]
                    elif tamper == "unmeasured":
                        item["arms"]["D"]["status"] = "UNRUN"
                    else:
                        item["baseline_receipt_sha256"] = "0" * 64
                    with self.assertRaises(ContractError):
                        validate_paired_receipt(pin, baseline, item, root, seal_path, workspace)
            (root / "L/measurements.json").write_text("{}")
            with self.assertRaisesRegex(ContractError, "asset byte length changed"):
                validate_baseline_receipt(pin, baseline, root, seal_path, workspace)
            seal_path.chmod(0o644)
            seal_path.write_text("{}")
            with self.assertRaisesRegex(ContractError, "seal bytes changed"):
                validate_baseline_receipt(pin, baseline, root, seal_path, workspace)


if __name__ == "__main__":
    unittest.main()
