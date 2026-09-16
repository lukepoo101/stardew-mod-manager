import React from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  Package,
  Layers,
  Activity,
  History,
  Settings,
} from "lucide-react";

const navItems = [
  { to: "/app/overview", label: "Overview", icon: LayoutDashboard },
  { to: "/app/mods", label: "Mods", icon: Package },
  { to: "/app/profiles", label: "Profiles", icon: Layers },
  { to: "/app/diagnostics", label: "Diagnostics", icon: Activity },
  { to: "/app/activity", label: "Activity", icon: History },
  { to: "/app/settings", label: "Settings", icon: Settings },
];

export const Sidebar: React.FC<{
  activeProfileName?: string;
  activeProfileRevision?: number;
}> = ({ activeProfileName, activeProfileRevision }) => {
  return (
    <aside className="w-60 border-r border-[var(--border)] bg-[var(--bg-surface)] flex flex-col justify-between shrink-0 select-none">
      <div>
        {/* Brand Header */}
        <div className="h-16 px-5 flex items-center gap-3 border-b border-[var(--border)]">
          <span className="text-2xl">🌾</span>
          <div>
            <h1 className="font-extrabold text-sm tracking-tight text-[var(--fg-primary)]">
              Stardew Mod Manager
            </h1>
            <span className="text-[10px] text-[var(--fg-muted)] uppercase tracking-wider font-mono">
              Desktop Edition
            </span>
          </div>
        </div>

        {/* Nav Links */}
        <nav className="p-3 space-y-1">
          {navItems.map((item) => {
            const Icon = item.icon;
            return (
              <NavLink
                key={item.to}
                to={item.to}
                className={({ isActive }) =>
                  `${isActive ? "!text-[var(--accent-primary)] !bg-[var(--bg-elevated)] font-semibold" : ""} flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium text-[var(--fg-muted)] hover:text-[var(--fg-primary)] hover:bg-[var(--bg-elevated)] transition-colors`
                }
              >
                <Icon className="w-4 h-4 shrink-0" />
                <span>{item.label}</span>
              </NavLink>
            );
          })}
        </nav>
      </div>

      {/* Active Profile Footer */}
      {activeProfileName && (
        <div className="p-4 border-t border-[var(--border)] bg-[var(--bg-elevated)]/40 m-3 rounded-xl">
          <div className="text-[10px] text-[var(--fg-muted)] uppercase font-mono tracking-wider mb-1">
            Active Profile
          </div>
          <div className="font-bold text-xs text-[var(--fg-primary)] truncate">
            {activeProfileName}
          </div>
          {activeProfileRevision !== undefined && (
            <div className="text-[10px] text-[var(--fg-muted)] font-mono">
              Revision {activeProfileRevision}
            </div>
          )}
        </div>
      )}
    </aside>
  );
};
