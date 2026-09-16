import type React from "react";
import { useTranslation } from "react-i18next";
import { BarChart3, BookOpen, Home, Upload } from "./icons";

export type ControlPanelView = "home" | "insights" | "dictionary" | "upload";

export interface ControlPanelNavItem {
  id: ControlPanelView;
  label: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
}

/**
 * Single source of truth for main-window navigation. The sidebar renders these
 * rows and the top bar shows the active item's label as the page title, so the
 * two can never disagree.
 */
export function useControlPanelNavItems(): ControlPanelNavItem[] {
  const { t } = useTranslation();
  return [
    { id: "home", label: t("sidebar.home"), icon: Home },
    { id: "insights", label: t("sidebar.insights"), icon: BarChart3 },
    { id: "upload", label: t("sidebar.upload"), icon: Upload },
    { id: "dictionary", label: t("sidebar.dictionary"), icon: BookOpen },
  ];
}
