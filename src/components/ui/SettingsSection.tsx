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
        <p className="text-xs font-medium text-foreground">{label}</p>
        {description && (
          <p className="text-xs text-muted-foreground/80 mt-0.5 leading-relaxed">{description}</p>
        )}
      </div>
      <div className={isCompact ? "" : "shrink-0"}>{children}</div>
    </div>
  );
};
