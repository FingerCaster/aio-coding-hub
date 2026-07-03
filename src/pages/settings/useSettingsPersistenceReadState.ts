import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";
import { logToConsole } from "../../services/consoleLog";
import type { AppSettings } from "../../services/settings/settings";
import { getSettingsReadProtection, SETTINGS_READONLY_MESSAGE } from "../../query/settings";
import {
  buildPersistedSettingsSnapshot,
  diffPersistedSettings,
  type PersistedSettings,
  DEFAULT_PERSISTED_SETTINGS,
} from "./settingsPersistenceModel";

type SettingsQueryState = {
  data: AppSettings | null | undefined;
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  dataUpdatedAt?: number;
};

type UseSettingsPersistenceReadStateInput = {
  settingsQuery: SettingsQueryState;
};

type LocalReadProtectionState = {
  message: string | null;
  blockedAtDataUpdatedAt: number | null;
};

const CLEAR_LOCAL_READ_PROTECTION: LocalReadProtectionState = {
  message: null,
  blockedAtDataUpdatedAt: null,
};

export function useSettingsPersistenceReadState(input: UseSettingsPersistenceReadStateInput) {
  const { settingsQuery } = input;
  const {
    data: settingsData,
    dataUpdatedAt,
    error: settingsError,
    isError: settingsIsError,
    isLoading: settingsLoading,
  } = settingsQuery;
  const [localReadProtection, setLocalReadProtection] = useState<LocalReadProtectionState>(
    CLEAR_LOCAL_READ_PROTECTION
  );

  const persistedSettingsRef = useRef<PersistedSettings>(DEFAULT_PERSISTED_SETTINGS);
  const desiredSettingsRef = useRef<PersistedSettings>(DEFAULT_PERSISTED_SETTINGS);
  const readFailureReportedRef = useRef<string | null>(null);
  const lastAppliedDataUpdatedAtRef = useRef<number | null>(null);
  const appliedSettingsVersionRef = useRef(0);

  const readProtection = getSettingsReadProtection({
    data: settingsData,
    isError: settingsIsError,
  });
  const queryReadErrorMessage = readProtection.settingsReadErrorMessage;
  const queryWriteBlocked = readProtection.settingsWriteBlocked;
  const currentDataUpdatedAt = settingsData ? (dataUpdatedAt ?? 0) : null;
  const localProtectionClearedByQuery =
    localReadProtection.message !== null &&
    !queryWriteBlocked &&
    !settingsLoading &&
    (settingsData
      ? localReadProtection.blockedAtDataUpdatedAt == null ||
        currentDataUpdatedAt! > localReadProtection.blockedAtDataUpdatedAt
      : true);

  if (localProtectionClearedByQuery) {
    setLocalReadProtection(CLEAR_LOCAL_READ_PROTECTION);
  }

  const localReadErrorMessage = localProtectionClearedByQuery ? null : localReadProtection.message;
  const settingsReadErrorMessage = queryReadErrorMessage ?? localReadErrorMessage;
  const settingsReady =
    !settingsLoading || settingsData != null || settingsIsError || localReadErrorMessage !== null;
  const settingsWriteBlocked = settingsReadErrorMessage !== null;

  const reportSettingsReadFailure = useCallback((error: unknown) => {
    const errorText = String(error);
    if (readFailureReportedRef.current === errorText) {
      return;
    }

    logToConsole("error", "读取设置失败", { error: errorText });
    toast(SETTINGS_READONLY_MESSAGE);
    readFailureReportedRef.current = errorText;
  }, []);

  const setSettingsReadErrorMessage = useCallback((message: string | null) => {
    setLocalReadProtection((current) => {
      const next: LocalReadProtectionState =
        message === null
          ? CLEAR_LOCAL_READ_PROTECTION
          : {
              message,
              blockedAtDataUpdatedAt: lastAppliedDataUpdatedAtRef.current,
            };

      if (
        current.message === next.message &&
        current.blockedAtDataUpdatedAt === next.blockedAtDataUpdatedAt
      ) {
        return current;
      }

      return next;
    });
  }, []);

  if (!settingsLoading && settingsData) {
    const nextSettings = buildPersistedSettingsSnapshot(settingsData);
    const nextUpdatedAt = dataUpdatedAt ?? 0;
    const hasFreshQueryData =
      lastAppliedDataUpdatedAtRef.current == null ||
      nextUpdatedAt > lastAppliedDataUpdatedAtRef.current;

    if (hasFreshQueryData) {
      const shouldSyncForm =
        hasFreshQueryData ||
        diffPersistedSettings(persistedSettingsRef.current, nextSettings).length > 0;

      persistedSettingsRef.current = nextSettings;
      desiredSettingsRef.current = nextSettings;
      lastAppliedDataUpdatedAtRef.current = nextUpdatedAt;

      if (shouldSyncForm) {
        appliedSettingsVersionRef.current += 1;
      }
    }
  }

  if (!settingsLoading && queryWriteBlocked) {
    reportSettingsReadFailure(settingsError);
  } else if (!settingsLoading) {
    readFailureReportedRef.current = null;
  }

  return {
    settingsReady,
    settingsReadErrorMessage,
    settingsWriteBlocked,
    setSettingsReadErrorMessage,
    reportSettingsReadFailure,
    appliedSettings: persistedSettingsRef.current,
    appliedSettingsVersion: appliedSettingsVersionRef.current,
    persistedSettingsRef,
    desiredSettingsRef,
  };
}
