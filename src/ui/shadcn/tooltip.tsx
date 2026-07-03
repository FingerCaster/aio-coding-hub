import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { cn } from "@/ui/shadcn/utils";

export const TooltipProvider = TooltipPrimitive.Provider;
export const Tooltip = TooltipPrimitive.Root;
export const TooltipTrigger = TooltipPrimitive.Trigger;

export function TooltipContent({
  className,
  sideOffset = 8,
  ref,
  ...props
}: React.ComponentPropsWithRef<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        ref={ref}
        sideOffset={sideOffset}
        className={cn(
          [
            "z-50 max-w-[280px] whitespace-normal rounded-lg bg-foreground px-2 py-1",
            "text-xs leading-snug text-background shadow-panel outline-none",
          ].join(" "),
          className
        )}
        {...props}
      />
    </TooltipPrimitive.Portal>
  );
}
