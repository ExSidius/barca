"""Rich-formatted display functions for CLI output."""

from __future__ import annotations

from pathlib import Path

from rich import box
from rich.console import Group, RenderableType
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from barca._models import (
    AssetDetail,
    AssetSummary,
    JobDetail,
    ReindexDiff,
    RunPassResult,
    SensorObservation,
)

# --- Style guide constants ---

STATUS_STYLES: dict[str, str] = {
    "success": "green",
    "done": "green",
    "failed": "bold red",
    "running": "yellow",
    "never run": "dim",
}

KIND_STYLES: dict[str, str] = {
    "asset": "blue",
    "sensor": "magenta",
    "effect": "cyan",
    "sink": "bright_blue",
}


def _status_text(status: str | None) -> Text:
    s = status or "never run"
    return Text(s, style=STATUS_STYLES.get(s, ""))


def _kind_text(kind: str) -> Text:
    return Text(kind, style=KIND_STYLES.get(kind, ""))


def _freshness_text(freshness: str) -> Text:
    if freshness.startswith("schedule:"):
        return Text(f"schedule({freshness[len('schedule:') :]})", style="yellow")
    if freshness == "manual":
        return Text("manual", style="dim")
    if freshness == "always":
        return Text("always", style="")
    return Text(freshness, style="dim")


def _kv_table() -> Table:
    t = Table(box=box.SIMPLE, show_header=False, padding=(0, 1))
    t.add_column("label", style="bold", justify="right", no_wrap=True)
    t.add_column("value")
    return t


def assets_table(assets: list[AssetSummary]) -> RenderableType:
    if not assets:
        return Text("No assets indexed.", style="dim italic")

    t = Table(box=box.SIMPLE_HEAD, show_edge=False)
    t.add_column("ID", style="dim", justify="right", no_wrap=True)
    t.add_column("Kind", no_wrap=True)
    t.add_column("Name")
    t.add_column("Module", style="dim")
    t.add_column("Function")
    t.add_column("Freshness", style="dim")
    t.add_column("Status")

    for a in assets:
        fn_cell = a.function_name
        if a.purity == "unsafe":
            fn_cell = f"{a.function_name} [unsafe]"
        t.add_row(
            str(a.asset_id),
            _kind_text(a.kind),
            a.logical_name,
            a.module_path,
            fn_cell if a.purity != "unsafe" else Text(fn_cell, style="yellow"),
            _freshness_text(a.freshness),
            _status_text(a.materialization_status),
        )
    return t


def asset_detail(detail: AssetDetail, repo_root: Path | None = None) -> RenderableType:
    a = detail.asset
    kv = _kv_table()
    kv.add_row("Name", a.logical_name)
    kv.add_row("Kind", _kind_text(a.kind))
    if a.purity == "unsafe":
        kv.add_row("Purity", Text("[unsafe]", style="yellow bold"))
    kv.add_row("Module", Text(a.module_path, style="dim"))
    kv.add_row("File", Text(a.file_path, style="dim"))
    kv.add_row("Function", a.function_name)
    kv.add_row("Definition hash", Text(a.definition_hash, style="dim"))
    kv.add_row("Serializer", Text(a.serializer_kind, style="dim"))
    if a.return_type:
        kv.add_row("Return type", Text(a.return_type, style="dim"))
    if a.sink_path:
        kv.add_row("Sink path", Text(a.sink_path, style="bright_blue"))

    if a.kind == "sensor":
        if detail.latest_observation:
            obs = detail.latest_observation
            update_style = "green" if obs.update_detected else "dim"
            kv.add_row(
                "Last observation",
                Text(
                    f"#{obs.observation_id} (update_detected: {obs.update_detected})",
                    style=update_style,
                ),
            )
            if obs.output_json:
                truncated = obs.output_json[:80] + ("..." if len(obs.output_json) > 80 else "")
                kv.add_row("Output", Text(truncated, style="dim"))
            kv.add_row("Observed at", Text(str(obs.created_at), style="dim"))
        else:
            kv.add_row("Last observation", Text("none", style="dim"))

    artifact_abs: str | None = None
    if detail.latest_materialization:
        m = detail.latest_materialization
        kv.add_row(
            "Last job",
            Text(f"#{m.materialization_id} ({m.status})", style=STATUS_STYLES.get(m.status, "")),
        )
        if m.stale_inputs_used:
            kv.add_row("", Text("⚠ stale inputs were used", style="yellow"))
        if m.last_error:
            kv.add_row("Error", Text(m.last_error, style="bold red"))
        if m.artifact_path:
            artifact_abs = str(repo_root / m.artifact_path) if repo_root is not None else m.artifact_path
    else:
        kv.add_row("Last job", Text("none", style="dim"))

    kind_label = (a.kind or "asset").capitalize()
    panel = Panel(
        kv,
        title=f"[bold]{kind_label} #{a.asset_id}[/bold]",
        box=box.ROUNDED,
        expand=False,
    )
    if artifact_abs:
        return Group(panel, Text(f"  {artifact_abs}", style="dim"))
    return panel


def jobs_table(jobs: list[JobDetail]) -> RenderableType:
    if not jobs:
        return Text("No jobs found.", style="dim italic")

    t = Table(box=box.SIMPLE_HEAD, show_edge=False)
    t.add_column("Job ID", style="dim", justify="right", no_wrap=True)
    t.add_column("Asset")
    t.add_column("Status")
    t.add_column("Run Hash", style="dim", no_wrap=True)

    for j in jobs:
        t.add_row(
            str(j.job.materialization_id),
            j.asset.function_name,
            _status_text(j.job.status),
            j.job.run_hash[:12],
        )
    return t


def job_detail(detail: JobDetail) -> RenderableType:
    j = detail.job
    a = detail.asset
    kv = _kv_table()
    kv.add_row("Asset", f"{a.function_name} (#{a.asset_id})")
    kv.add_row("Status", _status_text(j.status))
    kv.add_row("Run hash", Text(j.run_hash, style="dim"))
    if j.stale_inputs_used:
        kv.add_row("", Text("⚠ stale inputs were used", style="yellow"))
    if j.artifact_path:
        kv.add_row("Artifact", Text(j.artifact_path, style="dim"))
    if j.last_error:
        kv.add_row("Error", Text(j.last_error, style="bold red"))
    return Panel(
        kv,
        title=f"[bold]Job #{j.materialization_id}[/bold]",
        box=box.ROUNDED,
        expand=False,
    )


def reindex_diff_panel(diff: ReindexDiff) -> RenderableType:
    """Show the three-way diff from a reindex: added / removed / renamed."""
    lines: list[Text] = []
    if diff.added:
        lines.append(Text(f"Added ({len(diff.added)}):", style="bold green"))
        for name in diff.added:
            lines.append(Text(f"  + {name}", style="green"))
    if diff.removed:
        lines.append(Text(f"Removed ({len(diff.removed)}):", style="bold red"))
        for name in diff.removed:
            lines.append(Text(f"  - {name}", style="red"))
    if diff.renamed:
        lines.append(Text(f"Renamed ({len(diff.renamed)}):", style="bold yellow"))
        for old, new in diff.renamed:
            lines.append(Text(f"  {old}  →  {new}", style="yellow"))
    if not lines:
        return Text("No changes since last index.", style="dim italic")
    return Group(*lines)


def run_pass_summary(result: RunPassResult) -> RenderableType:
    kv = _kv_table()
    if result.executed_sensors:
        kv.add_row("Sensors executed", Text(str(result.executed_sensors), style="green"))
    if result.executed_assets:
        kv.add_row("Assets executed", Text(str(result.executed_assets), style="green"))
    if result.executed_effects:
        kv.add_row("Effects executed", Text(str(result.executed_effects), style="green"))
    if result.executed_sinks:
        kv.add_row("Sinks written", Text(str(result.executed_sinks), style="green"))
    if result.fresh:
        kv.add_row("Fresh (skipped)", Text(str(result.fresh), style="dim"))
    if result.manual_skipped:
        kv.add_row("Manual (skipped)", Text(str(result.manual_skipped), style="dim"))
    if result.stale_blocked:
        kv.add_row("Blocked by manual upstream", Text(str(result.stale_blocked), style="yellow"))
    if result.failed:
        kv.add_row("Failed", Text(str(result.failed), style="bold red"))
    if result.sink_failed:
        kv.add_row("Sinks failed", Text(str(result.sink_failed), style="bold red"))

    total = (
        result.executed_sensors
        + result.executed_assets
        + result.executed_effects
        + result.executed_sinks
        + result.fresh
        + result.manual_skipped
        + result.stale_blocked
        + result.failed
        + result.sink_failed
    )
    if total == 0:
        kv.add_row("", Text("No nodes found.", style="dim italic"))

    return Panel(kv, title="[bold]Run Pass[/bold]", box=box.ROUNDED, expand=False)


def sensor_observations_table(observations: list[SensorObservation]) -> RenderableType:
    if not observations:
        return Text("No observations recorded.", style="dim italic")

    t = Table(box=box.SIMPLE_HEAD, show_edge=False)
    t.add_column("ID", style="dim", justify="right", no_wrap=True)
    t.add_column("Update Detected", no_wrap=True)
    t.add_column("Output")
    t.add_column("Observed At", style="dim", no_wrap=True)

    for obs in observations:
        output = obs.output_json or ""
        if len(output) > 60:
            output = output[:57] + "..."
        update_style = "green" if obs.update_detected else "dim"
        t.add_row(
            str(obs.observation_id),
            Text(str(obs.update_detected), style=update_style),
            Text(output, style="dim"),
            Text(str(obs.created_at), style="dim"),
        )
    return t
