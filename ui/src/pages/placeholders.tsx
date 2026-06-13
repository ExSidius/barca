import { Activity, Layers, Clock, Book } from 'lucide-react'
import { EmptyState } from './EmptyState'

export function RunsPage() {
  return <EmptyState icon={Activity} title="Runs" note="run history · the graph is the good part" />
}

export function AssetsPage() {
  return <EmptyState icon={Layers} title="Asset catalog" note="browse + search assets · coming soon" />
}

export function SchedulesPage() {
  return <EmptyState icon={Clock} title="Schedules" note="cron + sensors · coming soon" />
}

export function DocsPage() {
  return <EmptyState icon={Book} title="Docs" note="getting started · coming soon" />
}
