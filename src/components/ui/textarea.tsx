import * as React from "react";

import { cn } from "../lib/utils";

export interface TextareaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {}

const Textarea = React.forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, ...props }, ref) => {
    return (
      <textarea
        className={cn(
          "flex min-h-[80px] w-full rounded-xl border border-border/70 bg-input px-3.5 py-3 text-sm text-foreground shadow-sm transition-colors duration-200 outline-none resize-y cursor-text",
          "placeholder:text-muted-foreground/70",
          "dark:bg-surface-1 dark:border-border-subtle/60 dark:shadow-none",
          "hover:border-border-hover",
          "focus:border-primary focus:ring-2 focus:ring-primary/10",
          "dark:focus:border-border-active dark:focus:ring-ring/10",
          "disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 disabled:bg-muted",
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);
Textarea.displayName = "Textarea";

export { Textarea };
