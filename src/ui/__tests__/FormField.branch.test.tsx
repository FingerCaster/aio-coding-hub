import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FormField } from "../FormField";

describe("ui/FormField (branch coverage)", () => {
  it("supports render-prop children with auto-generated id", () => {
    render(
      <FormField label="Render Prop">{(id) => <input id={id} data-testid="rp-input" />}</FormField>
    );

    const label = screen.getByText("Render Prop");
    const input = screen.getByTestId("rp-input");

    // label htmlFor should match input id
    expect(label.closest("label")).toHaveAttribute("for", input.id);
  });

  it("supports render-prop children with explicit htmlFor", () => {
    render(
      <FormField label="Explicit" htmlFor="my-id">
        {(id) => <input id={id} data-testid="explicit-input" />}
      </FormField>
    );

    const input = screen.getByTestId("explicit-input");
    expect(input.id).toBe("my-id");
  });

  it("exposes the hint id to render-prop controls", () => {
    render(
      <FormField label="Origin" hint="每行一个 HTTPS Origin">
        {(id, hintId) => <input id={id} aria-describedby={hintId} data-testid="hinted-input" />}
      </FormField>
    );

    const input = screen.getByTestId("hinted-input");
    const hint = screen.getByText("每行一个 HTTPS Origin");
    expect(hint).toHaveAttribute("id");
    expect(input).toHaveAttribute("aria-describedby", hint.id);
    expect(input).toHaveAccessibleDescription("每行一个 HTTPS Origin");
  });

  it("uses labelled group semantics for composite controls", () => {
    render(
      <FormField label="认证方式" hint="选择一种方式" group>
        <button type="button">API 密钥</button>
        <button type="button">OAuth</button>
      </FormField>
    );

    const group = screen.getByRole("group", { name: "认证方式" });
    expect(group).toHaveAccessibleDescription("选择一种方式");
    expect(screen.getByText("认证方式").closest("label")).toBeNull();
  });
});
