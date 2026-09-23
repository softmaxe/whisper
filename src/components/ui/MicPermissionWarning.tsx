import { useTranslation } from "react-i18next";
import { Button } from "./button";
import { AlertCircle } from "../icons";
import { cn } from "../lib/utils";

interface MicPermissionWarningProps {
  error: string | null;
  onOpenSoundSettings: () => void;
  onOpenPrivacySettings: () => void;
}

export default function MicPermissionWarning({
  error,
  onOpenSoundSettings,
  onOpenPrivacySettings,
}: MicPermissionWarningProps) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "rounded-md p-2.5 border",
        "bg-warning/8 border-warning/20 dark:bg-warning/10 dark:border-warning/20"
      )}
    >
      <div className="flex items-center gap-2.5">
        <div className="w-6 h-6 rounded-md bg-warning/15 flex items-center justify-center shrink-0">
          <AlertCircle className="w-3.5 h-3.5 text-amber-600 dark:text-warning" />
        </div>
        <p className="flex-1 text-xs text-amber-700 dark:text-warning/90 leading-snug">
          {error || t("hooks.permissions.warning.messages.macos")}
        </p>
        <div className="flex items-center gap-1 shrink-0">
          <Button
            variant="ghost"
            size="sm"
            onClick={onOpenSoundSettings}
            className="h-6 px-2 text-xs text-amber-700 hover:text-amber-800 hover:bg-amber-100 dark:text-warning dark:hover:text-warning dark:hover:bg-warning/10"
          >
            {t("hooks.permissions.warning.soundLabel")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={onOpenPrivacySettings}
            className="h-6 px-2 text-xs text-amber-700 hover:text-amber-800 hover:bg-amber-100 dark:text-warning dark:hover:text-warning dark:hover:bg-warning/10"
          >
            {t("hooks.permissions.warning.privacyLabel")}
          </Button>
        </div>
      </div>
    </div>
  );
}
