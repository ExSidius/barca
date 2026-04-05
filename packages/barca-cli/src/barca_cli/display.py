"""Table formatting for CLI output."""

from __future__ import annotations

from barca._models import AssetDetail, AssetSummary, EffectExecution, JobDetail, ReconcileResult, SensorObservation


def assets_table(assets: list[AssetSummary]) -> str:
    if not assets:
        return "No assets indexed."

    headers = ["ID", "Kind", "Name", "Module", "Function", "Schedule", "Status"]
    rows = []
    for a in assets:
        status = a.materialization_status or "never run"
        rows.append([
            str(a.asset_id), a.kind, a.logical_name, a.module_path,
            a.function_name, a.schedule, status,
        ])

    return _format_table(headers, rows)


def asset_detail(detail: AssetDetail) -> str:
    a = detail.asset
    kind_label = a.kind.capitalize() if a.kind else "Asset"
    lines = [
        f"{kind_label} #{a.asset_id}",
        f"  Name:            {a.logical_name}",
        f"  Kind:            {a.kind}",
        f"  Module:          {a.module_path}",
        f"  File:            {a.file_path}",
        f"  Function:        {a.function_name}",
        f"  Definition hash: {a.definition_hash}",
        f"  Serializer:      {a.serializer_kind}",
    ]
    if a.return_type:
        lines.append(f"  Return type:     {a.return_type}")
    if a.kind == "sensor":
        if detail.latest_observation:
            obs = detail.latest_observation
            lines.append(f"  Last observation: #{obs.observation_id} (update_detected: {obs.update_detected})")
            if obs.output_json:
                truncated = obs.output_json[:80] + ("..." if len(obs.output_json) > 80 else "")
                lines.append(f"  Output:          {truncated}")
            lines.append(f"  Observed at:     {obs.created_at}")
        else:
            lines.append("  Last observation: none")
    elif a.kind == "effect":
        if detail.latest_execution:
            ex = detail.latest_execution
            lines.append(f"  Last execution:  #{ex.execution_id} ({ex.status})")
            if ex.last_error:
                lines.append(f"  Error:           {ex.last_error}")
            lines.append(f"  Executed at:     {ex.created_at}")
        else:
            lines.append("  Last execution:  none")
    elif detail.latest_materialization:
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


def reconcile_summary(result: ReconcileResult) -> str:
    lines = ["Reconcile complete:"]
    if result.executed_sensors:
        lines.append(f"  Sensors executed:  {result.executed_sensors}")
    if result.executed_assets:
        lines.append(f"  Assets executed:   {result.executed_assets}")
    if result.executed_effects:
        lines.append(f"  Effects executed:  {result.executed_effects}")
    if result.fresh:
        lines.append(f"  Fresh (skipped):   {result.fresh}")
    if result.stale_waiting:
        lines.append(f"  Stale (waiting):   {result.stale_waiting}")
    if result.failed:
        lines.append(f"  Failed:            {result.failed}")
    total = (result.executed_sensors + result.executed_assets + result.executed_effects
             + result.fresh + result.stale_waiting + result.failed)
    if total == 0:
        lines.append("  No nodes found.")
    return "\n".join(lines)


def sensor_observations_table(observations: list[SensorObservation]) -> str:
    if not observations:
        return "No observations recorded."

    headers = ["ID", "Update Detected", "Output", "Observed At"]
    rows = []
    for obs in observations:
        output = obs.output_json or ""
        if len(output) > 60:
            output = output[:57] + "..."
        rows.append([
            str(obs.observation_id),
            str(obs.update_detected),
            output,
            str(obs.created_at),
        ])

    return _format_table(headers, rows)


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
