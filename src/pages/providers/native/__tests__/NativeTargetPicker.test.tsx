import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as service from "../../../../services/nativeCli";
import { deferred, nativeTarget } from "../../../../test/fixtures/native";
import { createQueryWrapper, createTestQueryClient } from "../../../../test/utils/reactQuery";
import { NativeTargetPicker } from "../NativeTargetPicker";
vi.mock("../../../../services/nativeCli", async (original) => ({
  ...(await original<typeof service>()),
  nativeCliTargetValidate: vi.fn(),
  nativeCliTargetSelect: vi.fn(),
}));
beforeEach(() => vi.clearAllMocks());
function mount(client: "pi" | "omp" = "omp") {
  const onSelect = vi.fn();
  render(
    <NativeTargetPicker
      client={client}
      targets={[nativeTarget({ client })]}
      targetId="pi:local:a"
      onSelect={onSelect}
    />,
    { wrapper: createQueryWrapper(createTestQueryClient()) }
  );
  return onSelect;
}
describe("native target selection", () => {
  it("validates OMP profile before explicitly selecting its canonical target", async () => {
    const target = nativeTarget({
      client: "omp",
      profile: "work",
      targetId: "omp:work",
      modelsPath: "C:/omp/profiles/work/models.yml",
    });
    vi.mocked(service.nativeCliTargetValidate).mockResolvedValue(target);
    vi.mocked(service.nativeCliTargetSelect).mockResolvedValue(target);
    const onSelect = mount();
    fireEvent.change(screen.getByLabelText("目标来源"), { target: { value: "profile" } });
    fireEvent.change(screen.getByLabelText("OMP Profile 名称"), { target: { value: "work" } });
    expect(screen.getByRole("button", { name: "使用此目标", hidden: true })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "验证目标", hidden: true }));
    await screen.findByText(/已验证：/);
    expect(service.nativeCliTargetSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "使用此目标", hidden: true }));
    await waitFor(() => expect(onSelect).toHaveBeenCalledWith(target));
    expect(vi.mocked(service.nativeCliTargetSelect).mock.calls[0][0]).toEqual({
      client: "omp",
      mode: "profile",
      agentDir: null,
      profile: "work",
    });
  });
  it("drops validation that resolves after the user changes the directory", async () => {
    const pending = deferred<ReturnType<typeof nativeTarget>>();
    vi.mocked(service.nativeCliTargetValidate).mockReturnValue(pending.promise);
    mount("pi");
    fireEvent.change(screen.getByLabelText("目标来源"), { target: { value: "custom" } });
    fireEvent.change(screen.getByLabelText("Agent 绝对目录"), { target: { value: "C:/first" } });
    fireEvent.click(screen.getByRole("button", { name: "验证目标", hidden: true }));
    fireEvent.change(screen.getByLabelText("Agent 绝对目录"), { target: { value: "C:/second" } });
    await act(async () => pending.resolve(nativeTarget()));
    expect(screen.queryByText(/已验证/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "使用此目标", hidden: true })).toBeDisabled();
    expect(screen.queryByText("OMP Profile")).not.toBeInTheDocument();
  });
});
