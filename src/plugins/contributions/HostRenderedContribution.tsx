import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { FormField } from "../../ui/FormField";
import { Input } from "../../ui/Input";
import { Switch } from "../../ui/Switch";
import { Textarea } from "../../ui/Textarea";
import { cn } from "../../utils/cn";
import type {
  BooleanField,
  ButtonField,
  HostRenderedContributionProps,
  HostRenderedField,
  HostRenderedSchema,
  InfoField,
  NumberField,
  PasswordField,
  SelectField,
  TextareaField,
  TextField,
} from "./types";

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isOptionalString(value: unknown): value is string | undefined {
  return value === undefined || typeof value === "string";
}

function isOptionalNumber(value: unknown): value is number | undefined {
  return value === undefined || typeof value === "number";
}

function isOptionalBoolean(value: unknown): value is boolean | undefined {
  return value === undefined || typeof value === "boolean";
}

function parseTextField(raw: Record<string, unknown>): TextField | PasswordField | null {
  if (
    (raw.type !== "text" && raw.type !== "password") ||
    !isString(raw.key) ||
    !isString(raw.label) ||
    !isOptionalString(raw.placeholder) ||
    !isOptionalBoolean(raw.required)
  ) {
    return null;
  }

  return {
    type: raw.type,
    key: raw.key,
    label: raw.label,
    placeholder: raw.placeholder,
    required: raw.required,
  };
}

function parseNumberField(raw: Record<string, unknown>): NumberField | null {
  if (
    raw.type !== "number" ||
    !isString(raw.key) ||
    !isString(raw.label) ||
    !isOptionalNumber(raw.min) ||
    !isOptionalNumber(raw.max) ||
    !isOptionalNumber(raw.step)
  ) {
    return null;
  }

  return {
    type: "number",
    key: raw.key,
    label: raw.label,
    min: raw.min,
    max: raw.max,
    step: raw.step,
  };
}

function parseBooleanField(raw: Record<string, unknown>): BooleanField | null {
  if (raw.type !== "boolean" || !isString(raw.key) || !isString(raw.label)) return null;
  return { type: "boolean", key: raw.key, label: raw.label };
}

function parseSelectField(raw: Record<string, unknown>): SelectField | null {
  if (raw.type !== "select" || !isString(raw.key) || !isString(raw.label)) return null;
  if (!Array.isArray(raw.options)) return null;

  const options = raw.options
    .map((option) => {
      if (!isRecord(option) || !isString(option.value) || !isString(option.label)) return null;
      return { value: option.value, label: option.label };
    })
    .filter((option): option is { value: string; label: string } => option !== null);

  if (options.length !== raw.options.length) return null;
  return { type: "select", key: raw.key, label: raw.label, options };
}

function parseTextareaField(raw: Record<string, unknown>): TextareaField | null {
  if (
    raw.type !== "textarea" ||
    !isString(raw.key) ||
    !isString(raw.label) ||
    !isOptionalNumber(raw.rows)
  ) {
    return null;
  }
  return { type: "textarea", key: raw.key, label: raw.label, rows: raw.rows };
}

function parseInfoField(raw: Record<string, unknown>): InfoField | null {
  if (raw.type !== "info" || !isString(raw.key) || !isString(raw.label) || !isString(raw.value)) {
    return null;
  }
  return { type: "info", key: raw.key, label: raw.label, value: raw.value };
}

function parseButtonField(raw: Record<string, unknown>): ButtonField | null {
  if (
    raw.type !== "button" ||
    !isString(raw.key) ||
    !isString(raw.label) ||
    !isString(raw.command)
  ) {
    return null;
  }
  return { type: "button", key: raw.key, label: raw.label, command: raw.command };
}

function parseField(raw: unknown): HostRenderedField | null {
  if (!isRecord(raw)) return null;

  return (
    parseTextField(raw) ??
    parseNumberField(raw) ??
    parseBooleanField(raw) ??
    parseSelectField(raw) ??
    parseTextareaField(raw) ??
    parseInfoField(raw) ??
    parseButtonField(raw)
  );
}

function parseSchema(raw: unknown): HostRenderedSchema | null {
  if (!isRecord(raw) || !isString(raw.type)) return null;

  if (raw.type === "section" || raw.type === "panel") {
    if (!Array.isArray(raw.fields)) return null;
    const fields = raw.fields.map(parseField);
    if (fields.some((field) => field === null)) return null;
    return { type: raw.type, fields: fields as HostRenderedField[] };
  }

  if (raw.type === "badge") {
    if (!isString(raw.label)) return null;
    const tone =
      raw.tone === "success" || raw.tone === "warning" || raw.tone === "danger"
        ? raw.tone
        : "neutral";
    return { type: "badge", label: raw.label, tone };
  }

  return null;
}

function stringValue(value: unknown) {
  return typeof value === "string" || typeof value === "number" ? String(value) : "";
}

function booleanValue(value: unknown) {
  return value === true;
}

function warningPanel() {
  return (
    <div
      role="alert"
      className="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-800/60 dark:bg-amber-900/20 dark:text-amber-200"
    >
      插件界面无法渲染
    </div>
  );
}

function renderField({
  field,
  contribution,
  value,
  disabled,
  onChange,
  onCommand,
}: {
  field: HostRenderedField;
  contribution: HostRenderedContributionProps["contribution"];
  value: unknown;
  disabled: boolean;
  onChange: NonNullable<HostRenderedContributionProps["onChange"]>;
  onCommand: NonNullable<HostRenderedContributionProps["onCommand"]>;
}) {
  if (field.type === "boolean") {
    return (
      <FormField key={field.key} label={field.label}>
        <div className="flex justify-end">
          <Switch
            aria-label={field.label}
            checked={booleanValue(value)}
            onCheckedChange={(checked) => onChange(field.key, checked)}
            disabled={disabled}
          />
        </div>
      </FormField>
    );
  }

  if (field.type === "info") {
    return (
      <div key={field.key} className="rounded-lg border border-line-subtle bg-surface-inset p-3">
        <div className="text-sm font-medium text-foreground">{field.label}</div>
        <div className="mt-1 whitespace-pre-wrap text-sm text-muted-foreground">{field.value}</div>
      </div>
    );
  }

  if (field.type === "button") {
    return (
      <div key={field.key} className="flex justify-end">
        <Button
          variant="secondary"
          size="sm"
          onClick={() =>
            onCommand(field.command, {
              pluginId: contribution.pluginId,
              contributionId: contribution.contributionId,
            })
          }
          disabled={disabled}
        >
          {field.label}
        </Button>
      </div>
    );
  }

  if (field.type === "select") {
    return (
      <FormField key={field.key} label={field.label}>
        {(fieldId) => (
          <select
            id={fieldId}
            value={stringValue(value)}
            onChange={(event) => onChange(field.key, event.currentTarget.value)}
            disabled={disabled}
            className="h-10 w-full rounded-lg border border-line bg-surface-inset px-3 text-sm text-foreground outline-none transition-colors focus:border-ring focus:bg-surface-panel focus:ring-2 focus:ring-ring/30 focus:ring-offset-2 focus:ring-offset-background disabled:cursor-not-allowed disabled:bg-surface-muted disabled:opacity-60"
          >
            {field.options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        )}
      </FormField>
    );
  }

  if (field.type === "textarea") {
    return (
      <FormField key={field.key} label={field.label}>
        {(fieldId) => (
          <Textarea
            id={fieldId}
            rows={field.rows ?? 3}
            value={stringValue(value)}
            onChange={(event) => onChange(field.key, event.currentTarget.value)}
            disabled={disabled}
          />
        )}
      </FormField>
    );
  }

  return (
    <FormField key={field.key} label={field.label}>
      {(fieldId) => (
        <Input
          id={fieldId}
          type={field.type === "number" ? "number" : field.type}
          value={stringValue(value)}
          placeholder={
            field.type === "text" || field.type === "password" ? field.placeholder : undefined
          }
          min={field.type === "number" ? field.min : undefined}
          max={field.type === "number" ? field.max : undefined}
          step={field.type === "number" ? field.step : undefined}
          required={field.type === "text" || field.type === "password" ? field.required : undefined}
          onChange={(event) => {
            const nextValue = event.currentTarget.value;
            onChange(
              field.key,
              field.type === "number" && nextValue !== "" ? Number(nextValue) : nextValue
            );
          }}
          disabled={disabled}
        />
      )}
    </FormField>
  );
}

function badgeClassName(tone: HostRenderedSchema & { type: "badge" }) {
  return cn(
    "inline-flex items-center rounded-full px-2.5 py-1 text-xs font-medium",
    tone.tone === "success"
      ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300"
      : tone.tone === "warning"
        ? "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300"
        : tone.tone === "danger"
          ? "bg-destructive/10 text-destructive"
          : "bg-secondary text-secondary-foreground"
  );
}

export function HostRenderedContribution({
  contribution,
  values = {},
  onChange = () => undefined,
  onCommand = () => undefined,
  disabled = false,
}: HostRenderedContributionProps) {
  const schema = parseSchema(contribution.schema);
  if (!schema) return warningPanel();

  if (schema.type === "badge") {
    return <span className={badgeClassName(schema)}>{schema.label}</span>;
  }

  const content = (
    <div className="space-y-3">
      {contribution.title ? (
        <span className="block text-sm font-semibold text-foreground">{contribution.title}</span>
      ) : null}
      <div className="space-y-3">
        {schema.fields.map((field) =>
          renderField({
            field,
            contribution,
            value: values[field.key],
            disabled,
            onChange,
            onCommand,
          })
        )}
      </div>
    </div>
  );

  if (schema.type === "panel") {
    return (
      <Card padding="sm" variant="inset">
        {content}
      </Card>
    );
  }

  return (
    <section className="rounded-lg border border-line-subtle bg-surface-panel p-4">
      {content}
    </section>
  );
}
