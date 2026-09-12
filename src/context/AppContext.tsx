import { createContext, useContext } from "react";
import type { ConnectionKind } from "../lib/connection";
import type { AppSettings, IndexStatus, SessionInfo } from "../types";

export interface AppContextValue {
  settings: AppSettings | null;
  indexStatus: IndexStatus | null;
  refreshSettings: () => Promise<void>;
  refreshIndexStatus: () => Promise<void>;
  openWhatsNew: () => void;
  session: SessionInfo | null;
  signOut: () => Promise<void>;
  connectionKind: ConnectionKind;
}

export const AppContext = createContext<AppContextValue>({
  settings: null,
  indexStatus: null,
  refreshSettings: async () => {},
  refreshIndexStatus: async () => {},
  openWhatsNew: () => {},
  session: null,
  signOut: async () => {},
  connectionKind: "cloud",
});

export function useApp() {
  return useContext(AppContext);
}
