import React from "react";

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

export const Toggle = ({ checked, onChange, disabled = false }: ToggleProps) => {
  const getTrackClasses = () => {
    if (disabled) {
      return checked ? "bg-primary/40" : "bg-muted";
    }
    return checked
      ? "bg-primary hover:bg-primary/90"
      : "bg-muted-foreground/30 hover:bg-muted-foreground/40 dark:bg-surface-raised dark:hover:bg-surface-3";
  };

  return (
    <button
      onClick={() => !disabled && onChange(!checked)}
      disabled={disabled}
      className={`relative inline-flex h-5 w-[34px] items-center rounded-full transition-colors duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:ring-offset-1 ${getTrackClasses()} ${
        disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer"
      }`}
    >
      <span
        className={`inline-block size-3.5 transform rounded-full transition-transform duration-150 ${
          checked ? "translate-x-[17px]" : "translate-x-[3px]"
        } ${disabled ? "bg-muted-foreground/50" : "bg-background shadow-sm"}`}
      />
    </button>
  );
};
