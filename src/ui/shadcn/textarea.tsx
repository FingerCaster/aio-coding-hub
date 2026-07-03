import { cn } from "@/ui/shadcn/utils";

export type TextareaProps = React.ComponentPropsWithRef<"textarea"> & {
  mono?: boolean;
};

export function Textarea({ className, mono, ref, ...props }: TextareaProps) {
  return (
    <textarea
      ref={ref}
      className={cn(
        "w-full resize-y rounded-lg border border-line bg-surface-inset px-3 py-2 text-sm text-foreground outline-none transition-colors",
        "placeholder:text-muted-foreground",
        "focus:border-ring focus:bg-surface-panel focus:ring-2 focus:ring-ring/30 focus:ring-offset-2 focus:ring-offset-background",
        "disabled:cursor-not-allowed disabled:bg-surface-muted disabled:opacity-60",
        mono ? "font-mono" : null,
        className
      )}
      {...props}
    />
  );
}
