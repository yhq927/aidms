import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { EntitySwitcher } from "./EntitySwitcher";
import { Onboarding } from "./Onboarding";

export function Layout() {
  return (
    <div className="flex h-screen w-screen overflow-hidden">
      {/* 首次启动聚光灯引导（非首次自动跳过） */}
      <Onboarding />
      <Sidebar />
      <main className="flex flex-1 flex-col overflow-hidden">
        <header className="flex h-14 shrink-0 items-center border-b bg-card px-4">
          <EntitySwitcher />
        </header>
        <div className="flex-1 overflow-auto p-6">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
