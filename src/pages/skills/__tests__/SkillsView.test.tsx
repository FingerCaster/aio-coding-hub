import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { SkillsView } from "../SkillsView";
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
} from "../../../query/skills";
import { tauriOpenPath, tauriRevealItemInDir } from "../../../test/mocks/tauri";

vi.mock("sonner", () => ({ toast: vi.fn() }));
vi.mock("../../../services/consoleLog", () => ({ logToConsole: vi.fn() }));

vi.mock("../../../query/skills", async () => {
  const actual =
    await vi.importActual<typeof import("../../../query/skills")>("../../../query/skills");
  return {
    ...actual,
    useSkillsInstalledListQuery: vi.fn(),
    useSkillsLocalListQuery: vi.fn(),
    useSkillSetEnabledMutation: vi.fn(),
    useSkillUninstallMutation: vi.fn(),
    useSkillReturnToLocalMutation: vi.fn(),
    useSkillLocalDeleteMutation: vi.fn(),
    useSkillImportLocalMutation: vi.fn(),
    useSkillCheckUpdatesMutation: vi.fn(),
    useSkillUpdateMutation: vi.fn(),
  };
});

describe("pages/skills/SkillsView", () => {
  it("supports enabling/deleting/returning installed skills and importing/deleting local skills", async () => {
    const installed = [
      {
        id: 1,
        name: "My Skill",
        description: "desc",
        enabled: false,
        source_git_url: "https://example.com/repo.git",
        source_branch: "main",
        source_subdir: "skills/my",
        updated_at: 123,
      },
    ] as any[];

    const localSkills = [
      { dir_name: "local-skill", name: "Local Skill", description: "d", path: "/tmp/local-skill" },
    ] as any[];

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: installed,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
    } as any);

    const toggleMutation = { isPending: false, mutateAsync: vi.fn() };
    toggleMutation.mutateAsync.mockResolvedValue({ ...installed[0], enabled: true });
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue(toggleMutation as any);

    const uninstallMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    uninstallMutation.mutateAsync.mockResolvedValue(true);
    vi.mocked(useSkillUninstallMutation).mockReturnValue(uninstallMutation as any);

    const returnToLocalMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    returnToLocalMutation.mutateAsync.mockResolvedValue(true);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue(returnToLocalMutation as any);

    const localDeleteMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    localDeleteMutation.mutateAsync.mockResolvedValue(true);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue(localDeleteMutation as any);

    const importMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    importMutation.mutateAsync.mockResolvedValue({ id: 2 });
    vi.mocked(useSkillImportLocalMutation).mockReturnValue(importMutation as any);

    const checkUpdatesMutation = { isPending: false, mutateAsync: vi.fn() };
    checkUpdatesMutation.mutateAsync.mockResolvedValue([]);
    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue(checkUpdatesMutation as any);

    const updateSkillMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    vi.mocked(useSkillUpdateMutation).mockReturnValue(updateSkillMutation as any);

    tauriOpenPath.mockRejectedValueOnce(new Error("no opener"));
    tauriRevealItemInDir.mockResolvedValueOnce(undefined as any);

    render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);

    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(toggleMutation.mutateAsync).toHaveBeenCalledWith({ skillId: 1, enabled: true })
    );

    fireEvent.click(screen.getByRole("button", { name: "删除通用技能 My Skill" }));
    let dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(uninstallMutation.mutateAsync).toHaveBeenCalledWith(1));

    fireEvent.click(screen.getByRole("button", { name: "返回本机已安装" }));
    await waitFor(() => expect(returnToLocalMutation.mutateAsync).toHaveBeenCalledWith(1));

    const importButton = await screen.findByRole("button", { name: "导入技能库" });
    fireEvent.click(importButton);
    await waitFor(() => expect(importMutation.mutateAsync).toHaveBeenCalledWith("local-skill"));

    fireEvent.click(screen.getByRole("button", { name: "删除本机技能 Local Skill" }));
    dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() =>
      expect(localDeleteMutation.mutateAsync).toHaveBeenCalledWith("local-skill")
    );

    fireEvent.click(screen.getByRole("button", { name: "打开目录" }));
    await waitFor(() => expect(tauriRevealItemInDir).toHaveBeenCalledWith("/tmp/local-skill"));
  });

  it("supports batch deleting installed and local skills", async () => {
    const installed = [
      {
        id: 1,
        name: "Skill A",
        description: "A",
        enabled: true,
        source_git_url: "https://example.com/repo-a.git",
        source_branch: "main",
        source_subdir: "skills/a",
        updated_at: 100,
      },
      {
        id: 2,
        name: "Skill B",
        description: "B",
        enabled: false,
        source_git_url: "https://example.com/repo-b.git",
        source_branch: "main",
        source_subdir: "skills/b",
        updated_at: 200,
      },
    ] as any[];

    const localSkills = [
      { dir_name: "local-a", name: "Local A", description: "A", path: "/tmp/local-a" },
      { dir_name: "local-b", name: "Local B", description: "B", path: "/tmp/local-b" },
    ] as any[];

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: installed,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
      refetch: vi.fn().mockResolvedValue({ data: localSkills }),
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    } as any);

    const uninstallMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    uninstallMutation.mutateAsync.mockResolvedValue(true);
    vi.mocked(useSkillUninstallMutation).mockReturnValue(uninstallMutation as any);

    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    const localDeleteMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    localDeleteMutation.mutateAsync.mockResolvedValue(true);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue(localDeleteMutation as any);

    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue([]),
    } as any);

    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);

    fireEvent.click(screen.getByRole("checkbox", { name: "全选通用技能" }));
    fireEvent.click(screen.getByRole("button", { name: "删除通用技能 (2)" }));

    let dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(uninstallMutation.mutateAsync).toHaveBeenCalledTimes(2));
    expect(uninstallMutation.mutateAsync.mock.calls.map(([skillId]) => skillId)).toEqual([1, 2]);

    fireEvent.click(screen.getByRole("checkbox", { name: "全选本机技能" }));
    fireEvent.click(screen.getByRole("button", { name: "删除本机技能 (2)" }));

    dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(localDeleteMutation.mutateAsync).toHaveBeenCalledTimes(2));
    expect(localDeleteMutation.mutateAsync.mock.calls.map(([dirName]) => dirName)).toEqual([
      "local-a",
      "local-b",
    ]);
  });

  it("keeps batch_init entry as refresh-only for local skills", async () => {
    const localSkills = [
      { dir_name: "local-skill", name: "Local Skill", description: "d", path: "/tmp/local-skill" },
      {
        dir_name: " another-skill ",
        name: "Another Skill",
        description: "d2",
        path: "/tmp/another-skill",
      },
      {
        dir_name: "local-skill",
        name: "Local Skill Dup",
        description: "dup",
        path: "/tmp/local-skill-dup",
      },
    ] as any[];

    const refetch = vi.fn().mockResolvedValue({ data: localSkills });

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: [],
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
      refetch,
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUninstallMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue([]),
    } as any);

    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    render(
      <SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace localImportMode="batch_init" />
    );

    expect(screen.queryByRole("button", { name: "导入技能库" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "初始化同步" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "刷新本机技能" }));
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));
  });

  it("supports refreshing local list", async () => {
    const localSkills = [
      { dir_name: "local-skill", name: "Local Skill", description: "d", path: "/tmp/local-skill" },
    ] as any[];
    const refetch = vi.fn().mockResolvedValue({ data: localSkills });

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: [],
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
      refetch,
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUninstallMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue([]),
    } as any);

    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);

    fireEvent.click(screen.getByRole("button", { name: "刷新本机技能" }));
    await waitFor(() => expect(refetch).toHaveBeenCalledTimes(1));
  });

  it("renders read-only local section when workspace is not active", () => {
    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: [],
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: [],
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUninstallMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue([]),
    } as any);

    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    render(<SkillsView workspaceId={1} cliKey="gemini" isActiveWorkspace={false} />);
    expect(screen.getByText(/仅当前工作区可扫描\/导入本机 Skill/)).toBeInTheDocument();
  });

  it("covers tauri-only + error branches and local delete/import guards when workspace becomes inactive", async () => {
    const installed = [
      {
        id: 1,
        name: "S1",
        description: null,
        enabled: false,
        source_git_url: "https://example.com/repo.git",
        source_branch: "",
        source_subdir: "skills/s1",
        updated_at: 123,
      },
      {
        id: 2,
        name: "S2",
        description: "d",
        enabled: true,
        source_git_url: "https://example.com/repo2.git",
        source_branch: "main",
        source_subdir: "skills/s2",
        updated_at: 456,
      },
    ] as any[];

    const localSkills = [
      { dir_name: "local-skill", name: "", description: null, path: "/tmp/local-skill" },
    ] as any[];

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: installed,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
      refetch: vi.fn().mockResolvedValue({ data: localSkills }),
    } as any);

    const toggleMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    toggleMutation.mutateAsync
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({ ...installed[1], enabled: false })
      .mockRejectedValueOnce(new Error("boom"));
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue(toggleMutation as any);

    const uninstallMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    uninstallMutation.mutateAsync
      .mockResolvedValueOnce(false)
      .mockRejectedValueOnce(new Error("boom"));
    vi.mocked(useSkillUninstallMutation).mockReturnValue(uninstallMutation as any);

    const returnToLocalMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    returnToLocalMutation.mutateAsync
      .mockResolvedValueOnce(false)
      .mockRejectedValueOnce(new Error("boom"));
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue(returnToLocalMutation as any);

    const localDeleteMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    localDeleteMutation.mutateAsync.mockResolvedValueOnce(false);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue(localDeleteMutation as any);

    const importMutation = { isPending: false, mutateAsync: vi.fn(), variables: null };
    importMutation.mutateAsync.mockResolvedValueOnce(null);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue(importMutation as any);

    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue([]),
    } as any);

    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    tauriOpenPath
      .mockResolvedValueOnce(undefined as any)
      .mockRejectedValueOnce(new Error("no opener"));
    tauriRevealItemInDir.mockRejectedValueOnce(new Error("reveal failed"));

    function Wrapper() {
      const [active, setActive] = useState(true);
      return (
        <div>
          <button type="button" onClick={() => setActive(false)}>
            deactivate
          </button>
          <SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace={active} />
        </div>
      );
    }

    render(<Wrapper />);

    fireEvent.click(screen.getAllByRole("switch")[0]!);
    await waitFor(() => expect(toggleMutation.mutateAsync).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getAllByRole("switch")[1]!);
    await waitFor(() => expect(toggleMutation.mutateAsync).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getAllByRole("switch")[0]!);
    await waitFor(() => expect(toggleMutation.mutateAsync).toHaveBeenCalledTimes(3));

    fireEvent.click(screen.getByRole("button", { name: "删除通用技能 S1" }));
    let dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(uninstallMutation.mutateAsync).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "删除通用技能 S1" }));
    dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(uninstallMutation.mutateAsync).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole("button", { name: "打开目录" }));
    await waitFor(() => expect(tauriOpenPath).toHaveBeenCalledWith("/tmp/local-skill"));

    fireEvent.click(screen.getByRole("button", { name: "打开目录" }));
    await waitFor(() =>
      expect(vi.mocked(toast)).toHaveBeenCalledWith("打开目录失败：请查看控制台日志")
    );

    fireEvent.click(screen.getAllByRole("button", { name: "返回本机已安装" })[0]!);
    await waitFor(() => expect(returnToLocalMutation.mutateAsync).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getAllByRole("button", { name: "返回本机已安装" })[0]!);
    await waitFor(() => expect(returnToLocalMutation.mutateAsync).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole("button", { name: "删除本机技能 local-skill" }));
    dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(localDeleteMutation.mutateAsync).toHaveBeenCalledTimes(1));

    const importButton = await screen.findByRole("button", { name: "导入技能库" });
    fireEvent.click(importButton);
    await waitFor(() => expect(importMutation.mutateAsync).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "删除本机技能 local-skill" }));
    dialog = within(screen.getByRole("dialog"));
    fireEvent.click(screen.getByRole("button", { name: "deactivate", hidden: true }));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() =>
      expect(vi.mocked(toast)).toHaveBeenCalledWith(
        expect.stringContaining("仅当前工作区可删除本机 Skill")
      )
    );
    expect(localDeleteMutation.mutateAsync).toHaveBeenCalledTimes(1);
    expect(importMutation.mutateAsync).toHaveBeenCalledTimes(1);

    const refreshButton = screen.getByRole("button", { name: "刷新本机技能", hidden: true });
    expect(refreshButton).toBeDisabled();
  });

  it("covers update checks, skill updates, partial delete failures, and selection pruning", async () => {
    const installed = [
      {
        id: 1,
        name: "Skill A",
        description: "A",
        enabled: true,
        source_git_url: "https://github.com/example/repo.git",
        source_branch: "main",
        source_subdir: "skills/a",
        updated_at: 100,
      },
      {
        id: 2,
        name: "Skill B",
        description: "B",
        enabled: true,
        source_git_url: "local://skill-b",
        source_branch: "main",
        source_subdir: "skills/b",
        updated_at: 200,
      },
      {
        id: 3,
        name: "Skill C",
        description: null,
        enabled: false,
        source_git_url: "",
        source_branch: "",
        source_subdir: "",
        updated_at: 300,
      },
    ] as any[];
    const localSkills = [
      {
        dir_name: "local-a",
        name: "Local A",
        description: "A",
        path: "/tmp/local-a",
        source_git_url: "https://github.com/example/repo.git",
        source_branch: "main",
        source_subdir: "skills/local-a",
      },
      {
        dir_name: "local-b",
        name: "",
        description: null,
        path: "/tmp/local-b",
      },
    ] as any[];

    let currentInstalled = installed;
    let currentLocal = localSkills;
    const refetchInstalled = vi.fn().mockResolvedValue({ data: installed });
    const refetchLocal = vi.fn().mockImplementation(() => {
      currentLocal = [localSkills[0]];
      return Promise.resolve({ data: currentLocal });
    });
    vi.mocked(useSkillsInstalledListQuery).mockImplementation(
      () =>
        ({
          data: currentInstalled,
          isFetching: false,
          error: null,
          refetch: refetchInstalled,
        }) as any
    );
    vi.mocked(useSkillsLocalListQuery).mockImplementation(
      () =>
        ({
          data: currentLocal,
          isFetching: false,
          error: null,
          refetch: refetchLocal,
        }) as any
    );

    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue({ ...installed[2], enabled: true }),
    } as any);
    const uninstallMutation = {
      isPending: false,
      mutateAsync: vi
        .fn()
        .mockResolvedValueOnce(true)
        .mockResolvedValueOnce(false)
        .mockRejectedValueOnce(new Error("delete c down")),
      variables: null,
    };
    vi.mocked(useSkillUninstallMutation).mockReturnValue(uninstallMutation as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue(true),
      variables: null,
    } as any);
    const localDeleteMutation = {
      isPending: false,
      mutateAsync: vi
        .fn()
        .mockResolvedValueOnce(true)
        .mockRejectedValueOnce(new Error("local down")),
      variables: null,
    };
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue(localDeleteMutation as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockRejectedValue(new Error("import down")),
      variables: null,
    } as any);
    const checkUpdates = vi
      .fn()
      .mockResolvedValueOnce([
        {
          skill_id: 1,
          has_update: true,
          current_rev: "a",
          latest_rev: "b",
        },
        {
          skill_id: 2,
          has_update: false,
          current_rev: "a",
          latest_rev: "a",
        },
      ])
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error("check down"));
    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: checkUpdates,
    } as any);
    const updateSkill = vi
      .fn()
      .mockResolvedValueOnce({ id: 10, name: "Skill A", enabled: true })
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(new Error("update down"));
    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: updateSkill,
      variables: null,
    } as any);
    tauriOpenPath.mockResolvedValue(undefined as any);

    const { rerender } = render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);

    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await waitFor(() => expect(checkUpdates).toHaveBeenCalledTimes(1));
    expect(toast).toHaveBeenCalledWith("发现 1 个技能有更新");
    expect(screen.getByText("有更新")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "更新" }));
    await waitFor(() => expect(updateSkill).toHaveBeenCalledWith(1));
    expect(toast).toHaveBeenCalledWith("技能已更新");
    expect(refetchInstalled).toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await waitFor(() => expect(checkUpdates).toHaveBeenCalledTimes(2));
    expect(toast).toHaveBeenCalledWith("没有发现可更新的技能");

    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await waitFor(() => expect(checkUpdates).toHaveBeenCalledTimes(3));
    expect(toast).toHaveBeenCalledWith(expect.stringContaining("检查更新失败"));

    fireEvent.click(screen.getByRole("checkbox", { name: "全选通用技能" }));
    fireEvent.click(screen.getByRole("button", { name: "删除通用技能 (3)" }));
    let dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(uninstallMutation.mutateAsync).toHaveBeenCalledTimes(3));
    expect(toast).toHaveBeenCalledWith("已删除通用技能");
    expect(toast).toHaveBeenCalledWith("2 个通用技能删除失败");

    fireEvent.click(screen.getByRole("checkbox", { name: "全选本机技能" }));
    fireEvent.click(screen.getByRole("button", { name: "删除本机技能 (2)" }));
    dialog = within(screen.getByRole("dialog"));
    fireEvent.click(dialog.getByRole("button", { name: "确认删除" }));
    await waitFor(() => expect(localDeleteMutation.mutateAsync).toHaveBeenCalledTimes(2));
    expect(toast).toHaveBeenCalledWith("已删除本机技能");
    expect(toast).toHaveBeenCalledWith("1 个本机技能删除失败");

    fireEvent.click(screen.getAllByRole("button", { name: "导入技能库" })[0]!);
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining("导入失败")));

    fireEvent.click(screen.getByRole("button", { name: "刷新本机技能" }));
    await waitFor(() => expect(refetchLocal).toHaveBeenCalled());

    currentInstalled = [installed[0]];
    rerender(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);
    await waitFor(() => expect(screen.queryByText("Skill B")).not.toBeInTheDocument());
  });

  it("renders pending states", async () => {
    const installed = Array.from({ length: 12 }, (_, index) => ({
      id: index + 1,
      name: `Skill ${index + 1}`,
      description: index % 2 === 0 ? `Description ${index + 1}` : "",
      enabled: index % 2 === 0,
      source_git_url: index === 0 ? "https://github.com/example/repo.git" : "",
      source_branch: "main",
      source_subdir: `skills/${index + 1}`,
      updated_at: 100 + index,
    })) as any[];
    const localSkills = Array.from({ length: 12 }, (_, index) => ({
      dir_name: `local-${index + 1}`,
      name: `Local ${index + 1}`,
      description: "",
      path: `/tmp/local-${index + 1}`,
    })) as any[];

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: installed,
      isFetching: false,
      error: null,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: true,
      variables: { skillId: 1 },
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUninstallMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: true,
      variables: 2,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: true,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: true,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue(null),
      variables: null,
    } as any);

    render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);

    expect(screen.getByRole("button", { name: "检查更新" })).toBeDisabled();
    expect(screen.getByText("检查中…")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "选择通用技能 Skill 1" })).toBeDisabled();
    expect(screen.getAllByRole("button", { name: "导入技能库" })[0]).toBeDisabled();
  });

  it("renders long delete previews for installed and local skills", async () => {
    const installed = Array.from({ length: 12 }, (_, index) => ({
      id: index + 1,
      name: `Skill ${index + 1}`,
      description: "",
      enabled: index % 2 === 0,
      source_git_url: "",
      source_branch: "",
      source_subdir: "",
      updated_at: 100 + index,
    })) as any[];
    const localSkills = Array.from({ length: 12 }, (_, index) => ({
      dir_name: `local-${index + 1}`,
      name: `Local ${index + 1}`,
      description: "",
      path: `/tmp/local-${index + 1}`,
    })) as any[];

    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: installed,
      isFetching: false,
      error: null,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: localSkills,
      isFetching: false,
      error: null,
      refetch: vi.fn(),
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      variables: null,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUninstallMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      variables: null,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue(null),
      variables: null,
    } as any);

    render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace />);

    fireEvent.click(screen.getByRole("checkbox", { name: "全选通用技能" }));
    fireEvent.click(screen.getByRole("button", { name: "删除通用技能 (12)" }));
    expect(screen.getByText("...还有 2 个")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));

    fireEvent.click(screen.getByRole("checkbox", { name: "全选本机技能" }));
    fireEvent.click(screen.getByRole("button", { name: "删除本机技能 (12)" }));
    expect(screen.getByText("...还有 2 个")).toBeInTheDocument();
  });

  it("shows inactive-workspace feedback for installed-skill local return", async () => {
    const installed = [
      {
        id: 1,
        name: "Remote Skill",
        description: "",
        enabled: true,
        source_git_url: "https://github.com/example/repo.git",
        source_branch: "main",
        source_subdir: "skills/remote",
        updated_at: 100,
      },
    ] as any[];
    vi.mocked(useSkillsInstalledListQuery).mockReturnValue({
      data: installed,
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillsLocalListQuery).mockReturnValue({
      data: [],
      isFetching: false,
      error: null,
    } as any);
    vi.mocked(useSkillSetEnabledMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    } as any);
    vi.mocked(useSkillUninstallMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillReturnToLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillLocalDeleteMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillImportLocalMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);
    vi.mocked(useSkillCheckUpdatesMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockResolvedValue([]),
    } as any);
    vi.mocked(useSkillUpdateMutation).mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: null,
    } as any);

    render(<SkillsView workspaceId={1} cliKey="claude" isActiveWorkspace={false} />);

    expect(screen.getByRole("button", { name: "返回本机已安装" })).toBeDisabled();
    expect(screen.getByText(/仅当前工作区可扫描\/导入本机 Skill/)).toBeInTheDocument();
  });
});
