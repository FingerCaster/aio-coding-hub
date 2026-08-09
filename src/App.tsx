import type { CSSProperties } from "react";
import { Toaster } from "sonner";
import { HashRouter } from "react-router-dom";
import { AppRoutes } from "./app/AppRoutes";
import { useInitializeAppSession } from "./app/appSession";
import {
  listenAndSyncAppStartupStatusSnapshot,
  useAppStartupStatus,
  useAppStartupStatusReady,
} from "./app/startupStatusStore";
import { useAppBootstrap } from "./app/useAppBootstrap";
import { useGlobalFileDropGuard } from "./app/useGlobalFileDropGuard";
import { useAsyncListener } from "./hooks/useAsyncListener";
import { Spinner } from "./ui/Spinner";

type CssVarsStyle = CSSProperties & Record<`--toast-${string}`, string | number>;

const TOASTER_STYLE: CssVarsStyle = {
  "--toast-close-button-start": "unset",
  "--toast-close-button-end": "0",
  "--toast-close-button-transform": "translate(35%, -35%)",
};

function NormalAppRuntime() {
  useAppBootstrap();
  useGlobalFileDropGuard();

  return <AppRoutes />;
}

export default function App() {
  useInitializeAppSession();
  useAsyncListener(
    listenAndSyncAppStartupStatusSnapshot,
    "listenAndSyncAppStartupStatusSnapshot",
    "应用启动状态监听初始化失败"
  );
  const startupStatus = useAppStartupStatus();
  const startupStatusReady = useAppStartupStatusReady();

  return (
    <>
      <Toaster richColors closeButton position="top-center" style={TOASTER_STYLE} />
      <HashRouter>
        {!startupStatusReady ? (
          <div className="flex h-screen items-center justify-center bg-background text-foreground">
            <Spinner />
          </div>
        ) : startupStatus.maintenanceMode ? (
          <AppRoutes />
        ) : (
          <NormalAppRuntime />
        )}
      </HashRouter>
    </>
  );
}
