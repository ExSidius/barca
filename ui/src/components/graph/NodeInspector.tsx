import { X, Download, Play, Terminal } from 'lucide-react'
import { Button, IconButton, StatusBadge, Tag, StatusDot, LogViewer } from '@/components'
import { useTriggerGet } from '@/hooks/useTriggerGet'
import { useTriggerRun } from '@/hooks/useTriggerRun'
import { freshnessLabel } from '@/lib/status'
import { shortName } from '@/lib/graph'
import type { AssetSummary, StatusKind, LogLine } from '@/lib/types'

interface NodeInspectorProps {
  asset: AssetSummary
  /** Live visual status for this node. */
  status: StatusKind
  /** Captured log lines for the active run (run-wide). */
  logs: LogLine[]
  /** Whether a run is currently streaming. */
  running: boolean
  /** Fired when a get/run is triggered, with the run handle + target node id. */
  onTrigger: (handle: string, nodeId: string) => void
  onClose: () => void
}

export function NodeInspector({
  asset,
  status,
  logs,
  running,
  onTrigger,
  onClose,
}: NodeInspectorProps) {
  const getTrigger = useTriggerGet()
  const runTrigger = useTriggerRun()
  const name = shortName(asset.id)

  // Canonical barca verbs: `run` a task (always re-executes), `get` an asset or
  // sensor (cache-aware). No "materialize".
  const isTask = asset.kind === 'task'
  const trigger = isTask ? runTrigger : getTrigger
  const verb = isTask ? 'run' : 'get'

  const onFire = () => {
    trigger.mutate(name, {
      onSuccess: (data) => onTrigger(data.run_id, asset.id),
    })
  }

  const showLogs = running || logs.length > 0

  return (
    <div className="barca-inspector">
      <div className="barca-insp-head">
        <div className="barca-insp-title">
          <StatusDot status={status} size={8} />
          <span>{name}</span>
        </div>
        <IconButton label="Close" size="sm" onClick={onClose}>
          <X size={14} />
        </IconButton>
      </div>

      <div className="barca-insp-body">
        <div className="barca-tagrow">
          <Tag tone="signal" dot>
            {asset.kind}
          </Tag>
          <StatusBadge status={status} size="sm" />
        </div>

        <div className="barca-insp-kv">
          <div className="barca-insp-kv-row">
            <span>id</span>
            <span>{asset.id}</span>
          </div>
          <div className="barca-insp-kv-row">
            <span>kind</span>
            <span>{asset.kind}</span>
          </div>
          <div className="barca-insp-kv-row">
            <span>freshness</span>
            <span>{freshnessLabel(asset.freshness)}</span>
          </div>
          <div className="barca-insp-kv-row">
            <span>inputs</span>
            <span>{asset.inputs.length}</span>
          </div>
        </div>

        {asset.inputs.length > 0 && (
          <div className="barca-insp-deps">
            {asset.inputs.map((input) => (
              <span className="barca-insp-dep" key={input}>
                {shortName(input)}
              </span>
            ))}
          </div>
        )}

        <div className="barca-insp-actions">
          <Button
            variant="signal"
            size="sm"
            iconLeft={isTask ? <Play size={12} /> : <Download size={12} />}
            loading={trigger.isPending || running}
            onClick={onFire}
          >
            {verb}
          </Button>
          <Button variant="ghost" size="sm" iconLeft={<Terminal size={13} />}>
            logs
          </Button>
        </div>

        {showLogs && (
          <div className="barca-insp-logs">
            <div className="barca-insp-logs-head">
              <Terminal size={12} />
              <span>output</span>
              {running && <span className="barca-insp-logs-live">live</span>}
            </div>
            <LogViewer lines={logs} live={running} height={220} />
          </div>
        )}

        {trigger.isError && (
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--status-failed)' }}>
            {(trigger.error as Error).message}
          </span>
        )}
      </div>
    </div>
  )
}
