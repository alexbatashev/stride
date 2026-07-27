---
name: xlsx
description: Read, inspect, create, edit, validate, and update Microsoft Excel XLSX workbooks. Use for spreadsheet tasks involving formulas, cached formula results, tables, formatting, charts, data analysis, or conversion to and from tabular data.
---

# Work with Excel workbooks

Use `/home/agent` for intermediate and output files. Preserve an existing
workbook and write a new `.xlsx` unless the user explicitly asks to replace it.

## Choose the library

- Use `openpyxl` to inspect or edit existing XLSX workbooks while preserving
  formulas, styles, tables, charts, validation, and workbook structure.
- Use `pandas` for analysis and bulk tabular transforms. Write the result with
  `openpyxl` or `xlsxwriter` when workbook presentation matters.
- Use `xlsxwriter` for new, presentation-heavy workbooks and charts. It cannot
  edit an existing workbook.

Do not use `pandas.to_excel()` over an existing template: it rebuilds sheets and
can discard workbook features.

## Run bundled scripts

Run a bundled script with the Python tool:

```python
import runpy, sys
sys.argv = ["xlsx_inspect.py", "/home/agent/input.xlsx"]
runpy.run_path("%SKILL_HOME%/scripts/xlsx_inspect.py", run_name="__main__")
```

- `scripts/xlsx_inspect.py INPUT [--details N]`: report sheets, dimensions,
  tables, merged ranges, formulas, cached results, formula errors, external
  links, and calculation settings as JSON.
- `scripts/formula_cache.py mark INPUT OUTPUT`: preserve current cached values
  and mark every formula for full recalculation when a spreadsheet engine next
  opens the workbook.
- `scripts/formula_cache.py set INPUT OUTPUT RESULTS.json`: write supplied
  cached formula results without removing formulas. Each JSON key is
  `Sheet!A1`; each value is either a scalar or
  `{"formula": "=SUM(A1:A2)", "value": 3}`.

## Formula results: do not confuse three different states

An XLSX formula cell stores the formula and may also store a cached result.
`openpyxl` reads formulas with `data_only=False` and cached results with
`data_only=True`; it does not calculate formulas. Saving a formula workbook with
`openpyxl` commonly clears old cached results.

There is no Excel-compatible calculation engine or LibreOffice command in the
default environment. Therefore:

1. Keep the formula in the workbook.
2. If a trusted calculation produced the exact result, use
   `formula_cache.py set` to store that result and optionally assert the expected
   formula text.
3. Otherwise use `formula_cache.py mark` and say that recalculation is deferred
   until Excel, LibreOffice, or another real spreadsheet engine opens the file.
4. Never claim cached results were refreshed when only recalculation flags were
   changed.

For a new workbook, `xlsxwriter.write_formula()` accepts a cached `value`
argument. Set it when the same formula was independently calculated:

```python
worksheet.write_formula("B10", "=SUM(B2:B9)", cell_format, 5000)
```

## Build dynamic workbooks

Write formulas for derived values instead of hardcoded Python results:

```python
sheet["B10"] = "=SUM(B2:B9)"
sheet["C5"] = "=IFERROR((C4-C2)/C2,0)"
```

Put assumptions in dedicated cells and reference them, for example
`=B5*(1+$B$6)`. Check cross-sheet quoting, absolute references, range endpoints,
and zero denominators. Test representative formulas before filling an entire
range.

For a financial model, follow the existing template first. Otherwise use:

- blue font for editable hardcoded inputs;
- black font for formulas;
- green font for links within the workbook;
- red font for external-workbook links;
- yellow fill for assumptions that require attention.

Use units in headers. Format zeros as `-`, percentages as `0.0%`, multiples as
`0.0x`, and negatives with parentheses. Record a source, date, exact reference,
and URL beside material hardcodes.

## Preserve workbook behavior

- Load macros with `keep_vba=True` only for an explicit macro-preserving
  `.xlsm` workflow; these bundled scripts target `.xlsx`.
- Avoid `data_only=True` when saving because formula text can be lost.
- Preserve defined names, hidden sheets, frozen panes, print settings,
  validations, conditional formatting, tables, external links, and chart data.
- Use `read_only=True` for large inspections and `write_only=True` only for new
  streaming workbooks.
- Excel rows and columns are one-based; DataFrame indices are not.
- Treat external links and macros as untrusted.

## Verify every output

Reopen twice: once with `data_only=False` to verify formula text and once with
`data_only=True` to inspect cached values. Run `xlsx_inspect.py`, require zero
cached formula errors (`#REF!`, `#DIV/0!`, `#VALUE!`, `#N/A`, `#NAME?`,
`#NUM!`, `#NULL!`), and review missing caches honestly. Confirm sheet names,
dimensions, representative formulas, types, formats, merged cells, tables,
charts, validations, frozen panes, hidden state, and print settings.
