// Usage: Installed/local skills view for a specific workspace.

import { ExternalLink } from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { toast } from "sonner";
import {
  useSkillCheckUpdatesMutation,
  useSkillImportLocalMutation,
  useSkillLocalDeleteMutation,
  useSkillReturnToLocalMutation,
  useSkillSetEnabledMutation,
  useSkillUninstallMutation,
  useSkillUpdateMutation,
  useSkillsInstalledListQuery,
  useSkillsLocalListQuery,
  type SkillUpdateInfo,
} from "../../query/skills";
import { logToConsole } from "../../services/consoleLog";
import { openDesktopPath, revealDesktopItem } from "../../services/desktop/opener";
import type { CliKey } from "../../services/providers/providers";
import {
  type InstalledSkillSummary,
  type LocalSkillSummary,
} from "../../services/workspace/skills";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { EmptyState } from "../../ui/EmptyState";
import { Spinner } from "../../ui/Spinner";
import { Switch } from "../../ui/Switch";
import { cn } from "../../utils/cn";
import { formatActionFailureToast } from "../../utils/errors";
import {
  displaySkillName,
  repoPrefixFromGitUrl,
  repositoryWebUrl,
  sourceHint,
} from "../../utils/skillSources";

const TOAST_LOCAL_IMPORT_REQUIRES_ACTIVE = "仅当前工作区可导入本机 Skill。请先切换该工作区为当前。";
const TOAST_RETURN_LOCAL_REQUIRES_ACTIVE = "仅当前工作区可返回本机 Skill。请先切换该工作区为当前。";
const TOAST_DELETE_LOCAL_REQUIRES_ACTIVE = "仅当前工作区可删除本机 Skill。请先切换该工作区为当前。";

function formatUnixSeconds(ts: number) {
  try {
    return new Date(ts * 1000).toLocaleString();
  } catch {
    return String(ts);
  }
}

function isUpdatableSkillSource(sourceGitUrl: string): boolean {
  const url = sourceGitUrl.trim().toLowerCase();
  return url.length > 0 && !url.startsWith("local://");
}

function pruneSelectionSet<T>(prev: Set<T>, allowed: Set<T>) {
  let changed = false;
  const next = new Set<T>();
  for (const value of prev) {
    if (allowed.has(value)) {
      next.add(value);
    } else {
      changed = true;
    }
  }
  return changed ? next : prev;
}

async function openPathOrReveal(path: string) {
  try {
    await openDesktopPath(path);
    return;
  } catch (err) {
    logToConsole("warn", "openPath 失败，尝试 revealItemInDir", {
      error: String(err),
      path,
    });
  }
  await revealDesktopItem(path);
}

export type SkillsViewProps = {
  workspaceId: number;
  cliKey: CliKey;
  isActiveWorkspace?: boolean;
  localImportMode?: "single" | "batch_init";
};

type SkillsUiState = {
  resetKey: string;
  selectedInstalledIds: Set<number>;
  selectedLocalDirNames: Set<string>;
  deleteInstalledDialogOpen: boolean;
  deleteLocalDialogOpen: boolean;
  localDeleteTargets: LocalSkillSummary[];
  deletingInstalled: boolean;
  deletingLocal: boolean;
  updateInfoMap: Map<number, SkillUpdateInfo>;
  updatingSkillId: number | null;
};

function createSkillsUiState(resetKey: string): SkillsUiState {
  return {
    resetKey,
    selectedInstalledIds: new Set(),
    selectedLocalDirNames: new Set(),
    deleteInstalledDialogOpen: false,
    deleteLocalDialogOpen: false,
    localDeleteTargets: [],
    deletingInstalled: false,
    deletingLocal: false,
    updateInfoMap: new Map(),
    updatingSkillId: null,
  };
}

type SkillsUiController = {
  state: SkillsUiState;
  setSelectedInstalledIds: Dispatch<SetStateAction<Set<number>>>;
  setSelectedLocalDirNames: Dispatch<SetStateAction<Set<string>>>;
  setDeleteInstalledDialogOpen: Dispatch<SetStateAction<boolean>>;
  setDeleteLocalDialogOpen: Dispatch<SetStateAction<boolean>>;
  setLocalDeleteTargets: Dispatch<SetStateAction<LocalSkillSummary[]>>;
  setDeletingInstalled: Dispatch<SetStateAction<boolean>>;
  setDeletingLocal: Dispatch<SetStateAction<boolean>>;
  setUpdateInfoMap: Dispatch<SetStateAction<Map<number, SkillUpdateInfo>>>;
  setUpdatingSkillId: Dispatch<SetStateAction<number | null>>;
};

function useSkillsUiState(
  resetKey: string,
  allowedInstalledIds: Set<number>,
  allowedLocalDirNames: Set<string>
): SkillsUiController {
  const [skillsUiState, setSkillsUiState] = useState<SkillsUiState>(() =>
    createSkillsUiState(resetKey)
  );
  let nextSkillsUiState = skillsUiState;

  if (skillsUiState.resetKey !== resetKey) {
    nextSkillsUiState = createSkillsUiState(resetKey);
  }

  const prunedSelectedInstalledIds = pruneSelectionSet(
    nextSkillsUiState.selectedInstalledIds,
    allowedInstalledIds
  );
  const prunedSelectedLocalDirNames = pruneSelectionSet(
    nextSkillsUiState.selectedLocalDirNames,
    allowedLocalDirNames
  );
  if (
    prunedSelectedInstalledIds !== nextSkillsUiState.selectedInstalledIds ||
    prunedSelectedLocalDirNames !== nextSkillsUiState.selectedLocalDirNames
  ) {
    nextSkillsUiState = {
      ...nextSkillsUiState,
      selectedInstalledIds: prunedSelectedInstalledIds,
      selectedLocalDirNames: prunedSelectedLocalDirNames,
    };
  }
  if (nextSkillsUiState !== skillsUiState) {
    setSkillsUiState(nextSkillsUiState);
  }

  const setSelectedInstalledIds: Dispatch<SetStateAction<Set<number>>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      selectedInstalledIds:
        typeof value === "function" ? value(current.selectedInstalledIds) : value,
    }));
  }, []);
  const setSelectedLocalDirNames: Dispatch<SetStateAction<Set<string>>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      selectedLocalDirNames:
        typeof value === "function" ? value(current.selectedLocalDirNames) : value,
    }));
  }, []);
  const setDeleteInstalledDialogOpen: Dispatch<SetStateAction<boolean>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      deleteInstalledDialogOpen:
        typeof value === "function" ? value(current.deleteInstalledDialogOpen) : value,
    }));
  }, []);
  const setDeleteLocalDialogOpen: Dispatch<SetStateAction<boolean>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      deleteLocalDialogOpen:
        typeof value === "function" ? value(current.deleteLocalDialogOpen) : value,
    }));
  }, []);
  const setLocalDeleteTargets: Dispatch<SetStateAction<LocalSkillSummary[]>> = useCallback(
    (value) => {
      setSkillsUiState((current) => ({
        ...current,
        localDeleteTargets: typeof value === "function" ? value(current.localDeleteTargets) : value,
      }));
    },
    []
  );
  const setDeletingInstalled: Dispatch<SetStateAction<boolean>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      deletingInstalled: typeof value === "function" ? value(current.deletingInstalled) : value,
    }));
  }, []);
  const setDeletingLocal: Dispatch<SetStateAction<boolean>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      deletingLocal: typeof value === "function" ? value(current.deletingLocal) : value,
    }));
  }, []);
  const setUpdateInfoMap: Dispatch<SetStateAction<Map<number, SkillUpdateInfo>>> = useCallback(
    (value) => {
      setSkillsUiState((current) => ({
        ...current,
        updateInfoMap: typeof value === "function" ? value(current.updateInfoMap) : value,
      }));
    },
    []
  );
  const setUpdatingSkillId: Dispatch<SetStateAction<number | null>> = useCallback((value) => {
    setSkillsUiState((current) => ({
      ...current,
      updatingSkillId: typeof value === "function" ? value(current.updatingSkillId) : value,
    }));
  }, []);

  return {
    state: nextSkillsUiState,
    setSelectedInstalledIds,
    setSelectedLocalDirNames,
    setDeleteInstalledDialogOpen,
    setDeleteLocalDialogOpen,
    setLocalDeleteTargets,
    setDeletingInstalled,
    setDeletingLocal,
    setUpdateInfoMap,
    setUpdatingSkillId,
  };
}

type SkillsViewActionsArgs = {
  workspaceId: number;
  cliKey: CliKey;
  isActiveWorkspace: boolean;
  canOperateLocal: boolean;
  installed: InstalledSkillSummary[];
  localSkills: LocalSkillSummary[];
  localLoading: boolean;
  selectedInstalledSkills: InstalledSkillSummary[];
  selectedLocalSkills: LocalSkillSummary[];
  allInstalledSelected: boolean;
  allLocalSelected: boolean;
  localDeleteTargets: LocalSkillSummary[];
  deletingInstalled: boolean;
  deletingLocal: boolean;
  updatingSkillId: number | null;
  setSelectedInstalledIds: Dispatch<SetStateAction<Set<number>>>;
  setSelectedLocalDirNames: Dispatch<SetStateAction<Set<string>>>;
  setDeleteInstalledDialogOpen: Dispatch<SetStateAction<boolean>>;
  setDeleteLocalDialogOpen: Dispatch<SetStateAction<boolean>>;
  setLocalDeleteTargets: Dispatch<SetStateAction<LocalSkillSummary[]>>;
  setDeletingInstalled: Dispatch<SetStateAction<boolean>>;
  setDeletingLocal: Dispatch<SetStateAction<boolean>>;
  setUpdateInfoMap: Dispatch<SetStateAction<Map<number, SkillUpdateInfo>>>;
  setUpdatingSkillId: Dispatch<SetStateAction<number | null>>;
  installedQuery: ReturnType<typeof useSkillsInstalledListQuery>;
  localQuery: ReturnType<typeof useSkillsLocalListQuery>;
  toggleMutation: ReturnType<typeof useSkillSetEnabledMutation>;
  uninstallMutation: ReturnType<typeof useSkillUninstallMutation>;
  returnToLocalMutation: ReturnType<typeof useSkillReturnToLocalMutation>;
  localDeleteMutation: ReturnType<typeof useSkillLocalDeleteMutation>;
  importMutation: ReturnType<typeof useSkillImportLocalMutation>;
  checkUpdatesMutation: ReturnType<typeof useSkillCheckUpdatesMutation>;
  updateSkillMutation: ReturnType<typeof useSkillUpdateMutation>;
};

function useSkillsViewActions({
  workspaceId,
  cliKey,
  isActiveWorkspace,
  canOperateLocal,
  installed,
  localSkills,
  localLoading,
  selectedInstalledSkills,
  selectedLocalSkills,
  allInstalledSelected,
  allLocalSelected,
  localDeleteTargets,
  deletingInstalled,
  deletingLocal,
  updatingSkillId,
  setSelectedInstalledIds,
  setSelectedLocalDirNames,
  setDeleteInstalledDialogOpen,
  setDeleteLocalDialogOpen,
  setLocalDeleteTargets,
  setDeletingInstalled,
  setDeletingLocal,
  setUpdateInfoMap,
  setUpdatingSkillId,
  installedQuery,
  localQuery,
  toggleMutation,
  uninstallMutation,
  returnToLocalMutation,
  localDeleteMutation,
  importMutation,
  checkUpdatesMutation,
  updateSkillMutation,
}: SkillsViewActionsArgs) {
  function toggleInstalledSelection(skillId: number) {
    setSelectedInstalledIds((prev) => {
      const next = new Set(prev);
      if (next.has(skillId)) {
        next.delete(skillId);
      } else {
        next.add(skillId);
      }
      return next;
    });
  }

  function toggleAllInstalledSelection() {
    if (allInstalledSelected) {
      setSelectedInstalledIds(new Set());
      return;
    }
    setSelectedInstalledIds(new Set(installed.map((skill) => skill.id)));
  }

  function toggleLocalSelection(dirName: string) {
    setSelectedLocalDirNames((prev) => {
      const next = new Set(prev);
      if (next.has(dirName)) {
        next.delete(dirName);
      } else {
        next.add(dirName);
      }
      return next;
    });
  }

  function toggleAllLocalSelection() {
    if (allLocalSelected) {
      setSelectedLocalDirNames(new Set());
      return;
    }
    setSelectedLocalDirNames(new Set(localSkills.map((skill) => skill.dir_name)));
  }

  function openInstalledDeleteDialog(skillIds: number[]) {
    setSelectedInstalledIds(new Set(skillIds));
    setDeleteInstalledDialogOpen(true);
  }

  function openSelectedInstalledDeleteDialog() {
    setDeleteInstalledDialogOpen(true);
  }

  function closeInstalledDeleteDialog() {
    setDeleteInstalledDialogOpen(false);
  }

  function openLocalDeleteDialog(dirNames: string[]) {
    setSelectedLocalDirNames(new Set(dirNames));
    setLocalDeleteTargets(localSkills.filter((skill) => dirNames.includes(skill.dir_name)));
    setDeleteLocalDialogOpen(true);
  }

  function openSelectedLocalDeleteDialog() {
    setLocalDeleteTargets(selectedLocalSkills);
    setDeleteLocalDialogOpen(true);
  }

  function closeLocalDeleteDialog() {
    setDeleteLocalDialogOpen(false);
    setLocalDeleteTargets([]);
  }

  async function toggleSkillEnabled(skill: InstalledSkillSummary, enabled: boolean) {
    if (toggleMutation.isPending || deletingInstalled) return;
    try {
      const next = await toggleMutation.mutateAsync({ skillId: skill.id, enabled });
      if (!next) {
        return;
      }
      if (enabled) {
        toast(isActiveWorkspace ? "已启用" : "已启用（非当前工作区，不会同步）");
      } else {
        toast(isActiveWorkspace ? "已禁用" : "已禁用");
      }
    } catch (err) {
      const formatted = formatActionFailureToast("切换启用", err);
      logToConsole("error", "切换 Skill 启用状态失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        cli: cliKey,
        workspace_id: workspaceId,
        skill_id: skill.id,
        enabled,
      });
      toast(formatted.toast);
    }
  }

  async function confirmDeleteInstalledSkills() {
    if (selectedInstalledSkills.length === 0 || deletingInstalled) return;

    const targets = selectedInstalledSkills.map((skill) => ({
      id: skill.id,
      name: skill.name,
    }));
    const failedIds = new Set<number>();
    let successCount = 0;
    let firstFailureToast: string | null = null;

    setDeletingInstalled(true);
    try {
      const outcomes = await Promise.all(
        targets.map(async (target) => {
          try {
            const ok = await uninstallMutation.mutateAsync(target.id);
            return {
              target,
              success: Boolean(ok),
              failureToast: ok ? null : `删除通用技能失败：${target.name}`,
            };
          } catch (err) {
            const formatted = formatActionFailureToast("删除通用技能", err);
            logToConsole("error", "删除通用 Skill 失败", {
              error: formatted.raw,
              error_code: formatted.error_code ?? undefined,
              cli: cliKey,
              workspace_id: workspaceId,
              skill_id: target.id,
            });
            return { target, success: false, failureToast: formatted.toast };
          }
        })
      );
      for (const outcome of outcomes) {
        if (outcome.success) {
          successCount += 1;
          continue;
        }
        failedIds.add(outcome.target.id);
        firstFailureToast ??= outcome.failureToast;
      }
    } finally {
      setDeletingInstalled(false);
    }

    if (successCount > 0) {
      toast(successCount === 1 ? "已删除通用技能" : `已删除 ${successCount} 个通用技能`);
    }
    if (failedIds.size > 0) {
      if (successCount === 0 && failedIds.size === 1 && firstFailureToast) {
        toast(firstFailureToast);
      } else {
        toast(`${failedIds.size} 个通用技能删除失败`);
      }
    }

    setSelectedInstalledIds(failedIds);
    setDeleteInstalledDialogOpen(false);
  }

  async function returnToLocalSkill(skill: InstalledSkillSummary) {
    if (!canOperateLocal) {
      toast(TOAST_RETURN_LOCAL_REQUIRES_ACTIVE);
      return;
    }
    if (returnToLocalMutation.isPending || deletingInstalled) return;
    const target = skill;
    try {
      const ok = await returnToLocalMutation.mutateAsync(target.id);
      if (!ok) {
        return;
      }
      toast("已返回本机已安装");
      logToConsole("info", "Skill 返回本机已安装", {
        cli: cliKey,
        workspace_id: workspaceId,
        skill: target,
      });
      setSelectedInstalledIds((prev) => {
        if (!prev.has(target.id)) return prev;
        const next = new Set(prev);
        next.delete(target.id);
        return next;
      });
    } catch (err) {
      const formatted = formatActionFailureToast("返回本机", err);
      logToConsole("error", "Skill 返回本机已安装失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        cli: cliKey,
        workspace_id: workspaceId,
        skill: target,
      });
      toast(formatted.toast);
    }
  }

  async function confirmDeleteLocalSkills() {
    if (localDeleteTargets.length === 0 || deletingLocal) return;
    if (!canOperateLocal) {
      toast(TOAST_DELETE_LOCAL_REQUIRES_ACTIVE);
      return;
    }

    const targets = localDeleteTargets.map((skill) => ({
      dirName: skill.dir_name,
      label: skill.name || skill.dir_name,
      path: skill.path,
    }));
    const failedDirNames = new Set<string>();
    let successCount = 0;
    let firstFailureToast: string | null = null;

    setDeletingLocal(true);
    try {
      const outcomes = await Promise.all(
        targets.map(async (target) => {
          try {
            const ok = await localDeleteMutation.mutateAsync(target.dirName);
            return {
              target,
              success: Boolean(ok),
              failureToast: ok ? null : `删除本机技能失败：${target.label}`,
            };
          } catch (err) {
            const formatted = formatActionFailureToast("删除本机技能", err);
            logToConsole("error", "删除本机 Skill 失败", {
              error: formatted.raw,
              error_code: formatted.error_code ?? undefined,
              cli: cliKey,
              workspace_id: workspaceId,
              dir_name: target.dirName,
              path: target.path,
            });
            return { target, success: false, failureToast: formatted.toast };
          }
        })
      );
      for (const outcome of outcomes) {
        if (outcome.success) {
          successCount += 1;
          continue;
        }
        failedDirNames.add(outcome.target.dirName);
        firstFailureToast ??= outcome.failureToast;
      }
    } finally {
      setDeletingLocal(false);
    }

    if (successCount > 0) {
      toast(successCount === 1 ? "已删除本机技能" : `已删除 ${successCount} 个本机技能`);
    }
    if (failedDirNames.size > 0) {
      if (successCount === 0 && failedDirNames.size === 1 && firstFailureToast) {
        toast(firstFailureToast);
      } else {
        toast(`${failedDirNames.size} 个本机技能删除失败`);
      }
    }

    setSelectedLocalDirNames(failedDirNames);
    setLocalDeleteTargets(localDeleteTargets.filter((skill) => failedDirNames.has(skill.dir_name)));
    setDeleteLocalDialogOpen(false);
  }

  async function importLocalSkill(skill: LocalSkillSummary) {
    if (importMutation.isPending || deletingLocal) return;
    if (!canOperateLocal) {
      toast(TOAST_LOCAL_IMPORT_REQUIRES_ACTIVE);
      return;
    }
    try {
      const next = await importMutation.mutateAsync(skill.dir_name);
      if (!next) {
        return;
      }

      toast("已导入到技能库");
      logToConsole("info", "导入本机 Skill", {
        cli: cliKey,
        workspace_id: workspaceId,
        imported: next,
      });
      setSelectedLocalDirNames((prev) => {
        if (!prev.has(skill.dir_name)) return prev;
        const nextSelected = new Set(prev);
        nextSelected.delete(skill.dir_name);
        return nextSelected;
      });
    } catch (err) {
      const formatted = formatActionFailureToast("导入", err);
      logToConsole("error", "导入本机 Skill 失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        cli: cliKey,
        workspace_id: workspaceId,
        skill,
      });
      toast(formatted.toast);
    }
  }

  function refreshInstalledSkills() {
    void installedQuery.refetch();
  }

  async function refreshLocalSkills() {
    if (!canOperateLocal || localLoading || deletingLocal) return;
    await localQuery.refetch();
  }

  async function checkForUpdates() {
    if (checkUpdatesMutation.isPending || deletingInstalled) return;
    try {
      const results = await checkUpdatesMutation.mutateAsync();
      if (!results || results.length === 0) {
        toast("没有发现可更新的技能");
        setUpdateInfoMap(new Map());
        return;
      }
      const newMap = new Map<number, SkillUpdateInfo>();
      let updatesCount = 0;
      for (const info of results) {
        newMap.set(info.skill_id, info);
        if (info.has_update) {
          updatesCount += 1;
        }
      }
      setUpdateInfoMap(newMap);
      if (updatesCount > 0) {
        toast(`发现 ${updatesCount} 个技能有更新`);
      } else {
        toast("所有技能已是最新版本");
      }
    } catch (err) {
      const formatted = formatActionFailureToast("检查更新", err);
      logToConsole("error", "检查技能更新失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        cli: cliKey,
        workspace_id: workspaceId,
      });
      toast(formatted.toast);
    }
  }

  async function updateSkill(skill: InstalledSkillSummary) {
    if (updatingSkillId || deletingInstalled) return;
    setUpdatingSkillId(skill.id);
    try {
      const next = await updateSkillMutation.mutateAsync(skill.id);
      if (!next) {
        return;
      }
      toast("技能已更新");
      logToConsole("info", "技能已更新", {
        cli: cliKey,
        workspace_id: workspaceId,
        old_skill_id: skill.id,
        new_skill: next,
      });
      setUpdateInfoMap((prev) => {
        const newMap = new Map(prev);
        newMap.delete(skill.id);
        return newMap;
      });
      await installedQuery.refetch();
    } catch (err) {
      const formatted = formatActionFailureToast("更新技能", err);
      logToConsole("error", "更新技能失败", {
        error: formatted.raw,
        error_code: formatted.error_code ?? undefined,
        cli: cliKey,
        workspace_id: workspaceId,
        skill_id: skill.id,
      });
      toast(formatted.toast);
    } finally {
      setUpdatingSkillId(null);
    }
  }

  async function openLocalSkillDir(skill: LocalSkillSummary) {
    try {
      await openPathOrReveal(skill.path);
    } catch (err) {
      logToConsole("error", "打开本机 Skill 目录失败", {
        error: String(err),
        cli: cliKey,
        workspace_id: workspaceId,
        path: skill.path,
      });
      toast("打开目录失败：请查看控制台日志");
    }
  }

  return {
    toggleInstalledSelection,
    toggleAllInstalledSelection,
    toggleLocalSelection,
    toggleAllLocalSelection,
    openInstalledDeleteDialog,
    openSelectedInstalledDeleteDialog,
    closeInstalledDeleteDialog,
    openLocalDeleteDialog,
    openSelectedLocalDeleteDialog,
    closeLocalDeleteDialog,
    toggleSkillEnabled,
    confirmDeleteInstalledSkills,
    returnToLocalSkill,
    confirmDeleteLocalSkills,
    importLocalSkill,
    refreshInstalledSkills,
    refreshLocalSkills,
    checkForUpdates,
    updateSkill,
    openLocalSkillDir,
  };
}

export function SkillsView({
  workspaceId,
  cliKey,
  isActiveWorkspace = true,
  localImportMode = "single",
}: SkillsViewProps) {
  const canOperateLocal = isActiveWorkspace;
  const batchInitMode = localImportMode === "batch_init";

  const installedQuery = useSkillsInstalledListQuery(workspaceId);
  const localQuery = useSkillsLocalListQuery(workspaceId, { enabled: canOperateLocal });

  const toggleMutation = useSkillSetEnabledMutation(workspaceId);
  const uninstallMutation = useSkillUninstallMutation(workspaceId);
  const returnToLocalMutation = useSkillReturnToLocalMutation(workspaceId);
  const localDeleteMutation = useSkillLocalDeleteMutation(workspaceId);
  const importMutation = useSkillImportLocalMutation(workspaceId);
  const checkUpdatesMutation = useSkillCheckUpdatesMutation(workspaceId);
  const updateSkillMutation = useSkillUpdateMutation(workspaceId);

  const installed: InstalledSkillSummary[] = installedQuery.data ?? [];
  const localSkills: LocalSkillSummary[] = canOperateLocal ? (localQuery.data ?? []) : [];

  const loading = installedQuery.isFetching;
  const localLoading = canOperateLocal ? localQuery.isFetching : false;
  const togglingSkillId = toggleMutation.isPending
    ? (toggleMutation.variables?.skillId ?? null)
    : null;
  const returningLocalSkillId = returnToLocalMutation.isPending
    ? (returnToLocalMutation.variables ?? null)
    : null;
  const importingLocal = importMutation.isPending;

  const allowedInstalledIds = useMemo(
    () => new Set((installedQuery.data ?? []).map((skill) => skill.id)),
    [installedQuery.data]
  );
  const allowedLocalDirNames = useMemo(
    () => new Set((canOperateLocal ? (localQuery.data ?? []) : []).map((skill) => skill.dir_name)),
    [canOperateLocal, localQuery.data]
  );
  const resetKey = `${workspaceId}:${cliKey}`;
  const skillsUi = useSkillsUiState(resetKey, allowedInstalledIds, allowedLocalDirNames);
  const {
    selectedInstalledIds,
    selectedLocalDirNames,
    deleteInstalledDialogOpen,
    deleteLocalDialogOpen,
    localDeleteTargets,
    deletingInstalled,
    deletingLocal,
    updateInfoMap,
    updatingSkillId,
  } = skillsUi.state;
  const {
    setSelectedInstalledIds,
    setSelectedLocalDirNames,
    setDeleteInstalledDialogOpen,
    setDeleteLocalDialogOpen,
    setLocalDeleteTargets,
    setDeletingInstalled,
    setDeletingLocal,
    setUpdateInfoMap,
    setUpdatingSkillId,
  } = skillsUi;

  const selectedInstalledSkills = installed.filter((skill) => selectedInstalledIds.has(skill.id));
  const selectedLocalSkills = localSkills.filter((skill) =>
    selectedLocalDirNames.has(skill.dir_name)
  );
  const allInstalledSelected =
    installed.length > 0 && selectedInstalledSkills.length === installed.length;
  const allLocalSelected =
    localSkills.length > 0 && selectedLocalSkills.length === localSkills.length;
  const actions = useSkillsViewActions({
    workspaceId,
    cliKey,
    isActiveWorkspace,
    canOperateLocal,
    installed,
    localSkills,
    localLoading,
    selectedInstalledSkills,
    selectedLocalSkills,
    allInstalledSelected,
    allLocalSelected,
    localDeleteTargets,
    deletingInstalled,
    deletingLocal,
    updatingSkillId,
    setSelectedInstalledIds,
    setSelectedLocalDirNames,
    setDeleteInstalledDialogOpen,
    setDeleteLocalDialogOpen,
    setLocalDeleteTargets,
    setDeletingInstalled,
    setDeletingLocal,
    setUpdateInfoMap,
    setUpdatingSkillId,
    installedQuery,
    localQuery,
    toggleMutation,
    uninstallMutation,
    returnToLocalMutation,
    localDeleteMutation,
    importMutation,
    checkUpdatesMutation,
    updateSkillMutation,
  });

  useEffect(() => {
    if (!installedQuery.error) return;
    logToConsole("error", "加载 Skills 数据失败", {
      error: String(installedQuery.error),
      workspace_id: workspaceId,
    });
    toast("加载失败：请查看控制台日志");
  }, [installedQuery.error, workspaceId]);

  useEffect(() => {
    if (!localQuery.error) return;
    logToConsole("error", "扫描本机 Skill 失败", {
      error: String(localQuery.error),
      cli: cliKey,
      workspace_id: workspaceId,
    });
    toast("扫描本机 Skill 失败：请查看控制台日志");
  }, [cliKey, localQuery.error, workspaceId]);

  return (
    <>
      <div className="grid h-full gap-4 lg:grid-cols-2">
        <InstalledSkillsCard
          installed={installed}
          loading={loading}
          selectedInstalledIds={selectedInstalledIds}
          allInstalledSelected={allInstalledSelected}
          deletingInstalled={deletingInstalled}
          togglingSkillId={togglingSkillId}
          returningLocalSkillId={returningLocalSkillId}
          togglePending={toggleMutation.isPending}
          returnPending={returnToLocalMutation.isPending}
          checkUpdatesPending={checkUpdatesMutation.isPending}
          updatingSkillId={updatingSkillId}
          updateInfoMap={updateInfoMap}
          canOperateLocal={canOperateLocal}
          onToggleAll={actions.toggleAllInstalledSelection}
          onOpenSelectedDelete={actions.openSelectedInstalledDeleteDialog}
          onCheckUpdates={() => void actions.checkForUpdates()}
          onRefresh={actions.refreshInstalledSkills}
          onToggleSelection={actions.toggleInstalledSelection}
          onToggleEnabled={actions.toggleSkillEnabled}
          onReturnToLocal={actions.returnToLocalSkill}
          onUpdateSkill={actions.updateSkill}
          onDelete={(skillId) => actions.openInstalledDeleteDialog([skillId])}
        />

        <LocalSkillsCard
          cliKey={cliKey}
          canOperateLocal={canOperateLocal}
          batchInitMode={batchInitMode}
          localSkills={localSkills}
          localLoading={localLoading}
          selectedLocalDirNames={selectedLocalDirNames}
          allLocalSelected={allLocalSelected}
          deletingLocal={deletingLocal}
          importingLocal={importingLocal}
          onToggleAll={actions.toggleAllLocalSelection}
          onOpenSelectedDelete={actions.openSelectedLocalDeleteDialog}
          onRefresh={() => void actions.refreshLocalSkills()}
          onToggleSelection={actions.toggleLocalSelection}
          onImport={actions.importLocalSkill}
          onDelete={actions.openLocalDeleteDialog}
          onOpenDir={actions.openLocalSkillDir}
        />
      </div>

      <SkillsDeleteDialogs
        deleteInstalledDialogOpen={deleteInstalledDialogOpen}
        selectedInstalledSkills={selectedInstalledSkills}
        deletingInstalled={deletingInstalled}
        onCloseInstalled={actions.closeInstalledDeleteDialog}
        onConfirmDeleteInstalled={() => void actions.confirmDeleteInstalledSkills()}
        deleteLocalDialogOpen={deleteLocalDialogOpen}
        localDeleteTargets={localDeleteTargets}
        deletingLocal={deletingLocal}
        onCloseLocal={actions.closeLocalDeleteDialog}
        onConfirmDeleteLocal={() => void actions.confirmDeleteLocalSkills()}
      />
    </>
  );
}

type InstalledSkillsCardProps = {
  installed: InstalledSkillSummary[];
  loading: boolean;
  selectedInstalledIds: Set<number>;
  allInstalledSelected: boolean;
  deletingInstalled: boolean;
  togglingSkillId: number | null;
  returningLocalSkillId: number | null;
  togglePending: boolean;
  returnPending: boolean;
  checkUpdatesPending: boolean;
  updatingSkillId: number | null;
  updateInfoMap: Map<number, SkillUpdateInfo>;
  canOperateLocal: boolean;
  onToggleAll: () => void;
  onOpenSelectedDelete: () => void;
  onCheckUpdates: () => void;
  onRefresh: () => void;
  onToggleSelection: (skillId: number) => void;
  onToggleEnabled: (skill: InstalledSkillSummary, enabled: boolean) => void;
  onReturnToLocal: (skill: InstalledSkillSummary) => void;
  onUpdateSkill: (skill: InstalledSkillSummary) => void;
  onDelete: (skillId: number) => void;
};

function InstalledSkillsCard({
  installed,
  loading,
  selectedInstalledIds,
  allInstalledSelected,
  deletingInstalled,
  togglingSkillId,
  returningLocalSkillId,
  togglePending,
  returnPending,
  checkUpdatesPending,
  updatingSkillId,
  updateInfoMap,
  canOperateLocal,
  onToggleAll,
  onOpenSelectedDelete,
  onCheckUpdates,
  onRefresh,
  onToggleSelection,
  onToggleEnabled,
  onReturnToLocal,
  onUpdateSkill,
  onDelete,
}: InstalledSkillsCardProps) {
  return (
    <Card className="flex min-h-[240px] flex-col lg:min-h-0" padding="md">
      <div className="flex shrink-0 items-start justify-between gap-3">
        <div className="text-sm font-semibold">通用技能</div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {installed.length > 0 ? (
            <label className="inline-flex items-center gap-2 text-xs text-muted-foreground">
              <input
                type="checkbox"
                checked={allInstalledSelected}
                onChange={onToggleAll}
                disabled={deletingInstalled || togglePending || returnPending}
                className="h-4 w-4 rounded border border-input bg-card accent-accent focus:ring-2 focus:ring-ring/30"
                aria-label="全选通用技能"
              />
              <span>全选</span>
            </label>
          ) : null}
          {selectedInstalledIds.size > 0 ? (
            <Button
              size="sm"
              variant="danger"
              disabled={deletingInstalled || togglePending || returnPending}
              onClick={onOpenSelectedDelete}
            >
              删除通用技能 ({selectedInstalledIds.size})
            </Button>
          ) : null}
          <Button
            size="sm"
            variant="secondary"
            aria-label="检查更新"
            onClick={onCheckUpdates}
            disabled={
              loading || deletingInstalled || checkUpdatesPending || updatingSkillId !== null
            }
          >
            {checkUpdatesPending ? "检查中…" : "检查更新"}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            aria-label="刷新通用技能"
            onClick={onRefresh}
            disabled={loading || deletingInstalled}
          >
            {loading ? "刷新中…" : "刷新"}
          </Button>
          <span className="rounded-full bg-secondary px-2 py-1 text-xs font-medium text-secondary-foreground dark:bg-secondary dark:text-secondary-foreground">
            {installed.length}
          </span>
        </div>
      </div>

      <div className="mt-4 min-h-0 flex-1 space-y-2 lg:overflow-y-auto lg:pr-1 scrollbar-overlay">
        {loading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner size="sm" />
            加载中…
          </div>
        ) : installed.length === 0 ? (
          <EmptyState title="暂无已安装 Skill。" variant="dashed" />
        ) : (
          installed.map((skill) => (
            <InstalledSkillRow
              key={skill.id}
              skill={skill}
              selected={selectedInstalledIds.has(skill.id)}
              deletingInstalled={deletingInstalled}
              togglingSkillId={togglingSkillId}
              returningLocalSkillId={returningLocalSkillId}
              updatingSkillId={updatingSkillId}
              updateInfo={updateInfoMap.get(skill.id)}
              canOperateLocal={canOperateLocal}
              onToggleSelection={onToggleSelection}
              onToggleEnabled={onToggleEnabled}
              onReturnToLocal={onReturnToLocal}
              onUpdateSkill={onUpdateSkill}
              onDelete={onDelete}
            />
          ))
        )}
      </div>
    </Card>
  );
}

type InstalledSkillRowProps = {
  skill: InstalledSkillSummary;
  selected: boolean;
  deletingInstalled: boolean;
  togglingSkillId: number | null;
  returningLocalSkillId: number | null;
  updatingSkillId: number | null;
  updateInfo: SkillUpdateInfo | undefined;
  canOperateLocal: boolean;
  onToggleSelection: (skillId: number) => void;
  onToggleEnabled: (skill: InstalledSkillSummary, enabled: boolean) => void;
  onReturnToLocal: (skill: InstalledSkillSummary) => void;
  onUpdateSkill: (skill: InstalledSkillSummary) => void;
  onDelete: (skillId: number) => void;
};

function InstalledSkillRow({
  skill,
  selected,
  deletingInstalled,
  togglingSkillId,
  returningLocalSkillId,
  updatingSkillId,
  updateInfo,
  canOperateLocal,
  onToggleSelection,
  onToggleEnabled,
  onReturnToLocal,
  onUpdateSkill,
  onDelete,
}: InstalledSkillRowProps) {
  const repoPrefix = repoPrefixFromGitUrl(skill.source_git_url);
  const repoUrl = repositoryWebUrl(skill.source_git_url);
  const hasUpdate = updateInfo?.has_update ?? false;

  return (
    <div className="rounded-lg border border-line-subtle bg-card p-3">
      <div className="flex items-start gap-3">
        <input
          type="checkbox"
          checked={selected}
          onChange={() => onToggleSelection(skill.id)}
          disabled={
            deletingInstalled || togglingSkillId === skill.id || returningLocalSkillId === skill.id
          }
          className="mt-0.5 h-4 w-4 rounded border border-input bg-card accent-accent focus:ring-2 focus:ring-ring/30"
          aria-label={`选择通用技能 ${skill.name}`}
        />

        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="min-w-0">
            {repoPrefix ? (
              <div className="mb-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                {repoPrefix}
              </div>
            ) : null}
            <div className="flex min-w-0 items-center gap-2">
              <span className="min-w-0 truncate text-sm font-semibold">
                {displaySkillName(skill.name, skill.source_git_url)}
              </span>
              {hasUpdate ? (
                <span className="shrink-0 rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-700 dark:bg-amber-900/30 dark:text-amber-400">
                  有更新
                </span>
              ) : null}
              {repoUrl ? (
                <a
                  href={repoUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
                  title={sourceHint(skill)}
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                </a>
              ) : null}
            </div>
          </div>

          {skill.description ? (
            <div className="text-xs text-muted-foreground">{skill.description}</div>
          ) : null}

          <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
            <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <span
                className={cn(
                  "rounded-full px-2 py-1 font-medium",
                  skill.enabled
                    ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400"
                    : "bg-secondary text-muted-foreground"
                )}
              >
                {skill.enabled ? "已启用" : "未启用"}
              </span>
              <span>更新 {formatUnixSeconds(skill.updated_at)}</span>
            </div>

            <div className="flex shrink-0 items-center gap-2">
              <span className="text-xs text-muted-foreground">启用</span>
              <Switch
                checked={skill.enabled}
                disabled={
                  deletingInstalled ||
                  togglingSkillId === skill.id ||
                  returningLocalSkillId === skill.id ||
                  updatingSkillId === skill.id
                }
                onCheckedChange={(next) => onToggleEnabled(skill, next)}
              />
              {isUpdatableSkillSource(skill.source_git_url) && hasUpdate ? (
                <Button
                  size="sm"
                  variant="primary"
                  title="更新该技能到最新版本"
                  disabled={
                    deletingInstalled ||
                    updatingSkillId !== null ||
                    togglingSkillId === skill.id ||
                    returningLocalSkillId === skill.id
                  }
                  onClick={() => onUpdateSkill(skill)}
                >
                  {updatingSkillId === skill.id ? "更新中…" : "更新"}
                </Button>
              ) : null}
              <Button
                size="sm"
                variant="secondary"
                title={canOperateLocal ? "将该 Skill 从通用技能返回到本机已安装" : undefined}
                disabled={
                  !canOperateLocal ||
                  deletingInstalled ||
                  returningLocalSkillId === skill.id ||
                  updatingSkillId === skill.id
                }
                onClick={() => onReturnToLocal(skill)}
              >
                返回本机已安装
              </Button>
              <Button
                size="sm"
                variant="danger"
                aria-label={`删除通用技能 ${skill.name}`}
                disabled={
                  deletingInstalled ||
                  togglingSkillId === skill.id ||
                  returningLocalSkillId === skill.id ||
                  updatingSkillId === skill.id
                }
                onClick={() => onDelete(skill.id)}
              >
                删除
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

type LocalSkillsCardProps = {
  cliKey: CliKey;
  canOperateLocal: boolean;
  batchInitMode: boolean;
  localSkills: LocalSkillSummary[];
  localLoading: boolean;
  selectedLocalDirNames: Set<string>;
  allLocalSelected: boolean;
  deletingLocal: boolean;
  importingLocal: boolean;
  onToggleAll: () => void;
  onOpenSelectedDelete: () => void;
  onRefresh: () => void;
  onToggleSelection: (dirName: string) => void;
  onImport: (skill: LocalSkillSummary) => void;
  onDelete: (dirNames: string[]) => void;
  onOpenDir: (skill: LocalSkillSummary) => void;
};

function LocalSkillsCard({
  cliKey,
  canOperateLocal,
  batchInitMode,
  localSkills,
  localLoading,
  selectedLocalDirNames,
  allLocalSelected,
  deletingLocal,
  importingLocal,
  onToggleAll,
  onOpenSelectedDelete,
  onRefresh,
  onToggleSelection,
  onImport,
  onDelete,
  onOpenDir,
}: LocalSkillsCardProps) {
  return (
    <Card className="flex min-h-[240px] flex-col lg:min-h-0" padding="md">
      <div className="flex shrink-0 items-start justify-between gap-3">
        <div className="text-sm font-semibold">本机已安装</div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {canOperateLocal && localSkills.length > 0 ? (
            <label className="inline-flex items-center gap-2 text-xs text-muted-foreground">
              <input
                type="checkbox"
                checked={allLocalSelected}
                onChange={onToggleAll}
                disabled={deletingLocal || importingLocal}
                className="h-4 w-4 rounded border border-input bg-card accent-accent focus:ring-2 focus:ring-ring/30"
                aria-label="全选本机技能"
              />
              <span>全选</span>
            </label>
          ) : null}
          {selectedLocalDirNames.size > 0 ? (
            <Button
              size="sm"
              variant="danger"
              onClick={onOpenSelectedDelete}
              disabled={!canOperateLocal || deletingLocal || importingLocal}
            >
              删除本机技能 ({selectedLocalDirNames.size})
            </Button>
          ) : null}
          <Button
            size="sm"
            variant="secondary"
            aria-label="刷新本机技能"
            onClick={onRefresh}
            disabled={!canOperateLocal || localLoading || deletingLocal}
          >
            {localLoading ? "刷新中…" : "刷新"}
          </Button>
          <span className="rounded-full bg-secondary px-2 py-1 text-xs font-medium text-secondary-foreground dark:bg-secondary dark:text-secondary-foreground">
            {canOperateLocal ? (localLoading ? "扫描中…" : `${localSkills.length}`) : "—"}
          </span>
        </div>
      </div>

      <div className="mt-4 min-h-0 flex-1 space-y-2 lg:overflow-y-auto lg:pr-1 scrollbar-overlay">
        {!canOperateLocal ? (
          <EmptyState
            title={`仅当前工作区可扫描/导入本机 Skill（因为会直接读取/写入 ${cliKey} 的真实目录）。`}
            variant="dashed"
          />
        ) : localLoading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner size="sm" />
            扫描中…
          </div>
        ) : localSkills.length === 0 ? (
          <EmptyState title="未发现本机 Skill。" variant="dashed" />
        ) : (
          localSkills.map((skill) => (
            <LocalSkillRow
              key={skill.path}
              skill={skill}
              selected={selectedLocalDirNames.has(skill.dir_name)}
              batchInitMode={batchInitMode}
              deletingLocal={deletingLocal}
              importingLocal={importingLocal}
              onToggleSelection={onToggleSelection}
              onImport={onImport}
              onDelete={onDelete}
              onOpenDir={onOpenDir}
            />
          ))
        )}
      </div>
    </Card>
  );
}

type LocalSkillRowProps = {
  skill: LocalSkillSummary;
  selected: boolean;
  batchInitMode: boolean;
  deletingLocal: boolean;
  importingLocal: boolean;
  onToggleSelection: (dirName: string) => void;
  onImport: (skill: LocalSkillSummary) => void;
  onDelete: (dirNames: string[]) => void;
  onOpenDir: (skill: LocalSkillSummary) => void;
};

function LocalSkillRow({
  skill,
  selected,
  batchInitMode,
  deletingLocal,
  importingLocal,
  onToggleSelection,
  onImport,
  onDelete,
  onOpenDir,
}: LocalSkillRowProps) {
  const label = skill.name || skill.dir_name;
  const displayLabel = displaySkillName(label, skill.source_git_url);
  const repoUrl = repositoryWebUrl(skill.source_git_url ?? "");
  const repoPrefix = repoPrefixFromGitUrl(skill.source_git_url ?? "");

  return (
    <div className="rounded-lg border border-line-subtle bg-card p-3">
      <div className="flex items-start gap-3">
        <input
          type="checkbox"
          checked={selected}
          onChange={() => onToggleSelection(skill.dir_name)}
          disabled={deletingLocal || importingLocal}
          className="mt-0.5 h-4 w-4 rounded border border-input bg-card accent-accent focus:ring-2 focus:ring-ring/30"
          aria-label={`选择本机技能 ${label}`}
        />

        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="min-w-0">
            {repoPrefix ? (
              <div className="mb-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                {repoPrefix}
              </div>
            ) : null}
            <div className="flex min-w-0 items-center gap-2">
              <span className="min-w-0 truncate text-sm font-semibold">{displayLabel}</span>
              {repoUrl ? (
                <a
                  href={repoUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
                  title={sourceHint(skill)}
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                </a>
              ) : null}
            </div>
          </div>

          {skill.description ? (
            <div className="text-xs text-muted-foreground">{skill.description}</div>
          ) : null}

          <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
              {skill.path}
            </span>

            <div className="flex shrink-0 items-center gap-2">
              {batchInitMode ? null : (
                <Button
                  size="sm"
                  variant="primary"
                  disabled={deletingLocal || importingLocal}
                  onClick={() => onImport(skill)}
                >
                  导入技能库
                </Button>
              )}
              <Button
                size="sm"
                variant="danger"
                aria-label={`删除本机技能 ${displayLabel}`}
                disabled={deletingLocal || importingLocal}
                onClick={() => onDelete([skill.dir_name])}
              >
                删除
              </Button>
              <Button size="sm" variant="secondary" onClick={() => onOpenDir(skill)}>
                打开目录
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

type SkillsDeleteDialogsProps = {
  deleteInstalledDialogOpen: boolean;
  selectedInstalledSkills: InstalledSkillSummary[];
  deletingInstalled: boolean;
  onCloseInstalled: () => void;
  onConfirmDeleteInstalled: () => void;
  deleteLocalDialogOpen: boolean;
  localDeleteTargets: LocalSkillSummary[];
  deletingLocal: boolean;
  onCloseLocal: () => void;
  onConfirmDeleteLocal: () => void;
};

function SkillsDeleteDialogs({
  deleteInstalledDialogOpen,
  selectedInstalledSkills,
  deletingInstalled,
  onCloseInstalled,
  onConfirmDeleteInstalled,
  deleteLocalDialogOpen,
  localDeleteTargets,
  deletingLocal,
  onCloseLocal,
  onConfirmDeleteLocal,
}: SkillsDeleteDialogsProps) {
  return (
    <>
      <ConfirmDialog
        open={deleteInstalledDialogOpen}
        title="确认删除通用技能"
        description={`将删除 ${selectedInstalledSkills.length} 个通用技能，并同步移除受管目录，此操作不可撤销。`}
        onClose={onCloseInstalled}
        onConfirm={onConfirmDeleteInstalled}
        confirmLabel="确认删除"
        confirmingLabel="删除中…"
        confirming={deletingInstalled}
        confirmVariant="danger"
        disabled={selectedInstalledSkills.length === 0}
      >
        <div className="max-h-40 overflow-auto text-sm text-muted-foreground">
          <ul className="space-y-1">
            {selectedInstalledSkills.slice(0, 10).map((skill) => (
              <li key={skill.id} className="truncate">
                {skill.name}
              </li>
            ))}
            {selectedInstalledSkills.length > 10 ? (
              <li className="text-muted-foreground">
                ...还有 {selectedInstalledSkills.length - 10} 个
              </li>
            ) : null}
          </ul>
        </div>
      </ConfirmDialog>

      <ConfirmDialog
        open={deleteLocalDialogOpen}
        title="确认删除本机技能"
        description={`将删除 ${localDeleteTargets.length} 个本机技能目录，此操作不可撤销。`}
        onClose={onCloseLocal}
        onConfirm={onConfirmDeleteLocal}
        confirmLabel="确认删除"
        confirmingLabel="删除中…"
        confirming={deletingLocal}
        confirmVariant="danger"
        disabled={localDeleteTargets.length === 0}
      >
        <div className="max-h-48 space-y-2 overflow-auto text-xs text-muted-foreground">
          {localDeleteTargets.slice(0, 10).map((skill) => (
            <div key={skill.path} className="rounded-lg border border-line-subtle bg-secondary p-3">
              <div className="font-medium text-foreground">{skill.name || skill.dir_name}</div>
              <div className="mt-1 break-all font-mono">{skill.path}</div>
            </div>
          ))}
          {localDeleteTargets.length > 10 ? (
            <div className="text-muted-foreground">...还有 {localDeleteTargets.length - 10} 个</div>
          ) : null}
        </div>
      </ConfirmDialog>
    </>
  );
}
