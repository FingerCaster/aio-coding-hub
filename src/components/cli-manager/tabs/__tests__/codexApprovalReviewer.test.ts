import { describe, expect, it } from "vitest";
import { classifyCodexApprovalReviewer } from "../codexApprovalReviewer";

describe("classifyCodexApprovalReviewer", () => {
  it.each([
    [null, null, "unset", "none", false],
    ["user", null, "user", "none", false],
    ["user", "on-request", "user", "none", false],
    ["user", "untrusted", "user", "none", false],
    ["user", "on-failure", "user", "none", false],
    ["user", "never", "user", "user-never", true],
    ["auto_review", null, "auto_review", "auto-review-inherited-policy", false],
    ["auto_review", "on-request", "auto_review", "none", false],
    ["auto_review", "never", "auto_review", "auto-review-inactive-policy", true],
    ["auto_review", "untrusted", "auto_review", "auto-review-inactive-policy", true],
    ["auto_review", "on-failure", "auto_review", "auto-review-inactive-policy", true],
    ["auto_review", "future-policy", "auto_review", "auto-review-inactive-policy", true],
  ] as const)(
    "classifies reviewer=%s policy=%s",
    (reviewer, policy, reviewerKind, notice, canSwitchToOnRequest) => {
      expect(classifyCodexApprovalReviewer(reviewer, policy)).toEqual({
        reviewerKind,
        unknownReviewer: null,
        notice,
        canSwitchToOnRequest,
      });
    }
  );

  it("preserves an unknown reviewer verbatim and does not infer policy behavior", () => {
    expect(classifyCodexApprovalReviewer(" future_reviewer ", "never")).toEqual({
      reviewerKind: "unknown",
      unknownReviewer: " future_reviewer ",
      notice: "unknown-reviewer",
      canSwitchToOnRequest: false,
    });
  });
});
