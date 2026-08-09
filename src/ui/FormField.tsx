import { useId, type ReactNode } from "react";
import { cn } from "../utils/cn";

type FormFieldBaseProps = {
  /** Visible label text. */
  label: string;
  /** Optional hint shown to the right of the label. */
  hint?: ReactNode;
  className?: string;
};

type FormFieldControlProps = FormFieldBaseProps & {
  /** Render content containing one primary labelable control with the generated control and hint ids. */
  children: (id: string, hintId?: string) => ReactNode;
  group?: false;
  /** Explicit id to associate the label with the control. When omitted a stable id is generated automatically. */
  htmlFor?: string;
};

type FormFieldGroupProps = FormFieldBaseProps & {
  /** Render composite controls as one labelled and optionally described group. */
  children: ReactNode;
  group: true;
  htmlFor?: never;
};

export type FormFieldProps = FormFieldControlProps | FormFieldGroupProps;

export function FormField(props: FormFieldProps) {
  const autoId = useId();
  const fieldId = props.group ? autoId : (props.htmlFor ?? autoId);
  const labelId = props.group ? `${fieldId}-label` : undefined;
  const hintId = props.hint ? `${fieldId}-hint` : undefined;

  return (
    <div className={cn("space-y-1.5", props.className)}>
      <div className="flex items-center justify-between gap-3">
        {props.group ? (
          <span id={labelId} className="text-sm font-medium text-foreground">
            {props.label}
          </span>
        ) : (
          <label htmlFor={fieldId} className="text-sm font-medium text-foreground">
            {props.label}
          </label>
        )}
        {props.hint ? (
          <div id={hintId} className="text-xs text-muted-foreground">
            {props.hint}
          </div>
        ) : null}
      </div>
      {props.group ? (
        <div role="group" aria-labelledby={labelId} aria-describedby={hintId}>
          {props.children}
        </div>
      ) : (
        props.children(fieldId, hintId)
      )}
    </div>
  );
}
