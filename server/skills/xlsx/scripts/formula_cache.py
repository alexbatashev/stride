import argparse
import json
import math
import os
import posixpath
import re
import tempfile
import zipfile
from pathlib import Path

from lxml import etree as ET


SHEET_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
OFFICE_REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PACKAGE_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
CELL_RE = re.compile(r"^[A-Z]{1,3}[1-9][0-9]*$")
ERRORS = {
    "#NULL!",
    "#DIV/0!",
    "#VALUE!",
    "#REF!",
    "#NAME?",
    "#NUM!",
    "#N/A",
    "#GETTING_DATA",
    "#SPILL!",
    "#CALC!",
    "#FIELD!",
    "#BLOCKED!",
    "#UNKNOWN!",
    "#CONNECT!",
    "#BUSY!",
    "#PYTHON!",
}


def qname(namespace, name):
    return f"{{{namespace}}}{name}"


def parse_location(location):
    if "!" not in location:
        raise SystemExit(f"invalid formula result key: {location}")
    sheet, cell = location.rsplit("!", 1)
    if sheet.startswith("'") and sheet.endswith("'"):
        sheet = sheet[1:-1].replace("''", "'")
    cell = cell.upper()
    if not sheet or not CELL_RE.fullmatch(cell):
        raise SystemExit(f"invalid formula result key: {location}")
    return sheet, cell


def parse_results(path):
    raw = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise SystemExit("results must be a JSON object")
    results = {}
    for location, item in raw.items():
        sheet, cell = parse_location(location)
        expected = None
        value = item
        if isinstance(item, dict):
            if "value" not in item:
                raise SystemExit(f"{location}: result object is missing value")
            value = item["value"]
            expected = item.get("formula")
            if expected is not None and not isinstance(expected, str):
                raise SystemExit(f"{location}: formula must be a string")
        if value is not None and not isinstance(value, (str, int, float, bool)):
            raise SystemExit(f"{location}: cached value must be a JSON scalar")
        if isinstance(value, float) and not math.isfinite(value):
            raise SystemExit(f"{location}: cached number must be finite")
        results[(sheet, cell)] = (expected, value)
    if not results:
        raise SystemExit("results object is empty")
    return results


def workbook_parts(archive):
    workbook = ET.fromstring(archive.read("xl/workbook.xml"))
    if workbook.tag != qname(SHEET_NS, "workbook"):
        raise SystemExit("only transitional XLSX workbooks are supported")
    relationships = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
    targets = {
        item.attrib["Id"]: item.attrib["Target"]
        for item in relationships.findall(qname(PACKAGE_REL_NS, "Relationship"))
    }
    sheets = {}
    for item in workbook.find(qname(SHEET_NS, "sheets")):
        relation = item.attrib[qname(OFFICE_REL_NS, "id")]
        target = targets[relation]
        part = (
            target.lstrip("/")
            if target.startswith("/")
            else posixpath.normpath(posixpath.join("xl", target))
        )
        sheets[item.attrib["name"]] = part
    return workbook, sheets


def set_calculation(workbook):
    calculation = workbook.find(qname(SHEET_NS, "calcPr"))
    if calculation is None:
        calculation = ET.SubElement(workbook, qname(SHEET_NS, "calcPr"))
    calculation.set("calcMode", "auto")
    calculation.set("fullCalcOnLoad", "1")
    calculation.set("forceFullCalc", "1")


def normalize_formula(value):
    return value.removeprefix("=").strip()


def set_cached_value(cell, value):
    value_element = cell.find(qname(SHEET_NS, "v"))
    formula = cell.find(qname(SHEET_NS, "f"))
    if value_element is None:
        value_element = ET.Element(qname(SHEET_NS, "v"))
        children = list(cell)
        cell.insert(children.index(formula) + 1, value_element)
    if value is None:
        cell.attrib.pop("t", None)
        value_element.text = None
    elif isinstance(value, bool):
        cell.set("t", "b")
        value_element.text = "1" if value else "0"
    elif isinstance(value, (int, float)):
        cell.attrib.pop("t", None)
        value_element.text = str(value)
    elif value in ERRORS:
        cell.set("t", "e")
        value_element.text = value
    else:
        cell.set("t", "str")
        value_element.text = value


def update_sheet(xml, updates, sheet_name):
    root = ET.fromstring(xml)
    cells = {
        cell.attrib["r"]: cell
        for cell in root.iter(qname(SHEET_NS, "c"))
        if "r" in cell.attrib
    }
    for coordinate, (expected, value) in updates.items():
        cell = cells.get(coordinate)
        if cell is None:
            raise SystemExit(f"{sheet_name}!{coordinate}: cell does not exist")
        formula = cell.find(qname(SHEET_NS, "f"))
        if formula is None:
            raise SystemExit(
                f"{sheet_name}!{coordinate}: cell does not contain a formula"
            )
        if expected is not None and normalize_formula(expected) != normalize_formula(
            formula.text or ""
        ):
            raise SystemExit(
                f"{sheet_name}!{coordinate}: formula differs from expected value"
            )
        set_cached_value(cell, value)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def write_workbook(source, output, results):
    source = Path(source)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with zipfile.ZipFile(source) as archive:
            workbook, sheet_parts = workbook_parts(archive)
            updates = {}
            if results:
                grouped = {}
                for (sheet, cell), result in results.items():
                    if sheet not in sheet_parts:
                        raise SystemExit(f"unknown sheet: {sheet}")
                    grouped.setdefault(sheet, {})[cell] = result
                for sheet, values in grouped.items():
                    part = sheet_parts[sheet]
                    updates[part] = update_sheet(archive.read(part), values, sheet)
            set_calculation(workbook)
            updates["xl/workbook.xml"] = ET.tostring(
                workbook, encoding="utf-8", xml_declaration=True
            )
            with tempfile.NamedTemporaryFile(
                dir=output.parent,
                prefix=f".{output.name}.",
                suffix=".tmp",
                delete=False,
            ) as handle:
                temporary = Path(handle.name)
            with zipfile.ZipFile(temporary, "w") as target:
                for info in archive.infolist():
                    target.writestr(
                        info, updates.get(info.filename, archive.read(info.filename))
                    )
        with zipfile.ZipFile(temporary) as check:
            ET.fromstring(check.read("xl/workbook.xml"))
        os.replace(temporary, output)
    finally:
        if temporary and temporary.exists():
            temporary.unlink()


def main():
    parser = argparse.ArgumentParser(
        description="Preserve XLSX formulas while marking recalculation or setting caches."
    )
    commands = parser.add_subparsers(dest="command", required=True)

    mark = commands.add_parser("mark")
    mark.add_argument("input")
    mark.add_argument("output")

    set_parser = commands.add_parser("set")
    set_parser.add_argument("input")
    set_parser.add_argument("output")
    set_parser.add_argument("results")

    args = parser.parse_args()
    results = parse_results(args.results) if args.command == "set" else None
    write_workbook(args.input, args.output, results)
    print(args.output)


if __name__ == "__main__":
    main()
