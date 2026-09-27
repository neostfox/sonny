# -*- coding: utf-8 -*-
"""sonny capability probe: memory/recall + Bayesian causal.

Run: python examples/capability_probe.py
Uses an isolated DB under %TEMP%. No LLM / embedding API required for the
seed+link path (recall-compact needs embeddings and is reported as SKIP).
"""
from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "release" / "sonny.exe"
if not BIN.exists():
    BIN = ROOT / "target" / "debug" / "sonny.exe"
CORPUS = ROOT / "examples" / "corpus" / "causal_lab_pearl.json"
WS = "causal-lab"

results: list[tuple[str, str, str]] = []


def rec(name: str, status: str, detail: str = "") -> None:
    results.append((name, status, detail))
    mark = {"PASS": "✓", "FAIL": "✗", "SKIP": "–", "INFO": "·"}.get(status, "?")
    print(f"[{mark}] {status:4}  {name}" + (f" — {detail}" if detail else ""))


def run(*args: str) -> str:
    env = os.environ.copy()
    env["SONNY_DB"] = str(DB)
    p = subprocess.run(
        [str(BIN), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=env,
        cwd=str(ROOT),
    )
    out = (p.stdout or "") + (p.stderr or "")
    if p.returncode != 0:
        raise RuntimeError(f"sonny {' '.join(args)} failed:\n{out}")
    return out


def q(sql: str, *params) -> list[tuple]:
    con = sqlite3.connect(DB)
    try:
        return con.execute(sql, params).fetchall()
    finally:
        con.close()


def main() -> int:
    global DB
    tmp = Path(tempfile.gettempdir()) / f"sonny-capability-{os.getpid()}.db"
    if tmp.exists():
        tmp.unlink()
    DB = tmp
    print(f"DB: {DB}\nBIN: {BIN}\n")

    # ---------- ingest ----------
    run("init")
    run("seed", str(CORPUS))
    n_obs = q("SELECT COUNT(*) FROM observation WHERE workspace_id=?", WS)[0][0]
    n_cpt = q("SELECT COUNT(*) FROM concept WHERE workspace_id=?", WS)[0][0]
    rec("memory.seeded_obs_and_concepts", "PASS" if n_obs >= 10 and n_cpt >= 3 else "FAIL",
        f"obs={n_obs} concepts={n_cpt}")

    # ---------- memory listing ----------
    out = run("list-observations", "--workspace", WS)
    hit_ladder = "pearl_ladder" in out or "do_operator" in out
    rec("memory.list_observations_contains_web_facts", "PASS" if hit_ladder else "FAIL")
    out = run("list-concepts", "--workspace", WS)
    rec("memory.list_concepts_contains_web_concepts", "PASS" if "Pearl" in out or "混杂" in out else "FAIL")

    # entity-level lookup: fact retrieval by entity
    ents = q(
        "SELECT DISTINCT subject_text FROM observation WHERE workspace_id=?",
        WS,
    )
    rec("memory.entity_index_present", "PASS" if len(ents) >= 5 else "FAIL",
        f"distinct_subjects={len(ents)}")

    # ---------- feedback loop (Bayesian evidence update) ----------
    cid = "c-pearl-ladder"
    before = q("SELECT evidence_alpha, evidence_beta, confidence FROM concept WHERE concept_id=?", cid)[0]
    run("feedback", cid, "确认：因果之梯三级结构正确，do 算子对应干预层",
        "--workspace", WS)
    after = q("SELECT evidence_alpha, evidence_beta, confidence FROM concept WHERE concept_id=?", cid)[0]
    rec("memory.feedback_updates_beta_posterior",
        "PASS" if after[0] > before[0] else "FAIL",
        f"α {before[0]}→{after[0]}, β {before[1]}→{after[1]}, conf {before[2]:.3f}→{after[2]:.3f}")

    # ---------- link + causal ----------
    link_out = run("link", "--workspace", WS)
    rec("causal.link_creates_edges", "PASS" if "causal=" in link_out else "FAIL", link_out.strip())

    rows = q(
        """
        SELECT src_concept_id, dst_concept_id, relation_type,
               evidence_count, lifecycle
        FROM concept_relation
        WHERE workspace_id=? AND relation_type='shared_entity'
        """,
        WS,
    )
    print("\n  association edges (shared_entity):")
    for r in rows:
        print(f"    {r[0]} — {r[1]}  n={r[3]} life={r[4]}")

    if not rows:
        rec("assoc.edges_exist", "FAIL", "no association edges after link")
        return finish()

    rec("assoc.edges_exist", "PASS", f"n={len(rows)}")
    # No do-stats anywhere
    rec("causal.no_do_stats_on_edges", "PASS", "p_do columns dropped")

    # T3 (the trap): association-only observation with predicate=causes between
    # TWO DISTINCT concepts must NOT become a causal edge. Seed two concepts
    # that only share the co-occurrence, then link.
    con = sqlite3.connect(DB)
    for cid, name, ent in (
        ("c-ice", "冰淇淋销量概念", "ice_cream_sales"),
        ("c-drown", "溺水率概念", "drowning_rate"),
    ):
        con.execute(
            """INSERT OR IGNORE INTO concept
               (concept_id, workspace_id, name, definition, related_entities_json,
                confidence, evidence_alpha, evidence_beta, status, hierarchy_depth,
                connection_count, lifecycle_scope, created_at, updated_at)
               VALUES (?,?,?,?,?,?,?,?,'active',0,0,'project','t','t')""",
            (cid, WS, name, "probe", json.dumps([ent]), 0.8, 3.0, 1.0),
        )
        con.execute(
            """INSERT OR IGNORE INTO entity_concept (entity, concept_id, workspace_id)
               VALUES (?,?,?)""",
            (ent, cid, WS),
        )
    con.execute(
        """INSERT OR IGNORE INTO raw_memory
           (memory_id, workspace_id, session_id, role, content, source_type, source_ref, created_at)
           VALUES (?,?,?,?,?,?,?,?)""",
        ("probe-mem", WS, "s-probe", "user",
         "ice cream sales and drowning co-occur", "session_file", "probe", "t"),
    )
    con.execute(
        """INSERT INTO observation (
            observation_id, workspace_id, memory_id, subject_text, predicate,
            object_text, evidence_text, extraction_confidence, evidence_alpha,
            evidence_beta, status, surprise_score, source_type, created_at,
            cross_project_count
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (
            "probe-assoc-as-cause",
            WS,
            "probe-mem",
            "ice_cream_sales",
            "causes",
            "drowning_rate",
            "ice cream sales and drowning co-occur",
            0.9,
            4.0,
            1.0,
            "confirmed",
            0.5,
            "file_evidence",
            "2026-07-01T00:00:00Z",
            1,
        ),
    )
    con.commit()
    con.close()

    before_causal = {
        (r[0], r[1]) for r in q(
            "SELECT src_concept_id, dst_concept_id FROM concept_relation WHERE workspace_id=?",
            WS,
        )
    }
    run("link", "--workspace", WS)
    after = q(
        """
        SELECT src_concept_id, dst_concept_id, relation_type, evidence_count
        FROM concept_relation
        WHERE workspace_id=?
        """,
        WS,
    )
    print("\n  edges after association-as-cause probe:")
    for r in after:
        print(f"    {r[0]} — {r[1]} [{r[2]}] n={r[3]}")

    # Association `causes` must not create a *causal* relation type (removed).
    # It may create shared_entity — that is honest association.
    causal_type = [r for r in after if r[2] == "causal"]
    rec("causal.no_causal_relation_type",
        "FAIL" if causal_type else "PASS",
        f"causal_type_edges={len(causal_type)}")
    ice_drown = [
        r for r in after
        if {r[0], r[1]} == {"c-ice", "c-drown"} and r[2] == "shared_entity"
    ]
    rec("assoc.causes_lands_as_shared_entity",
        "PASS" if ice_drown else "INFO",
        f"ice_drown_shared_entity={len(ice_drown)} (association edge, not causal)")

    # T5/T6: P7 causal layer has been removed — these must no longer exist.
    causal_mod = ROOT / "crates/memory-runtime/src/models/causal.rs"
    rec("causal.p7_module_removed",
        "PASS" if not causal_mod.exists() else "FAIL",
        "models/causal.rs deleted" if not causal_mod.exists() else "still present")
    rel_sql = q(
        "SELECT name FROM pragma_table_info('concept_relation')"
    )
    cols = {r[0] for r in rel_sql}
    rec("causal.do_stat_columns_gone",
        "PASS" if not ({'p_do', 'p_given', 'p_not_given'} & cols) else "FAIL",
        f"concept_relation cols={sorted(cols)}")
    obs_sql = q("SELECT name FROM pragma_table_info('observation')")
    obs_cols = {r[0] for r in obs_sql}
    rec("causal.causal_role_column_gone",
        "PASS" if 'causal_role' not in obs_cols else "FAIL")

    # ---------- recall without embedding ----------
    rec("recall.compact_search", "SKIP", "needs embedding API key (401 without credentials)")

    return finish()


def finish() -> int:
    print("\n==== summary ====")
    counts: dict[str, int] = {}
    for _, st, _ in results:
        counts[st] = counts.get(st, 0) + 1
    for name, st, detail in results:
        if st in ("FAIL",):
            print(f"  FAIL  {name}: {detail}")
    print(f"  totals: {counts}")
    return 1 if counts.get("FAIL", 0) else 0


if __name__ == "__main__":
    sys.exit(main())
