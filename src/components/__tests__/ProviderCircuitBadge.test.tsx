import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCircuitBadge } from "../ProviderCircuitBadge";

describe("components/ProviderCircuitBadge", () => {
  it("returns null when rows is empty", () => {
    const { container } = render(
      <ProviderCircuitBadge rows={[]} onResetProvider={() => {}} resettingProviderIds={new Set()} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders rows, opens popover, and calls onResetProvider", async () => {
    const onResetProvider = vi.fn();
    const nowUnix = Math.floor(Date.now() / 1000);
    render(
      <ProviderCircuitBadge
        rows={[
          {
            cli_key: "claude",
            provider_id: 1,
            provider_name: "P1",
            open_until: nowUnix + 10,
          },
          {
            cli_key: "claude",
            provider_id: 2,
            provider_name: "P2",
            open_until: null,
          },
          {
            cli_key: "codex",
            provider_id: 3,
            provider_name: "P3",
            open_until: nowUnix + 5,
          },
        ]}
        onResetProvider={onResetProvider}
        resettingProviderIds={new Set([2])}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "当前熔断 3" }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());

    expect(screen.getByText("熔断列表 (3)")).toBeInTheDocument();
    expect(screen.getByText("P1")).toBeInTheDocument();
    expect(screen.getByText("P2")).toBeInTheDocument();
    expect(screen.getByText("P3")).toBeInTheDocument();

    // disabled state
    expect(screen.getAllByRole("button", { name: "解除中..." })[0]).toBeDisabled();

    // click reset (first one is for P1 in this fixture)
    fireEvent.click(screen.getAllByRole("button", { name: "解除熔断" })[0]);
    expect(onResetProvider).toHaveBeenCalledWith(1);
  });

  it("auto closes popover when rows become empty", async () => {
    const nowUnix = Math.floor(Date.now() / 1000);
    const { rerender } = render(
      <ProviderCircuitBadge
        rows={[
          {
            cli_key: "claude",
            provider_id: 1,
            provider_name: "P1",
            open_until: nowUnix + 10,
          },
        ]}
        onResetProvider={() => {}}
        resettingProviderIds={new Set()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "当前熔断 1" }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());

    rerender(
      <ProviderCircuitBadge rows={[]} onResetProvider={() => {}} resettingProviderIds={new Set()} />
    );

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });
});
