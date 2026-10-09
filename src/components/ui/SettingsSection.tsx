import React from "react";
import { useSettingsLayout } from "./useSettingsLayout";

interface SettingsRowProps {
  label: string;
  description?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}

export const SettingsRow: React.FC<SettingsRowProps> = ({
  label,
  description,
  children,
  className = "",
}) => {
  const { isCompact } = useSettingsLayout();

  return (
    <div
      className={`flex ${
        isCompact ? "flex-col items-start gap-2" : "items-center justify-between gap-4"
      } ${className}`}
    >
      <div className="min-w-0 flex-1">
        <p className="text-[13px] font-medium text-foreground">{label}</p>
        {description && (
          <p className="text-xs text-muted-foreground/80 mt-0.5 leading-relaxed">{description}</p>
        )}
      </div>
      <div className={isCompact ? "" : "shrink-0"}>{children}</div>
    </div>
  );
};

interface SettingsFieldProps {
  label: string;
  description?: React.ReactNode;
  htmlFor?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}

// A settings row whose control spans the full width below its label, for text
// inputs and lists that do not fit beside the label like a toggle does.
export const SettingsField: React.FC<SettingsFieldProps> = ({
  label,
  description,
  htmlFor,
  action,
  children,
}) => (
  <div className="space-y-2">
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0 flex-1">
        <label htmlFor={htmlFor} className="block text-[13px] font-medium text-foreground">
          {label}
        </label>
        {description && (
          <p className="text-xs text-muted-foreground/80 mt-0.5 leading-relaxed">{description}</p>
        )}
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </div>
    {children}
  </div>
);

export function SettingsPanel({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`rounded-lg border border-border/70 dark:border-border-subtle/70 bg-card/50 dark:bg-surface-2/50 backdrop-blur-sm divide-y divide-border/60 dark:divide-border-subtle/50 ${className}`}
    >
      {children}
    </div>
  );
}

export function SettingsPanelRow({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  const { isCompact } = useSettingsLayout();

  return (
    <div className={`${isCompact ? "px-3 py-2.5" : "px-4 py-3"} ${className}`}>{children}</div>
  );
}

export function SectionHeader({ title, description }: { title: string; description?: string }) {
  return (
    <div className="mb-3">
      <h3 className="text-lg font-semibold text-foreground tracking-tight">{title}</h3>
      {description && (
        <p className="text-xs text-muted-foreground/80 mt-0.5 leading-relaxed">{description}</p>
      )}
    </div>
  );
}
