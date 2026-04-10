import { BrowserRouter, Routes, Route, NavLink, Navigate } from "react-router-dom";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useAction } from "@/hooks/useAction";
import { Dashboard } from "@/pages/Dashboard";
import { Assets } from "@/pages/Assets";
import { AssetDetail } from "@/pages/AssetDetail";
import { Jobs } from "@/pages/Jobs";
import { JobDetail } from "@/pages/JobDetail";
import { Sensors } from "@/pages/Sensors";
import { SensorDetail } from "@/pages/SensorDetail";

function LayoutDashboardIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <rect x="3" y="3" width="7" height="7" rx="1" /><rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" /><rect x="14" y="14" width="7" height="7" rx="1" />
    </svg>
  );
}
function PackageIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
    </svg>
  );
}
function ClockIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="10" /><polyline points="12 6 12 12 16 14" />
    </svg>
  );
}
function RadioIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M2 12h4M18 12h4M12 2v4M12 18v4" /><circle cx="12" cy="12" r="4" />
    </svg>
  );
}
function MenuIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <line x1="3" y1="6" x2="21" y2="6" /><line x1="3" y1="12" x2="21" y2="12" /><line x1="3" y1="18" x2="21" y2="18" />
    </svg>
  );
}
function RefreshIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <path d="M23 4v6h-6" /><path d="M1 20v-6h6" />
      <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
    </svg>
  );
}
function HelpIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
      <circle cx="12" cy="12" r="10" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01" />
    </svg>
  );
}

function AppShell() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const reconcile = useAction("/api/reconcile");

  return (
    <div className="flex h-screen overflow-hidden bg-background">
      {/* Sidebar */}
      <aside
        className={`flex flex-col border-r border-border bg-background transition-all duration-200 overflow-hidden ${
          sidebarOpen ? "w-[220px] min-w-[220px]" : "w-[52px] min-w-[52px]"
        }`}
      >
        <div className="flex h-12 items-center gap-2 border-b border-border px-4 shrink-0">
          <LayoutDashboardIcon />
          {sidebarOpen && (
            <span className="text-sm font-semibold text-foreground">Barca</span>
          )}
        </div>

        <nav className="flex flex-1 flex-col gap-0.5 p-2 overflow-y-auto">
          <SidebarLink to="/ui" icon={<LayoutDashboardIcon />} label="Dashboard" collapsed={!sidebarOpen} end />
          <SidebarLink to="/ui/assets" icon={<PackageIcon />} label="Assets" collapsed={!sidebarOpen} />
          <SidebarLink to="/ui/jobs" icon={<ClockIcon />} label="Jobs" collapsed={!sidebarOpen} />
          <SidebarLink to="/ui/sensors" icon={<RadioIcon />} label="Sensors" collapsed={!sidebarOpen} />
        </nav>

        <div className="border-t border-border p-2">
          <a
            href="/docs"
            className="flex items-center gap-3 rounded-md px-3 py-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          >
            <HelpIcon />
            {sidebarOpen && <span>API Docs</span>}
          </a>
        </div>
      </aside>

      {/* Main content */}
      <div className="flex flex-1 flex-col min-w-0 overflow-hidden">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border bg-background px-6">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setSidebarOpen(!sidebarOpen)}
            aria-label="Toggle sidebar"
          >
            <MenuIcon />
          </Button>

          <div className="ml-auto flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => reconcile.execute()}
              disabled={reconcile.loading}
            >
              {reconcile.loading ? (
                <RefreshIcon className="mr-1.5 animate-spin" />
              ) : (
                <RefreshIcon className="mr-1.5" />
              )}
              {reconcile.loading ? "Reconciling..." : "Reconcile"}
            </Button>
          </div>
        </header>

        <main className="flex-1 overflow-y-auto p-6">
          <Routes>
            <Route path="/ui" element={<Dashboard />} />
            <Route path="/ui/assets" element={<Assets />} />
            <Route path="/ui/assets/:id" element={<AssetDetail />} />
            <Route path="/ui/jobs" element={<Jobs />} />
            <Route path="/ui/jobs/:id" element={<JobDetail />} />
            <Route path="/ui/sensors" element={<Sensors />} />
            <Route path="/ui/sensors/:id" element={<SensorDetail />} />
            <Route path="*" element={<Navigate to="/ui" replace />} />
          </Routes>
        </main>
      </div>
    </div>
  );
}

function SidebarLink({
  to,
  icon,
  label,
  collapsed,
  end,
}: {
  to: string;
  icon: React.ReactNode;
  label: string;
  collapsed: boolean;
  end?: boolean;
}) {
  return (
    <NavLink
      to={to}
      end={end}
      className={({ isActive }) =>
        `flex items-center gap-3 rounded-md px-3 py-2 text-[13.5px] transition-colors ${
          isActive
            ? "bg-accent text-foreground"
            : "text-muted-foreground hover:bg-accent hover:text-foreground"
        }`
      }
    >
      <span className="shrink-0">{icon}</span>
      {!collapsed && <span>{label}</span>}
    </NavLink>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <AppShell />
    </BrowserRouter>
  );
}
