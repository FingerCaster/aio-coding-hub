import { useCallback, useState, type ReactNode } from "react";
import { cn } from "../utils/cn";
import { Popover as PopoverRoot, PopoverContent, PopoverTrigger } from "@/ui/shadcn/popover";

export type PopoverProps = {
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  placement?: "top" | "bottom";
  align?: "start" | "center" | "end";
  className?: string;
  contentClassName?: string;
  portalled?: boolean;
};

export function Popover({
  trigger,
  children,
  open: controlledOpen,
  onOpenChange,
  placement = "bottom",
  align = "end",
  className,
  contentClassName,
  portalled = true,
}: PopoverProps) {
  const [internalOpen, setInternalOpen] = useState(false);

  const isControlled = controlledOpen !== undefined;
  const open = isControlled ? controlledOpen : internalOpen;

  const setOpen = useCallback(
    (next: boolean) => {
      if (!isControlled) setInternalOpen(next);
      onOpenChange?.(next);
    },
    [isControlled, onOpenChange]
  );

  return (
    <PopoverRoot open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button type="button" className={cn("inline-flex", className)}>
          {trigger}
        </button>
      </PopoverTrigger>
      <PopoverContent
        side={placement}
        align={align}
        className={contentClassName}
        portalled={portalled}
      >
        {children}
      </PopoverContent>
    </PopoverRoot>
  );
}
