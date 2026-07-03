import { useCallback, useRef, useState } from "react";
import {
  applyPersistedSettingsPatch,
  replacePersistedSettingsKeys,
  type PersistKey,
  type PersistedSettings,
  type PersistedSettingsPatch,
  DEFAULT_PERSISTED_SETTINGS,
} from "./settingsPersistenceModel";

function assignPersistedSetting<K extends PersistKey>(
  target: PersistedSettings,
  key: K,
  value: PersistedSettings[K]
) {
  target[key] = value;
}

export function useSettingsFormController(input?: {
  snapshot?: PersistedSettings;
  snapshotVersion?: number;
}) {
  const snapshot = input?.snapshot ?? DEFAULT_PERSISTED_SETTINGS;
  const snapshotVersion = input?.snapshotVersion ?? 0;
  const [draftState, setDraftState] = useState<{
    snapshotVersion: number;
    draft: PersistedSettings;
  }>(() => ({
    snapshotVersion,
    draft: snapshot,
  }));
  const draftRef = useRef<PersistedSettings>(snapshot);
  let effectiveDraftState = draftState;

  if (draftState.snapshotVersion !== snapshotVersion) {
    effectiveDraftState = {
      snapshotVersion,
      draft: snapshot,
    };
    draftRef.current = snapshot;
    setDraftState(effectiveDraftState);
  }

  const commitDraft = useCallback(
    (nextOrFactory: PersistedSettings | ((current: PersistedSettings) => PersistedSettings)) => {
      const next =
        typeof nextOrFactory === "function" ? nextOrFactory(draftRef.current) : nextOrFactory;
      draftRef.current = next;
      setDraftState((current) => ({ ...current, draft: next }));
      return next;
    },
    []
  );

  const applyPatch = useCallback(
    (patch: PersistedSettingsPatch) => {
      commitDraft((current) => applyPersistedSettingsPatch(current, patch));
    },
    [commitDraft]
  );

  const setField = useCallback(
    <K extends PersistKey>(key: K, value: PersistedSettings[K]) => {
      commitDraft((current) => ({ ...current, [key]: value }));
    },
    [commitDraft]
  );

  const revertKeys = useCallback(
    (keys: PersistKey[], source: PersistedSettings) => {
      commitDraft((current) => replacePersistedSettingsKeys(current, source, keys));
    },
    [commitDraft]
  );

  const reconcileSettledKeys = useCallback(
    (
      desiredSnapshot: PersistedSettings,
      persistedSnapshot: PersistedSettings,
      settledKeys: PersistKey[]
    ) => {
      if (settledKeys.length === 0) return;

      commitDraft((current) => {
        let changed = false;
        const next = { ...current };

        for (const key of settledKeys) {
          if (current[key] !== desiredSnapshot[key]) {
            continue;
          }

          assignPersistedSetting(next, key, persistedSnapshot[key]);
          changed = true;
        }

        return changed ? next : current;
      });
    },
    [commitDraft]
  );

  return {
    draft: effectiveDraftState.draft,
    draftRef,
    applyPatch,
    setField,
    revertKeys,
    reconcileSettledKeys,
  };
}
