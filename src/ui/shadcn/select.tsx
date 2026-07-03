import { cn } from "@/ui/shadcn/utils";

export type SelectProps = React.ComponentPropsWithRef<"select"> & {
  mono?: boolean;
};

export function Select({ className, mono, ref, ...props }: SelectProps) {
  return (
    <select
      ref={ref}
      className={cn(
        "h-10 w-full rounded-lg border border-line bg-surface-inset px-3 text-sm text-foreground outline-none transition-colors",
        "focus:border-ring focus:bg-surface-panel focus:ring-2 focus:ring-ring/30 focus:ring-offset-2 focus:ring-offset-background",
        "disabled:cursor-not-allowed disabled:bg-surface-muted disabled:opacity-60",
        mono ? "font-mono" : null,
        className
      )}
      {...props}
    />
  );
}
