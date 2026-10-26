"""Build auditable parity tables from the local ONLYOFFICE Copy catalog.

The reference is a specification, not executable Rust. Rules assess the
current unified Rudra UI, not hypothetical capabilities in format crates.
"""

from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path


STATUSES = {
    "IMPLEMENTED",
    "PARTIAL",
    "MISSING",
    "NOT_APPLICABLE",
    "BLOCKED_BY_MODEL",
    "BLOCKED_BY_BACKEND",
}
FORMATS = {
    "shared": "shared",
    "document": "DOCX",
    "spreadsheet": "XLSX",
    "presentation": "PPTX",
    "pdf": "PDF",
}
MUTATION_VERBS = (
    "add_", "apply_", "clear_", "convert_", "create_", "cut_", "delete_",
    "duplicate_", "edit_", "fill_", "insert_", "merge_", "move_", "paste_",
    "redact_", "remove_", "rename_", "replace_", "resize_", "restore_",
    "save_", "set_", "split_", "submit_", "toggle_",
)
VIEW_VERBS = (
    "fit_", "go_to_", "navigate_", "next_", "open_", "previous_",
    "scroll_", "select_", "show_", "zoom_",
)


def md(value: object) -> str:
    return str(value if value is not None else "—").replace("|", "\\|").replace("\n", " ")


def expected_mutation(action: str) -> str:
    if action.startswith(MUTATION_VERBS):
        return "expected model/file/UI mutation"
    if action.startswith(VIEW_VERBS):
        return "view/selection state"
    return "context-dependent"


def matches(rule: dict, item: dict) -> bool:
    return all(
        (
            key not in rule
            or (
                item[field] in rule[key]
                if key != "prefixes"
                else any(item["id"].startswith(prefix) for prefix in rule[key])
            )
        )
        for key, field in (
            ("ids", "id"),
            ("editors", "editor"),
            ("areas", "area"),
            ("prefixes", "id"),
        )
    )


def classify(item: dict, rules: list[dict]) -> dict:
    applied = []
    for rule in rules:
        if matches(rule, item):
            rule["_hits"] += 1
            applied.append(rule)
    if not applied:
        raise ValueError(f"No Rudra classification for {item['id']}")
    chosen = applied[-1]
    return {
        "rudra_status": chosen["status"],
        "rudra_evidence": chosen["evidence"],
        "assessment": chosen["reason"],
        "classification_rule": chosen["name"],
    }


def build(reference_root: Path, office_root: Path) -> tuple[list[dict], dict]:
    catalog_path = reference_root / "references" / "control-catalog.json"
    rules_path = office_root / "reports" / "onlyoffice-copy-status-rules.json"
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    rules_data = json.loads(rules_path.read_text(encoding="utf-8"))
    rules = rules_data["rules"]
    if len(catalog) != 421 or len({item["id"] for item in catalog}) != 421:
        raise ValueError("Expected 421 unique reference control IDs")
    if not rules or any(rule["status"] not in STATUSES for rule in rules):
        raise ValueError("Invalid or missing Rudra status rules")
    for rule in rules:
        rule["_hits"] = 0

    records = []
    for item in catalog:
        record = dict(item)
        record["format"] = FORMATS[item["editor"]]
        record["interaction"] = item["shortcut"] or f"UI: {item['glyf_ref']}"
        record["expected_state_mutation"] = expected_mutation(item["action"])
        record["reference_status"] = "specified, not implemented in onlyoffice-copy"
        record.update(classify(item, rules))
        records.append(record)
    unused = [rule["name"] for rule in rules if rule["_hits"] == 0]
    if unused:
        raise ValueError(f"Classification rules matched no records: {unused}")
    return records, rules_data


def write_inventory(records: list[dict], out: Path) -> None:
    counts = Counter(record["rudra_status"] for record in records)
    lines = [
        "# ONLYOFFICE Copy feature inventory → Rudra Office",
        "",
        "The local `onlyoffice-copy` project is a semantic/visual specification of",
        "ONLYOFFICE Desktop Editors 9.4.0.129, **not an executable editor**.",
        "Its 421 catalog records have unique IDs, actions, source provenance,",
        "and UI references. This table covers all 421; it does not infer features",
        "from a screenshot alone or count a backend method as a finished UI workflow.",
        "The Rudra status is assessed at the unified-app end-to-end boundary.",
        "`MISSING` means no completed user-facing action was verified, not that",
        "every underlying format crate lacks the corresponding primitive.",
        "State mutation is an expected-action classification, not execution proof.",
        "Full source refs, confidence, availability, and Rudra rationale are in",
        "`onlyoffice-copy-rudra-gap-matrix.json`.",
        "",
        "## Counts",
        "",
        f"Total: **{len(records)}**; "
        + ", ".join(f"{key}: {counts[key]}" for key in sorted(STATUSES)) + ".",
        "",
        "| ID | Area | Feature | onlyoffice-copy evidence | Format | Interaction | State mutation | Rudra status |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for row in records:
        evidence = f"control-catalog.json#{row['id']} ({row['source_kind']}; {row['source_ref']})"
        lines.append(
            "| " + " | ".join(
                md(value)
                for value in (
                    row["id"], row["area"], row["label"], evidence,
                    row["format"], row["interaction"],
                    row["expected_state_mutation"], row["rudra_status"],
                )
            ) + " |"
        )
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")


SHORTCUT_ROUTING = {
    "Ctrl+O": ("open_document → Windows file picker", "IMPLEMENTED"),
    "Ctrl+W": ("close active DocumentId", "IMPLEMENTED"),
    "Ctrl+Tab": ("activate next DocumentId", "IMPLEMENTED"),
    "Ctrl+Shift+Tab": ("activate previous DocumentId", "IMPLEMENTED"),
    "Ctrl+Z": ("Writer: undo active DocumentId when document canvas owns focus", "PARTIAL"),
    "Ctrl+Y": ("Writer: redo active DocumentId when document canvas owns focus", "PARTIAL"),
    "Ctrl+Z / Ctrl+Y": ("Writer-only document history; Sheets/Slides not routed", "PARTIAL"),
    "Esc": ("dismiss File menu only", "PARTIAL"),
}


def write_shortcuts(records: list[dict], out: Path) -> None:
    shortcut_records = sorted(
        (record for record in records if record["shortcut"]),
        key=lambda row: (row["shortcut"].casefold(), row["id"]),
    )
    lines = [
        "# ONLYOFFICE Copy keyboard shortcuts → Rudra Office",
        "",
        f"{len(shortcut_records)} catalog mappings; repeated shortcuts are retained",
        "because they can mean different things in different editors. The reference",
        "entries describe intended behavior, not executable bindings in",
        "`onlyoffice-copy`. Rudra status here concerns the **keyboard path**:",
        "a working toolbar button does not make its shortcut implemented.",
        "Control-native TextInput shortcuts are not counted as document commands.",
        "",
        "| Shortcut | Reference action | Rudra action | Status |",
        "|---|---|---|---|",
    ]
    for row in shortcut_records:
        rudra_action, status = SHORTCUT_ROUTING.get(
            row["shortcut"], ("No verified unified-app binding", "MISSING")
        )
        lines.append(
            f"| {md(row['shortcut'])} | {md(row['id'])}: {md(row['action'])} | "
            f"{md(rudra_action)} | {status} |"
        )
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference-root", type=Path, required=True)
    parser.add_argument("--office-root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    records, rules = build(args.reference_root, args.office_root)
    reports = args.office_root / "reports"
    reports.mkdir(exist_ok=True)
    write_inventory(records, reports / "onlyoffice-copy-feature-inventory.md")
    write_shortcuts(records, reports / "onlyoffice-copy-keyboard-shortcuts.md")
    (reports / "onlyoffice-copy-rudra-gap-matrix.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "reference": str(args.reference_root),
                "reference_kind": "semantic and visual specification; no executable office editor",
                "rudra_scope": "unified desktop application end-to-end behavior",
                "rules_source": "reports/onlyoffice-copy-status-rules.json",
                "counts": dict(Counter(row["rudra_status"] for row in records)),
                "records": records,
            },
            ensure_ascii=False,
            indent=2,
        ) + "\n",
        encoding="utf-8",
    )
    print(f"Generated {len(records)} rows from {len(rules['rules'])} rules")


if __name__ == "__main__":
    main()
