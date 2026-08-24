import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "light" | "dark";

interface AppState {
  theme: Theme;
  setTheme: (t: Theme) => void;
  toggleTheme: () => void;
}

function apply(theme: Theme) {
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", theme);
  }
}

export const useAppStore = create<AppState>()(
  persist(
    (set, get) => ({
      theme: "light",
      setTheme: (t) => {
        apply(t);
        set({ theme: t });
      },
      toggleTheme: () => {
        const next: Theme = get().theme === "light" ? "dark" : "light";
        apply(next);
        set({ theme: next });
      },
    }),
    {
      name: "aidms-theme",
      onRehydrateStorage: () => (state) => {
        if (state) apply(state.theme);
      },
    }
  )
);
