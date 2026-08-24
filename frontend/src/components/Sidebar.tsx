import { NavLink } from "react-router-dom";
import { Library, Search, MessageSquare, Building2, Settings, Sun, Moon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/useAppStore";

const nav = [
  { to: "/", label: "资料库", icon: Library },
  { to: "/search", label: "搜索", icon: Search },
  { to: "/qa", label: "问答", icon: MessageSquare },
  { to: "/entities", label: "主体管理", icon: Building2 },
  { to: "/settings", label: "配置", icon: Settings },
];

export function Sidebar() {
  const theme = useAppStore((s) => s.theme);
  const toggleTheme = useAppStore((s) => s.toggleTheme);

  return (
    <aside className="flex h-full w-60 flex-col border-r bg-card">
      <div className="flex h-14 items-center px-4 text-base font-semibold">AIDMS 企业资料</div>
      <nav className="flex-1 space-y-1 px-2 py-2">
        {nav.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            id={to === "/" ? "nav-library" : to === "/search" ? "nav-search" : to === "/qa" ? "nav-qa" : undefined}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-sm",
                isActive
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
              )
            }
          >
            <Icon className="h-4 w-4" />
            {label}
          </NavLink>
        ))}
      </nav>

      <div className="border-t p-3">
        <button
          onClick={toggleTheme}
          className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm hover:bg-accent"
        >
          {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          {theme === "dark" ? "浅色" : "深色"}
        </button>
      </div>
    </aside>
  );
}
