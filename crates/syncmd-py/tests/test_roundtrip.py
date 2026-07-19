"""Python SDK round-trip + flow-sdk JSON-shape tests.

Run after `maturin develop` (so the `syncmd` module is importable):

    cd crates/syncmd-py && VIRTUAL_ENV=../../.venv-test ../../.venv-test/bin/maturin develop
    ../../.venv-test/bin/python -m pytest crates/syncmd-py/tests/test_roundtrip.py
"""

import os
import subprocess
import tempfile

import syncmd


def _make_repo(tmp):
    def git(*args):
        subprocess.run(["git", *args], cwd=tmp, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    git("init", "-q", "-b", "main")
    git("config", "user.name", "t")
    git("config", "user.email", "t@t.dev")
    with open(os.path.join(tmp, "CLAUDE.md"), "w") as f:
        f.write("rules v1\n")
    git("add", "-A")
    git("commit", "-q", "-m", "only claude")
    return tmp


def test_plan_does_not_write_and_is_flow_sdk_shaped():
    with tempfile.TemporaryDirectory() as tmp:
        _make_repo(tmp)
        r = syncmd.plan(tmp)

        # object API
        assert r.groups[0].name == "instructions"
        assert r.groups[0].decision == "propagated"
        assert r.groups[0].winner_path == "CLAUDE.md"
        assert r.summary.written == 0

        # flow-sdk dict shape: type first, id second, snake_case, nulls omitted
        d = r.to_dict()
        assert d["type"] == "sync_report"
        g0 = d["groups"][0]
        keys = list(g0.keys())
        assert keys[0] == "type" and keys[1] == "id"
        assert g0["type"] == "claude_md"
        assert g0["winner_reason"] == "bootstrap"
        assert "note" not in g0 and "overridden" not in g0

        # plan writes nothing
        assert not os.path.exists(os.path.join(tmp, "AGENTS.md"))


def test_sync_writes_and_exit_code_zero():
    with tempfile.TemporaryDirectory() as tmp:
        _make_repo(tmp)
        r = syncmd.sync(tmp, strategy="newest")
        assert r.summary.written == 4
        assert r.exit_code() == 0
        assert os.path.exists(os.path.join(tmp, "AGENTS.md"))
        with open(os.path.join(tmp, "AGENTS.md")) as f:
            assert f.read() == "rules v1\n"


def test_to_json_round_trips_to_dict():
    import json
    with tempfile.TemporaryDirectory() as tmp:
        _make_repo(tmp)
        r = syncmd.plan(tmp)
        assert json.loads(r.to_json()) == r.to_dict()
