#!/usr/bin/env python3
"""Opt-in Lake setup regression over a tiny disposable witness plugin.

Build once with --prepare-only, then use --already-built to exercise the saved
Lake setup without invoking Lake again. Successful checks remove the fixture;
failures retain it for diagnosis. No host, Proofs, Mathlib or service is used.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

REPO = Path(__file__).resolve().parents[1]
ROOT = REPO / "target/setup-semantic-check"
PROJECT = ROOT / "project"
PREFIX = Path.home() / ".elan/toolchains/leanprover--lean4---v4.36.0"
LIBRARY = REPO / "lean/.lake/build/lib/lean"
LIMIT = 256 * 1024 * 1024
SOURCE = """import Lean
open Lean
theorem demo (n : Nat) : n + 0 = n := by
  run_tac
    unless (← getOptionDecls).contains `r2PluginWitness do
      throwError "missing setup plugin"
  exact Nat.add_zero n
"""
PROBE = """import LeanSemanticSearch.Capability
open Lean LeanSemanticSearch
unsafe def main (args : List String) : IO Unit := do
  let [mode, setupPath, requestPath] := args
    | throw (IO.userError "expected mode, setup and request")
  let output ← IO.withStdout (← IO.getStderr) do
    let request ← IO.FS.readFile requestPath
    if mode == "legacy" then
      Capability.proofGoalFeatures request
    else
      let setup ← ModuleSetup.load setupPath
      let payload ← IO.ofExcept (Json.parse request)
      let response ← GoalElaboration.runWithSetup setup payload
      pure <| (match response with
        | .ok rows => JsonSupport.responseJson JsonSupport.proofGoalCommand
            JsonSupport.proofGoalFeatureCommandVersion JsonSupport.semanticFeatureVersion rows #[]
        | .error error => JsonSupport.errorResponseJson JsonSupport.proofGoalCommand
            JsonSupport.proofGoalFeatureCommandVersion JsonSupport.semanticFeatureVersion error).compress
  if output.utf8ByteSize + 1 > 1024 * 1024 then
    throw (IO.userError "setup regression response exceeds bound")
  (← IO.getStdout).putStrLn output
"""


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def allocated(path):
    if not path.exists():
        return 0
    return int(subprocess.check_output(["du", "-sk", str(path)], text=True).split()[0]) * 1024


def capacity():
    require(allocated(ROOT) < LIMIT, "fixture exceeds 256 MiB bound")
    require(shutil.disk_usage(REPO).free > 25 * 1024**3, "25 GiB disk reserve")


def environment():
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("DYLD_", "__XPC_DYLD_", "LEAN_RS_", "LEAN_HOST_MCP_"))
           and k not in ("LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT")}
    env.update(LEAN_SYSROOT=str(PREFIX), LEAN_NUM_THREADS="2",
               ELAN_TOOLCHAIN="leanprover/lean4:v4.36.0", TMPDIR=str(ROOT / "tmp") + "/",
               LAKE_CACHE_DIR=str(ROOT / "lake-cache"), LAKE_ARTIFACT_CACHE="false",
               LEAN_PATH=str(LIBRARY) + os.pathsep + str(PROJECT / ".lake/build/lib/lean"))
    return env


def execute(argv, timeout=30):
    capacity()
    process = subprocess.Popen([str(a) for a in argv], cwd=PROJECT, env=environment(),
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    print("start", process.pid, argv[1:3], flush=True)
    started = time.monotonic()
    try:
        while True:
            try:
                stdout, stderr = process.communicate(timeout=1)
                break
            except subprocess.TimeoutExpired:
                capacity()
                if time.monotonic() - started > timeout:
                    raise TimeoutError("bounded setup regression timed out")
    except BaseException:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        raise
    try:
        os.killpg(process.pid, 0)
    except ProcessLookupError:
        pass
    else:
        raise RuntimeError("owned process group has not retired; retain fixture")
    require(len(stdout) <= 1024**2 and len(stderr) <= 1024**2, "capture bound exceeded")
    require(process.returncode == 0, stderr.decode(errors="replace")[-3000:])
    capacity()
    print("closed", process.pid, "exit", process.returncode, flush=True)
    return stdout


def prepare():
    ROOT.mkdir(parents=True, exist_ok=False)
    PROJECT.mkdir()
    (ROOT / "tmp").mkdir()
    (PROJECT / "lean-toolchain").write_text("leanprover/lean4:v4.36.0\n")
    (PROJECT / "lakefile.lean").write_text(
        "import Lake\nopen Lake DSL\npackage demo\nlean_lib Witness\n"
        "lean_lib Demo where\n  plugins := #[`@demo/Witness:shared]\n")
    (PROJECT / "Witness.lean").write_text(
        'import Lean\nregister_option r2PluginWitness : Bool := '
        '{ defValue := false, descr := "plugin witness" }\n')
    (PROJECT / "Demo.lean").write_text(SOURCE)
    execute([PREFIX / "bin/lake", "--no-cache", "build", "+Demo"], timeout=90)
    setup = PROJECT / ".lake/build/ir/Demo.setup.json"
    require(setup.is_file(), "Lake did not emit the module setup")
    saved = ROOT / "saved.setup.json"
    saved.write_bytes(setup.read_bytes())
    require(json.loads(saved.read_text())["plugins"], "fixture has no selected plugin")
    (ROOT / "Probe.lean").write_text(PROBE)
    (ROOT / "request.json").write_text(json.dumps({
        "module": "Demo", "source_text": SOURCE, "file_label": str(PROJECT / "Demo.lean"),
        "declaration": "demo", "position": {"line": 4, "column": 3}}))
    print("prepared", saved, "sha256", hashlib.sha256(saved.read_bytes()).hexdigest(), flush=True)


def inventory():
    return {str(p.relative_to(PROJECT)): (p.stat().st_ino, p.stat().st_mtime_ns,
                                          hashlib.sha256(p.read_bytes()).hexdigest())
            for p in PROJECT.rglob("*") if p.is_file()}


def response(mode, setup):
    raw = execute([PREFIX / "bin/lean", "-DwarningAsError=true", "--run", ROOT / "Probe.lean",
                   mode, setup, ROOT / "request.json"])
    require(len(raw.splitlines()) == 1, "typed result mixed with diagnostics")
    data = json.loads(raw)
    require(data["command"] == "proof_goal_features" and
            data["command_version"] == "proof_goal_features.v1" and
            data["schema_version"] == "lean-semantic-search.capability.v1" and
            data["feature_version"] == "features.roles.v3", "semantic response contract")
    return data


def check():
    require((PROJECT / "Demo.lean").read_text() == SOURCE, "fixture source changed")
    require((PROJECT / ".lake/build/lib/lean/Demo.olean").is_file(), "prepare fixture first")
    before = inventory()
    saved = ROOT / "saved.setup.json"
    require(saved.read_bytes() == (PROJECT / ".lake/build/ir/Demo.setup.json").read_bytes(),
            "saved setup differs from Lake output")
    positive = response("setup", saved)
    require(len(positive["rows"]) == 1 and not positive["diagnostics"], "matching plugin did not yield a goal")
    require(positive["rows"][0]["fingerprints"] and positive["rows"][0]["role_features"],
            "matching plugin did not yield real semantic features")
    legacy = response("legacy", saved)
    require(not legacy["rows"] and any(d["severity"] == "error" for d in legacy["diagnostics"]),
            "legacy API unexpectedly accepted the plugin-dependent source")
    omitted = json.loads(saved.read_text())
    witness = [i for i, plugin in enumerate(omitted["plugins"])
               if "libdemo_Witness." in plugin["path"]]
    require(len(witness) == 1, "expected one Lake-selected witness plugin")
    witness_index = witness[0]
    # Keep Lake's other selected plugins; remove only the witness under test.
    omitted["plugins"].pop(witness_index)
    omitted_path = ROOT / "omitted.setup.json"
    omitted_path.write_text(json.dumps(omitted))
    denied = response("setup", omitted_path)
    require(not denied["rows"] and "missing setup plugin" in str(denied["diagnostics"]),
            "omitted plugin did not refuse")
    missing = json.loads(saved.read_text())
    missing["plugins"][witness_index]["path"] += ".missing"
    missing_path = ROOT / "missing.setup.json"
    missing_path.write_text(json.dumps(missing))
    denied = response("setup", missing_path)
    require(not denied["rows"] and any(d["severity"] == "error" for d in denied["diagnostics"]),
            "missing plugin did not refuse")
    require(inventory() == before, "no-build semantic checks changed the project")
    print("PASS real setup: one goal with features; legacy/omitted/missing controls refused; project unchanged")
    size = allocated(ROOT)
    shutil.rmtree(ROOT)
    require(not ROOT.exists(), "fixture cleanup incomplete")
    print("removed allocated bytes", size, "remaining", 0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--already-built", action="store_true")
    args = parser.parse_args()
    require(not (args.prepare_only and args.already_built), "choose prepare or already-built")
    require((PREFIX / "bin/lean").is_file() and (LIBRARY / "LeanSemanticSearch/GoalElaboration.olean").is_file(),
            "installed Lean 4.36 and built semantic library required")
    if not args.already_built:
        prepare()
    if not args.prepare_only:
        check()


if __name__ == "__main__":
    main()
