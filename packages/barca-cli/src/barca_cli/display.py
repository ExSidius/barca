"""Table formatting for CLI output."""

from __future__ import annotations

from barca._models import AssetDetail, AssetSummary, JobDetail


def assets_table(assets: list[AssetSummary]) -> str:
    if not assets:
        return "No assets indexed."

    headers = ["ID", "Name", "Module", "Function", "Status"]
    rows = []
    for a in assets:
        status = a.materialization_status or "never run"
        rows.append([str(a.asset_id), a.logical_name, a.module_path, a.function_name, status])

    return _format_table(headers, rows)


def asset_detail(detail: AssetDetail) -> str:
    a = detail.asset
    lines = [
        f"Asset #{a.asset_id}",
        f"  Name:            {a.logical_name}",
        f"  Module:          {a.module_path}",
        f"  File:            {a.file_path}",
        f"  Function:        {a.function_name}",
        f"  Definition hash: {a.definition_hash}",
        f"  Serializer:      {a.serializer_kind}",
    ]
    if a.return_type:
        lines.append(f"  Return type:     {a.return_type}")
    if detail.latest_materialization:
        m = detail.latest_materialization
        lines.append(f"  Last job:        #{m.materialization_id} ({m.status})")
        if m.last_error:
            lines.append(f"  Error:           {m.last_error}")
    else:
        lines.append("  Last job:        none")
    return "\n".join(lines)


def jobs_table(jobs: list[JobDetail]) -> str:
    if not jobs:
        return "No jobs found."

    headers = ["Job ID", "Asset", "Status", "Run Hash"]
    rows = []
    for j in jobs:
        short_hash = j.job.run_hash[:12]
        rows.append([str(j.job.materialization_id), j.asset.function_name, j.job.status, short_hash])

    return _format_table(headers, rows)


def job_detail(detail: JobDetail) -> str:
    j = detail.job
    a = detail.asset
    lines = [
        f"Job #{j.materialization_id}",
        f"  Asset:     {a.function_name} (#{a.asset_id}) ",
        f"  Status:    {j.status}",
        f"  Run hash:  {j.run_hash}",
    ]
    if j.artifact_path:
        lines.append(f"  Artifact:  {j.artifact_path}")
    if j.last_error:
        lines.append(f"  Error:     {j.last_error}")
    return "\n".join(lines)


def _format_table(headers: list[str], rows: list[list[str]]) -> str:
    """Simple aligned table formatter."""
    all_rows = [headers] + rows
    widths = [max(len(row[i]) for row in all_rows) for i in range(len(headers))]

    def fmt_row(row: list[str]) -> str:
        return " | ".join(cell.ljust(widths[i]) for i, cell in enumerate(row))

    lines = [fmt_row(headers)]
    lines.append("-+-".join("-" * w for w in widths))
    for row in rows:
        lines.append(fmt_row(row))
    return "\n".join(lines)
