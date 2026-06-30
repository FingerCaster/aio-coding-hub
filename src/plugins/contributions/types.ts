import type { ActiveUiContribution, JsonValue } from "../../generated/bindings";

export type UiContributionSlotId =
  | "providers.editor.sections"
  | "settings.sections"
  | "logs.detail.tabs";

export type ContributionValues = Record<string, JsonValue>;

export function contributionKey(
  contribution: Pick<ActiveUiContribution, "pluginId" | "contributionId">
) {
  return `${contribution.pluginId}\u0000${contribution.contributionId}`;
}

export type ContributionCommandContext = {
  pluginId: string;
  contributionId: string;
};

export type ContributionCommandHandler = (
  command: string,
  context: ContributionCommandContext
) => void;

export type HostRenderedContributionProps = {
  contribution: ActiveUiContribution;
  values?: ContributionValues;
  onChange?: (key: string, value: JsonValue) => void;
  onCommand?: ContributionCommandHandler;
  disabled?: boolean;
};

export type ContributionSlotProps = {
  slotId: UiContributionSlotId;
  valuesByContributionKey?: Record<string, ContributionValues>;
  onChange?: (contribution: ActiveUiContribution, key: string, value: JsonValue) => void;
  onCommand?: ContributionCommandHandler;
  disabled?: boolean;
};

export type HostRenderedSchema =
  | {
      type: "section" | "panel";
      fields: HostRenderedField[];
    }
  | {
      type: "badge";
      label: string;
      tone?: "neutral" | "success" | "warning" | "danger";
    };

export type HostRenderedField =
  | TextField
  | PasswordField
  | NumberField
  | BooleanField
  | SelectField
  | TextareaField
  | InfoField
  | ButtonField;

type BaseField = {
  key: string;
  label: string;
};

export type TextField = BaseField & {
  type: "text";
  placeholder?: string;
  required?: boolean;
};

export type PasswordField = BaseField & {
  type: "password";
  placeholder?: string;
  required?: boolean;
};

export type NumberField = BaseField & {
  type: "number";
  min?: number;
  max?: number;
  step?: number;
};

export type BooleanField = BaseField & {
  type: "boolean";
};

export type SelectField = BaseField & {
  type: "select";
  options: Array<{ value: string; label: string }>;
};

export type TextareaField = BaseField & {
  type: "textarea";
  rows?: number;
};

export type InfoField = BaseField & {
  type: "info";
  value: string;
};

export type ButtonField = BaseField & {
  type: "button";
  command: string;
};
