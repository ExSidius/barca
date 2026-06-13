import { createBrowserRouter, Navigate } from 'react-router'
import { AppShell } from '@/layouts/AppShell'
import { GraphPage } from '@/pages/GraphPage'
import { RunsPage, AssetsPage, SchedulesPage, DocsPage } from '@/pages/placeholders'

export const router = createBrowserRouter([
  {
    path: '/',
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/graph" replace /> },
      { path: 'graph', element: <GraphPage /> },
      { path: 'runs', element: <RunsPage /> },
      { path: 'assets', element: <AssetsPage /> },
      { path: 'schedules', element: <SchedulesPage /> },
      { path: 'docs', element: <DocsPage /> },
    ],
  },
])
