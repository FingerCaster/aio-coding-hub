import * as PopoverPrimitive from "@radix-ui/react-popover";
import { cn } from "@/ui/shadcn/utils";

export const Popover = PopoverPrimitive.Root;
export const PopoverTrigger = PopoverPrimitive.Trigger;

export function PopoverContent({
  className,
  align = "end",
  sideOffset = 8,
  ref,
  ...props
}: React.ComponentPropsWithRef<typeof PopoverPrimitive.Content>) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        ref={ref}
        align={align}
        sideOffset={sideOffset}
        className={cn(
          "z-50 rounded-lg border border-border bg-popover p-3 text-popover-foreground shadow-panel outline-none",
          className
        )}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
}
