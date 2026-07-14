export type CodexApprovalReviewerKind = "unset" | "user" | "auto_review" | "unknown";

export type CodexApprovalReviewerNotice =
  | "none"
  | "unknown-reviewer"
  | "auto-review-inherited-policy"
  | "auto-review-inactive-policy"
  | "user-never";

export type CodexApprovalReviewerPresentation = {
  reviewerKind: CodexApprovalReviewerKind;
  unknownReviewer: string | null;
  notice: CodexApprovalReviewerNotice;
  canSwitchToOnRequest: boolean;
};

export function classifyCodexApprovalReviewer(
  approvalsReviewer: string | null | undefined,
  approvalPolicy: string | null | undefined
): CodexApprovalReviewerPresentation {
  const reviewer = approvalsReviewer ?? "";
  const policy = approvalPolicy ?? "";

  if (reviewer === "") {
    return {
      reviewerKind: "unset",
      unknownReviewer: null,
      notice: "none",
      canSwitchToOnRequest: false,
    };
  }

  if (reviewer !== "user" && reviewer !== "auto_review") {
    return {
      reviewerKind: "unknown",
      unknownReviewer: reviewer,
      notice: "unknown-reviewer",
      canSwitchToOnRequest: false,
    };
  }

  if (reviewer === "auto_review") {
    if (policy === "") {
      return {
        reviewerKind: reviewer,
        unknownReviewer: null,
        notice: "auto-review-inherited-policy",
        canSwitchToOnRequest: false,
      };
    }

    if (policy !== "on-request") {
      return {
        reviewerKind: reviewer,
        unknownReviewer: null,
        notice: "auto-review-inactive-policy",
        canSwitchToOnRequest: true,
      };
    }
  }

  if (reviewer === "user" && policy === "never") {
    return {
      reviewerKind: reviewer,
      unknownReviewer: null,
      notice: "user-never",
      canSwitchToOnRequest: true,
    };
  }

  return {
    reviewerKind: reviewer,
    unknownReviewer: null,
    notice: "none",
    canSwitchToOnRequest: false,
  };
}
