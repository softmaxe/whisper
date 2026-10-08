import { Globe } from "../icons";
import { useTranslation } from "react-i18next";
import { cn } from "../lib/utils";

export interface ModelCardOption {
  value: string;
  label: string;
  description?: string;
  icon?: string;
  invertInDark?: boolean;
  // Explicit group for SearchableModelList; falls back to the "provider/"
  // prefix of `value` when absent (e.g. Bedrock ids carry no slash).
  group?: string;
}

const CARD_STYLES = {
  selected:
    "border-primary/30 bg-primary/8 dark:bg-primary/6 dark:border-primary/20 shadow-[0_0_0_1px_oklch(0.62_0.22_260/0.12),0_0_10px_-3px_oklch(0.62_0.22_260/0.18)]",
  default:
    "border-border bg-surface-1 hover:border-border-hover hover:bg-muted dark:border-white/10 dark:bg-white/3 dark:hover:border-white/20 dark:hover:bg-white/8",
};

interface ModelCardProps {
  model: ModelCardOption;
  isSelected: boolean;
  onSelect: (modelId: string) => void;
  // Long-form descriptions (e.g. OpenRouter) fill the row and ellipsize
  // instead of sitting flush-right like short metadata.
  truncateDescription?: boolean;
}

export function ModelCard({
  model,
  isSelected,
  onSelect,
  truncateDescription = false,
}: ModelCardProps) {
  const { t } = useTranslation();

  return (
    <div
      onClick={() => onSelect(model.value)}
      className={`relative w-full p-2 rounded-md border text-start transition-colors duration-200 group overflow-hidden cursor-pointer ${
        isSelected ? CARD_STYLES.selected : CARD_STYLES.default
      }`}
    >
      <div className="flex items-center gap-1.5">
        <div
          className={`w-1.5 h-1.5 rounded-full shrink-0 ${
            isSelected
              ? "bg-primary shadow-[0_0_6px_oklch(0.62_0.22_260/0.6)]"
              : "bg-muted-foreground/30"
          }`}
        />

        {model.icon ? (
          <img
            src={model.icon}
            alt=""
            className={`w-3.5 h-3.5 shrink-0 ${model.invertInDark ? "icon-monochrome" : ""}`}
            aria-hidden="true"
          />
        ) : (
          <Globe className="w-3.5 h-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        )}

        <span
          dir="ltr"
          className={cn(
            "text-sm font-semibold text-foreground truncate tracking-tight",
            truncateDescription && (model.description ? "shrink-0 max-w-[60%]" : "min-w-0 flex-1")
          )}
        >
          {model.label}
        </span>
        {model.description && (
          <span
            className={
              truncateDescription
                ? "text-xs text-muted-foreground/70 truncate min-w-0 flex-1"
                : "text-xs text-muted-foreground/70 tabular-nums shrink-0"
            }
          >
            {model.description}
          </span>
        )}

        <div className="ms-auto flex items-center gap-1.5 shrink-0">
          {isSelected && (
            <span className="text-xs font-medium text-primary px-2 py-0.5 bg-primary/10 rounded-sm">
              {t("common.active")}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

interface ModelCardListProps {
  models: ModelCardOption[];
  selectedModel: string;
  onModelSelect: (modelId: string) => void;
  truncateDescription?: boolean;
}

export default function ModelCardList({
  models,
  selectedModel,
  onModelSelect,
  truncateDescription = false,
}: ModelCardListProps) {
  const { t } = useTranslation();

  if (models.length === 0) {
    return <p className="text-sm text-muted-foreground py-2">{t("models.noneAvailable")}</p>;
  }

  return (
    <div className="space-y-0.5">
      {models.map((model) => (
        <ModelCard
          key={model.value}
          model={model}
          isSelected={selectedModel === model.value}
          onSelect={onModelSelect}
          truncateDescription={truncateDescription}
        />
      ))}
    </div>
  );
}
